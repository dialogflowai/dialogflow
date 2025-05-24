use std::collections::HashMap;
use std::default::Default;
use std::net::SocketAddr;
use std::sync::{LazyLock, Mutex};

use axum::Json;
use axum::body::Bytes;
use axum::extract::Query;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ai::huggingface::HuggingFaceModel;
use crate::ai::{asr, chat, completion, embedding, huggingface, tts};
use crate::db;
use crate::result::{Error, Result};
use crate::robot::dto::RobotQuery;
use crate::web::server::{self, to_res};

pub(crate) const TABLE: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("settings");
pub(crate) const SETTINGS_KEY: &str = "global-settings";

static SETTINGS_CACHE: LazyLock<Mutex<HashMap<String, Settings>>> =
    LazyLock::new(|| Mutex::new(HashMap::with_capacity(32)));

#[derive(Deserialize, Serialize)]
pub(crate) struct HfModelDownload {
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "accessToken")]
    pub(crate) access_token: String,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct GlobalSettings {
    pub(crate) ip: String,
    pub(crate) port: u16,
    #[serde(rename = "selectRandomPortWhenConflict")]
    pub(crate) select_random_port_when_conflict: bool,
    #[serde(rename = "hfModelDownload")]
    pub(crate) hf_model_download: HfModelDownload,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Settings {
    settings_version: u8,
    #[serde(rename = "maxSessionIdleSec")]
    pub(crate) max_session_idle_sec: u32,
    #[serde(rename = "chatProvider")]
    pub(crate) chat_provider: ChatProvider,
    #[serde(rename = "textGenerationProvider")]
    pub(crate) text_generation_provider: TextGenerationProvider,
    #[serde(rename = "sentenceEmbeddingProvider")]
    pub(crate) sentence_embedding_provider: SentenceEmbeddingProvider,
    #[serde(rename = "asrProvider")]
    pub(crate) asr_provider: AsrProvider,
    #[serde(rename = "ttsProvider")]
    pub(crate) tts_provider: TtsProvider,
    #[serde(rename = "smtpHost")]
    pub(crate) smtp_host: String,
    #[serde(rename = "smtpUsername")]
    pub(crate) smtp_username: String,
    #[serde(rename = "smtpPassword")]
    pub(crate) smtp_password: String,
    #[serde(rename = "smtpTimeoutSec")]
    pub(crate) smtp_timeout_sec: u16,
    #[serde(rename = "emailVerificationRegex")]
    pub(crate) email_verification_regex: String,
}

// #[test]
// fn deser() {
//     let s = Settings {
//         ip: String::new(),
//         port: 12715,
//         max_session_duration_min: 60,
//         embedding_provider: EmbeddingProvider {
//             provider: crate::intent::embedding::EmbeddingProvider::HuggingFace(
//                 crate::intent::embedding::HuggingFaceModel::AllMiniLML6V2,
//             ),
//             api_url: String::new(),
//             api_key: String::new(),
//             model: String::new(),
//             connect_timeout_millis: 1000,
//             read_timeout_millis: 5000,
//         },
//         smtp_host: String::new(),
//         smtp_username: String::new(),
//         smtp_password: String::new(),
//         smtp_timeout_sec: 30,
//         email_verification_regex: String::new(),
//         select_random_port_when_conflict: false,
//     };
//     let j = serde_json::to_string(&s);
//     assert!(j.is_ok());
//     println!("{}", j.unwrap());
//     let j = "{\"ip\":\"127.0.0.1\",\"port\":12715,\"selectRandomPortWhenConflict\":false,\"maxSessionDurationMin\":30,\"smtpHost\":\"\",\"smtpUsername\":\"\",\"smtpPassword\":\"\",\"smtpTimeoutSec\":60,\"emailVerificationRegex\":\"[-\\w\\.\\+]{1,100}@[A-Za-z0-9]{1,30}[A-Za-z\\.]{2,30}\",\"embeddingProvider\":{\"provider\":\"HuggingFace\",\"apiUrl\":\"Model will be downloaded locally at ./data/models\",\"apiKey\":\"\",\"model\":\"AllMiniLML6V2\",\"apiUrlDisabled\":true,\"showApiKeyInput\":false}}";
//     let r = serde_json::from_str(j);
//     assert!(r.is_ok());
//     let v: serde_json::Value = r.unwrap();
//     assert_eq!(v["embeddingProvider"]["provider"], "HuggingFace");
// }

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct ChatProvider {
    pub(crate) provider: chat::ChatProvider,
    #[serde(rename = "apiUrl")]
    pub(crate) api_url: String,
    #[serde(rename = "apiKey")]
    pub(crate) api_key: String,
    pub(crate) model: String,
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "maxResponseTokenLength")]
    pub(crate) max_response_token_length: u32,
    #[serde(rename = "proxyUrl")]
    pub(crate) proxy_url: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct TextGenerationProvider {
    pub(crate) provider: completion::TextGenerationProvider,
    #[serde(rename = "apiUrl")]
    pub(crate) api_url: String,
    #[serde(rename = "apiKey")]
    pub(crate) api_key: String,
    pub(crate) model: String,
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "maxResponseTokenLength")]
    pub(crate) max_response_token_length: u32,
    #[serde(rename = "proxyUrl")]
    pub(crate) proxy_url: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct SentenceEmbeddingProvider {
    pub(crate) provider: embedding::SentenceEmbeddingProvider,
    #[serde(rename = "similarityThreshold")]
    pub(crate) similarity_threshold: f32,
    #[serde(rename = "apiUrl")]
    pub(crate) api_url: String,
    #[serde(rename = "apiKey")]
    pub(crate) api_key: String,
    pub(crate) model: String,
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "proxyUrl")]
    pub(crate) proxy_url: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct AsrProvider {
    pub(crate) provider: asr::AsrProvider,
    #[serde(rename = "apiUrl")]
    pub(crate) api_url: String,
    #[serde(rename = "apiKey")]
    pub(crate) api_key: String,
    pub(crate) model: String,
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "proxyUrl")]
    pub(crate) proxy_url: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct TtsProvider {
    pub(crate) provider: tts::TtsProvider,
    #[serde(rename = "apiUrl")]
    pub(crate) api_url: String,
    #[serde(rename = "apiKey")]
    pub(crate) api_key: String,
    pub(crate) model: String,
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "proxyUrl")]
    pub(crate) proxy_url: String,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        GlobalSettings {
            ip: String::from("127.0.0.1"),
            port: 12715,
            select_random_port_when_conflict: false,
            hf_model_download: HfModelDownload {
                connect_timeout_millis: 2000,
                read_timeout_millis: 10000,
                access_token: String::new(),
            },
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            settings_version: 1u8,
            max_session_idle_sec: 1800,
            chat_provider: ChatProvider {
                provider: chat::ChatProvider::HuggingFace(
                    huggingface::HuggingFaceModel::TinyLlama1_1bChatV1_0,
                ),
                api_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                connect_timeout_millis: 5000,
                read_timeout_millis: 10000,
                max_response_token_length: 1000,
                proxy_url: String::new(),
            },
            text_generation_provider: TextGenerationProvider {
                provider: completion::TextGenerationProvider::HuggingFace(
                    huggingface::HuggingFaceModel::TinyLlama1_1bChatV1_0,
                ),
                api_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                connect_timeout_millis: 5000,
                read_timeout_millis: 10000,
                max_response_token_length: 1000,
                proxy_url: String::new(),
            },
            sentence_embedding_provider: SentenceEmbeddingProvider {
                provider: embedding::SentenceEmbeddingProvider::HuggingFace(
                    huggingface::HuggingFaceModel::AllMiniLML6V2,
                ),
                similarity_threshold: 0.85f32,
                api_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                connect_timeout_millis: 5000,
                read_timeout_millis: 10000,
                proxy_url: String::new(),
            },
            asr_provider: AsrProvider {
                provider: asr::AsrProvider::HuggingFace(
                    huggingface::HuggingFaceModel::WhisperLargeV3,
                ),
                api_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                connect_timeout_millis: 5000,
                read_timeout_millis: 10000,
                proxy_url: String::new(),
            },
            tts_provider: TtsProvider {
                provider: tts::TtsProvider::HuggingFace(
                    huggingface::HuggingFaceModel::ParlerTtsMiniV1,
                ),
                api_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                connect_timeout_millis: 5000,
                read_timeout_millis: 10000,
                proxy_url: String::new(),
            },
            smtp_host: String::new(),
            smtp_username: String::new(),
            smtp_password: String::new(),
            smtp_timeout_sec: 60u16,
            email_verification_regex: String::new(),
        }
    }
}

pub(crate) fn init_table() -> Result<()> {
    db::init_table(TABLE)
}

pub(crate) fn exists() -> Result<bool> {
    let cnt = db::count(TABLE)?;
    Ok(cnt > 0)
}

pub(crate) fn init_global() -> Result<GlobalSettings> {
    let settings = GlobalSettings::default();
    db::write(TABLE, SETTINGS_KEY, &settings)?;
    let format = time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]]")
        .expect("Invalid format description");
    let t = time::OffsetDateTime::now_utc();
    let t_str = t.format(&format).map_err(Error::TimeFormat)?;
    db::write(TABLE, "db_init_time", &t_str)?;
    db::write(TABLE, "version", &String::from(server::VERSION))?;
    Ok(settings)
}

pub(crate) fn init(robot_id: &str) -> Result<Settings> {
    let settings = Settings::default();
    db::write(TABLE, robot_id, &settings)?;
    Ok(settings)
}

pub(crate) fn get_global_settings() -> Result<Option<GlobalSettings>> {
    db::query(TABLE, SETTINGS_KEY)
}

pub(crate) async fn rest_get_global_settings() -> impl IntoResponse {
    to_res(get_global_settings())
}

pub(crate) fn get_settings(robot_id: &str) -> Result<Option<Settings>> {
    let l = SETTINGS_CACHE.lock()?;
    let op = l.get(robot_id);
    if let Some(s) = op {
        return Ok(Some(s.clone()));
    }
    db::query(TABLE, robot_id)
}

pub(crate) async fn get(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    to_res::<Option<Settings>>(get_settings(&q.robot_id))
}

pub(crate) async fn save(
    Query(q): Query<RobotQuery>,
    Json(data): Json<Settings>,
) -> impl IntoResponse {
    to_res(save_settings(&q.robot_id, data))
}

pub(crate) fn save_global_settings(data: &GlobalSettings) -> Result<()> {
    let addr = format!("{}:{}", data.ip, data.port);
    let _: SocketAddr = addr.parse().map_err(|_| {
        log::error!("Saving invalid listen IP: {}", &addr);
        Error::WithMessage(String::from("lang.settings.invalidIp"))
    })?;
    db::write(TABLE, SETTINGS_KEY, &data)
}

pub(crate) async fn rest_save_global_settings(
    Json(data): Json<GlobalSettings>,
) -> impl IntoResponse {
    to_res(save_global_settings(&data))
}

pub(crate) fn save_settings(robot_id: &str, data: Settings) -> Result<()> {
    if let completion::TextGenerationProvider::HuggingFace(m) =
        &data.text_generation_provider.provider
    {
        if let Err(e) = crate::ai::chat::replace_model_cache(robot_id, m) {
            log::warn!(
                "Hugging face model files for chat were incorrect. Err: {:?}",
                &e
            );
        }
    }

    if let completion::TextGenerationProvider::HuggingFace(m) =
        &data.text_generation_provider.provider
    {
        if let Err(e) = completion::replace_model_cache(robot_id, m) {
            log::warn!(
                "Hugging face model files for completion were incorrect. Err: {:?}",
                &e
            );
        }
    }

    if let embedding::SentenceEmbeddingProvider::HuggingFace(m) =
        &data.sentence_embedding_provider.provider
    {
        match crate::ai::huggingface::load_bert_model_files(m.get_info().repository) {
            Ok(m) => embedding::replace_model_cache(robot_id, m),
            Err(e) => {
                log::warn!(
                    "Hugging face model files for sentence embedding were incorrect. Err: {:?}",
                    &e
                );
            }
        }
    }
    db::write(TABLE, robot_id, &data)?;
    let mut l = SETTINGS_CACHE.lock()?;
    l.insert(String::from(robot_id), data);
    Ok(())
}

pub(crate) async fn smtp_test(Json(settings): Json<Settings>) -> impl IntoResponse {
    to_res(check_smtp_settings(&settings))
}

pub(crate) fn check_smtp_settings(settings: &Settings) -> Result<bool> {
    use lettre::SmtpTransport;
    use lettre::transport::smtp::authentication::Credentials;
    let creds = Credentials::new(
        settings.smtp_username.to_owned(),
        settings.smtp_password.to_owned(),
    );

    let mailer = SmtpTransport::relay(&settings.smtp_host)?
        .credentials(creds)
        .timeout(Some(core::time::Duration::from_secs(
            settings.smtp_timeout_sec as u64,
        )))
        .build();

    Ok(mailer.test_connection()?)
}

pub(crate) async fn download_model_files(Json(m): Json<HuggingFaceModel>) -> impl IntoResponse {
    let global_settings = get_global_settings();
    if global_settings.is_err() {
        return to_res(Err(Error::WithMessage(String::from(
            "Load global settings failed.",
        ))));
    }
    let global_settings = global_settings.unwrap();
    if global_settings.is_none() {
        return to_res(Err(Error::WithMessage(String::from(
            "Global settings not found.",
        ))));
    }
    let global_settings = global_settings.unwrap();
    tokio::spawn(async move {
        match huggingface::download_hf_models(
            &m.get_info(),
            &global_settings.hf_model_download.access_token,
            global_settings.hf_model_download.connect_timeout_millis as u64,
            global_settings.hf_model_download.read_timeout_millis as u64,
        )
        .await
        {
            Ok(_) => log::info!("All model files download successfully."),
            Err(e) => log::error!("Model file downloaded failed, err: {:?}", &e),
        }
        // if let Some(s) = huggingface::DOWNLOAD_STATUS.get() {
        //     if let Ok(mut v) = s.lock() {
        //         v.downloading = false;
        //     }
        // }
    });
    to_res(Ok(()))
}

pub(crate) async fn download_model_progress() -> impl IntoResponse {
    let r = huggingface::get_download_status();
    to_res(Ok(r))
}

pub(crate) async fn check_model_files(bytes: Bytes) -> impl IntoResponse {
    match serde_json::from_slice::<Vec<HuggingFaceModel>>(bytes.as_ref()) {
        Ok(repositories) => {
            let mut map = Map::new();
            for model in repositories.iter() {
                let info = model.get_info();
                let r = match huggingface::check_model_files(&info) {
                    Ok(_) => true,
                    Err(e) => {
                        log::warn!(
                            "Hugging face model {} files incorrect. Err: {:?}",
                            info.repository,
                            &e
                        );
                        false
                    }
                };
                map.insert(model.to_string(), Value::from(r));
            }
            to_res(Ok(map))
        }
        Err(e) => to_res(Err(Error::WithMessage(format!(
            "Invalid request body, err {:?}",
            &e
        )))),
    }
}

pub(crate) async fn check_embedding_model(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    let r = if let Ok(r) = get_settings(&q.robot_id) {
        if let Some(settings) = r {
            match settings.sentence_embedding_provider.provider {
                embedding::SentenceEmbeddingProvider::HuggingFace(m) => {
                    let info = m.get_info();
                    huggingface::check_model_files(&info)
                }
                embedding::SentenceEmbeddingProvider::OpenAI(_) => {
                    if settings.sentence_embedding_provider.api_key.is_empty() {
                        Err(Error::WithMessage(String::from("OPENAI_API_KEY is empty.")))
                    } else {
                        Ok(())
                    }
                }
                embedding::SentenceEmbeddingProvider::Ollama(_) => Ok(()),
            }
        } else {
            Err(Error::WithMessage(String::from(
                "Can NOT find settings of this robot.",
            )))
        }
    } else {
        Err(Error::WithMessage(String::from(
            "Failed to get settings of this robot.",
        )))
    };
    to_res(r)
}
