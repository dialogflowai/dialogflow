use std::sync::OnceLock;
use std::vec::Vec;

// use futures_util::StreamExt;
// use sqlx::{Row, Sqlite};

use super::dto::IntentPhraseData;
use crate::ai::embedding::embedding;
use crate::man::settings;
use crate::result::{Error, Result};
use crate::retry_on_busy;

// type SqliteConnPool = sqlx::Pool<Sqlite>;

// static DATA_SOURCE: OnceCell<SqliteConnPool> = OnceCell::new();
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
// static INDEXES: LazyLock<Mutex<HashMap<String, usearch::Index>>> =
//     LazyLock::new(|| Mutex::new(HashMap::with_capacity(32)));

// fn get_sqlite_path() -> std::path::PathBuf {
//     let p = std::path::Path::new(".").join("data");
//     if !p.exists() {
//         std::fs::create_dir_all(&p).expect("Create data directory failed.");
//     }
//     p.join("ripd.dat")
// }

pub(crate) async fn init_datasource() -> Result<()> {
    let p = std::path::Path::new(".").join("data");
    if !p.exists() {
        std::fs::create_dir_all(&p).expect("Create data directory failed.");
    }
    let p = p.join("phrase.dat");
    // log::info!("Init phrase datasource, path = {:?}", p.as_path().to_str().unwrap());
    let turso = turso::Builder::new_local(p.as_path().to_str().unwrap())
        .build()
        .await?;
    DATA_SOURCE
        .set(turso)
        .map_err(|_| Error::WithMessage(String::from("Datasource has been set.")))
}

// pub(crate) async fn init_tables(robot_id: &str) -> Result<()> {
//     // println!("Init database");
//     // let ddl = include_str!("./embedding_ddl.sql");
//     let sql = format!(
//         "CREATE TABLE {robot_id} (
//             id INTEGER NOT NULL PRIMARY KEY,
//             -- id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
//             intent_id TEXT NOT NULL,
//             intent_name TEXT NOT NULL,
//             phrase_vec F32_BLOB(384)
//         );",
//     );
//     // log::info!("sql = {}", &sql);
//     let mut stream = sqlx::raw_sql(&sql).execute_many(DATA_SOURCE.get().unwrap());
//     while let Some(res) = stream.next().await {
//         match res {
//             Ok(_r) => log::info!("Initialized phrase table"),
//             Err(e) => log::error!("Create table failed, err: {e:?}"),
//         }
//     }
//     // let dml = include_str!("../resource/sql/dml.sql");
//     // if let Err(e) = sqlx::query(dml).execute(&pool).await {
//     //     panic!("{:?}", e);
//     // }
//     Ok(())
// }

pub(crate) async fn search(robot_id: &str, vectors: &Vec<f32>) -> Result<Vec<(String, f64)>> {
    let conn = conn()?;
    //*
    let sql = format!(
        "SELECT intent_name, vector_distance_cos(phrase_vec, vector32(?1)) AS distance FROM {robot_id}",
    );
    let mut results = conn.query(sql, [serde_json::to_string(vectors)?]).await?;
    while let Some(r) = results.next().await? {
        log::info!(
            "intent_name = {}, distance = {}",
            r.get_value(0)?.as_text().unwrap(),
            r.get_value(1)?.as_real().unwrap()
        );
    }
    //*/
    let sql = format!(
        "SELECT intent_id, intent_name, vector_distance_cos(phrase_vec, vector32(?1)) AS distance FROM {robot_id} ORDER BY distance ASC LIMIT 1",
    );
    // log::info!("sql = {} {}", &sql, serde_json::to_string(vectors)?);
    let mut results = conn.query(sql, [serde_json::to_string(vectors)?]).await?;
    // let results = sqlx::query::<Sqlite>(&sql)
    //     .bind(serde_json::to_string(vectors)?)
    //     .fetch_all(DATA_SOURCE.get().unwrap())
    //     .await?;
    let mut names = Vec::with_capacity(1);
    while let Some(r) = results.next().await? {
        names.push((
            r.get_value(1)?.as_text().unwrap().to_string(),
            r.get_value(2)?.as_real().unwrap().clone(),
        ));
    }
    Ok(names)
}

// fn update_idx(robot_id: &str, key: u64, vec: &[f32]) -> Result<()> {
//     let mut idxes = INDEXES.lock()?;
//     let p = std::path::Path::new("ipvd.vec");
//     let s = p.display().to_string();
//     if !idxes.contains_key(robot_id) {
//         let options = usearch::IndexOptions {
//             dimensions: vec.len(),
//             metric: usearch::MetricKind::Cos,
//             quantization: usearch::ScalarKind::F32,
//             connectivity: 0,                        // zero for auto
//             expansion_add: 0,                       // zero for auto
//             expansion_search: 0,                    // zero for auto
//             multi: false,
//         };
//         let index: usearch::Index = usearch::new_index(&options).unwrap();
//         if p.exists() {
//             index.load(&s)?;
//         }
//         idxes.insert(String::from(robot_id), index);
//     }
//     let idx = idxes.get(robot_id).unwrap();
//     log::info!("idx memory_usage: {}", idx.memory_usage());
//     idx.add(key, vec)?;
//     idx.save(&s)?;
//     Ok(())
// }

pub(crate) async fn add(
    robot_id: &str,
    vec_row_id: Option<i64>,
    intent_id: &str,
    intent_name: &str,
    phrase: &str,
) -> Result<i64> {
    if intent_name.is_empty() {
        let err = format!("{phrase} intent_name is empty");
        log::warn!("{}", &err);
        return Err(Error::WithMessage(err));
    }
    // check_datasource(robot_id, intent_id).await?;
    let vectors = embedding(robot_id, phrase).await?;
    if vectors.0.is_empty() {
        let err = format!("{phrase} embedding data is empty");
        log::warn!("{}", &err);
        return Err(Error::WithMessage(err));
    }
    // log::info!("vectors.0.len() = {}", vectors.0.len());
    // log::info!("vectors.0.len() = {}", vectors.0.len());
    // 向量只算一次、不进重试 —— 重放模型推理太贵。下面的 SQL 入参都不被修改，
    // 重跑只会原样重放这两条语句。
    let vec_len = vectors.0.len();
    let vec_json = serde_json::to_string(&vectors.0)?;
    let id = retry_on_busy!(async {
        let mut conn = conn()?;
        // CREATE TABLE 和 INSERT/UPDATE 收在同一个事务里：原先它们是两次独立加锁，
        // 中途失败会留下"表建好了但没数据"的中间态，重试又要重新抢两次锁。
        let tx = conn.transaction().await?;
        let id = if vec_row_id.is_none() {
            let sql = format!(
                "CREATE TABLE IF NOT EXISTS {robot_id} (
                    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                    intent_id TEXT NOT NULL,
                    intent_name TEXT NOT NULL,
                    phrase TEXT NOT NULL,
                    phrase_vec F32_BLOB({vec_len}) NOT NULL
                )"
            );
            tx.execute(sql.as_str(), ()).await?;
            let sql = format!(
                "INSERT INTO {robot_id} (intent_id, intent_name, phrase, phrase_vec)VALUES(?1, ?2, ?3, vector32(?4))",
            );
            let mut stmt = tx.prepare(&sql).await?;
            stmt.execute((
                String::from(intent_id),
                String::from(intent_name),
                String::from(phrase),
                vec_json.clone(),
            ))
            .await?;
            tx.last_insert_rowid()
        } else {
            let row_id = vec_row_id.unwrap();
            let sql = format!(
                "UPDATE {robot_id} SET phrase = ?1, phrase_vec = vector32(?2) WHERE id = ?3",
            );
            let mut stmt = tx.prepare(&sql).await?;
            stmt.execute((String::from(phrase), vec_json.clone(), row_id))
                .await?;
            row_id
        };
        tx.commit().await?;
        log::info!("last_insert_rowid = {}", id);
        Ok(id)
    })?;
    // 短语向量已经落库，记下它是用哪个模型/维度算的（见
    // settings::stamp_embedding_index）。打标失败不影响这条短语。
    if let Err(e) = settings::stamp_embedding_index(robot_id).await {
        log::warn!("Stamping embedding index of {robot_id} failed: {e:?}");
    }
    Ok(id)
}

/// 给一组**已存在**的短语重算向量。
///
/// 阶段 2 改了两处：
///
/// 1. 推理全部提到事务外。原来是每条短语一次 `add`，每次都是一次独立自动提交 +
///    一次模型推理 —— N 条短语就是 N 次加锁、N 次 fsync，而且推理夹在写锁之间。
/// 2. 全部写操作合并进**一个**事务，成功或失败都是原子的。
///
/// 语义与原来的逐条 `add(Some(id), …)` 一致：按 id 更新已存在的向量行。
pub(crate) async fn reindex(
    robot_id: &str,
    intents: &[crate::intent::dto::IntentDetail],
) -> Result<usize> {
    let mut n = 0usize;
    for d in intents {
        if d.phrases.is_empty() {
            continue;
        }
        batch_add(robot_id, &d.intent_id, &d.intent_name, &d.phrases).await?;
        n += d.phrases.len();
    }
    Ok(n)
}

pub(crate) async fn batch_add(
    robot_id: &str,
    intent_id: &str,
    intent_name: &str,
    phrases: &[IntentPhraseData],
) -> Result<()> {
    if phrases.is_empty() {
        return Ok(());
    }
    if intent_name.is_empty() {
        let err = format!("intent_id {intent_id} intent_name is empty");
        log::warn!("{}", &err);
        return Err(Error::WithMessage(err));
    }
    // check_datasource(robot_id, intent_id).await?;
    let mut vec_len = 0usize;
    let mut vecs: Vec<String> = Vec::with_capacity(phrases.len());
    for p in phrases.iter() {
        let vectors = embedding(robot_id, &p.phrase).await?;
        if vectors.0.is_empty() {
            let err = format!("{} embedding data is empty", &p.phrase);
            log::warn!("{}", &err);
            return Err(Error::WithMessage(err));
        }
        if vec_len == 0 {
            vec_len = vectors.0.len();
        }
        vecs.push(serde_json::to_string(&vectors.0)?);
    }
    retry_on_busy!(async {
        let mut conn = conn()?;
        let tx = conn.transaction().await?;
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS {robot_id} (
                id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                intent_id TEXT NOT NULL,
                intent_name TEXT NOT NULL,
                phrase TEXT NOT NULL,
                phrase_vec F32_BLOB({vec_len}) NOT NULL
            )"
        );
        tx.execute(sql.as_str(), ()).await?;
        let sql = format!(
            "UPDATE {robot_id} SET intent_id = ?1, intent_name = ?2, phrase = ?3, phrase_vec = vector32(?4) WHERE id = ?5",
        );
        let mut stmt = tx.prepare(&sql).await?;
        for (p, vec_json) in phrases.iter().zip(vecs.iter()) {
            stmt.execute((
                String::from(intent_id),
                String::from(intent_name),
                p.phrase.clone(),
                vec_json.clone(),
                p.id,
            ))
            .await?;
        }
        tx.commit().await?;
        Ok(())
    })?;
    if let Err(e) = settings::stamp_embedding_index(robot_id).await {
        log::warn!("Stamping embedding index of {robot_id} failed: {e:?}");
    }
    Ok(())
}

pub(crate) async fn remove(robot_id: &str, id: i64) -> Result<()> {
    // INDEXES.lock()?.get(robot_id).and_then(|idx| {idx.remove(id as u64); None::<()>});
    let sql = format!("DELETE FROM {robot_id} WHERE id = ?1");
    // sqlx::query::<Sqlite>(&sql)
    //     .bind(id)
    //     .execute(DATA_SOURCE.get().unwrap())
    //     .await?;
    retry_on_busy!(async {
        let conn = conn()?;
        conn.execute(sql.as_str(), [id]).await?;
        Ok(())
    })
}

pub(crate) async fn remove_by_intent_id(robot_id: &str, intent_id: &str) -> Result<()> {
    let sql = format!("DELETE FROM {robot_id} WHERE intent_id = ?1");
    // match sqlx::query::<Sqlite>(&sql)
    //     .bind(intent_id)
    //     .execute(DATA_SOURCE.get().unwrap())
    //     .await
    // {
    //     Ok(_) => return Ok(()),
    //     Err(e) => match e {
    //         sqlx::Error::Database(database_error) => {
    //             if let Some(code) = database_error.code() {
    //                 if code.eq("1") {
    //                     return Ok::<_, Error>(());
    //                 }
    //             }
    //         }
    //         _ => return Err(e.into()),
    //     },
    // };
    retry_on_busy!(async {
        let conn = conn()?;
        conn.execute(sql.as_str(), [String::from(intent_id)]).await?;
        Ok(())
    })
}

pub(crate) async fn remove_tables(robot_id: &str) -> Result<()> {
    // 表只在第一次 `add` 时创建，没加过短语的机器人根本没有这张表 ——
    // 缺 IF EXISTS 会让 `robot::purge` 在第一步就整个中断，什么也删不掉。
    let sql = format!("DROP TABLE IF EXISTS {robot_id}");
    // match sqlx::query::<Sqlite>(&sql)
    //     .execute(DATA_SOURCE.get().unwrap())
    //     .await
    // {
    //     Ok(_) => return Ok(()),
    //     Err(e) => match e {
    //         sqlx::Error::Database(database_error) => {
    //             if let Some(code) = database_error.code() {
    //                 if code.eq("1") {
    //                     return Ok::<_, Error>(());
    //                 }
    //             }
    //         }
    //         _ => return Err(e.into()),
    //     },
    // };
    retry_on_busy!(async {
        let conn = conn()?;
        conn.execute(sql.as_str(), ()).await?;
        Ok(())
    })
}
