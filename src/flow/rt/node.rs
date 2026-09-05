use core::time::Duration;
// use std::ops::DerefMut;

// use enum_dispatch::enum_dispatch;
use lettre::transport::smtp::PoolConfig;
use rkyv::{Archive, Deserialize, Serialize, util::AlignedVec};

use super::condition::ConditionData;
use super::context::Context;
use super::dto::{
    AnswerContentType, AnswerData, CollectData, Request, ResponseChannelWrapper, ResponseData,
    StreamingResponseData,
};
use crate::ai::chat::{ResultSender, SenderWrapper};
use crate::ai::completion::Prompt;
use crate::external::http::client as http;
use crate::flow::rt::collector;
use crate::flow::subflow::dto::NextActionType;
use crate::man::settings::get_settings;
use crate::result::Result;
use crate::variable::crud as variable;
use crate::variable::dto::{VariableType, VariableValue};

const VAR_WRAP_SYMBOL: char = '`';

// #[repr(u8)]
// #[derive(PartialEq)]
// pub(in crate::flow::rt) enum RuntimeNodeTypeId {
//     TextNode = 1,
//     GotoAnotherNode = 2,
//     CollectNode = 3,
//     ConditionNode = 4,
//     TerminateNode = 5,
// }

// #[enum_dispatch]
#[derive(Archive, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) enum RuntimeNodeEnum {
    TextNode(TextNode),
    LlmGenTextNode(LlmGenTextNode),
    ConditionNode(ConditionNode),
    GotoAnotherNode(GotoAnotherNode),
    GotoMainFlowNode(GotoMainFlowNode),
    CollectNode(CollectNode),
    ExternalHttpCallNode(ExternalHttpCallNode),
    TerminateNode(TerminateNode),
    SendEmailNode(SendEmailNode),
    LlmChatNode(LlmChatNode),
    KnowledgeBaseAnswerNode(KnowledgeBaseAnswerNode),
}

impl RuntimeNode for RuntimeNodeEnum {
    async fn exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        response: &mut ResponseData,
        channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        match self {
            RuntimeNodeEnum::TextNode(n) => n.exec(req, ctx, response, channel_sender).await,
            RuntimeNodeEnum::LlmGenTextNode(n) => n.exec(req, ctx, response, channel_sender).await,
            RuntimeNodeEnum::ConditionNode(n) => n.exec(req, ctx, response, channel_sender).await,
            RuntimeNodeEnum::GotoAnotherNode(n) => n.exec(req, ctx, response, channel_sender).await,
            RuntimeNodeEnum::GotoMainFlowNode(n) => {
                n.exec(req, ctx, response, channel_sender).await
            }
            RuntimeNodeEnum::CollectNode(n) => n.exec(req, ctx, response, channel_sender).await,
            RuntimeNodeEnum::ExternalHttpCallNode(n) => {
                n.exec(req, ctx, response, channel_sender).await
            }
            RuntimeNodeEnum::TerminateNode(n) => n.exec(req, ctx, response, channel_sender).await,
            RuntimeNodeEnum::SendEmailNode(n) => n.exec(req, ctx, response, channel_sender).await,
            RuntimeNodeEnum::LlmChatNode(n) => n.exec(req, ctx, response, channel_sender).await,
            RuntimeNodeEnum::KnowledgeBaseAnswerNode(n) => {
                n.exec(req, ctx, response, channel_sender).await
            }
        }
    }
}

// #[enum_dispatch(RuntimeNodeEnum)]
pub(crate) trait RuntimeNode {
    async fn exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        response: &mut ResponseData,
        channel_sender: &mut ResponseChannelWrapper,
    ) -> bool;
}

async fn replace_vars(text: &str, req: &Request, ctx: &mut Context) -> Result<String> {
    let mut new_str = String::with_capacity(128);
    let mut start = 0usize;
    while let Some(mut begin) = text[start..].find(VAR_WRAP_SYMBOL) {
        begin += start;
        new_str.push_str(&text[start..begin]);
        if let Some(mut end) = text[begin + 1..].find(VAR_WRAP_SYMBOL) {
            end = begin + end + 1;
            // println!("{} {} {} {}", &text[begin + 1..],start, begin,end);
            let var = variable::get(&req.robot_id, &text[begin + 1..end])?;
            if let Some(v) = var {
                if let Some(value) = v.get_value2(req, ctx).await {
                    new_str.push_str(&value.val_to_string());
                }
                start = end + 1;
            } else {
                new_str.push_str(&text[begin..end]);
                start = end;
            }
            // new_str.push_str(&variable::get_value(&text[begin + 1..end - 1], req, ctx));
        } else {
            start = begin;
            break;
        }
    }
    // loop {
    //     if let Some(mut begin) = text[start..].find(VAR_WRAP_SYMBOL) {
    //         begin += start;
    //         new_str.push_str(&text[start..begin]);
    //         if let Some(mut end) = text[begin + 1..].find(VAR_WRAP_SYMBOL) {
    //             end = begin + end + 1;
    //             // println!("{} {} {} {}", &text[begin + 1..],start, begin,end);
    //             let var = variable::get(&req.robot_id, &text[begin + 1..end])?;
    //             if let Some(v) = var {
    //                 if let Some(value) = v.get_value(req, ctx) {
    //                     new_str.push_str(&value.val_to_string());
    //                 }
    //                 start = end + 1;
    //             } else {
    //                 new_str.push_str(&text[begin..end]);
    //                 start = end;
    //             }
    //             // new_str.push_str(&variable::get_value(&text[begin + 1..end - 1], req, ctx));
    //         } else {
    //             start = begin;
    //             break;
    //         }
    //     } else {
    //         break;
    //     }
    // }
    new_str.push_str(&text[start..]);
    Ok(new_str)
}

#[inline]
fn add_next_node(ctx: &mut Context, next_node_id: &str) {
    ctx.add_node(next_node_id);
}

#[derive(Archive, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct TextNode {
    pub(super) text: String,
    pub(crate) text_type: AnswerContentType,
    pub(super) ret: bool,
    pub(super) next_node_id: String,
}

impl RuntimeNode for TextNode {
    async fn exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        response: &mut ResponseData,
        channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // log::info!("Into TextNode {}", &self.text);
        // let now = std::time::Instant::now();
        match replace_vars(&self.text, req, ctx).await {
            Ok(answer) => {
                if channel_sender.sender.is_some() {
                    let sender = channel_sender.sender.as_ref().unwrap().clone();
                    let streaming = StreamingResponseData {
                        content_seq: Some(ctx.add_answer_history(&answer)),
                        content: answer,
                    };
                    crate::sse_send!(sender, streaming);
                } else {
                    response.answers.push(AnswerData {
                        content: answer,
                        content_type: self.text_type.clone(),
                    })
                }
            }
            Err(e) => log::error!("{e:?}"),
        };
        // log::info!("add {}", &self.next_node_id);
        add_next_node(ctx, &self.next_node_id);
        // log::info!("TextNode used time:{:?}", now.elapsed());
        self.ret
    }
}

#[derive(Archive, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct LlmGenTextNode {
    pub(super) prompt: String,
    pub(crate) fallback_text: String,
    pub(super) context_len: u8,
    pub(crate) connect_timeout: Option<u32>,
    pub(crate) read_timeout: Option<u32>,
    pub(crate) response_streaming: bool,
    pub(super) ret: bool,
    pub(super) next_node_id: String,
}

impl RuntimeNode for LlmGenTextNode {
    async fn exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        response: &mut ResponseData,
        channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // log::info!("Into LlmGenTextNode");
        // let now = std::time::Instant::now();
        let mut chat_history: Vec<Prompt> = Vec::with_capacity(5);
        if self.context_len > 0 && !ctx.chat_history.is_empty() {
            let len = ctx.chat_history.len();
            let context_len = self.context_len as usize;
            if len > context_len {
                // ctx.chat_history.drain(0..ctx.chat_history.len() - self.context_len as usize);
                chat_history.extend_from_slice(&ctx.chat_history[len - context_len..len - 1]);
            } else {
                chat_history.extend_from_slice(&ctx.chat_history);
            };
        };
        let p = Prompt {
            role: "system".to_string(),
            content: self.prompt.clone(),
        };
        chat_history.push(p);
        if self.response_streaming {
            // let r = super::facade::get_sender(req.session_id.as_ref().unwrap());
            // if r.is_err() {
            //     add_next_node(ctx, &self.next_node_id);
            //     return false;
            // }
            // let s_op = r.unwrap();
            // if s_op.is_none() {
            //     add_next_node(ctx, &self.next_node_id);
            //     return false;
            // }
            // let s = s_op.unwrap();
            // let ticket = String::new();
            let robot_id = req.robot_id.clone();
            let connect_timeout = self.connect_timeout;
            let read_timeout = self.read_timeout;
            // let (s, r) = tokio::sync::mpsc::channel::<String>(1);
            if channel_sender.sender.is_none() {
                let (s, r) = tokio::sync::mpsc::channel::<StreamingResponseData>(2);
                channel_sender.sender = Some(s);
                channel_sender.receiver = Some(r);
            }
            let s = channel_sender.sender.clone().unwrap();
            let res_data = serde_json::to_string(response).unwrap();
            let content_seq = ctx.add_answer_history("");
            tokio::task::spawn(async move {
                let send_data = StreamingResponseData {
                    content_seq: None,
                    content: res_data,
                };
                if let Err(e) = s.send(send_data).await {
                    log::warn!("LlmGenTextNode response failed, err: {:?}", &e);
                    return;
                }
                let sender_wrappoer = SenderWrapper {
                    sender: s,
                    content_seq,
                };
                if let Err(e) = crate::ai::chat::chat(
                    &robot_id,
                    Some(chat_history),
                    connect_timeout,
                    read_timeout,
                    ResultSender::ChannelSender(sender_wrappoer),
                )
                .await
                {
                    log::warn!("LlmGenTextNode response failed, err: {:?}", &e);
                }
            });
        } else {
            let now = std::time::Instant::now();
            let mut s = String::with_capacity(1024);
            if let Err(e) = crate::ai::chat::chat(
                &req.robot_id,
                Some(chat_history),
                self.connect_timeout,
                self.read_timeout,
                ResultSender::StrBuf(&mut s),
            )
            .await
            {
                log::error!("LlmGenTextNode response failed, err: {:?}", &e);
                s.push_str(&self.fallback_text);
            } else {
                log::info!("LLM response |{}|", &s);
                if s.is_empty() {
                    response.answers.push(AnswerData {
                        content: self.fallback_text.clone(),
                        content_type: AnswerContentType::TextPlain,
                    });
                } else {
                    response.answers.push(AnswerData {
                        content: s,
                        content_type: AnswerContentType::TextPlain,
                    });
                }
            }
            log::info!("LLM response took {:?}", now.elapsed());
            // let (s, rev) = std::sync::mpsc::channel::<String>();
            // let robot_id = req.robot_id.clone();
            // let prompt = self.prompt.clone();
            // tokio::task::spawn(async move {
            //     let mut r = String::with_capacity(1024);
            //     if let Err(e) =
            //         crate::ai::chat::chat(&robot_id, &prompt, ResultReceiver::StrBuf(&mut r)).await
            //     {
            //         log::info!("LlmChatNode response failed, err: {:?}", &e);
            //         drop(s);
            //         return;
            //     }
            //     if let Err(_) = s.send(r) {
            //         log::info!("LlmChatNode sent response failed.");
            //     }
            // });
            // match rev.recv() {
            //     Ok(s) => {
            //         log::info!("LLM response {}", &s);
            //         response.answers.push(AnswerData {
            //             text: s,
            //             answer_type: AnswerType::TextPlain,
            //         });
            //     }
            //     // Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {}
            //     Err(e) => log::info!("LlmChatNode response failed, err: {:?}", &e),
            // }
            // let mut s = String::with_capacity(1024);
            // if let Err(e) = tokio::runtime::Handle::current().block_on(async {
            //     crate::ai::chat::chat(&req.robot_id, &self.prompt, ResultReceiver::StrBuf(&mut s))
            //         .await
            // }) {
            //     log::info!("LlmChatNode response failed, err: {:?}", &e);
            // } else {
            //     log::info!("LLM response {}", &s);
            //     response.answers.push(AnswerData {
            //         text: s,
            //         answer_type: AnswerType::TextPlain,
            //     });
            // }
        }
        // log::info!("add {}", &self.next_node_id);
        add_next_node(ctx, &self.next_node_id);
        // log::info!("LlmGenTextNode used time:{:?}", now.elapsed());
        self.ret
    }
}

#[derive(Archive, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct GotoMainFlowNode {
    pub(super) main_flow_id: String,
    pub(super) next_node_id: String,
}

impl RuntimeNode for GotoMainFlowNode {
    async fn exec(
        &mut self,
        _req: &Request,
        ctx: &mut Context,
        _response: &mut ResponseData,
        _channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // println!("Into GotoMainFlowNode");
        ctx.main_flow_id.clear();
        ctx.main_flow_id.push_str(&self.main_flow_id);
        add_next_node(ctx, &self.next_node_id);
        false
    }
}

#[derive(Archive, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct GotoAnotherNode {
    pub(super) next_node_id: String,
}

impl RuntimeNode for GotoAnotherNode {
    async fn exec(
        &mut self,
        _req: &Request,
        ctx: &mut Context,
        _response: &mut ResponseData,
        _channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // println!("Into GotoAnotherNode");
        add_next_node(ctx, &self.next_node_id);
        false
    }
}

#[derive(Archive, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct CollectNode {
    pub(super) var_name: String,
    pub(super) collect_type: collector::CollectType,
    pub(super) successful_node_id: String,
    pub(super) failed_node_id: String,
}

impl RuntimeNode for CollectNode {
    async fn exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        response: &mut ResponseData,
        _channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // println!("Into CollectNode");
        if let Some(r) = collector::collect(&req.user_input, &self.collect_type) {
            // println!("{} {}", &self.var_name, r);
            let v = VariableValue::new(r, &VariableType::Str);
            ctx.vars.insert(self.var_name.clone(), v);
            let collect_data = CollectData {
                var_name: self.var_name.clone(),
                value: String::from(r),
            };
            response.collect_data.push(collect_data);
            add_next_node(ctx, &self.successful_node_id);
            // println!("{} {}", r, &self.successful_node_id);
        } else {
            add_next_node(ctx, &self.failed_node_id);
        }
        false
    }
}

#[derive(Archive, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct ConditionNode {
    pub(super) next_node_id: String,
    pub(super) goto_node_id: String,
    pub(super) conditions: Vec<Vec<ConditionData>>,
}

impl RuntimeNode for ConditionNode {
    async fn exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        _response: &mut ResponseData,
        _channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // println!("Into ConditionNode");
        let mut r = false;
        for and_conditions in self.conditions.iter() {
            for cond in and_conditions.iter() {
                r = cond.compare(req, ctx).await;
                if !r {
                    break;
                }
            }
            if r {
                add_next_node(ctx, &self.goto_node_id);
                return false;
            }
        }
        add_next_node(ctx, &self.next_node_id);
        false
    }
}

#[derive(Archive, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct TerminateNode {}

impl RuntimeNode for TerminateNode {
    async fn exec(
        &mut self,
        _req: &Request,
        _ctx: &mut Context,
        response: &mut ResponseData,
        channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // log::info!("Into TerminateNode");
        response.next_action = NextActionType::Terminate;
        if channel_sender.sender.is_some() {
            channel_sender.send_response(response);
        }
        true
    }
}

#[derive(Archive, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct ExternalHttpCallNode {
    pub(super) successful_node_id: String,
    pub(super) next_node_id: String,
    pub(super) http_api_id: String,
    pub(super) timeout_milliseconds: u64,
    pub(super) async_req: bool,
}

impl RuntimeNode for ExternalHttpCallNode {
    async fn exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        _response: &mut ResponseData,
        _channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // println!("Into ExternalHttpCallNode");
        let mut goto_node_id = &self.next_node_id;
        if let Ok(Some(api)) =
            crate::external::http::crud::get_detail(&req.robot_id, self.http_api_id.as_str())
        {
            if self.async_req {
                tokio::spawn(http::status_code(
                    api,
                    self.timeout_milliseconds,
                    ctx.vars.clone(),
                ));
            } else {
                match http::status_code(api, self.timeout_milliseconds, ctx.vars.clone()).await {
                    Ok(r) => {
                        if r == 200u16 {
                            goto_node_id = &self.successful_node_id;
                        }
                    }
                    Err(e) => {
                        log::error!("{e:?}");
                    }
                };
            }
        }
        add_next_node(ctx, goto_node_id);
        false
    }
}

#[derive(Archive, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct SendEmailNode {
    pub(super) from: String,
    pub(super) to_recipients: Vec<String>,
    pub(super) cc_recipients: Vec<String>,
    pub(super) bcc_recipients: Vec<String>,
    pub(super) subject: String,
    pub(super) content: String,
    pub(super) content_type: String,
    pub(super) async_send: bool,
    pub(super) successful_node_id: String,
    pub(super) goto_node_id: Option<String>,
}

impl SendEmailNode {
    fn send_email(&self, settings: &crate::man::settings::Settings) -> Result<()> {
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::{
            AsyncSmtpTransport, AsyncTransport, SmtpTransport, Tokio1Executor, Transport,
            message::{
                Mailboxes, MessageBuilder,
                header::{Bcc, Cc, ContentType, To},
            },
        };
        let mailboxes: Mailboxes = self.to_recipients.join(",").parse()?;
        let to_header: To = mailboxes.into();
        let mut builder = MessageBuilder::new().mailbox(to_header);
        if !self.cc_recipients.is_empty() {
            let mailboxes: Mailboxes = self.cc_recipients.join(",").parse()?;
            let cc_header: Cc = mailboxes.into();
            builder = builder.mailbox(cc_header);
        }
        if !self.bcc_recipients.is_empty() {
            let mailboxes: Mailboxes = self.bcc_recipients.join(",").parse()?;
            let bcc_header: Bcc = mailboxes.into();
            builder = builder.mailbox(bcc_header);
        }

        let content_type: ContentType = if self.content_type.eq("HTML") {
            ContentType::TEXT_HTML
        } else {
            ContentType::TEXT_PLAIN
        };

        let email = builder
            .from(self.from.parse()?)
            .subject(&self.subject)
            .header(content_type)
            .body(self.content.clone())
            // .singlepart(SinglePart::html(&self.content))
            ?;
        let creds = Credentials::new(
            settings.smtp_username.to_owned(),
            settings.smtp_password.to_owned(),
        );
        let pool = PoolConfig::new()
            .min_idle(1)
            .max_size(2)
            .idle_timeout(Duration::from_secs(300));
        if self.async_send {
            let builder = AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.smtp_host)?;
            let mailer = builder
                .credentials(creds)
                .timeout(Some(core::time::Duration::from_secs(
                    settings.smtp_timeout_sec as u64,
                )))
                .pool_config(pool)
                .build();
            tokio::spawn(async move {
                // mailer.send(email) // will be wrong
                if let Err(e) = mailer.send(email).await {
                    log::error!("Failed to send email, failure reason is: {e:?}");
                }
            });
            Ok(())
        } else {
            let mailer = SmtpTransport::relay(&settings.smtp_host)?
                .credentials(creds)
                .timeout(Some(core::time::Duration::from_secs(
                    settings.smtp_timeout_sec as u64,
                )))
                .pool_config(pool)
                .build();

            Ok(mailer.send(&email).map(|r| {
                log::info!("Sent email response: {r:?}");
            })?)
        }
    }
}

impl RuntimeNode for SendEmailNode {
    async fn exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        _response: &mut ResponseData,
        _channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // println!("Into SendEmailNode");
        if let Ok(Some(settings)) = get_settings(&req.robot_id) {
            if !settings.smtp_host.is_empty() {
                match self.send_email(&settings) {
                    Ok(_) => add_next_node(ctx, &self.successful_node_id),
                    Err(_) => add_next_node(ctx, self.goto_node_id.as_ref().unwrap()),
                }
            }
        }
        false
    }
}

#[derive(Archive, Clone, Deserialize, Serialize, serde::Deserialize)]
#[rkyv(compare(PartialEq))]
pub(crate) enum LlmChatNodeExitCondition {
    Intent(String),
    SpecialInputs(String),
    LlmResultContains(String),
    MaxChatTimes(u8),
}

#[derive(Archive, Clone, Deserialize, Serialize, serde::Deserialize)]
#[rkyv(compare(PartialEq))]
pub(crate) enum LlmChatAnswerTimeoutThen {
    GotoAnotherNode,
    ResponseAlternateText(String),
    DoNothing,
}

#[derive(Archive, Clone, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct LlmChatNode {
    // pub(super) prompt: String,
    pub(super) context_len: u8,
    pub(super) cur_run_times: u8,
    pub(super) exit_condition: LlmChatNodeExitCondition,
    pub(super) answer_timeout_then: LlmChatAnswerTimeoutThen,
    pub(crate) connect_timeout: Option<u32>,
    pub(crate) read_timeout: Option<u32>,
    pub(crate) response_streaming: bool,
    pub(super) next_node_id: String,
}

impl LlmChatNode {
    async fn inner_exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        response: &mut ResponseData,
        channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // log::info!("Into LlmChatNode");
        self.cur_run_times += 1;
        let mut check_contains_str: Option<&String> = None;
        match &self.exit_condition {
            LlmChatNodeExitCondition::Intent(i) => match ctx.get_user_input_intent(req).await {
                Ok(crate::flow::rt::context::UserInputIntent::Detected(intent)) => {
                    if intent.eq(i) {
                        return false;
                    }
                }
                _ => {}
            },
            LlmChatNodeExitCondition::SpecialInputs(s) => {
                if req.user_input.eq(s) {
                    // log::info!("886 {}", &self.next_node_id);
                    return false;
                }
            }
            LlmChatNodeExitCondition::LlmResultContains(s) => {
                check_contains_str = Some(s);
            }
            LlmChatNodeExitCondition::MaxChatTimes(t) => {
                if self.cur_run_times > *t {
                    return false;
                }
            }
        }
        // log::info!("self.response_streaming {}", self.response_streaming);
        let chat_history = if ctx.chat_history.is_empty() {
            None
        } else {
            Some(ctx.chat_history.clone())
        };
        if self.response_streaming {
            // let r = super::facade::get_sender(req.session_id.as_ref().unwrap());
            // if r.is_err() {
            //     add_next_node(ctx, &self.next_node_id);
            //     return false;
            // }
            // let s_op = r.unwrap();
            // if s_op.is_none() {
            //     add_next_node(ctx, &self.next_node_id);
            //     return false;
            // }
            // let s = s_op.unwrap();
            // let ticket = String::new();
            let robot_id = req.robot_id.clone();
            let connect_timeout = self.connect_timeout;
            let read_timeout = self.read_timeout;
            // let (s, r) = tokio::sync::mpsc::channel::<String>(1);
            if channel_sender.sender.is_none() {
                let (s, r) = tokio::sync::mpsc::channel::<StreamingResponseData>(2);
                channel_sender.sender = Some(s);
                channel_sender.receiver = Some(r);
            }
            let s = channel_sender.sender.clone().unwrap();
            let sender_wrapper = SenderWrapper {
                sender: s,
                content_seq: ctx.add_answer_history(""),
            };
            tokio::task::spawn(async move {
                if let Err(e) = crate::ai::chat::chat(
                    &robot_id,
                    chat_history,
                    connect_timeout,
                    read_timeout,
                    ResultSender::ChannelSender(sender_wrapper),
                )
                .await
                {
                    log::info!("LlmChatNode response failed, err: {:?}", &e);
                }
            });
            true
        } else {
            let now = std::time::Instant::now();
            let mut s = String::with_capacity(1024);
            if let Err(e) = crate::ai::chat::chat(
                &req.robot_id,
                chat_history,
                self.connect_timeout,
                self.read_timeout,
                ResultSender::StrBuf(&mut s),
            )
            .await
            {
                log::error!("LlmChatNode response failed, err: {:?}", &e);
                match &self.answer_timeout_then {
                    LlmChatAnswerTimeoutThen::GotoAnotherNode => {
                        return false;
                    }
                    LlmChatAnswerTimeoutThen::ResponseAlternateText(t) => s.push_str(t),
                    LlmChatAnswerTimeoutThen::DoNothing => return false,
                }
            } else {
                log::info!("LLM response |{}|", &s);
                if !s.is_empty() {
                    let mut contains_certain_str = false;
                    if check_contains_str.is_some() {
                        log::info!(
                            "check_contains_str |{}|",
                            check_contains_str.as_ref().unwrap()
                        );
                        contains_certain_str = s.contains(check_contains_str.unwrap());
                        log::info!("contains_certain_str {contains_certain_str}");
                    }
                    response.answers.push(AnswerData {
                        content: s,
                        content_type: AnswerContentType::TextPlain,
                    });
                    if contains_certain_str {
                        return false;
                    }
                }
            }
            log::info!("LLM response took {:?}", now.elapsed());
            // let (s, rev) = std::sync::mpsc::channel::<String>();
            // let robot_id = req.robot_id.clone();
            // let prompt = self.prompt.clone();
            // tokio::task::spawn(async move {
            //     let mut r = String::with_capacity(1024);
            //     if let Err(e) =
            //         crate::ai::chat::chat(&robot_id, &prompt, ResultReceiver::StrBuf(&mut r)).await
            //     {
            //         log::info!("LlmChatNode response failed, err: {:?}", &e);
            //         drop(s);
            //         return;
            //     }
            //     if let Err(_) = s.send(r) {
            //         log::info!("LlmChatNode sent response failed.");
            //     }
            // });
            // match rev.recv() {
            //     Ok(s) => {
            //         log::info!("LLM response {}", &s);
            //         response.answers.push(AnswerData {
            //             text: s,
            //             answer_type: AnswerType::TextPlain,
            //         });
            //     }
            //     // Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {}
            //     Err(e) => log::info!("LlmChatNode response failed, err: {:?}", &e),
            // }
            // let mut s = String::with_capacity(1024);
            // if let Err(e) = tokio::runtime::Handle::current().block_on(async {
            //     crate::ai::chat::chat(&req.robot_id, &self.prompt, ResultReceiver::StrBuf(&mut s))
            //         .await
            // }) {
            //     log::info!("LlmChatNode response failed, err: {:?}", &e);
            // } else {
            //     log::info!("LLM response {}", &s);
            //     response.answers.push(AnswerData {
            //         text: s,
            //         answer_type: AnswerType::TextPlain,
            //     });
            // }
            true
        }
    }
}

impl RuntimeNode for LlmChatNode {
    async fn exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        response: &mut ResponseData,
        channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // log::info!("Into LlmChatNode");
        let r = self.inner_exec(req, ctx, response, channel_sender).await;
        if r {
            let r = RuntimeNodeEnum::LlmChatNode(self.clone());
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&r).unwrap();
            ctx.node = Some(bytes.into_vec());
        } else {
            add_next_node(ctx, &self.next_node_id);
        }
        r
        /*
        self.cur_run_times = self.cur_run_times + 1;
        let mut check_contains_str: Option<&String> = None;
        match &self.exit_condition {
            LlmChatNodeExitCondition::Intent(i) => {
                if req.user_input_intent.is_some() && req.user_input_intent.as_ref().unwrap().eq(i)
                {
                    add_next_node(ctx, &self.next_node_id);
                    return false;
                }
            }
            LlmChatNodeExitCondition::SpecialInputs(s) => {
                if req.user_input.eq(s) {
                    // log::info!("886 {}", &self.next_node_id);
                    add_next_node(ctx, &self.next_node_id);
                    return false;
                }
            }
            LlmChatNodeExitCondition::LlmResultContains(s) => {
                check_contains_str = Some(s);
            }
            LlmChatNodeExitCondition::MaxChatTimes(t) => {
                if self.cur_run_times > *t {
                    add_next_node(ctx, &self.next_node_id);
                    return false;
                }
            }
        }
        let r = RuntimeNnodeEnum::LlmChatNode(self.clone());
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&r).unwrap();
        ctx.node = Some(bytes.into_vec());
        // log::info!("self.response_streaming {}", self.response_streaming);
        let chat_history = if ctx.chat_history.is_empty() {
            None
        } else {
            Some(ctx.chat_history.clone())
        };
        if self.response_streaming {
            // let r = super::facade::get_sender(req.session_id.as_ref().unwrap());
            // if r.is_err() {
            //     add_next_node(ctx, &self.next_node_id);
            //     return false;
            // }
            // let s_op = r.unwrap();
            // if s_op.is_none() {
            //     add_next_node(ctx, &self.next_node_id);
            //     return false;
            // }
            // let s = s_op.unwrap();
            // let ticket = String::new();
            let robot_id = req.robot_id.clone();
            let connect_timeout = self.connect_timeout.clone();
            let read_timeout = self.read_timeout.clone();
            // let (s, r) = tokio::sync::mpsc::channel::<String>(1);
            let (s, r) = tokio::sync::mpsc::channel::<String>(2);
            channel_sender.receiver = Some(r);
            tokio::task::spawn(async move {
                if let Err(e) = crate::ai::chat::chat(
                    &robot_id,
                    chat_history,
                    connect_timeout,
                    read_timeout,
                    ResultSender::ChannelSender(&s),
                )
                .await
                {
                    log::info!("LlmChatNode response failed, err: {:?}", &e);
                }
            });
            true
        } else {
            let now = std::time::Instant::now();
            let mut s = String::with_capacity(1024);
            if let Err(e) = tokio::task::block_in_place(|| {
                // log::info!("prompt |{}|", &self.prompt);
                // tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(crate::ai::chat::chat(
                        &req.robot_id,
                        chat_history,
                        self.connect_timeout,
                        self.read_timeout,
                        ResultSender::StrBuf(&mut s),
                    ))
                // })
            }) {
                log::error!("LlmChatNode response failed, err: {:?}", &e);
                match &self.answer_timeout_then {
                    LlmChatAnswerTimeoutThen::GotoAnotherNode => {
                        ctx.node = None;
                        add_next_node(ctx, &self.next_node_id);
                        return false;
                    }
                    LlmChatAnswerTimeoutThen::ResponseAlternateText(t) => s.push_str(t),
                    LlmChatAnswerTimeoutThen::DoNothing => return false,
                }
            } else {
                log::info!("LLM response |{}|", &s);
                if !s.is_empty() {
                    let mut contains_certain_str = false;
                    if check_contains_str.is_some() {
                        log::info!("check_contains_str |{}|", check_contains_str.as_ref().unwrap());
                        contains_certain_str = s.contains(check_contains_str.unwrap());
                        log::info!("contains_certain_str {}", contains_certain_str);
                    }
                    response.answers.push(AnswerData {
                        content: s,
                        content_type: AnswerContentType::TextPlain,
                    });
                    if contains_certain_str {
                        add_next_node(ctx, &self.next_node_id);
                        return false;
                    }
                }
            }
            log::info!("LLM response took {:?}", now.elapsed());
            // let (s, rev) = std::sync::mpsc::channel::<String>();
            // let robot_id = req.robot_id.clone();
            // let prompt = self.prompt.clone();
            // tokio::task::spawn(async move {
            //     let mut r = String::with_capacity(1024);
            //     if let Err(e) =
            //         crate::ai::chat::chat(&robot_id, &prompt, ResultReceiver::StrBuf(&mut r)).await
            //     {
            //         log::info!("LlmChatNode response failed, err: {:?}", &e);
            //         drop(s);
            //         return;
            //     }
            //     if let Err(_) = s.send(r) {
            //         log::info!("LlmChatNode sent response failed.");
            //     }
            // });
            // match rev.recv() {
            //     Ok(s) => {
            //         log::info!("LLM response {}", &s);
            //         response.answers.push(AnswerData {
            //             text: s,
            //             answer_type: AnswerType::TextPlain,
            //         });
            //     }
            //     // Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {}
            //     Err(e) => log::info!("LlmChatNode response failed, err: {:?}", &e),
            // }
            // let mut s = String::with_capacity(1024);
            // if let Err(e) = tokio::runtime::Handle::current().block_on(async {
            //     crate::ai::chat::chat(&req.robot_id, &self.prompt, ResultReceiver::StrBuf(&mut s))
            //         .await
            // }) {
            //     log::info!("LlmChatNode response failed, err: {:?}", &e);
            // } else {
            //     log::info!("LLM response {}", &s);
            //     response.answers.push(AnswerData {
            //         text: s,
            //         answer_type: AnswerType::TextPlain,
            //     });
            // }
            true
        }
        */
    }
}

#[derive(Archive, Clone, Deserialize, Serialize, serde::Deserialize)]
#[rkyv(compare(PartialEq))]
pub(crate) enum KnowledgeBaseAnswerNoRecallThen {
    GotoAnotherNode,
    ReturnAlternateAnswerInstead(String),
}

#[derive(Archive, Clone, Debug, Deserialize, Serialize, serde::Deserialize)]
#[rkyv(compare(PartialEq))]
pub(crate) enum KnowledgeBaseAnswerSource {
    QnA,
    Doc,
}

#[derive(Archive, Clone, Deserialize, Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct KnowledgeBaseAnswerNode {
    pub(super) recall_distance: f64,
    pub(super) retrieve_answer_sources: Vec<crate::flow::rt::node::KnowledgeBaseAnswerSource>,
    pub(super) no_recall_then: KnowledgeBaseAnswerNoRecallThen,
    pub(super) next_node_id: String,
}

impl KnowledgeBaseAnswerNode {
    async fn retrieve_qa_answer(&self, req: &Request) -> Option<String> {
        match crate::kb::qa::retrieve_answer(&req.robot_id, &req.user_input).await {
            Ok((answer, distance)) => {
                log::info!(
                    "distance {} recall_distance {}",
                    distance,
                    self.recall_distance
                );
                if let Some(a) = answer
                    && distance <= self.recall_distance
                {
                    Some(a.answer)
                } else {
                    None
                }
            }
            Err(e) => {
                log::error!("KnowledgeBaseAnswerNode retrieve QnA failed: {:?}", &e);
                None
            }
        }
    }
    async fn retrieve_doc_answer(&self, req: &Request) -> Option<String> {
        let r = crate::kb::doc::search_doc(
            &req.robot_id,
            &req.user_input,
            self.recall_distance,
            1000,
            5000,
        )
        .await;
        match r {
            Ok(op) => op,
            Err(e) => {
                log::warn!("KnowledgeBaseAnswerNode retrieve doc failed {:?}", &e);
                None
            }
        }
    }
    fn fallback_answer(&self, ctx: &mut Context, response: &mut ResponseData) -> bool {
        match &self.no_recall_then {
            KnowledgeBaseAnswerNoRecallThen::GotoAnotherNode => {
                add_next_node(ctx, &self.next_node_id);
                false
            }
            KnowledgeBaseAnswerNoRecallThen::ReturnAlternateAnswerInstead(s) => {
                response.answers.push(AnswerData {
                    content: s.clone(),
                    content_type: AnswerContentType::TextPlain,
                });
                let r = RuntimeNodeEnum::KnowledgeBaseAnswerNode(self.clone());
                let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&r).unwrap();
                ctx.node = Some(bytes.into_vec());
                true
            }
        }
    }
}

impl RuntimeNode for KnowledgeBaseAnswerNode {
    async fn exec(
        &mut self,
        req: &Request,
        ctx: &mut Context,
        response: &mut ResponseData,
        _channel_sender: &mut ResponseChannelWrapper,
    ) -> bool {
        // log::info!("Into LlmChaKnowledgeBaseAnswerNodetNode");
        for answer_source in &self.retrieve_answer_sources {
            log::info!("answer_source={:?}", &answer_source);
            let r = match answer_source {
                KnowledgeBaseAnswerSource::QnA => self.retrieve_qa_answer(req).await,
                KnowledgeBaseAnswerSource::Doc => self.retrieve_doc_answer(req).await,
            };
            if let Some(content) = r
                && !content.is_empty()
            {
                response.answers.push(AnswerData {
                    content: content,
                    content_type: AnswerContentType::TextPlain,
                });
                add_next_node(ctx, &self.next_node_id);
                return false;
            }
        }
        self.fallback_answer(ctx, response)
        /*
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(crate::kb::qa::retrieve_answer(
                &req.robot_id,
                &req.user_input,
            ))
        });
        match result {
            Ok((answer, distance)) => {
                log::info!(
                    "distance {} recall_distance {}",
                    distance,
                    self.recall_distance
                );
                if answer.is_some() && distance <= self.recall_distance {
                    response.answers.push(AnswerData {
                        text: answer.unwrap().answer,
                        answer_type: AnswerType::TextPlain,
                    });
                    add_next_node(ctx, &self.next_node_id);
                    false
                } else {
                    self.fallback_answer(ctx, response)
                }
            }
            Err(e) => {
                log::error!("KnowledgeBaseAnswerNode answer failed: {:?}", &e);
                self.fallback_answer(ctx, response)
            }
        }
        */
    }
}

pub(crate) fn deser_node(bytes: &[u8]) -> Result<RuntimeNodeEnum> {
    // let now = std::time::Instant::now();
    let mut v = AlignedVec::<256>::with_capacity(bytes.len());
    v.extend_from_slice(bytes);
    let r = rkyv::from_bytes::<RuntimeNodeEnum, rkyv::rancor::Error>(&v).unwrap();
    // let archived = rkyv::access::<ArchivedRuntimeNnodeEnum, rkyv::rancor::Error>(bytes).unwrap();
    // let deserialized = rkyv::deserialize::<RuntimeNnodeEnum, rkyv::rancor::Error>(archived).unwrap();
    // log::info!("deser_node time {:?}", now.elapsed());
    Ok(r)
}
