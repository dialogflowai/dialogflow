use std::sync::OnceLock;
use std::vec::Vec;

use futures_util::StreamExt;
use sqlx::{Row, Sqlite};

use super::dto::{QuestionAnswerPair, QuestionData};
use crate::ai::embedding::embedding;
use crate::result::{Error, Result};

type SqliteConnPool = sqlx::Pool<Sqlite>;

// static DATA_SOURCE: OnceCell<SqliteConnPool> = OnceCell::new();
static DATA_SOURCE: OnceLock<SqliteConnPool> = OnceLock::new();
// static DATA_SOURCES: OnceLock<Mutex<HashMap<String, SqliteConnPool>>> = OnceLock::new();

fn get_sqlite_path() -> std::path::PathBuf {
    let p = std::path::Path::new(".").join("data");
    if !p.exists() {
        std::fs::create_dir_all(&p).expect("Create data directory failed.");
    }
    p.join("kbqaev.dat")
}

pub(crate) async fn init_datasource() -> Result<()> {
    let p = get_sqlite_path();
    let pool = crate::db::init_sqlite_datasource(p.as_path()).await?;
    DATA_SOURCE
        .set(pool)
        .map_err(|_| Error::WithMessage(String::from("Datasource has been set.")))
}

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

pub(crate) async fn init_tables(robot_id: &str) -> Result<()> {
    // println!("Init database");
    // let ddl = include_str!("./embedding_ddl.sql");
    let sql = format!(
        "CREATE TABLE {}_question_vec_row_id (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT
        );
        CREATE TABLE {}_qa (
            id TEXT NOT NULL PRIMARY KEY,
            qa_data TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX idx_{}_created_at ON {}_qa (created_at);",
        robot_id, robot_id, robot_id, robot_id
    );
    // log::info!("sql = {}", &sql);
    let mut stream = sqlx::raw_sql(&sql).execute_many(DATA_SOURCE.get().unwrap());
    while let Some(res) = stream.next().await {
        match res {
            Ok(_r) => log::info!("Initialized QnA table"),
            Err(e) => log::error!("Create table failed, err: {:?}", e),
        }
    }
    // let dml = include_str!("../resource/sql/dml.sql");
    // if let Err(e) = sqlx::query(dml).execute(&pool).await {
    //     panic!("{:?}", e);
    // }
    Ok(())
}

// sqlite_trans!(
//     fn dq(    robot_id: &str,
//         mut d: QuestionAnswersPair,
//         transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,) -> Result<String> {
//         Ok(String::new())
//     }
// );

pub(crate) async fn list(robot_id: &str) -> Result<Vec<QuestionAnswerPair>> {
    let sql = format!(
        "SELECT qa_data FROM {}_qa ORDER BY created_at DESC",
        robot_id
    );
    let results = sqlx::query::<Sqlite>(&sql)
        .fetch_all(DATA_SOURCE.get().unwrap())
        .await?;
    let mut d: Vec<QuestionAnswerPair> = Vec::with_capacity(results.len());
    for r in results.iter() {
        d.push(serde_json::from_str(dbg!(r.try_get(0)?))?);
    }
    Ok(d)
}

// crate::sqlite_trans! {
//     fn save2(robot_id: &str, d: QuestionAnswerPair) -> Result<()> {
//         Ok(())
//     }
// }

pub(crate) async fn save(robot_id: &str, d: QuestionAnswerPair) -> Result<String> {
    let ds = DATA_SOURCE.get().unwrap();
    let mut transaction = ds.begin().await?;
    let r = save_inner(robot_id, d, &mut transaction).await;
    if r.is_ok() {
        transaction.commit().await?;
    } else {
        transaction.rollback().await?;
    }
    r
}

async fn save_inner(
    robot_id: &str,
    mut d: QuestionAnswerPair,
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<String> {
    let mut questions: Vec<&mut QuestionData> = Vec::with_capacity(5);
    questions.push(&mut d.question);
    if !d.similar_questions.is_empty() {
        let similar_questions: Vec<&mut QuestionData> = d.similar_questions.iter_mut().collect();
        questions.extend(similar_questions);
    }
    let mut new_record = false;
    if d.id.is_none() {
        d.id = Some(scru128::new_string());
        new_record = true;
    }
    let mut vec_row_ids: Vec<i64> = Vec::with_capacity(questions.len());
    for q in questions.iter_mut() {
        let vectors = embedding(robot_id, &q.question).await?;
        if vectors.0.is_empty() {
            let err = format!("{} embedding data is empty", &q.question);
            log::warn!("{}", &err);
            return Err(Error::WithMessage(err));
        }

        log::info!("vectors.0.len() = {}", vectors.0.len());
        if q.vec_row_id.is_none() {
            let sql = format!(
                "INSERT INTO {}_question_vec_row_id (id)VALUES(NULL)",
                robot_id
            );
            let last_insert_rowid = sqlx::query::<Sqlite>(&sql)
                .execute(&mut **transaction)
                .await?
                .last_insert_rowid();
            let sql = format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS {} USING vec0 (
                qa_id TEXT NOT NULL,
                vectors float[{}]
            );
            INSERT INTO {} (rowid, qa_id, vectors)VALUES(?, ?, ?)",
                //  ON CONFLICT(rowid) DO UPDATE SET vectors = excluded.vectors;
                robot_id,
                vectors.0.len(),
                robot_id
            );
            sqlx::query::<Sqlite>(&sql)
                .bind(last_insert_rowid)
                .bind(d.id.as_ref().unwrap())
                .bind(serde_json::to_string(&vectors.0)?)
                .execute(&mut **transaction)
                .await?;
            q.vec_row_id = Some(last_insert_rowid);
        } else {
            let sql = format!("UPDATE {} SET vectors = ? WHERE rowid = ?", robot_id);
            let vec_row_id = q.vec_row_id.unwrap();
            sqlx::query::<Sqlite>(&sql)
                .bind(serde_json::to_string(&vectors.0)?)
                .bind(vec_row_id)
                .execute(&mut **transaction)
                .await?;
            vec_row_ids.push(vec_row_id);
        };
    }
    if !vec_row_ids.is_empty() {
        let params = format!("?{}", ", ?".repeat(vec_row_ids.len() - 1));
        let sql = format!("DELETE FROM {} WHERE rowid NOT IN ({})", robot_id, params);
        let mut query = sqlx::query(&sql);
        for i in vec_row_ids {
            query = query.bind(i);
        }
        query.fetch_all(&mut **transaction).await?;
    }
    if new_record {
        let sql = format!(
            "INSERT INTO {}_qa(id, qa_data, created_at)VALUES(?, ?, unixepoch())",
            robot_id
        );
        sqlx::query::<Sqlite>(&sql)
            .bind(d.id.as_ref().unwrap())
            .bind(dbg!(serde_json::to_string(&d)?))
            .execute(&mut **transaction)
            .await?;
    } else {
        let sql = format!("UPDATE {}_qa SET qa_data = ? WHERE id = ?", robot_id);
        sqlx::query::<Sqlite>(&sql)
            .bind(dbg!(serde_json::to_string(&d)?))
            .bind(d.id.as_ref().unwrap())
            .execute(&mut **transaction)
            .await?;
    }
    Ok(d.id.unwrap())
}

pub(crate) async fn delete(robot_id: &str, d: QuestionAnswerPair) -> Result<()> {
    let ds = DATA_SOURCE.get().unwrap();
    let mut transaction = ds.begin().await?;
    let r = delete_inner(robot_id, d, &mut transaction).await;
    if r.is_ok() {
        transaction.commit().await?;
    } else {
        transaction.rollback().await?;
    }
    r
}

async fn delete_inner(
    robot_id: &str,
    d: QuestionAnswerPair,
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<()> {
    //todo sqlx prepare statement
    let sql = format!(
        "
    DELETE FROM {} WHERE qa_id = ?;
    DELETE FROM {}_qa WHERE id = ?;
    ",
        robot_id, robot_id
    );
    let qa_id = d.id.as_ref().unwrap();
    let r = sqlx::query(&sql)
        .bind(qa_id)
        .bind(qa_id)
        .execute(&mut **transaction)
        .await?;
    log::info!("{}", r.rows_affected());
    Ok(())
}

pub(crate) async fn retrieve_answer(
    robot_id: &str,
    question: &str,
) -> Result<(Option<QuestionAnswerPair>, f64)> {
    let vectors = embedding(robot_id, question).await?;
    if vectors.0.is_empty() {
        let err = format!("{} embedding data is empty", question);
        log::warn!("{}", &err);
        return Err(Error::WithMessage(err));
    }

    let sql = format!(
        "
        SELECT qa_data, v.distance FROM {}_qa q INNER JOIN
        (SELECT qa_id, distance FROM {} WHERE vectors MATCH ? ORDER BY distance ASC LIMIT 1) v
        ON q.id = v.qa_id
        ",
        robot_id, robot_id
    );
    let results = sqlx::query::<Sqlite>(&sql)
        .bind(serde_json::to_string(&vectors.0)?)
        .fetch_all(DATA_SOURCE.get().unwrap())
        .await?;
    if !results.is_empty() {
        return Ok((
            Some(serde_json::from_str(results[0].try_get(0)?)?),
            results[0].try_get(1)?,
        ));
    }
    Ok((None, 1.0))
}
