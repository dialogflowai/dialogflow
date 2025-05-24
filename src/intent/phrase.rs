use std::sync::OnceLock;
use std::vec::Vec;

use futures_util::StreamExt;
use sqlx::{Row, Sqlite};

use super::dto::IntentPhraseData;
use crate::ai::embedding::embedding;
use crate::result::{Error, Result};

type SqliteConnPool = sqlx::Pool<Sqlite>;

// static DATA_SOURCE: OnceCell<SqliteConnPool> = OnceCell::new();
static DATA_SOURCE: OnceLock<SqliteConnPool> = OnceLock::new();
// static DATA_SOURCES: OnceLock<Mutex<HashMap<String, SqliteConnPool>>> = OnceLock::new();
// static INDEXES: LazyLock<Mutex<HashMap<String, usearch::Index>>> =
//     LazyLock::new(|| Mutex::new(HashMap::with_capacity(32)));

fn get_sqlite_path() -> std::path::PathBuf {
    let p = std::path::Path::new(".").join("data");
    if !p.exists() {
        std::fs::create_dir_all(&p).expect("Create data directory failed.");
    }
    p.join("ripd.dat")
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
        "CREATE TABLE {}_idt (
                id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT
            );",
        robot_id
    );
    // log::info!("sql = {}", &sql);
    let mut stream = sqlx::raw_sql(&sql).execute_many(DATA_SOURCE.get().unwrap());
    while let Some(res) = stream.next().await {
        match res {
            Ok(_r) => log::info!("Initialized phrase table"),
            Err(e) => log::error!("Create table failed, err: {:?}", e),
        }
    }
    // let dml = include_str!("../resource/sql/dml.sql");
    // if let Err(e) = sqlx::query(dml).execute(&pool).await {
    //     panic!("{:?}", e);
    // }
    Ok(())
}

pub(crate) async fn search(robot_id: &str, vectors: &Vec<f32>) -> Result<Vec<(String, f64)>> {
    let sql = format!(
        "SELECT intent_id, intent_name, distance FROM {} WHERE phrase_vec MATCH ? ORDER BY distance ASC LIMIT 1",
        robot_id
    );
    let results = sqlx::query::<Sqlite>(&sql)
        .bind(serde_json::to_string(vectors)?)
        .fetch_all(DATA_SOURCE.get().unwrap())
        .await?;
    let mut names = Vec::with_capacity(results.len());
    for r in results.iter() {
        names.push((r.try_get(1)?, r.try_get(2)?));
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
    // check_datasource(robot_id, intent_id).await?;
    let vectors = embedding(robot_id, phrase).await?;
    if vectors.0.is_empty() {
        let err = format!("{phrase} embedding data is empty");
        log::warn!("{}", &err);
        return Err(Error::WithMessage(err));
    }
    // log::info!("vectors.0.len() = {}", vectors.0.len());
    let mut txn = DATA_SOURCE.get().unwrap().begin().await?;
    async fn inner_add(
        robot_id: &str,
        vec_row_id: Option<i64>,
        intent_id: &str,
        intent_name: &str,
        phrase: &str,
        vec: &Vec<f32>,
        txn: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<i64> {
        if vec_row_id.is_none() {
            let sql = format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS {} USING vec0 (
                            id INTEGER NOT NULL PRIMARY KEY,
                            intent_id TEXT NOT NULL,
                            +intent_name TEXT NOT NULL,
                            +phrase TEXT NOT NULL,
                            phrase_vec float[{}]
                        );",
                robot_id,
                vec.len()
            );
            sqlx::query::<Sqlite>(&sql).execute(&mut **txn).await?;
            let sql = format!("INSERT INTO {}_idt(id) VALUES(NULL)", robot_id);
            let last_insert_rowid = sqlx::query::<Sqlite>(&sql)
                .execute(&mut **txn)
                .await?
                .last_insert_rowid();
            let sql = format!(
                "INSERT INTO {} (id, intent_id, intent_name, phrase, phrase_vec)VALUES(?, ?, ?, ?, ?)",
                robot_id
            );
            sqlx::query::<Sqlite>(&sql)
                .bind(last_insert_rowid)
                .bind(intent_id)
                .bind(intent_name)
                .bind(phrase)
                .bind(serde_json::to_string(vec)?)
                .execute(&mut **txn)
                .await?;
            Ok(last_insert_rowid)
        } else {
            let sql = format!(
                "UPDATE {} SET phrase = ?, phrase_vec = ? WHERE = ?",
                robot_id
            );
            let vec_row_id = vec_row_id.unwrap();
            sqlx::query::<Sqlite>(&sql)
                .bind(phrase)
                .bind(serde_json::to_string(vec)?)
                .bind(vec_row_id)
                .execute(&mut **txn)
                .await?;
            Ok(vec_row_id)
        }
    }
    match inner_add(
        robot_id,
        vec_row_id,
        intent_id,
        intent_name,
        phrase,
        &vectors.0,
        &mut txn,
    )
    .await
    {
        Ok(last_insert_rowid) => {
            txn.commit().await?;
            Ok(last_insert_rowid)
        }
        Err(e) => {
            txn.rollback().await?;
            Err(e)
        }
    }
    // Ok(last_insert_rowid)
}

pub(crate) async fn batch_add(
    robot_id: &str,
    intent_id: &str,
    intent_name: &str,
    phrases: &[IntentPhraseData],
) -> Result<()> {
    // check_datasource(robot_id, intent_id).await?;
    for p in phrases.iter() {
        add(robot_id, Some(p.id), intent_id, intent_name, &p.phrase).await?;
    }
    Ok(())
}

pub(crate) async fn remove(robot_id: &str, id: i64) -> Result<()> {
    // INDEXES.lock()?.get(robot_id).and_then(|idx| {idx.remove(id as u64); None::<()>});
    let sql = format!("DELETE FROM {} WHERE id = ?", robot_id);
    sqlx::query::<Sqlite>(&sql)
        .bind(id)
        .execute(DATA_SOURCE.get().unwrap())
        .await?;
    Ok(())
}

pub(crate) async fn remove_by_intent_id(robot_id: &str, intent_id: &str) -> Result<()> {
    let sql = format!("DELETE FROM {} WHERE intent_id = ?", robot_id);
    match sqlx::query::<Sqlite>(&sql)
        .bind(intent_id)
        .execute(DATA_SOURCE.get().unwrap())
        .await
    {
        Ok(_) => return Ok(()),
        Err(e) => match e {
            sqlx::Error::Database(database_error) => {
                if let Some(code) = database_error.code() {
                    if code.eq("1") {
                        return Ok::<_, Error>(());
                    }
                }
            }
            _ => return Err(e.into()),
        },
    };
    Ok(())
}

pub(crate) async fn remove_tables(robot_id: &str) -> Result<()> {
    let sql = format!("DROP TABLE {}", robot_id);
    match sqlx::query::<Sqlite>(&sql)
        .execute(DATA_SOURCE.get().unwrap())
        .await
    {
        Ok(_) => return Ok(()),
        Err(e) => match e {
            sqlx::Error::Database(database_error) => {
                if let Some(code) = database_error.code() {
                    if code.eq("1") {
                        return Ok::<_, Error>(());
                    }
                }
            }
            _ => return Err(e.into()),
        },
    };
    Ok(())
}
