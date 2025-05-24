use std::collections::HashMap;
// use std::borrow::BorrowMut;
use std::vec::Vec;

use axum::Json;
use axum::extract::Query;
use axum::response::IntoResponse;

use super::dto::Variable;
use super::dto::{VariableObtainValueExpressionType, VariableType, VariableValueSource};
use crate::db;
use crate::db_executor;
use crate::flow::rt::context::Context;
use crate::flow::rt::dto::Request;
use crate::result::{Error, Result};
use crate::web::server::to_res;

// const TABLE: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("variables");
// pub(crate) const VARIABLE_LIST_KEY: &str = "variables";
pub(crate) const TABLE_SUFFIX: &str = "vars";

// #[macro_export]
// macro_rules! db_executor (
//     ($func: expr, $robot_id: expr, $($bind: expr),*) => ({
//         let table_name = format!("{}vars", $robot_id);
//         let table: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new(&table_name);
//         $func(table $(,($bind))*)
//     });
// );

// #[inline]
// fn get_table_name(robot_id: &str) -> String {
//     format!("{}vars", robot_id)
// }

pub(crate) fn init(robot_id: &str, is_en: bool) -> Result<()> {
    let v = Variable {
        var_name: String::from(if is_en {
            "CollectionVar"
        } else {
            "采集变量"
        }),
        var_type: VariableType::Str,
        var_val_source: VariableValueSource::Collect,
        var_constant_value: String::new(),
        var_associate_data: String::new(),
        obtain_value_expression_type: VariableObtainValueExpressionType::None,
        obtain_value_expression: String::new(),
        timeout_milliseconds: 1500u64,
        cache_enabled: true,
    };
    // let result = db_executor!(db::write, robot_id, &v.var_name, &v);
    // let table_name = get_table_name(robot_id);
    // let table: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new(&table_name);
    // db::write(table, &v.var_name, &v)
    db_executor!(db::write, robot_id, TABLE_SUFFIX, &v.var_name, &v)
}

pub(crate) async fn list(Query(q): Query<HashMap<String, String>>) -> impl IntoResponse {
    // let result:Result<Vec<Variable>> = db_executor!(db::get_all, "robot_id",);
    // to_res::<Vec<Variable>>(db::get_all(TABLE))
    if let Some(robot_id) = q.get("robotId") {
        to_res::<Vec<Variable>>(db_executor!(db::get_all, robot_id, TABLE_SUFFIX,))
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robotId is missing.",
        ))))
    }
}

pub(crate) async fn add(
    Query(q): Query<HashMap<String, String>>,
    Json(v): Json<Variable>,
) -> impl IntoResponse {
    /*
    let r: Result<Option<Vec<Variable>>> = db::query(TABLE, VARIABLE_LIST_KEY);
    let r = r.and_then(|op| {
        let mut new_record = true;
        if let Some(mut d) = op {
            d.retain_mut(|x| {
                if x.var_name.eq(&v.var_name) {
                    x.var_type = v.var_type.clone();
                    x.var_val_source = v.var_val_source.clone();
                    new_record = false;
                }
                true
            });
            if new_record {
                d.push(v.clone());
            }
            db::write(TABLE, VARIABLE_LIST_KEY, &d)
        } else {
            let d = vec![v];
            db::write(TABLE, VARIABLE_LIST_KEY, &d)
        }
    });
    to_res(r)
    */
    // to_res(db::write(TABLE, &v.var_name, &v))
    if let Some(robot_id) = q.get("robotId") {
        to_res(db_executor!(
            db::write,
            robot_id,
            TABLE_SUFFIX,
            &v.var_name,
            &v
        ))
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robotId is missing.",
        ))))
    }
}

// fn t1<R, F: FnMut(String) -> R>(s: String, mut f: F) -> R {
//     f(s)
// }

// fn t2() -> Result<()> {
//     let r = t1(String::new(), |s| Ok(()));
//     r
// }

pub(crate) async fn delete(
    Query(q): Query<HashMap<String, String>>,
    Json(v): Json<Variable>,
) -> impl IntoResponse {
    /*
    let r = v
        .var_name
        .parse::<usize>()
        .map_err(|e| Error::ErrorWithMessage(format!("{:?}", e)))
        .and_then(|idx| {
            let mut op: Option<Vec<Variable>> = db::query(TABLE, VARIABLE_LIST_KEY)?;
            if op.is_some() {
                let d = op.as_mut().unwrap();
                d.remove(idx);
                db::write(TABLE, VARIABLE_LIST_KEY, &d)?;
            }
            Ok(())
        });
    to_res(r)
    */
    // to_res(db::remove(TABLE, v.var_name.as_str()))
    if let Some(robot_id) = q.get("robotId") {
        to_res(db_executor!(
            db::remove,
            robot_id,
            TABLE_SUFFIX,
            v.var_name.as_str()
        ))
    } else {
        to_res(Err(Error::WithMessage(String::from(
            "Parameter: robotId is missing.",
        ))))
    }
}

pub(crate) fn get(robot_id: &str, name: &str) -> Result<Option<Variable>> {
    /*
    db::query(TABLE, VARIABLE_LIST_KEY).and_then(|op: Option<Vec<Variable>>| {
        if let Some(d) = op {
            for v in d {
                if v.var_name.eq(name) {
                    return Ok(Some(v.clone()));
                }
            }
        }
        return Ok(None);
    })
    */
    // db::query(TABLE, name)
    db_executor!(db::query, robot_id, TABLE_SUFFIX, name)
}

pub(crate) fn get_value(name: &str, req: &Request, ctx: &mut Context) -> String {
    if let Ok(Some(v)) = get(&req.robot_id, name) {
        if let Some(val) = v.get_value(req, ctx) {
            return val.val_to_string();
        }
    }
    String::new()
}
