use std::collections::HashMap;
use std::path::Path;

use axum::{
    Json,
    extract::{Multipart, Query},
    response::IntoResponse,
};

use super::doc;
use super::dto::QuestionAnswerPair;
use crate::result::{Error, Result};
use crate::robot::dto::RobotQuery;
use crate::web::server::to_res;

pub(crate) async fn list_doc(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    let r = super::doc::list(&q.robot_id).await;
    to_res(r)
}

pub(crate) async fn upload_doc(
    Query(q): Query<RobotQuery>,
    multipart: Multipart,
) -> impl IntoResponse {
    let r = upload_doc_inner(&q.robot_id, multipart).await;
    to_res(r)
}

async fn upload_doc_inner(robot_id: &str, mut multipart: Multipart) -> Result<String> {
    let p = Path::new(".")
        .join("data")
        .join(robot_id)
        .join("kb")
        .join("docs")
        .join("upload");
    if !p.exists() {
        std::fs::create_dir_all(&p)?;
    }
    loop {
        let field = multipart.next_field().await?;
        if field.is_none() {
            return Err(Error::WithMessage(String::from("File not found.")));
        }
        let field = field.unwrap();
        let Some(file_name) = field.file_name() else {
            return Err(Error::WithMessage(String::from("File name is missing.")));
        };
        let file_name = file_name.to_string();
        let Some(content_type) = field.content_type() else {
            return Err(Error::WithMessage(String::from("Content type is missing.")));
        };
        let content_type = content_type.to_string();
        let data = field.bytes().await?;

        log::info!(
            "Length of `{file_name}`: `{content_type}` is {} bytes",
            data.len()
        );

        let text = match content_type.as_str() {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                doc::parse_docx(data.to_vec())?
            }
            _ => return Err(Error::WithMessage(String::from("Unsupported format"))),
        };
        log::info!("Extract text: {text}");
        super::doc::save(robot_id, &file_name, data.len(), &text).await?;
        return Ok(text);
    }
}

pub(crate) async fn list_qa(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    let r = super::qa::list(&q.robot_id).await;
    to_res(r)
}

pub(crate) async fn save_qa(
    Query(q): Query<RobotQuery>,
    Json(d): Json<QuestionAnswerPair>,
) -> impl IntoResponse {
    let r = super::qa::save(&q.robot_id, d).await;
    // let r = sqlite_trans!(super::qa::add, &q.robot_id, d).await;
    to_res(r)
}

pub(crate) async fn delete_qa(
    Query(q): Query<RobotQuery>,
    Json(d): Json<QuestionAnswerPair>,
) -> impl IntoResponse {
    let r = super::qa::delete(&q.robot_id, d).await;
    to_res(r)
}

pub(crate) async fn qa_dryrun(Query(q): Query<HashMap<String, String>>) -> impl IntoResponse {
    let r = q.get("robotId");
    let t = q.get("text");
    if r.is_none() || t.is_none() {
        let res = Err(Error::WithMessage(String::from(
            "robotId or text was missing.",
        )));
        return to_res(res);
    }
    let r = super::qa::retrieve_answer(r.unwrap(), t.unwrap()).await;
    to_res(r)
}
