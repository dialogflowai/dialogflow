use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};

use axum::extract::Query;
use axum::{Json, response::IntoResponse};
// use redb::TableDefinition;
use std::sync::Mutex;

use super::dto::MainFlowDetail;
use crate::db;
use crate::db_executor;
use crate::flow::subflow::crud as subflow;
use crate::result::{Error, Result};
use crate::web::server::to_res;

// const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("mainflows");
pub(crate) const TABLE_SUFFIX: &str = "mainflows";

static LOCK: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));
static DEFAULT_NAMES: OnceLock<(String, String)> = OnceLock::new();

pub(crate) fn init_default_names(is_en: bool) -> Result<()> {
    let (name, subflow_name) = if is_en {
        ("The first main flow", "First sub-flow")
    } else {
        ("第一个主流程", "第一个子流程")
    };
    DEFAULT_NAMES
        .set((String::from(name), String::from(subflow_name)))
        .map_err(|_| Error::WithMessage(String::from("Dup")))
}

pub(crate) fn init(robot_id: &str) -> Result<MainFlowDetail> {
    // let table_name = format!("{}-mainflows", robot_id);
    // let main_flow_table: TableDefinition<&str, &[u8]> = TableDefinition::new(&table_name);
    // db::init_table(main_flow_table)?;
    db_executor!(db::init_table, robot_id, TABLE_SUFFIX,)?;
    // let table_name = format!("{}-subflows", robot_id);
    // let sub_flow_table: TableDefinition<&str, &[u8]> = TableDefinition::new(&table_name);
    // db::init_table(sub_flow_table)?;
    db_executor!(
        db::init_table,
        robot_id,
        crate::flow::subflow::crud::TABLE_SUFFIX,
    )?;
    create_main_flow(robot_id, &DEFAULT_NAMES.get().unwrap().0)
}

pub(crate) async fn list(Query(q): Query<HashMap<String, String>>) -> impl IntoResponse {
    // to_res::<Vec<MainFlowDetail>>(db::get_all(TABLE))
    if let Some(robot_id) = q.get("robotId") {
        to_res::<Vec<MainFlowDetail>>(db_executor!(db::get_all, robot_id, TABLE_SUFFIX,))
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robot_id is missing.",
        ))))
    }
}

pub(crate) async fn new(
    Query(q): Query<HashMap<String, String>>,
    Json(data): Json<MainFlowDetail>,
) -> impl IntoResponse {
    if let Some(robot_id) = q.get("robotId") {
        to_res::<MainFlowDetail>(create_main_flow(robot_id, &data.name))
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robotId is missing.",
        ))))
    }
}

fn create_main_flow(robot_id: &str, name: &str) -> Result<MainFlowDetail> {
    let _lock = LOCK.lock();
    // let count = db::count(TABLE)?;
    let count = db_executor!(db::count, robot_id, TABLE_SUFFIX,)?;
    let mut buffer = itoa::Buffer::new();
    let count = buffer.format(count + 1);
    let id = format!("{}{}", count, scru128::new_string());
    let main_flow = MainFlowDetail {
        id,
        name: String::from(name),
        enabled: true,
    };
    // db::write(TABLE, main_flow.id.as_str(), &main_flow)?;
    db_executor!(
        db::write,
        robot_id,
        TABLE_SUFFIX,
        main_flow.id.as_str(),
        &main_flow
    )?;
    subflow::new_subflow(robot_id, &main_flow.id, &DEFAULT_NAMES.get().unwrap().1)?;
    Ok(main_flow)
}

pub(crate) async fn save(
    Query(q): Query<HashMap<String, String>>,
    Json(data): Json<MainFlowDetail>,
) -> impl IntoResponse {
    if let Some(robot_id) = q.get("robotId") {
        let main_flow = MainFlowDetail {
            id: data.id.clone(),
            name: data.name.clone(),
            enabled: data.enabled,
        };
        to_res(db_executor!(
            db::write,
            robot_id,
            TABLE_SUFFIX,
            &data.id,
            &main_flow
        ))
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robotId is missing.",
        ))))
    }
}

pub(crate) async fn delete(
    Query(q): Query<HashMap<String, String>>,
    Json(data): Json<MainFlowDetail>,
) -> impl IntoResponse {
    if let Some(robot_id) = q.get("robotId") {
        let main_flow_id = data.id.as_str();
        match crate::flow::rt::crud::remove_runtime_nodes(main_flow_id) {
            Ok(_) => to_res(db_executor!(
                db::remove,
                robot_id,
                TABLE_SUFFIX,
                main_flow_id
            )),
            Err(e) => to_res(Err(e)),
        }
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robotId is missing.",
        ))))
    }
}
