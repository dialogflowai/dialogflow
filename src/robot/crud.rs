use std::fs::DirEntry;
use std::path::Path;
use std::vec::Vec;

use axum::extract::Query;
use axum::{Json, response::IntoResponse};
use redb::TableDefinition;

use super::dto::{RobotData, RobotQuery, RobotType};
use crate::db_executor;
use crate::external::http::crud as http;
use crate::flow::mainflow::crud as mainflow;
use crate::intent::crud as intent;
use crate::man::settings;
use crate::result::{Error, Result};
use crate::variable::crud as variable;
use crate::web::server;
use crate::{db, web::server::to_res};

const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("robots");

fn get_robot_id() -> String {
    let mut id = String::with_capacity(32);
    id.push('r');
    let gen_id = scru128::new_string();
    id.push_str(&gen_id);
    id
}

pub(crate) async fn init(is_en: bool) -> Result<()> {
    db::init_table(TABLE)?;
    let name = if is_en {
        "My first robot"
    } else {
        "我的第一个机器人"
    };
    let d = RobotData {
        robot_id: get_robot_id(),
        robot_name: String::from(name),
        robot_type: RobotType::TextBot,
    };
    new(&d, is_en).await
}

pub(crate) async fn save(
    headers: axum::http::HeaderMap,
    Json(mut d): Json<RobotData>,
) -> impl IntoResponse {
    if d.robot_id.is_empty() {
        let is_en = server::is_en(&headers);
        d.robot_id = get_robot_id();
        if let Err(e) = new(&d, is_en).await {
            return to_res(Err(Error::WithMessage(format!(
                "Failed to create robot, error detail was: {:?}",
                &e
            ))));
        }
    }
    let r = persist(&d);
    to_res(r)
}

async fn new(d: &RobotData, is_en: bool) -> Result<()> {
    persist(d)?;
    // 机器人意图
    settings::init(&d.robot_id)?;
    crate::intent::phrase::init_tables(&d.robot_id).await?;
    crate::kb::qa::init_tables(&d.robot_id).await?;
    crate::kb::doc::init_tables(&d.robot_id).await?;
    // 意图
    intent::init(&d.robot_id, is_en)?;
    // 变量
    variable::init(&d.robot_id, is_en)?;
    // 主流程
    mainflow::init(&d.robot_id)?;
    // Http 接口
    http::init(&d.robot_id)?;
    Ok(())
}

fn persist(d: &RobotData) -> Result<()> {
    db::write(TABLE, d.robot_id.as_str(), &d)?;
    Ok(())
}

pub(crate) async fn list() -> impl IntoResponse {
    to_res::<Vec<RobotData>>(db::get_all(TABLE))
}

pub(crate) async fn detail(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    to_res::<Option<RobotData>>(db::query(TABLE, q.robot_id.as_str()))
}

pub(crate) async fn delete(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    to_res(purge(&q.robot_id).await)
}

fn delete_entry(entry: &DirEntry) -> Result<()> {
    log::info!("Deleting file {:?}", entry.path());
    std::fs::remove_file(entry.path())?;
    Ok(())
}

fn delete_dirs(dir: &Path, cb: &dyn Fn(&DirEntry) -> Result<()>) -> Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                delete_dirs(&path, cb)?;
                log::info!("Deleting dir {:?}", &path);
                std::fs::remove_dir(path)?;
            } else {
                cb(&entry)?;
            }
        }
    }
    Ok(())
}

async fn purge(robot_id: &str) -> Result<()> {
    crate::intent::phrase::remove_tables(robot_id).await?;
    // let root = &format!("{}{}", crate::intent::detector::SAVING_PATH_ROOT, robot_id);
    // let path = Path::new(&root);
    // if path.exists() {
    //     delete_dirs(&path, &delete_entry)?;
    //     std::fs::remove_dir(path)?;
    // }
    // if let Err(e) = std::fs::remove_dir(format!(
    //     "{}{}",
    //     crate::intent::detector::SAVING_PATH_ROOT,
    //     robot_id
    // )) {
    //     if e.kind() != std::io::ErrorKind::NotFound {
    //         return Err(e.into());
    //     }
    // }
    db::remove(crate::man::settings::TABLE, robot_id)?;
    db_executor!(
        db::delete_table,
        robot_id,
        crate::external::http::crud::TABLE_SUFFIX,
    )?;
    db_executor!(
        db::delete_table,
        robot_id,
        crate::variable::crud::TABLE_SUFFIX,
    )?;
    db_executor!(
        db::delete_table,
        robot_id,
        crate::intent::crud::TABLE_SUFFIX,
    )?;
    db_executor!(
        db::delete_table,
        robot_id,
        crate::flow::subflow::crud::TABLE_SUFFIX,
    )?;
    let r: Vec<crate::flow::mainflow::dto::MainFlowDetail> = db_executor!(
        db::get_all,
        robot_id,
        crate::flow::mainflow::crud::TABLE_SUFFIX,
    )?;
    for v in r.iter() {
        crate::flow::rt::crud::remove_runtime_nodes(&v.id)?;
    }
    db_executor!(
        db::delete_table,
        robot_id,
        crate::flow::mainflow::crud::TABLE_SUFFIX,
    )?;
    db::remove(TABLE, robot_id)
}
