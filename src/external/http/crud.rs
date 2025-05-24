use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query};
use axum::response::IntoResponse;

use super::dto::HttpReqInfo;
use crate::db;
use crate::db_executor;
use crate::result::{Error, Result};
use crate::web::server::to_res;

// pub(crate) const TABLE: redb::TableDefinition<&str, &[u8]> =
//     redb::TableDefinition::new("externalHttpApis");
pub(crate) const TABLE_SUFFIX: &str = "externalHttpApis";

pub(crate) fn init(robot_id: &str) -> Result<()> {
    // db::init_table(TABLE)
    db_executor!(db::init_table, robot_id, TABLE_SUFFIX,)
}

pub(crate) async fn list(Query(q): Query<HashMap<String, String>>) -> impl IntoResponse {
    // let r: Result<Vec<HttpReqInfo>> = db::get_all(TABLE);
    if let Some(robot_id) = q.get("robotId") {
        let r: Result<Vec<HttpReqInfo>> = db_executor!(db::get_all, &robot_id, TABLE_SUFFIX,);
        to_res(r)
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robotId is missing.",
        ))))
    }
}

pub(crate) fn get_detail(robot_id: &str, id: &str) -> Result<Option<HttpReqInfo>> {
    // db::query(TABLE, id)
    db_executor!(db::query, robot_id, TABLE_SUFFIX, id)
}

pub(crate) async fn detail(
    Query(q): Query<HashMap<String, String>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Some(robot_id) = q.get("robotId") {
        let r: Result<Option<HttpReqInfo>> = get_detail(robot_id, id.as_str());
        to_res(r)
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robotId is missing.",
        ))))
    }
}

pub(crate) async fn save(
    Query(q): Query<HashMap<String, String>>,
    Json(mut params): Json<HttpReqInfo>,
) -> impl IntoResponse {
    if let Some(robot_id) = q.get("robotId") {
        if params.id.is_empty() || params.id.eq("new") {
            params.id = scru128::new_string();
        }
        let r = db_executor!(db::write, robot_id, TABLE_SUFFIX, &params.id, &params);
        to_res(r)
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robotId is missing.",
        ))))
    }
}

pub(crate) async fn remove(
    Query(q): Query<HashMap<String, String>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Some(robot_id) = q.get("robotId") {
        let r = db_executor!(db::remove, &robot_id, TABLE_SUFFIX, id.as_str());
        to_res(r)
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robotId is missing.",
        ))))
    }
}
