use core::time::Duration;
use std::fs::OpenOptions as StdOpenOptions;
use std::io::Read;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::vec::Vec;

use candle::{DType, Device};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use candle_transformers::models::gemma::{Config as GemmaConfig, Model as GemmaModel};
use candle_transformers::models::llama::{Cache as LlamaCache, Llama, LlamaConfig, LlamaEosToks};
use candle_transformers::models::parler_tts::{Config as ParlerTtsConfig, Model as ParlerTtsModel};
use candle_transformers::models::phi3::{Config as Phi3Config, Model as Phi3};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use tokenizers::{AddedToken, PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use crate::result::{Error, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum HuggingFaceModel {
    AllMiniLML6V2,
    ParaphraseMLMiniLML12V2,
    ParaphraseMLMpnetBaseV2,
    BgeSmallEnV1_5,
    BgeBaseEnV1_5,
    BgeLargeEnV1_5,
    BgeM3,
    NomicEmbedTextV1_5,
    MultilingualE5Small,
    MultilingualE5Base,
    MultilingualE5Large,
    MxbaiEmbedLargeV1,
    Phi3Mini4kInstruct,
    TinyLlama1_1bChatV1_0,
    Gemma2bInstruct,
    Gemma7bInstruct,
    ParlerTtsMiniV1,
    ParlerTtsLargeV1,
    WhisperLargeV3,
}

pub(crate) enum LoadedHuggingFaceModel {
    Bert((BertModel, Tokenizer)),
    Llama((Device, Llama, LlamaCache, Tokenizer, Option<LlamaEosToks>)),
    Gemma((Device, GemmaModel, Tokenizer)),
    Phi3((Device, Phi3, Tokenizer)),
}

impl LoadedHuggingFaceModel {
    pub(super) fn load(m: &HuggingFaceModel) -> Result<LoadedHuggingFaceModel> {
        let info = m.get_info();
        let m = match info.model_type {
            HuggingFaceModelType::Llama => {
                LoadedHuggingFaceModel::Llama(load_llama_model_files(&info)?)
            }
            HuggingFaceModelType::Gemma => {
                LoadedHuggingFaceModel::Gemma(load_gemma_model_files(&info)?)
            }
            HuggingFaceModelType::Phi3 => {
                LoadedHuggingFaceModel::Phi3(load_phi3_model_files(&info)?)
            }
            HuggingFaceModelType::Bert => {
                LoadedHuggingFaceModel::Bert(load_bert_model_files(info.repository)?)
            }
        };
        Ok(m)
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) enum HuggingFaceModelType {
    Bert,
    Llama,
    Gemma,
    Phi3,
}

// enum LoadedHfModel {
//     Bert(BertModel, Tokenizer),
//     Phi3(Phi3),
// }

pub(crate) struct HuggingFaceModelInfo {
    pub(super) model_type: HuggingFaceModelType,
    pub(crate) repository: &'static str,
    mirror: &'static str,
    model_files: Vec<&'static str>,
    model_index_file: &'static str,
    tokenizer_filename: &'static str,
    dimenssions: u32,
}

impl HuggingFaceModelInfo {
    pub(super) fn convert_prompt(
        &self,
        s: &str,
        history: Option<Vec<crate::ai::completion::Prompt>>,
    ) -> Result<String> {
        let mut system = String::new();
        let mut user = String::new();
        if !s.is_empty() && s.starts_with("[") {
            let mut prompts: Vec<super::completion::Prompt> = serde_json::from_str(s)?;
            for p in prompts.iter_mut() {
                if p.role.eq("system") {
                    std::mem::swap(&mut system, &mut p.content);
                } else if p.role.eq("user") {
                    std::mem::swap(&mut user, &mut p.content);
                }
            }
        }
        match self.model_type {
            HuggingFaceModelType::Bert => {
                let m = String::from("Bert model doesn't support prompt.");
                log::warn!("{}", &m);
                Err(Error::WithMessage(m))
            }
            HuggingFaceModelType::Llama => {
                let mut p = String::with_capacity(s.len());
                if !system.is_empty() {
                    p.push_str("<|system|>\n");
                    p.push_str(&system);
                    p.push_str("</s>\n");
                }
                if let Some(h) = history {
                    for i in h.iter() {
                        if i.content.is_empty() {
                            continue;
                        }
                        p.push_str("<|");
                        p.push_str(&i.role);
                        p.push_str("|>\n");
                        p.push_str(&i.content);
                        p.push_str("</s>\n");
                    }
                }
                if !user.is_empty() {
                    p.push_str("<|user|>\n");
                    p.push_str(&user);
                    p.push_str("</s>\n");
                }
                p.push_str("<|assistant|>");
                // p.push_str("<|begin_of_text|>");
                // if !system.is_empty() {
                //     p.push_str("<|start_header_id|>system<|end_header_id|>");
                //     p.push_str(&system);
                //     p.push_str("<|eot_id|>");
                // }
                // p.push_str("<|start_header_id|>user<|end_header_id|>");
                // p.push_str(&user);
                // p.push_str("<|eot_id|><|start_header_id|>assistant<|end_header_id|><|eot_id|>");
                Ok(p)
            }
            HuggingFaceModelType::Gemma => {
                let mut p = String::with_capacity(s.len());
                // p.push_str("<bos>");
                if let Some(h) = history {
                    for i in h.iter() {
                        p.push_str("<start_of_turn>");
                        if i.role.eq("assistant") {
                            p.push_str("model");
                        } else {
                            p.push_str(&i.role);
                        }
                        p.push('\n');
                        p.push_str(&i.content);
                        p.push_str("<end_of_turn>\n");
                    }
                }
                p.push_str("<start_of_turn>user\n");
                p.push_str(&user);
                p.push_str("<end_of_turn>\n<start_of_turn>model");
                Ok(p)
            }
            HuggingFaceModelType::Phi3 => {
                let mut p = String::with_capacity(s.len());
                p.push_str("<s>");
                if !system.is_empty() {
                    p.push_str("<|system|>\n");
                    p.push_str(&system);
                    p.push_str("<|end|>\n");
                }
                if let Some(h) = history {
                    for i in h.iter() {
                        p.push_str("<|");
                        p.push_str(&i.role);
                        p.push_str("|>\n");
                        p.push_str(&i.content);
                        p.push_str("<|end|>\n");
                    }
                }
                p.push_str("<|user|>\n");
                p.push_str(&user);
                p.push_str("<|end|>\n<|assistant|>");
                Ok(p)
            }
        }
    }
}

fn get_common_model_files() -> Vec<&'static str> {
    vec![
        "model.safetensors",
        "tokenizer.json",
        "config.json",
        "special_tokens_map.json",
        "tokenizer_config.json",
    ]
}

impl HuggingFaceModel {
    pub(crate) fn get_info(&self) -> HuggingFaceModelInfo {
        match self {
            HuggingFaceModel::AllMiniLML6V2 => HuggingFaceModelInfo {
                repository: "sentence-transformers/all-MiniLM-L6-v2",
                mirror: "sentence-transformers/all-MiniLM-L6-v2",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 384,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::ParaphraseMLMiniLML12V2 => HuggingFaceModelInfo {
                repository: "sentence-transformers/paraphrase-MiniLM-L12-v2",
                mirror: "sentence-transformers/paraphrase-MiniLM-L12-v2",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 384,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::ParaphraseMLMpnetBaseV2 => HuggingFaceModelInfo {
                repository: "sentence-transformers/paraphrase-multilingual-mpnet-base-v2",
                mirror: "sentence-transformers/paraphrase-multilingual-mpnet-base-v2",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 768,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::BgeSmallEnV1_5 => HuggingFaceModelInfo {
                repository: "BAAI/bge-small-en-v1.5",
                mirror: "BAAI/bge-small-en-v1.5",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 384,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::BgeBaseEnV1_5 => HuggingFaceModelInfo {
                repository: "BAAI/bge-base-en-v1.5",
                mirror: "BAAI/bge-base-en-v1.5",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 768,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::BgeLargeEnV1_5 => HuggingFaceModelInfo {
                repository: "BAAI/bge-large-en-v1.5",
                mirror: "BAAI/bge-large-en-v1.5",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::BgeM3 => HuggingFaceModelInfo {
                repository: "BAAI/bge-m3",
                mirror: "BAAI/bge-m3",
                model_files: vec!["onnx/model.onnx", "onnx/model.onnx_data"],
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::NomicEmbedTextV1_5 => HuggingFaceModelInfo {
                repository: "nomic-ai/nomic-embed-text-v1.5",
                mirror: "nomic-ai/nomic-embed-text-v1.5",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 768,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::MultilingualE5Small => HuggingFaceModelInfo {
                repository: "intfloat/multilingual-e5-small",
                mirror: "intfloat/multilingual-e5-small",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 384,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::MultilingualE5Base => HuggingFaceModelInfo {
                repository: "intfloat/multilingual-e5-base",
                mirror: "intfloat/multilingual-e5-base",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 768,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::MultilingualE5Large => HuggingFaceModelInfo {
                repository: "intfloat/multilingual-e5-large",
                mirror: "intfloat/multilingual-e5-large",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::MxbaiEmbedLargeV1 => HuggingFaceModelInfo {
                repository: "mixedbread-ai/mxbai-embed-large-v1",
                mirror: "mixedbread-ai/mxbai-embed-large-v1",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::Phi3Mini4kInstruct => HuggingFaceModelInfo {
                repository: "microsoft/Phi-3-mini-4k-instruct",
                mirror: "microsoft/Phi-3-mini-4k-instruct",
                model_files: {
                    let mut v = get_common_model_files();
                    let mut idx = 0usize;
                    for &f in v.iter() {
                        if f.eq("model.safetensors") {
                            break;
                        }
                        idx += 1;
                    }
                    v.remove(idx);
                    v
                },
                model_index_file: "model.safetensors.index.json",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                model_type: HuggingFaceModelType::Phi3,
            },
            HuggingFaceModel::TinyLlama1_1bChatV1_0 => HuggingFaceModelInfo {
                repository: "TinyLlama/TinyLlama-1.1B-Chat-v1.0",
                mirror: "TinyLlama/TinyLlama-1.1B-Chat-v1.0",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                model_type: HuggingFaceModelType::Llama,
            },
            HuggingFaceModel::Gemma2bInstruct => HuggingFaceModelInfo {
                repository: "google/gemma-2b-it",
                mirror: "google/gemma-2b-it",
                model_files: {
                    let mut v = get_common_model_files();
                    let mut idx = 0usize;
                    for &f in v.iter() {
                        if f.eq("model.safetensors") {
                            break;
                        }
                        idx += 1;
                    }
                    v.remove(idx);
                    v
                },
                model_index_file: "model.safetensors.index.json",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                model_type: HuggingFaceModelType::Gemma,
            },
            HuggingFaceModel::Gemma7bInstruct => HuggingFaceModelInfo {
                repository: "google/gemma-7b-it",
                mirror: "google/gemma-7b-it",
                model_files: {
                    let mut v = get_common_model_files();
                    let mut idx = 0usize;
                    for &f in v.iter() {
                        if f.eq("model.safetensors") {
                            break;
                        }
                        idx += 1;
                    }
                    v.remove(idx);
                    v
                },
                model_index_file: "model.safetensors.index.json",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                model_type: HuggingFaceModelType::Gemma,
            },
            HuggingFaceModel::ParlerTtsMiniV1 => HuggingFaceModelInfo {
                repository: "parler-tts/parler-tts-mini-v1",
                mirror: "parler-tts/parler-tts-mini-v1",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                model_type: HuggingFaceModelType::Llama,
            },
            HuggingFaceModel::ParlerTtsLargeV1 => HuggingFaceModelInfo {
                repository: "parler-tts/parler-tts-large-v1",
                mirror: "parler-tts/parler-tts-large-v1",
                model_files: {
                    let mut v = get_common_model_files();
                    let mut idx = 0usize;
                    for &f in v.iter() {
                        if f.eq("model.safetensors") {
                            break;
                        }
                        idx += 1;
                    }
                    v.remove(idx);
                    v
                },
                model_index_file: "model.safetensors.index.json",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                model_type: HuggingFaceModelType::Gemma,
            },
            HuggingFaceModel::WhisperLargeV3 => todo!(),
        }
    }
}

impl std::fmt::Display for HuggingFaceModel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
        // or, alternatively:
        // fmt::Debug::fmt(self, f)
    }
}

const HUGGING_FACE_MODEL_ROOT: &str = "./data/hf_hub/";

#[derive(Clone, Serialize)]
pub(crate) struct DownloadStatus {
    pub(crate) downloading: bool,
    #[serde(rename = "totalLen")]
    pub(crate) total_len: u64,
    #[serde(rename = "downloadedLen")]
    pub(crate) downloaded_len: u64,
    pub(crate) url: String,
    pub(crate) err: String,
}

pub(crate) static DOWNLOAD_STATUS: OnceLock<Mutex<DownloadStatus>> = OnceLock::new();

pub(crate) fn get_download_status() -> Option<DownloadStatus> {
    if let Some(op) = DOWNLOAD_STATUS.get() {
        return match op.lock() {
            Ok(s) => Some(s.clone()),
            Err(e) => {
                log::error!("{:?}", &e);
                None
            }
        };
    }
    None
}

fn download_status<'l>() -> Result<std::sync::MutexGuard<'l, DownloadStatus>> {
    let locker = match DOWNLOAD_STATUS
        .get_or_init(|| {
            Mutex::new(DownloadStatus {
                downloading: false,
                total_len: 1,
                downloaded_len: 0,
                url: String::new(),
                err: String::new(),
            })
        })
        .try_lock()
    {
        Ok(l) => l,
        Err(e) => match e {
            std::sync::TryLockError::Poisoned(pe) => {
                log::warn!("{:?}", &pe);
                pe.into_inner()
            }
            std::sync::TryLockError::WouldBlock => {
                return Err(Error::WithMessage(String::from(
                    "Model files are downloading.",
                )));
            }
        },
    };
    Ok(locker)
}

pub(crate) async fn download_hf_models(
    info: &HuggingFaceModelInfo,
    huggingface_token: &str,
    connect_timeout: u64,
    read_timeout: u64,
) -> Result<()> {
    // if let Ok(v) = DOWNLOAD_STATUS
    //     .get_or_init(|| {
    //         Mutex::new(DownloadStatus {
    //             downloading: false,
    //             total_len: 1,
    //             downloaded_len: 0,
    //             url: String::new(),
    //         })
    //     })
    //     .lock()
    // {
    //     if v.downloading {
    //         return Err(Error::ErrorWithMessage(String::from(
    //             "Model files are downloading.",
    //         )));
    //     }
    // }
    {
        let mut status = download_status()?;
        if status.downloading {
            return Err(Error::WithMessage(String::from(
                "Model files are downloading.",
            )));
        }
        status.downloading = true;
    }
    let root_path = format!("{}{}", HUGGING_FACE_MODEL_ROOT, info.repository);
    tokio::fs::create_dir_all(&root_path).await?;

    let mut headers = HeaderMap::new();
    let user_agent = format!(
        "unkown/None; dialogflowai/{}; rust/unknown",
        crate::web::server::VERSION
    );
    headers.insert("User-Agent", HeaderValue::from_str(&user_agent)?);
    if !huggingface_token.is_empty() {
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {huggingface_token}"))?,
        );
    }

    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(connect_timeout))
        .read_timeout(Duration::from_millis(read_timeout))
        .default_headers(headers);
    if let Ok(proxy) = std::env::var("https_proxy") {
        if !proxy.is_empty() {
            log::info!("Detected proxy setting: {}", &proxy);
            builder = builder.proxy(reqwest::Proxy::https(&proxy)?)
        }
    }
    let client = builder.build()?;
    let mut files = info
        .model_files
        .iter()
        .map(|&s| String::from(s))
        .collect::<Vec<_>>();
    let mut r: Result<_> = Ok(());
    if !info.model_index_file.is_empty() {
        let model_index_file = construct_model_file_path(info.mirror, info.model_index_file);
        let path = std::path::Path::new(&model_index_file);
        if !path.exists() {
            r = download_hf_file(&client, info, &root_path, info.model_index_file).await;
        }
        if r.is_ok() {
            let f = load_safetensors(info.mirror, info.model_index_file)?;
            files.extend_from_slice(&f);
        }
    };
    if r.is_ok() {
        for f in files.iter() {
            r = download_hf_file(&client, info, &root_path, f).await;
            if r.is_err() {
                break;
            }
        }
    }
    {
        let mut status = download_status()?;
        if r.is_err() {
            status.err = format!("Download failed,err: {:?}", r.as_ref().err());
        }
        status.downloading = false;
    }

    r
}

async fn download_hf_file(
    client: &reqwest::Client,
    info: &HuggingFaceModelInfo,
    root_path: &str,
    f: &str,
) -> Result<()> {
    let file_path_str = format!("{}/{}", root_path, f);
    let file_path = std::path::Path::new(&file_path_str);
    if tokio::fs::try_exists(file_path).await? {
        return Ok(());
    }
    let u = format!("https://huggingface.co/{}/resolve/main/{}", &info.mirror, f);
    if let Some(s) = DOWNLOAD_STATUS.get() {
        if let Ok(mut v) = s.lock() {
            v.url = String::from(f);
        }
    }
    let res = client.get(&u).query(&[("download", "true")]).send().await?;
    let total_size = res.content_length().unwrap();
    // println!("Downloading {f}, total size {total_size}");
    if let Some(s) = DOWNLOAD_STATUS.get() {
        if let Ok(mut v) = s.lock() {
            v.total_len = total_size;
        }
    }
    // let b = res.bytes().await?;
    // fs::write("./temp.file", b.as_ref()).await?;
    // let mut downloaded = 0u64;
    let mut stream = res.bytes_stream();
    let mut file = OpenOptions::new()
        .read(false)
        .write(true)
        .truncate(false)
        .create_new(true)
        .open(file_path)
        .await?;
    // let mut file = File::create("./temp.file").await?;

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk).await?;
        if let Some(s) = DOWNLOAD_STATUS.get() {
            if let Ok(mut v) = s.lock() {
                let new = std::cmp::min(v.downloaded_len + (chunk.len() as u64), total_size);
                // log::info!("Downloaded {new}");
                v.downloaded_len = new;
            }
        }
    }
    Ok(())
}

pub(super) fn construct_model_file_path(mirror: &str, f: &str) -> String {
    format!("{}{}/{}", HUGGING_FACE_MODEL_ROOT, mirror, f)
}

pub(super) fn device() -> Result<Device> {
    if candle::utils::cuda_is_available() {
        Ok(Device::new_cuda(0)?)
    } else if candle::utils::metal_is_available() {
        Ok(Device::new_metal(0)?)
    } else {
        Ok(Device::Cpu)
    }
}

// type TokenizerImpl = tokenizers::TokenizerImpl<
//     tokenizers::ModelWrapper,
//     tokenizers::NormalizerWrapper,
//     tokenizers::PreTokenizerWrapper,
//     tokenizers::PostProcessorWrapper,
//     tokenizers::DecoderWrapper,
// >;

pub(crate) fn check_model_files(info: &HuggingFaceModelInfo) -> Result<()> {
    // let f = construct_model_file_path(repo, "config.json");
    // let config = std::fs::read(&f)?;
    // let config: serde_json::Value = serde_json::from_slice(&config)?;
    // let arch = &config["architectures"];
    // if !arch.is_array() {
    //     return Ok(false)
    // }
    // let architectures=arch.as_array().unwrap();
    // if architectures.len() <1{
    //     return Ok(false)
    // }
    // let arch=architectures.get(0).unwrap();
    // if !arch.is_string() {
    //     return Ok(false)
    // }
    // if arch.as_str().unwrap().starts_with("Bert") {
    let files = get_model_files(info)?;
    for f in files.iter() {
        let p = Path::new(f);
        if !p.exists() {
            return Err(Error::WithMessage(format!("Path {:?} is not exist.", p)));
        }
        let ext = p.extension();
        if ext.is_none() {
            return Err(Error::WithMessage(format!(
                "{:?} doesn't have extension.",
                p
            )));
        }
        let ext = ext.unwrap();
        if ext.eq("json") {
            // https://github.com/serde-rs/json/issues/160
            // let file = StdOpenOptions::new().read(true).write(false).create(false).open(&f)?;
            // let br = std::io::BufReader::with_capacity(4096, file);
            // let _ = serde_json::from_reader(br)?;
            let mut file = StdOpenOptions::new()
                .read(true)
                .write(false)
                .create(false)
                .open(f)?;
            let mut bytes = Vec::with_capacity(4096);
            file.read_to_end(&mut bytes)?;
            let _: serde::de::IgnoredAny = serde_json::from_slice(&bytes)?;
        } else if ext.eq("safetensors") {
            let metadata = std::fs::metadata(p)?;
            if metadata.len() < 62914560u64 {
                return Err(Error::WithMessage(format!(
                    "{:?} file size is too small.",
                    p
                )));
            }
        }
    }
    Ok(())
    // match info.model_type {
    //     HuggingFaceModelType::Bert => load_bert_model_files(&info.repository)
    //         .map(|_| true)
    //         .or_else(|e| {
    //             log::warn!("Check bert model files failed,err: {:?}", &e);
    //             Ok(false)
    //         }),
    //     HuggingFaceModelType::Gemma => {
    //         let device = device()?;
    //         load_gemma_model_files(&info, &device)
    //             .map(|_| true)
    //             .or_else(|e| {
    //                 log::warn!("Check gemma model files failed,err: {:?}", &e);
    //                 Ok(false)
    //             })
    //     }
    //     HuggingFaceModelType::Llama => {
    //         let device = device()?;
    //         load_llama_model_files(&info, &device)
    //             .map(|_| true)
    //             .or_else(|e| {
    //                 log::warn!("Check llama model files failed,err: {:?}", &e);
    //                 Ok(false)
    //             })
    //     }
    //     HuggingFaceModelType::Phi3 => {
    //         let device = device()?;
    //         load_phi3_model_files(&info, &device)
    //             .map(|_| true)
    //             .or_else(|e| {
    //                 log::warn!("Check phi3 model files failed,err: {:?}", &e);
    //                 Ok(false)
    //             })
    //     }
    // }
}

fn init_tokenizer(repo: &str) -> Result<Tokenizer> {
    let f = construct_model_file_path(repo, "tokenizer.json");
    match Tokenizer::from_file(&f) {
        Ok(t) => Ok(t),
        Err(e) => Err(Error::WithMessage(format!("{}", &e))),
    }
}

fn set_tokenizer_config(
    mirror: &str,
    mut tokenizer: Tokenizer,
    pad_token_id: u32,
) -> Result<Tokenizer> {
    let f = construct_model_file_path(mirror, "tokenizer_config.json");
    let p = std::path::Path::new(&f);
    let t = if p.exists() {
        let j: serde_json::Value = serde_json::from_slice(std::fs::read(&f)?.as_slice())?;
        let model_max_length = j["model_max_length"]
            .as_f64()
            .expect("Error reading model_max_length from tokenizer_config.json")
            as f32;
        let max_length = 8192.min(model_max_length as usize);
        let pad_token = j["pad_token"]
            .as_str()
            .expect("Error reading pad_token from tokenier_config.json")
            .into();
        // log::info!("p1 {}", tokenizer.get_padding().unwrap().pad_token);
        // log::info!("t1 {}", tokenizer.get_truncation().unwrap().max_length);
        tokenizer
            .with_padding(Some(PaddingParams {
                strategy: PaddingStrategy::BatchLongest,
                pad_token,
                pad_id: pad_token_id,
                ..Default::default()
            }))
            .with_truncation(Some(TruncationParams {
                max_length,
                ..Default::default()
            }))
    } else {
        tokenizer.with_padding(None).with_truncation(None)
    };
    let t = match t {
        Ok(t) => t.clone().into(),
        Err(e) => {
            log::warn!("{:?}", &e);
            tokenizer
        }
    };

    Ok(t)
    // log::info!("p2 {}", tokenizer.get_padding().unwrap().pad_token);
    // log::info!("t2 {}", tokenizer.get_truncation().unwrap().max_length);
}

fn set_special_tokens_map(mirror: &str, tokenizer: &mut Tokenizer) -> Result<()> {
    let f = construct_model_file_path(mirror, "special_tokens_map.json");
    let p = std::path::Path::new(&f);
    if !p.exists() {
        return Ok(());
    }
    if let serde_json::Value::Object(root_object) =
        serde_json::from_slice(std::fs::read(&f)?.as_slice())?
    {
        for (_, value) in root_object.iter() {
            if value.is_string() {
                tokenizer.add_special_tokens(&[AddedToken {
                    content: value.as_str().unwrap().into(),
                    special: true,
                    ..Default::default()
                }]);
            } else if value.is_object() {
                tokenizer.add_special_tokens(&[AddedToken {
                    content: value["content"].as_str().unwrap().into(),
                    special: true,
                    single_word: value["single_word"].as_bool().unwrap(),
                    lstrip: value["lstrip"].as_bool().unwrap(),
                    rstrip: value["rstrip"].as_bool().unwrap(),
                    normalized: value["normalized"].as_bool().unwrap(),
                }]);
            }
        }
    }
    Ok(())
}

pub(crate) fn load_bert_model_files(mirror: &str) -> Result<(BertModel, Tokenizer)> {
    let f = construct_model_file_path(mirror, "config.json");
    let config = std::fs::read_to_string(&f)?;
    let config: serde_json::Value = serde_json::from_str(&config)?;
    let pad_token_id = config["pad_token_id"].as_u64().unwrap_or(0) as u32;
    let config: Config = serde_json::from_value(config)?;
    let tokenizer = init_tokenizer(mirror)?;
    let mut tokenizer = set_tokenizer_config(mirror, tokenizer, pad_token_id)?;
    set_special_tokens_map(mirror, &mut tokenizer)?;
    let f = construct_model_file_path(mirror, "model.safetensors");
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[&f], DTYPE, &device()?)? };
    let model = BertModel::load(vb, &config)?;
    Ok((model, tokenizer))
}

fn load_safetensors(mirror: &str, json_file: &str) -> Result<Vec<String>> {
    let json_file = construct_model_file_path(mirror, json_file);
    let json_file = std::fs::File::open(json_file)?;
    let json: serde_json::Value =
        serde_json::from_reader(&json_file).map_err(candle::Error::wrap)?;
    let weight_map = match json.get("weight_map") {
        None => {
            return Err(Error::WithMessage(format!(
                "no weight map in {json_file:?}"
            )));
        }
        Some(serde_json::Value::Object(map)) => map,
        Some(_) => {
            return Err(Error::WithMessage(format!(
                "weight map in {json_file:?} is not a map"
            )));
        }
    };
    let mut safetensors_files = std::collections::HashSet::new();
    for value in weight_map.values() {
        if let Some(file) = value.as_str() {
            safetensors_files.insert(file.to_string());
        }
    }
    Ok(Vec::from_iter(safetensors_files))
}

pub(crate) fn load_phi3_model_files(
    info: &HuggingFaceModelInfo,
) -> Result<(Device, Phi3, Tokenizer)> {
    let device = device()?;
    let dtype = if device.is_cuda() {
        DType::BF16
    } else {
        DType::F32
    };
    let filenames = load_safetensors(info.mirror, info.model_index_file)?
        .iter()
        .map(|v| std::path::PathBuf::from(construct_model_file_path(info.mirror, v)))
        .collect::<Vec<_>>();
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&filenames, dtype, &device)? };
    let config_filename = construct_model_file_path(info.mirror, "config.json");
    let config = std::fs::read_to_string(config_filename)?;
    let config: Phi3Config = serde_json::from_str(&config)?;
    let phi3 = Phi3::new(&config, vb)?;
    let tokenizer = init_tokenizer(info.repository)?;
    Ok((device, phi3, tokenizer))
}

fn get_model_files(info: &HuggingFaceModelInfo) -> Result<Vec<String>> {
    let f = if info.model_index_file.is_empty() {
        vec![construct_model_file_path(info.mirror, "model.safetensors")]
    } else {
        load_safetensors(info.repository, info.model_index_file)?
            .iter()
            .map(|v| construct_model_file_path(info.mirror, v))
            .collect::<Vec<_>>()
    };
    Ok(f)
}

pub(crate) fn load_llama_model_files(
    info: &HuggingFaceModelInfo,
) -> Result<(Device, Llama, LlamaCache, Tokenizer, Option<LlamaEosToks>)> {
    log::info!("load_llama_model_files start");
    let tokenizer = init_tokenizer(info.repository)?;
    let device = device()?;

    let config_filename = construct_model_file_path(info.repository, "config.json");
    let config: LlamaConfig = serde_json::from_slice(&std::fs::read(config_filename)?)?;
    let config = config.into_config(device.is_cuda());
    let dtype = DType::F16;
    let cache = LlamaCache::new(true, dtype, &config, &device)?;
    let filenames = get_model_files(info)?;
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&filenames, dtype, &device)? };
    let m = Llama::load(vb, &config)?;
    let eos_token_id = config
        .eos_token_id
        .or_else(|| tokenizer.token_to_id("</s>").map(LlamaEosToks::Single));
    log::info!("load_llama_model_files end");
    Ok((device, m, cache, tokenizer, eos_token_id))
}

pub(crate) fn load_gemma_model_files(
    info: &HuggingFaceModelInfo,
) -> Result<(Device, GemmaModel, Tokenizer)> {
    let tokenizer = init_tokenizer(info.repository)?;
    let device = device()?;

    let config_filename = construct_model_file_path(info.repository, "config.json");
    let config: GemmaConfig = serde_json::from_reader(std::fs::File::open(config_filename)?)?;
    let dtype = if device.is_cuda() {
        DType::BF16
    } else {
        DType::F32
    };
    let filenames = get_model_files(info)?;
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&filenames, dtype, &device)? };
    let model = GemmaModel::new(device.is_cuda(), &config, vb)?;
    Ok((device, model, tokenizer))
}

pub(crate) fn load_parler_tts_model_files(
    info: &HuggingFaceModelInfo,
) -> Result<(Device, ParlerTtsModel, Tokenizer)> {
    let tokenizer = init_tokenizer(info.repository)?;
    let device = device()?;
    let filenames = get_model_files(info)?;
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&filenames, DType::F32, &device)? };
    let config_filename = construct_model_file_path(info.repository, "config.json");
    let config: ParlerTtsConfig = serde_json::from_reader(std::fs::File::open(config_filename)?)?;
    let model = ParlerTtsModel::new(&config, vb)?;
    Ok((device, model, tokenizer))
}

pub(crate) fn load_pytorch_mode_files(info: &HuggingFaceModelInfo, device: &Device) -> Result<()> {
    let weights_filename = construct_model_file_path(info.repository, "pytorch_model.bin");
    let vb = VarBuilder::from_pth(&weights_filename, DType::BF16, device)?;
    Ok(())
}
