use std::collections::HashMap;
use std::vec::Vec;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::flow::rt::collector::CollectType;
use crate::flow::rt::condition::{CompareType, ConditionType, TargetDataVariant};
use crate::result::{Error, Result};

#[derive(Deserialize)]
pub(crate) struct SubFlowFormData {
    #[serde(rename = "robotId")]
    pub(crate) robot_id: String,
    #[serde(rename = "mainFlowId")]
    pub(crate) main_flow_id: String,
    pub(crate) data: String,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct SubFlowDetail {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) canvas: String,
    // #[serde(skip_serializing)]
    // pub(crate) nodes: String,
    // pub(crate) valid: bool,
}

impl SubFlowDetail {
    pub(crate) fn new(name: &str) -> Self {
        SubFlowDetail {
            id: scru128::new_string(),
            name: String::from(name),
            canvas: String::new(),
            // nodes: String::new(),
            // valid: false,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct CanvasCells {
    pub(crate) cells: Vec<CanvasCell>,
}

#[derive(Deserialize)]
pub(crate) struct CanvasCell {
    pub(crate) shape: String,
    pub(crate) data: Option<Node>,
    #[serde(flatten)]
    pub(in crate::flow) extra: HashMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(tag = "nodeType")]
pub(crate) enum Node {
    DialogNode(DialogNode),
    LlmChatNode(LlmChatNode),
    ConditionNode(ConditionNode),
    CollectNode(CollectNode),
    GotoNode(GotoNode),
    ExternalHttpNode(ExternalHttpNode),
    SendEmailNode(SendEmailNode),
    EndNode(EndNode),
    KnowledgeBaseAnswerNode(KnowledgeBaseAnswerNode),
}

impl Node {
    fn err(f: &SubFlowDetail, node_type: &str, node_name: &str, m: &str) -> Result<()> {
        let message = if node_name.is_empty() {
            format!("SubFlow: {} {} node {}", f.name, node_type, m)
        } else {
            format!(
                "SubFlow: {} {} node: {} {}",
                f.name, node_type, node_name, m
            )
        };
        Err(Error::WithMessage(message))
    }

    pub(crate) fn is_valid(&self, f: &SubFlowDetail) -> Result<()> {
        // println!("{}", std::any::type_name_of_val(&self));
        match self {
            Node::DialogNode(n) => {
                let t = "Dialog";
                if !n.valid {
                    Self::err(f, t, &n.node_name, "verification failed")
                } else if n.node_name.is_empty() {
                    Self::err(f, t, &n.node_name, "node name not filled in")
                } else if n.dialog_text.is_empty() {
                    Self::err(f, t, &n.node_name, "No dialog text filled in")
                } else if n.branches.len() != 1 {
                    Self::err(f, t, &n.node_name, "Branch information is incorrect")
                } else {
                    Ok(())
                }
            }
            Node::LlmChatNode(n) => {
                let t = "Dialog";
                if !n.valid {
                    Self::err(f, t, &n.node_name, "verification failed")
                } else if n.node_name.is_empty() {
                    Self::err(f, t, &n.node_name, "node name not filled in")
                } else if n.prompt.is_empty() {
                    Self::err(f, t, &n.node_name, "No prompt filled in")
                } else if n.branches.len() != 1 {
                    Self::err(f, t, &n.node_name, "Branch information is incorrect")
                } else {
                    Ok(())
                }
            }
            Node::ConditionNode(n) => {
                let t = "Condition";
                if !n.valid {
                    Self::err(f, t, &n.node_name, "verification failed")
                } else if n.node_name.is_empty() {
                    Self::err(f, t, &n.node_name, "node name not filled in")
                } else if n.branches.is_empty() {
                    Self::err(f, t, &n.node_name, "Conditional branch not set")
                } else {
                    Ok(())
                }
            }
            Node::CollectNode(n) => {
                let t = "Collect";
                if !n.valid {
                    Self::err(f, t, &n.node_name, "verification failed")
                } else if n.node_name.is_empty() {
                    Self::err(f, t, &n.node_name, "node name not filled in")
                } else if n.branches.len() != 2 {
                    Self::err(f, t, &n.node_name, "Branch information is incorrect")
                } else {
                    Ok(())
                }
            }
            Node::GotoNode(n) => {
                let t = "Goto";
                if !n.valid {
                    Self::err(f, t, &n.node_name, "verification failed")
                } else if n.node_name.is_empty() {
                    Self::err(f, t, &n.node_name, "node name not filled in")
                } else {
                    match n.goto_type {
                        NextActionType::GotoSubFlow => {
                            if n.goto_subflow_id.is_empty() {
                                Self::err(f, t, &n.node_name, "No sub-flow selected")
                            } else {
                                Ok(())
                            }
                        }
                        NextActionType::GotoExternalLink => {
                            if n.external_link.is_empty() {
                                Self::err(
                                    f,
                                    t,
                                    &n.node_name,
                                    "External link to goto is not filled in",
                                )
                            } else {
                                Ok(())
                            }
                        }
                        _ => Ok(()),
                    }
                }
            }
            Node::ExternalHttpNode(n) => {
                let t = "External HTTP";
                if !n.valid {
                    Self::err(f, t, &n.node_name, "verification failed")
                } else if n.node_name.is_empty() {
                    Self::err(f, t, &n.node_name, "node name not filled in")
                } else if n.branches.is_empty() || n.branches.len() > 2 {
                    Self::err(f, t, &n.node_name, "Branch information is incorrect")
                } else if n.http_api_id.is_empty() {
                    Self::err(f, t, &n.node_name, "No HTTP interface selected")
                } else {
                    Ok(())
                }
            }
            Node::SendEmailNode(n) => {
                let t = "Send email";
                if !n.valid {
                    Self::err(f, t, &n.node_name, "verification failed")
                } else if n.node_name.is_empty() {
                    Self::err(f, t, &n.node_name, "node name not filled in")
                } else if n.to_recipients.is_empty() {
                    Self::err(f, t, &n.node_name, "need to fill in the email recipient")
                } else if n.subject.is_empty() {
                    Self::err(f, t, &n.node_name, "need to fill in the email subject")
                } else if (n.async_send && n.branches.len() != 1)
                    || (!n.async_send && n.branches.len() != 2)
                {
                    Self::err(f, t, &n.node_name, "Branch information is incorrect")
                } else if n.content.is_empty() {
                    Self::err(f, t, &n.node_name, "need to fill in the email content")
                } else {
                    Ok(())
                }
            }
            Node::EndNode(n) => {
                let t = "End email";
                if !n.valid {
                    Self::err(f, t, &n.node_name, "verification failed")
                } else if n.node_name.is_empty() {
                    Self::err(f, t, &n.node_name, "node name not filled in")
                } else if n.ending_text.len() > 10000 {
                    Self::err(f, t, &n.node_name, "ending text were too long")
                } else {
                    Ok(())
                }
            }
            Node::KnowledgeBaseAnswerNode(n) => {
                let t = "Knowledge answer";
                if !n.valid {
                    Self::err(f, t, &n.node_name, "verification failed")
                } else if n.node_name.is_empty() {
                    Self::err(f, t, &n.node_name, "node name not filled in")
                } else if n.recall_thresholds < 1 || n.recall_thresholds > 100 {
                    Self::err(
                        f,
                        t,
                        &n.node_name,
                        "recall thresholds must between 1 and 100",
                    )
                } else {
                    Ok(())
                }
            }
        }
    }

    pub(crate) fn get_node_id(&self) -> String {
        match self {
            Self::DialogNode(n) => n.node_id.clone(),
            Self::LlmChatNode(n) => n.node_id.clone(),
            Self::ConditionNode(n) => n.node_id.clone(),
            Self::CollectNode(n) => n.node_id.clone(),
            Self::GotoNode(n) => n.node_id.clone(),
            Self::ExternalHttpNode(n) => n.node_id.clone(),
            Self::SendEmailNode(n) => n.node_id.clone(),
            Self::EndNode(n) => n.node_id.clone(),
            Self::KnowledgeBaseAnswerNode(n) => n.node_id.clone(),
        }
    }

    pub(crate) fn get_branch_target_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = Vec::with_capacity(10);
        match self {
            Self::DialogNode(n) => {
                n.branches
                    .iter()
                    .for_each(|b| ids.push(b.target_node_id.clone()));
            }
            Self::LlmChatNode(n) => {
                n.branches
                    .iter()
                    .for_each(|b| ids.push(b.target_node_id.clone()));
            }
            Self::EndNode(_) | Self::GotoNode(_) => {}
            Self::ConditionNode(n) => {
                n.branches
                    .iter()
                    .for_each(|b| ids.push(b.target_node_id.clone()));
            }
            Self::CollectNode(n) => {
                n.branches
                    .iter()
                    .for_each(|b| ids.push(b.target_node_id.clone()));
            }
            Self::ExternalHttpNode(n) => {
                n.branches
                    .iter()
                    .for_each(|b| ids.push(b.target_node_id.clone()));
            }
            Self::SendEmailNode(n) => {
                n.branches
                    .iter()
                    .for_each(|b| ids.push(b.target_node_id.clone()));
            }
            Self::KnowledgeBaseAnswerNode(n) => {
                n.branches
                    .iter()
                    .for_each(|b| ids.push(b.target_node_id.clone()));
            }
        };
        ids
    }

    pub(crate) fn get_branches(&mut self) -> Option<&mut Vec<Branch>> {
        match self {
            Self::DialogNode(n) => Some(&mut n.branches),
            Self::LlmChatNode(n) => Some(&mut n.branches),
            Self::EndNode(_) | Self::GotoNode(_) => None,
            Self::ConditionNode(n) => Some(&mut n.branches),
            Self::CollectNode(n) => Some(&mut n.branches),
            Self::ExternalHttpNode(n) => Some(&mut n.branches),
            Self::SendEmailNode(n) => Some(&mut n.branches),
            Self::KnowledgeBaseAnswerNode(n) => Some(&mut n.branches),
        }
    }
}

#[derive(Deserialize, PartialEq, Eq)]
pub(crate) enum BranchType {
    GotoAnotherNode,
    Condition,
    InfoCollectedSuccessfully,
    EmailSentSuccessfully,
}

#[derive(Deserialize)]
pub(crate) struct Branch {
    #[serde(rename = "branchId")]
    pub(crate) branch_id: String,
    #[serde(rename = "branchName")]
    pub(crate) branch_name: String,
    #[serde(rename = "branchType")]
    pub(crate) branch_type: BranchType,
    #[serde(rename = "targetNodeId")]
    pub(crate) target_node_id: String,
    #[serde(rename = "conditionGroup")]
    pub(crate) condition_group: Vec<Vec<BranchCondition>>,
}

#[derive(Deserialize)]
pub(crate) struct BranchCondition {
    #[serde(rename = "conditionType")]
    pub(crate) condition_type: ConditionType,
    #[serde(rename = "refChoice")]
    pub(crate) ref_choice: String,
    #[serde(rename = "compareType")]
    pub(crate) compare_type: CompareType,
    #[serde(rename = "targetValue")]
    pub(crate) target_value: String,
    #[serde(rename = "targetValueVariant")]
    pub(crate) target_value_variant: TargetDataVariant,
    #[serde(rename = "caseSensitiveComparison")]
    pub(crate) case_sensitive_comparison: bool,
}

#[derive(Deserialize)]
pub(crate) struct DialogNode {
    pub(crate) valid: bool,
    #[serde(rename = "nodeId")]
    pub(crate) node_id: String,
    #[serde(rename = "nodeName")]
    pub(crate) node_name: String,
    #[serde(rename = "dialogText")]
    pub(crate) dialog_text: String,
    #[serde(rename = "dialogTextType")]
    pub(crate) dialog_text_type: crate::flow::rt::dto::AnswerContentType,
    #[serde(rename = "nextStep")]
    pub(crate) next_step: NextActionType,
    pub(crate) branches: Vec<Branch>,
}

#[derive(Deserialize)]
pub(crate) struct LlmChatNode {
    pub(crate) valid: bool,
    #[serde(rename = "nodeId")]
    pub(crate) node_id: String,
    #[serde(rename = "nodeName")]
    pub(crate) node_name: String,
    #[serde(rename = "prompt")]
    pub(crate) prompt: String,
    #[serde(rename = "contextLength")]
    pub(crate) context_length: u8,
    #[serde(rename = "exitCondition")]
    pub(crate) exit_condition: crate::flow::rt::node::LlmChatNodeExitCondition,
    #[serde(rename = "whenTimeoutThen")]
    pub(crate) when_timeout_then: crate::flow::rt::node::LlmChatAnswerTimeoutThen,
    #[serde(rename = "responseStreaming")]
    pub(crate) response_streaming: bool,
    pub(crate) branches: Vec<Branch>,
    #[serde(rename = "connectTimeout")]
    pub(crate) connect_timeout: Option<u32>,
    #[serde(rename = "readTimeout")]
    pub(crate) read_timeout: Option<u32>,
}

#[derive(Deserialize)]
pub(crate) struct ConditionNode {
    pub(crate) valid: bool,
    #[serde(rename = "nodeId")]
    pub(crate) node_id: String,
    #[serde(rename = "nodeName")]
    pub(crate) node_name: String,
    pub(crate) branches: Vec<Branch>,
}

#[derive(Deserialize)]
pub(crate) struct CollectNode {
    pub(crate) valid: bool,
    #[serde(rename = "nodeId")]
    pub(crate) node_id: String,
    #[serde(rename = "nodeName")]
    pub(crate) node_name: String,
    pub(crate) collect_type: CollectType,
    #[serde(rename = "collectSaveVarName")]
    pub(crate) collect_save_var_name: String,
    pub(crate) branches: Vec<Branch>,
}

#[derive(Deserialize, PartialEq, Eq, Serialize)]
pub(crate) enum NextActionType {
    None,
    GotoMainFlow,
    GotoSubFlow,
    GotoExternalLink,
    GotoNextNode,
    Terminate,
    WaitUserResponse,
}

#[derive(Deserialize)]
pub(crate) struct GotoNode {
    pub(crate) valid: bool,
    #[serde(rename = "nodeId")]
    pub(crate) node_id: String,
    #[serde(rename = "nodeName")]
    pub(crate) node_name: String,
    #[serde(rename = "gotoType")]
    pub(crate) goto_type: NextActionType,
    #[serde(rename = "gotoMainFlowId")]
    pub(crate) goto_mainflow_id: String,
    #[serde(rename = "gotoSubFlowId")]
    pub(crate) goto_subflow_id: String,
    #[serde(rename = "gotoSubFlowName")]
    pub(crate) goto_subflow_name: String,
    #[serde(rename = "externalLink")]
    pub(crate) external_link: String,
}

#[derive(Deserialize)]
pub(crate) struct ExternalHttpNode {
    pub(crate) valid: bool,
    #[serde(rename = "nodeId")]
    pub(crate) node_id: String,
    #[serde(rename = "nodeName")]
    pub(crate) node_name: String,
    #[serde(rename = "httpApiId")]
    pub(crate) http_api_id: String,
    #[serde(rename = "timeoutMilliseconds")]
    pub(crate) timeout_milliseconds: u64,
    #[serde(rename = "asyncReq")]
    pub(crate) async_req: bool,
    pub(crate) branches: Vec<Branch>,
}

#[derive(Deserialize)]
pub(crate) struct SendEmailNode {
    pub(crate) valid: bool,
    #[serde(rename = "nodeId")]
    pub(crate) node_id: String,
    #[serde(rename = "nodeName")]
    pub(crate) node_name: String,
    #[serde(rename = "from")]
    pub(crate) from: String,
    #[serde(rename = "toRecipients")]
    pub(crate) to_recipients: Vec<String>,
    #[serde(rename = "ccRecipients")]
    pub(crate) cc_recipients: Vec<String>,
    #[serde(rename = "bccRecipients")]
    pub(crate) bcc_recipients: Vec<String>,
    pub(crate) subject: String,
    pub(crate) content: String,
    #[serde(rename = "contentType")]
    pub(crate) content_type: String,
    pub(crate) branches: Vec<Branch>,
    #[serde(rename = "asyncSend")]
    pub(crate) async_send: bool,
}

#[derive(Deserialize)]
pub(crate) struct EndNode {
    pub(crate) valid: bool,
    #[serde(rename = "nodeId")]
    pub(crate) node_id: String,
    #[serde(rename = "nodeName")]
    pub(crate) node_name: String,
    #[serde(rename = "endingText")]
    pub(crate) ending_text: String,
}

#[derive(Deserialize)]
pub(crate) struct KnowledgeBaseAnswerNode {
    pub(crate) valid: bool,
    #[serde(rename = "nodeId")]
    pub(crate) node_id: String,
    #[serde(rename = "nodeName")]
    pub(crate) node_name: String,
    pub(crate) branches: Vec<Branch>,
    #[serde(rename = "recallThresholds")]
    pub(crate) recall_thresholds: u8,
    #[serde(rename = "noAnswerThen")]
    pub(crate) no_answer_then: crate::flow::rt::node::KnowledgeBaseAnswerNoRecallThen,
    #[serde(rename = "retrieveAnswerSources")]
    pub(crate) retrieve_answer_sources: Vec<crate::flow::rt::node::KnowledgeBaseAnswerSource>,
}
