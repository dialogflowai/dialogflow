use std::sync::OnceLock;
use std::vec::Vec;

// use futures_util::StreamExt;
// use sqlx::{Row, Sqlite};

use super::dto::QuestionAnswerPair;
use crate::ai::embedding;
use crate::man::settings;
use crate::result::{Error, Result};
use crate::retry_on_busy;

// type SqliteConnPool = sqlx::Pool<Sqlite>;

// // static DATA_SOURCE: OnceCell<SqliteConnPool> = OnceCell::new();
// static DATA_SOURCE: OnceLock<SqliteConnPool> = OnceLock::new();
static DATA_SOURCE: OnceLock<turso::Database> = OnceLock::new();
// static DATA_SOURCES: OnceLock<Mutex<HashMap<String, SqliteConnPool>>> = OnceLock::new();

/// 写锁争用的总等待时长。
///
/// turso 的忙等是基于 yield 的、不阻塞线程（见 turso_core 的 busy.rs），所以
/// 它既能把并发写串行化成"排队等待"，又不会占住 tokio worker。
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// 所有连接都必须走这里。
///
/// 连接是每次调用新建、从不复用的，`busy_timeout` 又是连接级设置，所以散在
/// 各调用点设置必然漏 —— 集中在这一个 helper 里。
fn conn() -> Result<turso::Connection> {
    let c = DATA_SOURCE.get().unwrap().connect()?;
    c.busy_timeout(BUSY_TIMEOUT)?;
    Ok(c)
}

pub(crate) async fn init_datasource() -> Result<()> {
    let p = std::path::Path::new(".").join("data");
    if !p.exists() {
        std::fs::create_dir_all(&p).expect("Create data directory failed.");
    }
    let p = p.join("qa.dat");
    let turso = turso::Builder::new_local(p.as_path().to_str().unwrap())
        .build()
        .await?;
    DATA_SOURCE
        .set(turso)
        .map_err(|_| Error::WithMessage(String::from("Datasource has been set.")))
    // let p = get_sqlite_path();
    // let pool = crate::db::init_sqlite_datasource(p.as_path()).await?;
    // DATA_SOURCE
    //     .set(pool)
    //     .map_err(|_| Error::WithMessage(String::from("Datasource has been set.")))
}

/*
pub async fn shutdown_db() {
    // let mut r = match DATA_SOURCES.lock() {
    //     Ok(l) => l,
    //     Err(e) => e.into_inner(),
    // };
    // let all_keys: Vec<String> = r.keys().map(|k| String::from(k)).collect();
    // let mut pools: Vec<SqliteConnPool> = Vec::with_capacity(all_keys.len());
    // for key in all_keys {
    //     let v = r.remove(&key).unwrap();
    //     pools.push(v);
    // }
    // tokio::task::spawn_blocking(|| async move {
    //     for p in pools.iter() {
    //         p.close().await;
    //     }
    // });
    DATA_SOURCE.get().unwrap().close().await;
}
*/

pub(crate) async fn init_tables(robot_id: &str) -> Result<()> {
    // println!("Init database");
    // 索引名在 SQLite 里是库级而非表级的，必须带上 robot_id，否则第二个机器人
    // 建表时会撞名。IF NOT EXISTS 是给 robot::new 的重复调用兜底。
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS {robot_id} (
            id INTEGER NOT NULL PRIMARY KEY,
            qa_data TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_{robot_id}_created_at ON {robot_id} (created_at);"
    );
    retry_on_busy!(async {
        let conn = conn()?;
        // 必须用 `execute_batch`：`execute` 走的是 `prepare_single`，多语句字符串里
        // **只有第一条**会被执行（实测：`execute("CREATE TABLE a…; CREATE TABLE b…")`
        // 之后 b 不存在，`execute_batch` 则两条都建）。这里两条语句都要生效。
        conn.execute_batch(sql.as_str()).await?;
        Ok(())
    })
}

/// 列出该机器人的全部问答对。
///
/// 重建索引的编排层（`man::reindex`）要遍历它们逐个重算向量。直接复用 `save` 而不是
/// 自己拼 INSERT：`save` 会连带把 `qa_data` 里的 `vec_row_id` 刷新成新行号，漏掉这
/// 一步会让后续编辑指向已经不存在（或已换人）的向量行。
pub(crate) async fn list(robot_id: &str) -> Result<Vec<QuestionAnswerPair>> {
    let conn = conn()?;
    let sql = format!("SELECT qa_data FROM {robot_id} ORDER BY created_at DESC",);
    let mut rows = conn.query(sql, ()).await?;
    let mut d: Vec<QuestionAnswerPair> = Vec::with_capacity(10);
    while let Some(row) = rows.next().await? {
        d.push(serde_json::from_str(row.get_value(0)?.as_text().unwrap())?);
    }
    Ok(d)

}

pub(crate) async fn save(robot_id: &str, mut d: QuestionAnswerPair) -> Result<i64> {
    // 重试安全性：`d.id` 一旦被赋值，下一轮尝试就会走 UPDATE 分支去更新一行从未
    // 提交（事务已回滚）的记录 —— 记录凭空消失、只剩向量。所以"是不是新建"只在
    // 重试之外判一次，就地修改全部作用在副本上，只有 commit 成功了才落回 `d`。
    // 见 result::retry_on_busy 的文档。
    let is_new = d.id.is_none();

    // 阶段 2：向量（模型推理）全部算在事务之外。
    //
    // 原先是「开事务 → 在循环里逐条 await 推理 → 提交」，一次 QA 编辑要跑
    // 1 + N 条推理，全程握着 turso 的写锁不松手，其他写者只能干等 busy_timeout
    // 耗尽。现在事务里只剩纯 SQL（微秒级），重放也便宜。
    //
    // 顺序必须和下面写库时的顺序一致：[主问题] + similar_questions。
    let mut vecs: Vec<turso::Value> = Vec::with_capacity(1 + d.similar_questions.len());
    let mut vec_len = 0usize;
    for q in std::iter::once(&d.question).chain(d.similar_questions.iter()) {
        let vectors = embedding::embedding(robot_id, &q.question).await?;
        if vectors.0.is_empty() {
            let err = format!("{} embedding data is empty", &q.question);
            log::warn!("{}", &err);
            return Err(Error::WithMessage(err));
        }
        if vec_len == 0 {
            vec_len = vectors.0.len();
        }
        vecs.push(embedding::vec_to_db(&vectors.0));
    }
    log::info!("vectors.0.len() = {}", vec_len);

    let record_id = retry_on_busy!(async {
        let mut work = d.clone();
        let mut conn = conn()?;
        let tx = conn.transaction().await?;
        let record_id: i64;
        if is_new {
            let sql = format!("INSERT INTO {robot_id}(qa_data, created_at)VALUES(?, unixepoch())");
            let mut stmt = tx.prepare(&sql).await?;
            stmt.execute((serde_json::to_string(&work)?,)).await?;
            record_id = tx.last_insert_rowid();
            work.id = Some(record_id);
        } else {
            record_id = work.id.unwrap();
            // Drop all existing vectors of this QnA; they are re-inserted below.
            // This also cleans up rows orphaned by similar questions removed in
            // this edit, which a plain per-row UPDATE would leave behind.
            let sql = format!("DELETE FROM {robot_id}_vec WHERE qa_id = ?1");
            let mut stmt = tx.prepare(&sql).await?;
            stmt.execute([turso::Value::Integer(record_id)]).await?;
        }

        let mut created_table = false;

        let mut insert_stmt = Option::None::<turso::Statement>;
        // 每行向量的 rowid，顺序与 `vecs` 一致（主问题在前，相似问题在后）。
        let mut row_ids: Vec<u64> = Vec::with_capacity(vecs.len());
        for v in vecs.iter() {
            if !created_table {
                let sql = format!(
                    "CREATE TABLE IF NOT EXISTS {robot_id}_vec (
                        id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                        qa_id INTEGER NOT NULL,
                        qa_vec F32_BLOB({vec_len}) NOT NULL
                    );
                    ",
                );
                tx.execute(sql.as_str(), ()).await?;
                created_table = true;
            }
            if insert_stmt.is_none() {
                let sql =
                    format!("INSERT INTO {robot_id}_vec (qa_id, qa_vec)VALUES(?1, vector32(?2))",);
                insert_stmt = Some(tx.prepare(&sql).await?);
            }
            insert_stmt
                .as_mut()
                .unwrap()
                .execute((record_id, v.clone()))
                .await?;
            row_ids.push(tx.last_insert_rowid() as u64);
        }
        work.question.vec_row_id = Some(row_ids[0]);
        for (q, id) in work.similar_questions.iter_mut().zip(row_ids[1..].iter()) {
            q.vec_row_id = Some(*id);
        }
        // Write the JSON back with the real id and the vec row ids assigned
        // above, otherwise edits would re-insert vectors every time.
        let sql = format!("UPDATE {robot_id} SET qa_data = ? WHERE id = ?");
        let mut stmt = tx.prepare(&sql).await?;
        stmt.execute((serde_json::to_string(&work)?, record_id))
            .await?;
        tx.commit().await?;
        // 提交成功，现在这些 id 才是真实存在的，可以安全地落到入参上。
        d = work;
        Ok(record_id)
    })?;
    // 向量真的落库了，记下它是用哪个模型/维度算出来的（见
    // settings::stamp_embedding_index）。打标失败不回滚、不报错：库里已经有正确的
    // 向量，丢的只是一条"将来能提醒用户换过模型"的线索。
    if let Err(e) = settings::stamp_embedding_index(robot_id).await {
        log::warn!("Stamping embedding index of {robot_id} failed: {e:?}");
    }
    Ok(record_id)
}

/// 只删向量表（重建索引用）。
///
/// 与 [`remove_tables`] 的区别：**主表不动**。`{robot_id}` 里存的是 `qa_data`（问题、
/// 相似问题、答案的 JSON），它是重建向量的**载荷**；`{robot_id}_vec` 才是可以随便
/// 丢掉重算的那一半。
///
/// 为什么重建要整表丢掉而不是逐条覆盖：已经被删掉的问答对会留下孤儿向量行，它们的
/// 维度还是旧配置的，`vector_distance_cos` 扫到就会让整条检索报错——那正是重建要修的
/// 东西。`qa::save` 每次会重新指派 `qa_data` 里的 `vec_row_id`，所以丢掉行号也没有
/// 后顾之忧。
pub(crate) async fn remove_vector_table(robot_id: &str) -> Result<()> {
    let sql = format!("DROP TABLE IF EXISTS {robot_id}_vec;");
    retry_on_busy!(async {
        let conn = conn()?;
        conn.execute(sql.as_str(), ()).await?;
        Ok(())
    })
}

/// 删掉该机器人在 `qa.dat` 里的两张表（`robot::purge` 用）。
///
/// 两张表都是懒创建的（第一次 `list`/`save` 才会建），所以必须 `IF EXISTS`。
pub(crate) async fn remove_tables(robot_id: &str) -> Result<()> {
    let sql = format!(
        "DROP TABLE IF EXISTS {robot_id};
         DROP TABLE IF EXISTS {robot_id}_vec;"
    );
    retry_on_busy!(async {
        let conn = conn()?;
        // 同 `init_tables`：多语句必须走 `execute_batch`，否则 `_vec` 表会被漏掉。
        conn.execute_batch(sql.as_str()).await?;
        Ok(())
    })
}

pub(crate) async fn delete(robot_id: &str, d: QuestionAnswerPair) -> Result<()> {
    // 入参不被修改，且两个 DELETE 都在同一个事务里，重跑无残留。
    let id = d.id.unwrap();
    retry_on_busy!(async {
        let mut conn = conn()?;
        let tx = conn.transaction().await?;
        let sql = format!("DELETE FROM {robot_id}_vec WHERE qa_id = ?1");
        let mut stmt = tx.prepare(&sql).await?;
        let value = turso::Value::Integer(id);
        stmt.execute([value.clone()]).await?;
        let sql = format!("DELETE FROM {robot_id} WHERE id = ?1");
        let mut stmt = tx.prepare(&sql).await?;
        stmt.execute([value]).await?;
        tx.commit().await?;
        Ok(())
    })
}

pub(crate) async fn retrieve_answer(
    robot_id: &str,
    question: &str,
) -> Result<(Option<QuestionAnswerPair>, f64)> {
    let vectors = embedding::embedding(robot_id, question).await?;
    if vectors.0.is_empty() {
        let err = format!("{question} embedding data is empty");
        log::warn!("{}", &err);
        return Err(Error::WithMessage(err));
    }
    let conn = conn()?;

    let sql = format!(
        "
        SELECT qa_data, v.distance FROM {robot_id} q INNER JOIN
        (SELECT qa_id, vector_distance_cos(qa_vec, vector32(?1)) AS distance FROM {robot_id}_vec ORDER BY distance ASC LIMIT 1) v
        ON q.id = v.qa_id
        "
    );
    let mut results = conn.query(sql, [embedding::vec_to_db(&vectors.0)]).await?;
    if let Some(row) = results.next().await? {
        return Ok((
            Some(serde_json::from_str(row.get_value(0)?.as_text().unwrap())?),
            row.get_value(1)?.as_real().unwrap().clone(),
        ));
    }
    Ok((None, 1.0))
}
