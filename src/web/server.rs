// use std::net::SocketAddr;
use std::sync::LazyLock;
use std::vec::Vec;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use colored::Colorize;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;

use super::asset::ASSETS_MAP;
use crate::ai::crud as ai;
use crate::external::http::crud as http;
use crate::flow::mainflow::crud as mainflow;
use crate::flow::rt::facade as rt;
use crate::flow::subflow::crud as subflow;
use crate::intent::crud as intent;
use crate::kb::crud as kb;
use crate::man::settings;
use crate::result::Error;
use crate::robot::crud as robot;
use crate::variable::crud as variable;

//https://stackoverflow.com/questions/27840394/how-can-a-rust-program-access-metadata-from-its-cargo-package
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
static VERSION_NUM: LazyLock<u64> = LazyLock::new(|| convert_version(VERSION));

const ASSETS: &[(&[u8], &str)] = &include!("asset.txt");

pub(crate) static IS_EN: LazyLock<bool> = LazyLock::new(|| {
    let language = get_lang();
    // println!("Your OS language is: {}", language);
    language[0..2].eq("en")
});

// https://doc.rust-lang.org/reference/conditional-compilation.html
#[cfg(windows)]
fn get_lang() -> String {
    let mut v = [0u16; windows::Win32::System::SystemServices::LOCALE_NAME_MAX_LENGTH as usize];
    unsafe {
        let l = windows::Win32::Globalization::GetUserDefaultLocaleName(&mut v) as usize;
        String::from_utf16(&v[0..l]).unwrap()
        // windows::Win32::Globalization::GetUserDefaultLangID().to_string()
    }
}

#[cfg(not(windows))]
fn get_lang() -> String {
    std::env::var("LANG").unwrap_or(String::from("en_US"))
}

// fn invalid_ip_msg(addr: &String) -> String {
//     format!("Invalid listening addr {}, please reset the configuration parameters by adding the startup parameter: {}",
//     addr.bright_red(), "-rs".bright_yellow())
// }

pub async fn start_app() {
    // unsafe {
    //     libsqlite3_sys::sqlite3_auto_extension(Some(std::mem::transmute(
    //         sqlite_vec::sqlite3_vec_init as *const (),
    //     )));
    // }

    crate::intent::phrase::init_datasource()
        .await
        .expect("Failed initialize intent phrase vector database.");

    crate::kb::qa::init_datasource()
        .await
        .expect("Failed initialize knowledge base QnA vector database.");

    crate::kb::doc::init_datasource()
        .await
        .expect("Failed initialize knowledge base QnA vector database.");

    let settings = {
        let mut s = crate::db::init().await.expect("Initialize database failed");
        for argument in std::env::args() {
            if argument.eq("-rs") {
                s = settings::GlobalSettings::default();
                match crate::db::global_store().await {
                    Ok(store) => settings::save_global_settings(store, &s).await,
                    Err(e) => Err(e),
                }
                .expect("Reset settings failed");
                break;
            }
        }
        s
    };

    let mut listening_ip = String::with_capacity(32);
    let mut port: u16 = 0;
    let mut set_listening_ip = false;
    let mut set_listening_port = false;
    for argument in std::env::args() {
        if set_listening_ip {
            listening_ip.push_str(&argument);
            set_listening_ip = false;
            continue;
        }
        if argument.eq("-ip") {
            set_listening_ip = true;
            continue;
        }
        if set_listening_port {
            port = argument.parse::<u16>().unwrap();
            set_listening_port = false;
            continue;
        }
        if argument.eq("-port") {
            set_listening_port = true;
            continue;
        }
    }
    if listening_ip.is_empty() {
        listening_ip.push_str(&settings.ip);
    }
    if port == 0 {
        port = settings.port;
    }

    let (sender, recv) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(crate::flow::rt::context::clean_expired_session(recv));

    let r: Router = gen_router();
    let app = r.fallback(fallback);
    // let socket_addr: SocketAddr = addr.parse().expect(&invalid_ip_msg(&addr));
    let mut bind_res;
    // let mut port = settings.port;
    loop {
        let addr = format!("{}:{}", &listening_ip, port);
        bind_res = tokio::net::TcpListener::bind(&addr).await;
        if bind_res.is_ok() {
            break;
        }
        if !settings.select_random_port_when_conflict {
            log::error!("The listening port is occupied and the program fails to start.");
            log::info!("Tip: You can check the random port in the settings to avoid this problem.");
            std::process::exit(-1);
        }
        port += 1;
        if port == settings.port {
            log::error!("The listening port is occupied and the program fails to start.");
            log::info!("Tip: You can check the random port in the settings to avoid this problem.");
            std::process::exit(-1);
        }
        if port == 65535 {
            port = 1025;
        }
    }
    let listener = bind_res.unwrap();

    #[cfg(target_os = "windows")]
    colored::control::set_virtual_terminal(true).unwrap();

    log::info!(
        "-->  {} {}{}:{}",
        if *IS_EN {
            "The server is running, please open a browser and visit"
        } else {
            "服务已启动，请用浏览器访问"
        },
        "http://".bright_green(),
        if listening_ip.eq("0.0.0.0") {
            "<Your machine IP>".bright_green()
        } else {
            listening_ip.bright_green()
        },
        port.to_string().blue()
    );
    log::info!(
        "Tip: {} {} {} {} {}",
        if *IS_EN {
            "You can use"
        } else {
            "你可以使用"
        },
        "-ip".yellow(),
        if *IS_EN { "and" } else { "和" },
        "-port".yellow(),
        if *IS_EN {
            "to customize the listening IP and port"
        } else {
            "来自定义监听的IP和端口"
        },
    );
    log::info!("---------------------------------------------");
    log::info!("Current version: {VERSION}");
    log::info!("Visiting https://dialogflowai.github.io/ for the latest releases");

    log::info!("-->  To close the server, hit {}", "Ctrl+C".bright_red());

    // let addr = format!("{}:{}", settings.ip, settings.port);
    // let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    // let addr = SocketAddr::from((settings.ip, settings.port));
    let serve = axum::serve(listener, app);
    // log::info!("{:?}", serve.local_addr().unwrap());
    serve
        .with_graceful_shutdown(shutdown_signal(sender))
        .await
        .unwrap();
}

fn gen_router() -> Router {
    Router::new()
        .route(
            "/robot",
            get(robot::list).post(robot::save).delete(robot::delete),
        )
        .route("/robot/detail", get(robot::detail))
        .route(
            "/intent",
            get(intent::list).post(intent::add).delete(intent::remove),
        )
        .route("/intent/detect", post(intent::detect))
        .route("/intent/detail", get(intent::detail))
        .route(
            "/intent/keyword",
            post(intent::add_keyword).delete(intent::remove_keyword),
        )
        .route(
            "/intent/regex",
            post(intent::add_regex).delete(intent::remove_regex),
        )
        .route(
            "/intent/phrase",
            post(intent::add_phrase).delete(intent::remove_phrase),
        )
        .route(
            "/intent/phrase/regenerate-all",
            get(intent::regenerate_embeddings),
        )
        .route(
            "/variable",
            get(variable::list)
                .post(variable::add)
                .delete(variable::delete),
        )
        .route(
            "/mainflow",
            get(mainflow::list)
                .post(mainflow::new)
                .put(mainflow::save)
                .delete(mainflow::delete),
        )
        .route("/mainflow/release", get(subflow::release))
        .route(
            "/subflow",
            get(subflow::list)
                .post(subflow::save)
                .delete(subflow::delete),
        )
        .route("/subflow/simple", get(subflow::simple_list))
        .route("/subflow/new", post(subflow::new))
        .route("/external/http", get(http::list))
        .route(
            "/external/http/{id}",
            get(http::detail).post(http::save).delete(http::remove),
        )
        .route(
            "/management/global-settings",
            get(settings::rest_get_global_settings).post(settings::rest_save_global_settings),
        )
        .route(
            "/management/settings",
            get(settings::get).post(settings::save),
        )
        .route(
            "/management/settings/model/download",
            post(settings::download_model_files),
        )
        .route(
            "/management/settings/model/download/progress",
            get(settings::download_model_progress),
        )
        .route(
            "/management/settings/model/check/files",
            post(settings::check_model_files),
        )
        .route(
            "/management/settings/model/local/path",
            post(settings::local_model_paths),
        )
        .route(
            "/management/settings/model/load/progress",
            get(settings::model_load_progress),
        )
        .route(
            "/management/settings/model/load",
            post(settings::load_model_now),
        )
        .route(
            "/management/settings/model/check/embedding",
            get(settings::check_embedding_model),
        )
        .route(
            "/management/settings/embedding/vector-dimensions",
            get(settings::vector_dimensions),
        )
        .route(
            "/management/settings/embedding/reindex",
            post(crate::man::reindex::start),
        )
        .route(
            "/management/settings/embedding/reindex/progress",
            get(crate::man::reindex::progress),
        )
        .route(
            "/management/settings/embedding/reindex/hint",
            get(crate::man::reindex::hint),
        )
        .route(
            "/management/settings/model/openai/list",
            get(settings::list_openai_models),
        )
        .route(
            "/kb/qa",
            get(kb::list_qa).post(kb::save_qa).delete(kb::delete_qa),
        )
        .route(
            "/kb/doc",
            get(kb::list_doc)
                .post(kb::update_doc)
                .delete(kb::delete_doc),
        )
        .route("/kb/doc/upload", post(kb::upload_doc))
        .route("/kb/qa/dryrun", get(kb::qa_dryrun))
        .route("/management/settings/smtp/test", post(settings::smtp_test))
        .route("/flow/answer", post(rt::answer))
        .route("/flow/answer/multipart", post(rt::answer_multipart))
        .route("/ai/text/generation", post(ai::gen_text))
        .route("/version.json", get(version))
        .route("/check-new-version.json", get(check_new_version))
        // .route("/o", get(subflow::output))
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(
            250 * 1024 * 1024, /* 250mb */
        ))
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(
                    |_origin: &HeaderValue, _request_parts| {
                        // println!("{}", String::from_utf8_lossy(origin.as_bytes()));
                        // origin.as_bytes().ends_with(b"localhost")
                        true
                    },
                ))
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
                .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::PUT]),
        )
}

// https://docs.rs/axum/0.6.18/axum/response/index.html

async fn fallback(uri: Uri) -> Response {
    let v = ASSETS_MAP.get(uri.path());
    if v.is_some() {
        let idx = v.unwrap();
        let d = ASSETS[*idx];
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, d.1.parse().unwrap());
        headers.insert(header::CONTENT_ENCODING, "gzip".parse().unwrap());
        (StatusCode::OK, headers, d.0).into_response()
    } else {
        (StatusCode::NOT_FOUND, format!("Not Found: {}", uri.path())).into_response()
    }
}

fn convert_version(ver: &str) -> u64 {
    let arr: Vec<&str> = ver.split('.').collect();
    let mut v = String::with_capacity(VERSION.len() + 4);
    v.push_str(arr[0]);
    if arr[1].len() == 1 {
        v.push_str("00");
    } else if arr[1].len() == 2 {
        v.push('0');
    }
    v.push_str(arr[1]);
    if arr[2].len() == 1 {
        v.push_str("00");
    } else if arr[2].len() == 2 {
        v.push('0');
    }
    v.push_str(arr[2]);
    // log::info!("vernum={}", &v);
    v.parse().expect("Wrong version")
}

async fn version() -> impl IntoResponse {
    let mut v = String::with_capacity(15);
    v.push('"');
    v.push_str(VERSION);
    v.push('"');
    v
}

async fn check_new_version() -> impl IntoResponse {
    let r = reqwest::get("https://dialogflowai.github.io/check-new-version.json").await;
    if let Err(e) = r {
        return to_res(Err(Error::NetworkConnectTimeout(Box::new(e))));
    }
    r.unwrap().text().await.map_or_else(
        |e| to_res(Err(Error::NetworkReadTimeout(Box::new(e)))),
        |s| {
            #[derive(Debug, Deserialize, Serialize)]
            struct VersionInfo {
                version: String,
                changelog: Vec<String>,
            }
            let obj: core::result::Result<VersionInfo, _> = serde_json::from_str(&s);
            if let Err(e) = obj {
                return to_res(Err(Error::InvalidJsonStructure(Box::new(e))));
            }
            let v = obj.unwrap();
            if convert_version(&v.version) > *VERSION_NUM {
                to_res(Ok(Some(v)))
            } else {
                to_res(Ok(None))
            }
        },
    )
}

async fn shutdown_signal(sender: tokio::sync::oneshot::Sender<()>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    match sender.send(()) {
        Ok(_) => {}
        Err(_) => log::info!("中断 ctx 失败"),
    };

    // crate::intent::phrase::shutdown_db().await;
    // crate::kb::qa::shutdown_db().await;
    // crate::kb::doc::shutdown_db().await;

    let m = if *IS_EN {
        "This program has been terminated"
    } else {
        "应用已退出"
    };
    log::info!("{m}");
}

#[derive(Serialize)]
struct ResponseData<D> {
    pub(crate) status: u16,
    pub(crate) data: Option<D>,
    pub(crate) err: Option<Error>,
}

/// Serializes one result as `{status, data, err}`.
///
/// Both transports use this: the single-document response is exactly this text,
/// and the terminal frame of a streamed response carries the same text in its
/// `content`, so a client reads a result the same way either way. The status is
/// inside the document rather than on the HTTP response, which is why a failure
/// is a 200 at the HTTP level.
pub(crate) fn envelope_json<D>(r: Result<D, Error>) -> String
where
    D: serde::Serialize,
{
    let res: ResponseData<D> = match r {
        Ok(d) => ResponseData {
            status: StatusCode::OK.as_u16(),
            data: Some(d),
            err: None,
        },
        Err(e) => ResponseData {
            status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            data: None,
            err: Some(e),
        },
    };
    serde_json::to_string(&res).unwrap()
}

pub(crate) fn to_res2<D>(r: Result<D, Error>) -> axum::response::Response
where
    D: serde::Serialize + 'static + std::marker::Send,
{
    let body = axum::body::Body::from(envelope_json(r));
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap()
}

/// Streams frames as newline-delimited JSON: one JSON value per line, so a
/// client can split on `\n` and parse each line on its own.
///
/// `X-Accel-Buffering` is not decoration — nginx buffers proxied chunked
/// responses by default, which would hold every frame until the end and defeat
/// the whole point of streaming in the most common deployment.
pub(crate) fn to_ndjson(
    receiver: tokio::sync::mpsc::UnboundedReceiver<crate::flow::rt::dto::StreamingResponseData>,
) -> axum::response::Response {
    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let body = axum::body::Body::from_stream(stream.map(|frame| {
        Ok::<_, std::convert::Infallible>(format!("{}\n", serde_json::to_string(&frame).unwrap()))
    }));
    Response::builder()
        .status(200)
        .header(header::TRANSFER_ENCODING, "chunked")
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(body)
        .unwrap()
}

// pub(crate) enum ResponseDataHolder<D> {
//     Normal(D),
//     Chunked(tokio::sync::mpsc::UnboundedReceiver<D>),
// }

// pub(crate) fn t<D>(d: ResponseDataHolder<D>) -> axum::response::Response
// where
//     D: serde::Serialize + 'static + std::marker::Send,
// {
//     let builder = Response::builder().status(200);
//     match d {
//         ResponseDataHolder::Normal(d) => {
//             let res = ResponseData {
//                 status: StatusCode::OK.as_u16(),
//                 data: Some(d),
//                 err: None,
//             };
//             let body = axum::body::Body::from(serde_json::to_string(&res).unwrap());
//             builder
//                 .header(header::CONTENT_TYPE, "application/json")
//                 .body(body)
//                 .unwrap()
//         }
//         ResponseDataHolder::Chunked(r) => {
//             let s = tokio_stream::wrappers::UnboundedReceiverStream::new(r);
//             let body = axum::body::Body::from_stream(s.map(|d| {
//                 let res = ResponseData {
//                     status: StatusCode::OK.as_u16(),
//                     data: Some(d),
//                     err: None,
//                 };
//                 let body = serde_json::to_string(&res).unwrap();
//                 Ok::<_, std::convert::Infallible>(body)
//             }));
//             builder
//                 .header(header::TRANSFER_ENCODING, "chunked")
//                 .body(body)
//                 .unwrap()
//         }
//     }
// }

pub(crate) fn to_res<D>(r: Result<D, Error>) -> impl IntoResponse
where
    D: serde::Serialize + 'static,
{
    // let now = std::time::Instant::now();
    let data = match r {
        Ok(d) => {
            let res = ResponseData {
                status: StatusCode::OK.as_u16(),
                data: Some(&d),
                err: None,
            };
            serde_json::to_string(&res).unwrap()
            // simd_json::to_string(&res).unwrap()
        }
        Err(e) => {
            let res: ResponseData<D> = ResponseData {
                status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                data: None,
                err: Some(e),
            };
            serde_json::to_string(&res).unwrap()
            // simd_json::to_string(&res).unwrap()
        }
    };
    // log::info!("serialize used time:{:?}", now.elapsed());
    let mut header_map = HeaderMap::new();
    header_map.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    (StatusCode::OK, header_map, data)
}

pub(crate) fn is_en(headers: &axum::http::HeaderMap) -> bool {
    let client_language = headers
        .get("Accept-Language")
        .map_or_else(|| "en-US", |v| v.to_str().unwrap_or("en-US"));
    if !client_language.is_empty() && client_language.starts_with("en") {
        true
    } else {
        *IS_EN
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::rt::dto::{Request, ResponseChannelWrapper, ResponseData};
    use crate::flow::subflow::dto::NextActionType;

    fn req() -> Request {
        serde_json::from_str(
            r#"{"robotId":"rb","mainFlowId":"mf","sessionId":"ss","userInputResult":"Successful","userInput":"hi"}"#,
        )
        .unwrap()
    }

    /// The transport contract every client depends on: one JSON value per line,
    /// each line newline-terminated. Without that delimiter a client has to
    /// guess where a frame ends — which is what the shipped JavaScript SDK did,
    /// by splitting on `}{`.
    #[tokio::test]
    async fn frames_are_newline_delimited_and_the_last_one_is_the_terminal_frame() {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let channel = ResponseChannelWrapper::new(sender);
        assert!(channel.push_frame(0, String::from("Hel")));
        assert!(channel.push_frame(0, String::from("lo")));
        let mut data = ResponseData::new(&req());
        data.next_action = NextActionType::Terminate;
        assert!(channel.push_terminal(envelope_json(Ok(data))));
        // Dropping the only sender ends the body, exactly as finishing the flow
        // task does.
        drop(channel);

        let res = to_ndjson(receiver);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/x-ndjson"
        );
        assert!(
            res.headers().contains_key("x-accel-buffering"),
            "a proxy must be told not to buffer the frames"
        );
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = core::str::from_utf8(&body).unwrap();
        let Some(text) = text.strip_suffix('\n') else {
            panic!("the body must end with a newline: {text:?}");
        };
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 3, "one line per frame: {lines:?}");

        let delta: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(delta["contentSeq"], 0);
        assert_eq!(delta["content"], "Hel");

        let last: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert!(
            last["contentSeq"].is_null(),
            "a null sequence is what marks the terminal frame"
        );
        let envelope: serde_json::Value =
            serde_json::from_str(last["content"].as_str().unwrap()).unwrap();
        assert_eq!(envelope["status"], 200);
        assert_eq!(envelope["data"]["nextAction"], "Terminate");
    }

    /// The point of the whole exercise, checked on the wire rather than through
    /// a `Body`: the frames have to leave as they are produced. Reading the body
    /// in-process would happily buffer everything and still pass.
    #[tokio::test]
    async fn frames_reach_the_socket_one_by_one() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        async fn stream_slowly() -> axum::response::Response {
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let channel = ResponseChannelWrapper::new(sender);
            tokio::spawn(async move {
                channel.push_frame(0, String::from("first"));
                tokio::time::sleep(core::time::Duration::from_millis(300)).await;
                channel.push_frame(0, String::from("second"));
                tokio::time::sleep(core::time::Duration::from_millis(300)).await;
                let mut data = ResponseData::new(&req());
                data.next_action = NextActionType::Terminate;
                channel.push_terminal(envelope_json(Ok(data)));
            });
            to_ndjson(receiver)
        }

        let app = axum::Router::new().route("/flow/answer", axum::routing::post(stream_slowly));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket
            .write_all(
                b"POST /flow/answer HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\
                  Connection: close\r\n\r\n",
            )
            .await
            .unwrap();

        let expected = [
            r#"{"contentSeq":0,"content":"first"}"#,
            r#"{"contentSeq":0,"content":"second"}"#,
            r#""contentSeq":null"#,
        ];
        let mut seen: Vec<Option<std::time::Instant>> = vec![None; expected.len()];
        let mut raw: Vec<u8> = Vec::with_capacity(4096);
        loop {
            let mut chunk = [0u8; 4096];
            let n = socket.read(&mut chunk).await.unwrap();
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&raw);
            for (i, want) in expected.iter().enumerate() {
                if seen[i].is_none() && text.contains(want) {
                    seen[i] = Some(std::time::Instant::now());
                }
            }
        }

        let text = String::from_utf8_lossy(&raw);
        let head = text.split("\r\n\r\n").next().unwrap().to_lowercase();
        assert!(head.starts_with("http/1.1 200 ok"), "{head}");
        assert_eq!(
            head.matches("transfer-encoding: chunked").count(),
            1,
            "the response must be chunked exactly once: {head}"
        );
        assert!(head.contains("content-type: application/x-ndjson"), "{head}");

        let seen: Vec<std::time::Instant> = seen
            .into_iter()
            .map(|t| t.expect("a frame never arrived"))
            .collect();
        // The flow waits 300 ms between the first two frames, so a body that was
        // buffered until the end would show almost no spread.
        let spread = seen[2].duration_since(seen[0]);
        assert!(
            spread >= core::time::Duration::from_millis(250),
            "the frames arrived {spread:?} apart, they were held back"
        );
    }

    /// A failure is carried inside the terminal frame with the same shape a
    /// non-streaming request uses, because the HTTP status is already committed
    /// by the time the failure is known.
    #[tokio::test]
    async fn a_failure_is_reported_in_the_terminal_frame() {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let channel = ResponseChannelWrapper::new(sender);
        channel.push_terminal(envelope_json::<ResponseData>(Err(Error::WithMessage(
            String::from("boom"),
        ))));
        drop(channel);

        let res = to_ndjson(receiver);
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let frame: serde_json::Value =
            serde_json::from_str(core::str::from_utf8(&body).unwrap().trim()).unwrap();
        assert!(frame["contentSeq"].is_null());
        let envelope: serde_json::Value =
            serde_json::from_str(frame["content"].as_str().unwrap()).unwrap();
        assert_eq!(envelope["status"], 500);
        assert_eq!(envelope["err"]["message"], "boom");
        assert!(envelope["data"].is_null());
    }
}
