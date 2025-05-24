// use std::collections::VecDeque;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::vec::Vec;

use candle::{IndexOp, Tensor};
use candle_transformers::models::bert::BertModel;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokenizers::Tokenizer;

use super::huggingface::{HuggingFaceModel, HuggingFaceModelInfo, load_bert_model_files};
use crate::man::settings;
use crate::result::{Error, Result};

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "id", content = "model")]
pub(crate) enum SentenceEmbeddingProvider {
    HuggingFace(HuggingFaceModel),
    OpenAI(String),
    Ollama(String),
}

pub(crate) async fn embedding(robot_id: &str, s: &str) -> Result<(Vec<f32>, f32)> {
    if let Some(settings) = settings::get_settings(robot_id)? {
        let v = match settings.sentence_embedding_provider.provider {
            SentenceEmbeddingProvider::HuggingFace(m) => hugging_face(robot_id, &m.get_info(), s),
            SentenceEmbeddingProvider::OpenAI(m) => {
                open_ai(
                    &m,
                    s,
                    &settings.sentence_embedding_provider.api_key,
                    settings.sentence_embedding_provider.connect_timeout_millis,
                    settings.sentence_embedding_provider.read_timeout_millis,
                    &settings.sentence_embedding_provider.proxy_url,
                )
                .await
            }
            SentenceEmbeddingProvider::Ollama(m) => {
                ollama(
                    &settings.sentence_embedding_provider.api_url,
                    &m,
                    s,
                    settings.sentence_embedding_provider.connect_timeout_millis,
                    settings.sentence_embedding_provider.read_timeout_millis,
                    &settings.sentence_embedding_provider.proxy_url,
                )
                .await
            }
        }?;
        Ok((v, settings.sentence_embedding_provider.similarity_threshold))
    } else {
        Err(Error::WithMessage(format!(
            "Can not find settings of {}",
            robot_id
        )))
    }
}

static EMBEDDING_MODEL: OnceLock<Mutex<HashMap<String, (BertModel, Tokenizer)>>> = OnceLock::new();

pub(crate) fn replace_model_cache(robot_id: &str, c: (BertModel, Tokenizer)) {
    if let Some(lock) = EMBEDDING_MODEL.get() {
        if let Ok(mut cache) = lock.lock() {
            cache.insert(String::from(robot_id), c);
        }
    }
}

fn hugging_face(robot_id: &str, info: &HuggingFaceModelInfo, s: &str) -> Result<Vec<f32>> {
    let lock = EMBEDDING_MODEL.get_or_init(|| Mutex::new(HashMap::with_capacity(32)));
    let mut model = lock.lock().unwrap_or_else(|e| {
        log::warn!("{:#?}", &e);
        e.into_inner()
    });
    if !model.contains_key(robot_id) {
        let r = load_bert_model_files(info.repository)?;
        model.insert(String::from(robot_id), r);
    };
    let (m, t) = model.get_mut(robot_id).unwrap();
    // let tokenizer = match t.with_padding(None).with_truncation(None) {
    //     Ok(t) => t,
    //     Err(e) => return Err(Error::ErrorWithMessage(format!("{}", &e))),
    // };
    let tokens = match t.encode(s, true) {
        Ok(t) => t.get_ids().to_vec(),
        Err(e) => return Err(Error::WithMessage(format!("{}", &e))),
    };
    let token_ids = Tensor::new(&tokens[..], &m.device)?.unsqueeze(0)?;
    let token_type_ids = token_ids.zeros_like()?;
    // following attention_mask parameter is needed when batch inputs are of different token length
    // let attention_mask = tokens
    //     .iter()
    //     .map(|tokens| {
    //         let tokens = tokens.get_attention_mask().to_vec();
    //         Ok(Tensor::new(tokens.as_slice(), device)?)
    //     })
    //     .collect::<Result<Vec<_>>>()?;
    let outputs = m.forward(&token_ids, &token_type_ids, None)?;
    let (_n_sentence, n_tokens, _hidden_size) = outputs.dims3()?;
    let embeddings = (outputs.sum(1)? / (n_tokens as f64))?;
    let embeddings = embeddings.broadcast_div(&embeddings.sqr()?.sum_keepdim(1)?.sqrt()?)?;
    let r = embeddings.i(0)?.to_vec1::<f32>()?;
    Ok(r)
}

// fn tt() {
//     let prs = vec![0.1f32,0.1f32,0.1f32,0.1f32,];
//     let mut top: Vec<_> = prs.iter().enumerate().collect();
//     top.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
//     let top = top.into_iter().take(5).collect::<Vec<_>>();

//     for &(i, p) in &top {
//         println!(
//             "{:50}: {:.2}%",
//             i,
//             p * 100.0
//         );
//     }
// }

async fn open_ai(
    m: &str,
    s: &str,
    api_key: &str,
    connect_timeout_millis: u32,
    read_timeout_millis: u32,
    proxy_url: &str,
) -> Result<Vec<f32>> {
    let client = crate::external::http::get_client(
        connect_timeout_millis.into(),
        read_timeout_millis.into(),
        proxy_url,
    )?;
    let mut map = Map::new();
    map.insert(String::from("input"), Value::String(String::from(s)));
    map.insert(String::from("model"), Value::String(String::from(m)));
    let obj = Value::Object(map);
    let authorization = format!("Bearer {}", api_key);
    let req = client
        .post("https://api.openai.com/v1/embeddings")
        .header("Content-Type", "application/json")
        .header("Authorization", &authorization)
        .body(serde_json::to_string(&obj)?);
    let r = req
        // .timeout(Duration::from_millis(60000))
        .send()
        .await?
        .text()
        .await?;
    let v: Value = serde_json::from_str(&r)?;
    let mut embedding_result: Vec<f32> = Vec::with_capacity(3072);
    if let Some(d) = v["data"].as_array() {
        for item in d.iter() {
            if let Some(embedding) = item["embedding"].as_array() {
                for e in embedding.iter() {
                    if let Some(n) = e.as_number() {
                        if let Some(num) = n.as_f64() {
                            let s = format!("{:.9}", num);
                            embedding_result.push(s.parse::<f32>()?);
                        }
                    }
                }
            }
        }
    }
    Ok(embedding_result)
}

async fn ollama(
    u: &str,
    m: &str,
    s: &str,
    connect_timeout_millis: u32,
    read_timeout_millis: u32,
    proxy_url: &str,
) -> Result<Vec<f32>> {
    let client = crate::external::http::get_client(
        connect_timeout_millis.into(),
        read_timeout_millis.into(),
        proxy_url,
    )?;
    let mut map = Map::new();
    map.insert(String::from("prompt"), Value::String(String::from(s)));
    map.insert(String::from("model"), Value::String(String::from(m)));
    let obj = Value::Object(map);
    let body = serde_json::to_string(&obj)?;
    // log::info!("Url {} Body {}", &u, &body);
    let req = client
        .post(u)
        .header("Content-Type", "application/json")
        .body(body);
    let r = req.send().await?.text().await?;
    if r.len() < 10 {
        // log::info!("Response {}",&r);
        return Err(Error::WithMessage(String::from("Invalid Ollama response.")));
    }
    // log::info!("Ollama embedding result {}", &r[0..50]);
    // log::info!("Ollama embedding result {}", &r);
    let v: Value = serde_json::from_str(&r)?;
    let mut embedding_result: Vec<f32> = Vec::with_capacity(3072);
    if let Some(embedding) = v["embedding"].as_array() {
        for e in embedding.iter() {
            if let Some(n) = e.as_number() {
                if let Some(num) = n.as_f64() {
                    // let s = format!("{:.9}", num);
                    // embedding_result.push(s.parse::<f32>()?);
                    embedding_result.push(num as f32);
                }
            }
        }
    }
    // log::info!(
    //     "Ollama embedding result {:?} {:?}",
    //     embedding_result.get(0),
    //     embedding_result.get(1)
    // );
    Ok(embedding_result)
}
