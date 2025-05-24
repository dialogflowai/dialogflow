use std::str::FromStr;

use bigdecimal::BigDecimal;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::flow::rt::context::Context;
use crate::flow::rt::dto::{Request, UserInputResult};
use crate::variable::crud as variable;
use crate::variable::dto::VariableType;

#[derive(
    Clone, Copy, Deserialize, Serialize, rkyv::Archive, rkyv::Deserialize, rkyv::Serialize,
)]
#[rkyv(compare(PartialEq))]
pub(crate) enum ConditionType {
    UserInput,
    UserIntent,
    FlowVariable,
    CustomJavascript,
    CustomRegex,
}

#[derive(
    Clone, Copy, Deserialize, Serialize, rkyv::Archive, rkyv::Deserialize, rkyv::Serialize,
)]
#[rkyv(compare(PartialEq))]
pub(crate) enum CompareType {
    HasValue,
    DoesNotHaveValue,
    EmptyString,
    Eq,
    NotEq,
    Contains,
    NotContains,
    NGT,
    NGTE,
    NLT,
    NLTE,
    Timeout,
}

#[derive(
    Copy, Clone, Debug, Deserialize, Serialize, rkyv::Archive, rkyv::Deserialize, rkyv::Serialize,
)]
#[rkyv(compare(PartialEq))]
pub(crate) enum TargetDataVariant {
    Const,
    Variable,
    ZeroShotTextClassification,
}

// #[macro_export]
macro_rules! compare_numbers {
    ($req: expr, $ctx: expr, $ref_data: expr, $comparsion: tt, $self: ident) => ({
        let r = variable::get(&$req.robot_id, $ref_data);
        if r.is_err() {
            log::error!("err");
            return false;
        }
        let v = r.unwrap();
        if v.is_none() {
            return false;
        }
        let v = v.unwrap();
        if v.var_type == VariableType::Str {
            return false;
        }
        let val = v.get_value($req, $ctx);
        if val.is_none() {
            return false;
        }
        let val = val.unwrap();
        let n1 = match BigDecimal::from_str(&val.val_to_string()) {
            Ok(n) => n,
            Err(e) => {
                log::warn!("{:?}",&e);
                return false;
            }
        };
        let n2 = match BigDecimal::from_str(&$self.get_target_data($req, $ctx)) {
            Ok(n) => n,
            Err(e) => {
                log::warn!("{:?}",&e);
                return false;
            }
        };
        n1 $comparsion n2
    });
}

#[derive(Clone, Deserialize, Serialize, rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
#[rkyv(compare(PartialEq))]
pub(crate) struct ConditionData {
    pub(crate) condition_type: ConditionType,
    pub(crate) compare_type: CompareType,
    pub(crate) ref_data: String,
    pub(crate) target_data: String,
    pub(crate) target_data_variant: TargetDataVariant,
    pub(crate) case_sensitive_comparison: bool,
}

impl ConditionData {
    fn get_target_data(&self, req: &Request, ctx: &mut Context) -> String {
        match self.target_data_variant {
            TargetDataVariant::Const => self.target_data.clone(),
            TargetDataVariant::Variable => variable::get_value(&self.target_data, req, ctx),
            TargetDataVariant::ZeroShotTextClassification => {
                //todo
                //do text zero shot classification of user input
                //and try matching label (from self.ref_data) and get a score
                String::new()
            }
        }
    }
    pub(in crate::flow::rt) fn compare(&self, req: &Request, ctx: &mut Context) -> bool {
        // let target_data = match self.target_data_variant {
        //     TargetDataVariant::Const => self.target_data.clone(),
        //     TargetDataVariant::Variable => variable::get_value(&self.target_data, req, ctx),
        // };
        // println!("{} {}", &target_data, &req.user_input);
        match self.condition_type {
            ConditionType::UserInput => match self.compare_type {
                CompareType::Eq => {
                    if self.case_sensitive_comparison {
                        println!(
                            "{} {} {}",
                            self.get_target_data(req, ctx),
                            &req.user_input,
                            self.get_target_data(req, ctx).eq(&req.user_input)
                        );
                        self.get_target_data(req, ctx).eq(&req.user_input)
                    } else {
                        unicase::eq(&self.get_target_data(req, ctx), &req.user_input)
                    }
                }
                CompareType::Contains => {
                    if self.case_sensitive_comparison {
                        req.user_input.contains(&self.get_target_data(req, ctx))
                    } else {
                        let mut s = self.get_target_data(req, ctx);
                        s.make_ascii_lowercase();
                        s.contains(&req.user_input.to_lowercase())
                    }
                }
                CompareType::Timeout => UserInputResult::Timeout == req.user_input_result,
                CompareType::EmptyString => req.user_input.is_empty(),
                _ => false,
            },
            ConditionType::UserIntent => {
                // println!("{} {}", &target_data, req.user_input_intent.is_some());
                req.user_input_intent.is_some()
                    && self
                        .get_target_data(req, ctx)
                        .eq(req.user_input_intent.as_ref().unwrap())
            }
            ConditionType::FlowVariable => match self.compare_type {
                CompareType::HasValue => {
                    if let Ok(Some(v)) = variable::get(&req.robot_id, &self.ref_data) {
                        v.get_value(req, ctx).is_some()
                    } else {
                        false
                    }
                }
                CompareType::DoesNotHaveValue => {
                    if let Ok(Some(v)) = variable::get(&req.robot_id, &self.ref_data) {
                        v.get_value(req, ctx).is_none()
                    } else {
                        true
                    }
                }
                CompareType::EmptyString => {
                    if let Ok(Some(v)) = variable::get(&req.robot_id, &self.ref_data) {
                        if v.var_type == VariableType::Num {
                            false
                        } else {
                            let val = v.get_value(req, ctx);
                            val.is_none() || val.as_ref().unwrap().val_to_string().is_empty()
                        }
                    } else {
                        false
                    }
                }
                CompareType::Eq => {
                    if let Ok(Some(v)) = variable::get(&req.robot_id, &self.ref_data) {
                        if let Some(val) = v.get_value(req, ctx) {
                            if self.case_sensitive_comparison {
                                val.val_to_string().eq(&self.get_target_data(req, ctx))
                            } else {
                                unicase::eq(&val.val_to_string(), &self.get_target_data(req, ctx))
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                CompareType::NotEq => {
                    if let Ok(Some(v)) = variable::get(&req.robot_id, &self.ref_data) {
                        if let Some(val) = v.get_value(req, ctx) {
                            if self.case_sensitive_comparison {
                                !val.val_to_string().eq(&self.get_target_data(req, ctx))
                            } else {
                                !unicase::eq(&val.val_to_string(), &self.get_target_data(req, ctx))
                            }
                        } else {
                            true
                        }
                    } else {
                        true
                    }
                }
                CompareType::Contains => {
                    if let Ok(Some(v)) = variable::get(&req.robot_id, &self.ref_data) {
                        if v.var_type == VariableType::Num {
                            false
                        } else if let Some(val) = v.get_value(req, ctx) {
                            if self.case_sensitive_comparison {
                                val.val_to_string()
                                    .contains(&self.get_target_data(req, ctx))
                            } else {
                                let mut s = val.val_to_string();
                                s.make_ascii_lowercase();
                                s.contains(&self.get_target_data(req, ctx).to_lowercase())
                            }
                            // val.val_to_string()
                            //     .find(&self.get_target_data(req, ctx))
                            //     .is_some()
                        } else {
                            true
                        }
                    } else {
                        true
                    }
                }
                CompareType::NotContains => {
                    if let Ok(Some(v)) = variable::get(&req.robot_id, &self.ref_data) {
                        if v.var_type == VariableType::Num {
                            false
                        } else if let Some(val) = v.get_value(req, ctx) {
                            !val.val_to_string()
                                .contains(&self.get_target_data(req, ctx))
                        } else {
                            true
                        }
                    } else {
                        true
                    }
                }
                CompareType::NGT => {
                    compare_numbers!(req, ctx, &self.ref_data, >, self)
                    // if let Ok(op) = variable::get(&req.robot_id, &self.ref_data) {
                    //     if let Some(v) = op {
                    //         if v.var_type == VariableType::Str {
                    //             false
                    //         } else {
                    //             if let Some(val) = v.get_value(req, ctx) {
                    //                 if let Ok(n1) = BigDecimal::from_str(&val.val_to_string()) {
                    //                     // println!("get_target_data {} {:?} |{}|", self.target_data, self.target_data_variant, self.get_target_data(req, ctx));
                    //                     if let Ok(n2) =
                    //                         BigDecimal::from_str(&self.get_target_data(req, ctx))
                    //                     {
                    //                         // println!("{} {}", n1, n2);
                    //                         n1 > n2
                    //                     } else {
                    //                         false
                    //                     }
                    //                 } else {
                    //                     false
                    //                 }
                    //             } else {
                    //                 false
                    //             }
                    //         }
                    //     } else {
                    //         false
                    //     }
                    // } else {
                    //     false
                    // }
                }
                CompareType::NGTE => {
                    compare_numbers!(req, ctx, &self.ref_data, >=, self)
                    // if let Ok(op) = variable::get(&req.robot_id, &self.ref_data) {
                    //     if let Some(v) = op {
                    //         if v.var_type == VariableType::Str {
                    //             false
                    //         } else {
                    //             if let Some(val) = v.get_value(req, ctx) {
                    //                 if let Ok(n1) = BigDecimal::from_str(&val.val_to_string()) {
                    //                     if let Ok(n2) =
                    //                         BigDecimal::from_str(&self.get_target_data(req, ctx))
                    //                     {
                    //                         n1 >= n2
                    //                     } else {
                    //                         false
                    //                     }
                    //                 } else {
                    //                     false
                    //                 }
                    //             } else {
                    //                 false
                    //             }
                    //         }
                    //     } else {
                    //         false
                    //     }
                    // } else {
                    //     false
                    // }
                }
                CompareType::NLT => {
                    compare_numbers!(req, ctx, &self.ref_data, <, self)
                    // if let Ok(op) = variable::get(&req.robot_id, &self.ref_data) {
                    //     if let Some(v) = op {
                    //         if v.var_type == VariableType::Str {
                    //             false
                    //         } else {
                    //             if let Some(val) = v.get_value(req, ctx) {
                    //                 if let Ok(n1) = BigDecimal::from_str(&val.val_to_string()) {
                    //                     if let Ok(n2) =
                    //                         BigDecimal::from_str(&self.get_target_data(req, ctx))
                    //                     {
                    //                         n1 < n2
                    //                     } else {
                    //                         false
                    //                     }
                    //                 } else {
                    //                     false
                    //                 }
                    //             } else {
                    //                 false
                    //             }
                    //         }
                    //     } else {
                    //         false
                    //     }
                    // } else {
                    //     false
                    // }
                }
                CompareType::NLTE => {
                    compare_numbers!(req, ctx, &self.ref_data, <=, self)
                    // if let Ok(op) = variable::get(&req.robot_id, &self.ref_data) {
                    //     if let Some(v) = op {
                    //         if v.var_type == VariableType::Str {
                    //             false
                    //         } else {
                    //             if let Some(val) = v.get_value(req, ctx) {
                    //                 if let Ok(n1) = BigDecimal::from_str(&val.val_to_string()) {
                    //                     if let Ok(n2) =
                    //                         BigDecimal::from_str(&self.get_target_data(req, ctx))
                    //                     {
                    //                         n1 <= n2
                    //                     } else {
                    //                         false
                    //                     }
                    //                 } else {
                    //                     false
                    //                 }
                    //             } else {
                    //                 false
                    //             }
                    //         }
                    //     } else {
                    //         false
                    //     }
                    // } else {
                    //     false
                    // }
                }
                // let mut n = false;
                // if let Ok(r) = variable::get(&self.ref_data) {
                //     if let Some(ref_v) = r {
                //         if let Some(val) = ref_v.get_value(req, ctx) {
                //             n = val.val_to_string().eq(&target_data);
                //         }
                //     }
                // }
                _ => false,
            },
            ConditionType::CustomJavascript => todo!(),
            ConditionType::CustomRegex => {
                if let Ok(re) = Regex::new(&self.get_target_data(req, ctx)) {
                    return re.is_match(&req.user_input);
                }
                false
            }
        }
    }
}
