use std::sync::LazyLock;

use regex::Regex;

use super::context::Context;
use super::dto::{Request, ResponseData, ResponseSenderWrapper};
use crate::ai::completion::Prompt;
use crate::flow::rt::dto::UserInputResult;
use crate::flow::rt::node::RuntimeNode;
use crate::intent::detector;
use crate::result::{Error, Result};

static HTML_TAG_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());

pub(in crate::flow::rt) async fn process(
    req: &mut Request,
) -> Result<(ResponseData, Option<tokio::sync::mpsc::Receiver<String>>)> {
    // log::info!("user input: {}", &req.user_input);
    // let now = std::time::Instant::now();
    if req.session_id.is_none() || req.session_id.as_ref().unwrap().is_empty() {
        req.session_id = Some(scru128::new_string());
    }
    let mut ctx = Context::get(&req.robot_id, req.session_id.as_ref().unwrap());
    // log::info!("get ctx {:?}", now.elapsed());
    // let now = std::time::Instant::now();
    if ctx.no_node() {
        if ctx.main_flow_id.is_empty() {
            ctx.main_flow_id.push_str(&req.main_flow_id);
        }
        ctx.add_node(&req.main_flow_id);
    }
    // log::info!("add_node time {:?}", now.elapsed());
    // let now = std::time::Instant::now();
    if req.user_input_intent.is_none()
        && req.user_input_result == UserInputResult::Successful
        && !req.user_input.is_empty()
    {
        req.user_input_intent = detector::detect(&req.robot_id, &req.user_input).await?;
        // println!("{:?}", req.user_input_intent);
    }
    // log::info!("Intent detection took {:?}", now.elapsed());
    if req.import_variables.is_some() {
        let import_variables = Option::take(&mut req.import_variables);
        let mut import_variables = import_variables.unwrap();
        for v in import_variables.iter_mut() {
            let k = std::mem::take(&mut v.var_name);
            let v = crate::variable::dto::VariableValue::new(&v.var_val, &v.var_type);
            ctx.vars.insert(k, v);
        }
    }
    // println!("intent detect {:?}", now.elapsed());
    // let now = std::time::Instant::now();
    ctx.chat_history.push(Prompt {
        role: String::from("user"),
        content: HTML_TAG_REGEX.replace_all(&req.user_input, "").to_string(),
    });
    let r = exec(req, &mut ctx);
    if r.is_ok() {
        let (res, _receiver) = r.as_ref().unwrap();
        if !res.answers.is_empty() {
            for a in res.answers.iter() {
                ctx.chat_history.push(Prompt {
                    role: String::from("assistant"),
                    content: HTML_TAG_REGEX.replace_all(&a.content, "").to_string(),
                });
            }
        }
    }
    // println!("exec {:?}", now.elapsed());
    // let now = std::time::Instant::now();
    ctx.save()?;
    // log::info!("ctx save time {:?}", now.elapsed());
    r
}

pub(in crate::flow::rt) fn exec(
    req: &Request,
    ctx: &mut Context,
) -> Result<(ResponseData, Option<tokio::sync::mpsc::Receiver<String>>)> {
    // let now = std::time::Instant::now();
    let mut response = ResponseData::new(req);
    let mut sender_wapper = ResponseSenderWrapper { receiver: None };
    for _i in 0..100 {
        // let now = std::time::Instant::now();
        if let Some(mut n) = ctx.pop_node() {
            // println!("pop node {:?}", now.elapsed());
            let ret = n.exec(req, ctx, &mut response, &mut sender_wapper);
            // println!("node exec {:?}", now.elapsed());
            if ret {
                // log::info!("exec time {:?}", now.elapsed());
                return Ok((response, sender_wapper.receiver));
            }
        } else {
            return Ok((response, sender_wapper.receiver));
        }
    }
    let m = if *crate::web::server::IS_EN {
        "Too many executions, please check if the process configuration is correct."
    } else {
        "执行次数太多，请检查流程配置是否正确。"
    };
    Err(Error::WithMessage(String::from(m)))
}
