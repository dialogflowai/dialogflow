// use std::fs::File;
// use std::io::Read;
// use std::path::Path;
use std::io::{Cursor, Read};
use std::sync::OnceLock;
use std::vec::Vec;

use futures_util::StreamExt;
use quick_xml::Reader;
use quick_xml::events::Event;
use sqlx::{Row, Sqlite};
use zip::ZipArchive;

use super::dto::DocData;
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
    p.join("kbdocev.dat")
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
        "CREATE TABLE {}_doc (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            file_name TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            doc_content TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE {}_vec_row_id (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT
        );",
        robot_id, robot_id
    );
    // log::info!("sql = {}", &sql);
    let mut stream = sqlx::raw_sql(&sql).execute_many(DATA_SOURCE.get().unwrap());
    while let Some(res) = stream.next().await {
        match res {
            Ok(_r) => log::info!("Initialized doc table"),
            Err(e) => log::error!("Create table failed, err: {:?}", e),
        }
    }
    // let dml = include_str!("../resource/sql/dml.sql");
    // if let Err(e) = sqlx::query(dml).execute(&pool).await {
    //     panic!("{:?}", e);
    // }
    Ok(())
}

// crate::sqlite_trans! {
//     fn save2(robot_id: &str,
//         file_name: &str,
//         file_size: usize,
//         doc_content: &str) -> Result<()> {
//             let sql = format!(
//                 "INSERT INTO {}_doc(file_name, file_size, doc_content, created_at)VALUES(?, ?, ?, unixepoch())",
//                 robot_id
//             );
//             sqlx::query::<Sqlite>(&sql)
//                 .bind(file_name)
//                 .bind(file_size as i64)
//                 .bind(doc_content)
//                 .execute(&mut **transaction)
//                 .await?;
//         Ok(())
//     }
// }

pub(super) async fn list(robot_id: &str) -> Result<Vec<DocData>> {
    let sql = format!("SELECT * FROM {}_doc ORDER BY created_at DESC", robot_id);
    let results = sqlx::query_as::<Sqlite, DocData>(&sql)
        .fetch_all(DATA_SOURCE.get().unwrap())
        .await?;
    Ok(results)
}

pub(super) async fn save(
    robot_id: &str,
    file_name: &str,
    file_size: usize,
    doc_content: &str,
) -> Result<()> {
    async fn inner(
        robot_id: &str,
        file_name: &str,
        file_size: usize,
        doc_content: &str,
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<()> {
        let sql = format!(
            "INSERT INTO {}_doc(file_name, file_size, doc_content, created_at)VALUES(?, ?, ?, unixepoch())",
            robot_id
        );
        sqlx::query::<Sqlite>(&sql)
            .bind(file_name)
            .bind(file_size as i64)
            .bind(doc_content)
            .execute(&mut **transaction)
            .await?;
        Ok(())
    }
    let mut transaction = DATA_SOURCE.get().unwrap().begin().await?;
    let r = inner(
        robot_id,
        file_name,
        file_size,
        doc_content,
        &mut transaction,
    )
    .await;
    if r.is_ok() {
        transaction.commit().await?;
    } else {
        transaction.rollback().await?;
    }
    Ok(())
}

pub(super) fn parse_docx(b: Vec<u8>) -> Result<String> {
    // let mut file = File::open("./numbering.docx")?;
    // let mut buf = Vec::with_capacity(3096);
    // file.read_to_end(&mut buf)?;
    let mut doc_text = String::with_capacity(3096);
    let reader = Cursor::new(b);
    let mut archive = ZipArchive::new(reader)?;
    let mut zip_file = archive.by_name("word/document.xml")?;
    let mut cache = String::with_capacity(zip_file.size() as usize);
    zip_file.read_to_string(&mut cache)?;

    // 创建 XML 解析器
    let mut reader = Reader::from_str(&cache);
    reader.config_mut().trim_text(false);
    let mut in_paragraph = false;

    // 读取 XML 内容
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) if e.name().as_ref() == b"w:p" => in_paragraph = true,
            Ok(Event::End(ref e)) if e.name().as_ref() == b"w:p" => {
                doc_text.push('\n');
                in_paragraph = false;
            }
            Ok(Event::Empty(ref e)) if e.name().as_ref() == b"w:p" => doc_text.push('\n'),
            Ok(Event::Text(e)) if in_paragraph => {
                doc_text.push_str(&e.unescape()?);
            }
            Ok(Event::Eof) => break,
            Err(e) => panic!("Error at position {}: {:?}", reader.error_position(), e),
            _ => (),
        }
    }
    Ok(doc_text)
}

fn parse_pdf() {}
