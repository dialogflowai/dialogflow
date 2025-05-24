use std::sync::{LazyLock, Mutex};

use axum::Json;
use axum::extract::Query;
use axum::http::{StatusCode, header::HeaderMap};
use axum::response::{IntoResponse, Response};
// use redb::TableDefinition;

use super::dto::{SubFlowDetail, SubFlowFormData};
use crate::db;
use crate::db_executor;
use crate::flow::demo;
use crate::result::{Error, Result};
use crate::web::server::{self, to_res};

pub(crate) const TABLE_SUFFIX: &str = "subflows";
// pub(crate) const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("subflows");
// pub(crate) const SUB_FLOW_LIST_KEY: &str = "subflows";
static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

// pub(crate) fn init(is_en: bool, mainflow_id: &str) -> Result<()> {
//     let flow = vec![SubFlowDetail::new(if is_en {
//         "First sub-flow"
//     } else {
//         "第一个子流程"
//     })];
//     db::write(TABLE, mainflow_id, &flow)
// }

pub(crate) async fn list(headers: HeaderMap, Query(q): Query<SubFlowFormData>) -> Response {
    let is_en = server::is_en(&headers);
    let template = demo::get_demo(is_en, &q.main_flow_id);
    if let Some(t) = template {
        return (StatusCode::OK, t).into_response();
    }
    // to_res::<Option<Vec<SubFlowDetail>>>(db::query(TABLE, q.main_flow_id.as_str())).into_response()
    to_res::<Option<Vec<SubFlowDetail>>>(db_executor!(
        db::query,
        &q.robot_id,
        TABLE_SUFFIX,
        q.main_flow_id.as_str()
    ))
    .into_response()
    // let r = db::process_data(FLOW_LIST_KEY, |mut flows: Vec<FlowDetail>| {
    //     flows.iter_mut().for_each(|f| f.nodes.clear());
    //     Ok(flows)
    // });
    // to_res(r)
}

pub(crate) async fn simple_list(Query(q): Query<SubFlowFormData>) -> Response {
    // let r: Result<Option<Vec<SubFlowDetail>>> = db::query(TABLE, q.main_flow_id.as_str());
    let r: Result<Option<Vec<SubFlowDetail>>> = db_executor!(
        db::query,
        &q.robot_id,
        TABLE_SUFFIX,
        q.main_flow_id.as_str()
    );
    if let Ok(Some(mut d)) = r {
        for f in d.iter_mut() {
            f.canvas.clear();
        }
        return to_res::<Vec<SubFlowDetail>>(Ok(d)).into_response();
    }
    "[]".into_response()
}

pub(crate) fn new_subflow(
    robot_id: &str,
    mainflow_id: &str,
    subflow_name: &str,
) -> Result<Vec<SubFlowDetail>> {
    let _lock = LOCK.lock();
    db_executor!(db::query, robot_id, TABLE_SUFFIX, mainflow_id)
        .map(|op: Option<Vec<SubFlowDetail>>| {
            let mut subflow = SubFlowDetail::new(subflow_name);
            if let Some(mut flows) = op {
                flows.push(subflow);
                flows
            } else {
                subflow.id.clear();
                subflow.id.push_str(mainflow_id);
                vec![subflow]
            }
        })
        .and_then(|subflows| {
            db_executor!(db::write, robot_id, TABLE_SUFFIX, mainflow_id, &subflows)?;
            Ok(subflows)
        })
}

pub(crate) async fn new(Query(form): Query<SubFlowFormData>) -> impl IntoResponse {
    to_res(new_subflow(&form.robot_id, &form.main_flow_id, &form.data))
}

pub(crate) async fn save(
    Query(q): Query<SubFlowFormData>,
    Json(data): Json<SubFlowDetail>,
) -> impl IntoResponse {
    let r: Result<Vec<SubFlowDetail>> = q
        .data
        .parse::<usize>()
        .map_err(|e| Error::WithMessage(format!("{:?}", e)))
        .and_then(|idx| {
            // let op: Option<Vec<SubFlowDetail>> = db::query(TABLE, form.main_flow_id.as_str())?;
            let op: Option<Vec<SubFlowDetail>> = db_executor!(
                db::query,
                &q.robot_id,
                TABLE_SUFFIX,
                q.main_flow_id.as_str()
            )?;
            if let Some(mut flows) = op {
                if let Some(flow) = flows.get_mut(idx) {
                    flow.canvas = data.canvas.clone();
                    // db::write(TABLE, &q.main_flow_id, &flows)?;
                    db_executor!(
                        db::write,
                        &q.robot_id,
                        TABLE_SUFFIX,
                        &q.main_flow_id,
                        &flows
                    )?;
                }
                Ok(flows)
            } else {
                Ok(vec![])
            }
        });
    to_res(r)
}

pub(crate) async fn delete(Query(q): Query<SubFlowFormData>) -> impl IntoResponse {
    let r = q
        .data
        .parse::<usize>()
        .map_err(|e| Error::WithMessage(format!("{:?}", e)))
        .and_then(|idx| {
            let result: Result<Option<Vec<SubFlowDetail>>> = db_executor!(
                db::query,
                &q.robot_id,
                TABLE_SUFFIX,
                q.main_flow_id.as_str()
            );
            if let Ok(Some(mut flows)) = result {
                if idx < flows.len() {
                    flows.remove(idx);
                    db_executor!(
                        db::write,
                        &q.robot_id,
                        TABLE_SUFFIX,
                        &q.main_flow_id,
                        &flows
                    )?;
                }
            }
            Ok(())
        });
    to_res(r)
}

pub(crate) async fn release(
    headers: HeaderMap,
    Query(q): Query<SubFlowFormData>,
) -> impl IntoResponse {
    // let now = std::time::Instant::now();
    let is_en = server::is_en(&headers);
    let r = crate::flow::rt::convertor::convert_flow(is_en, &q.robot_id, &q.main_flow_id);
    // println!("release used time:{:?}", now.elapsed());
    to_res(r)
}

pub(crate) async fn output(Query(q): Query<SubFlowFormData>) -> impl IntoResponse {
    // let flows: Option<Vec<SubFlowDetail>> = db::query(TABLE, q.main_flow_id.as_str()).unwrap();
    let flows: Option<Vec<SubFlowDetail>> = db_executor!(
        db::query,
        &q.robot_id,
        TABLE_SUFFIX,
        q.main_flow_id.as_str()
    )
    .unwrap();
    serde_json::to_string(&flows).unwrap()
}
