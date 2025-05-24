use candle::{DType, Device, IndexOp, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::phi3::Model;
use frand::Rand;
use tokenizers::Tokenizer;

use super::chat::ResultSender;
use crate::result::{Error, Result};

// static TEXT_GENERATION_MODEL: OnceLock<Mutex<HashMap<String, (Model, Tokenizer)>>> =
//     OnceLock::new();

// pub(super) fn replace_model_cache(robot_id: &str, info: &HuggingFaceModelInfo) -> Result<()> {
//     let device = device()?;
//     let c = load_phi3_model_files(info, &device)?;
//     if let Some(lock) = TEXT_GENERATION_MODEL.get() {
//         if let Ok(mut cache) = lock.lock() {
//             cache.insert(String::from(robot_id), c);
//         }
//     }
//     Ok(())
// }

pub(super) fn gen_text(
    device: &Device,
    model: &Model,
    tokenizer: &Tokenizer,
    prompt: &str,
    sample_len: usize,
    top_p: Option<f64>,
    result_sender: &mut ResultSender<'_, String>,
) -> Result<()> {
    // let device = device()?;
    // let lock = TEXT_GENERATION_MODEL.get_or_init(|| Mutex::new(HashMap::with_capacity(32)));
    // let mut model = lock.lock().unwrap_or_else(|e| {
    //     log::warn!("{:#?}", &e);
    //     e.into_inner()
    // });
    // if !model.contains_key(robot_id) {
    //     let r = load_phi3_model_files(info, &device)?;
    //     model.insert(String::from(robot_id), r);
    // };
    // let (model, tokenizer) = model.get(robot_id).unwrap();

    // log::info!("starting the inference loop");
    let mut tokenizer = super::token_output_stream::TokenOutputStream::new(tokenizer.clone());
    let mut tokens = match tokenizer.tokenizer().encode(prompt, true) {
        Ok(t) => t.get_ids().to_vec(),
        Err(e) => return Err(Error::WithMessage(format!("{}", &e))),
    };
    if tokens.is_empty() {
        return Err(Error::WithMessage(String::from(
            "Empty prompts are not supported in the phi model.",
        )));
    }
    let mut generated_tokens = 0usize;
    let eos_token = match tokenizer.get_token("<|endoftext|>") {
        Some(token) => token,
        None => {
            return Err(Error::WithMessage(String::from(
                "cannot find the endoftext token",
            )));
        }
    };
    // log::info!("{prompt}");
    // std::io::stdout().flush()?;
    let start_gen = std::time::Instant::now();
    let mut pos = 0;
    let mut rng = Rand::new();
    let mut model = model.clone();
    let mut logits_processor = LogitsProcessor::new(
        rng.r#gen::<u64>(),
        Some(super::completion::TEMPERATURE),
        top_p,
    );
    for index in 0..sample_len {
        let context_size = if index > 0 { 1 } else { tokens.len() };
        let ctxt = &tokens[tokens.len().saturating_sub(context_size)..];
        let input = Tensor::new(ctxt, device)?.unsqueeze(0)?;
        let logits = model.forward(&input, pos)?.i((.., 0, ..))?;
        let logits = logits.squeeze(0)?.to_dtype(DType::F32)?;
        let logits = if super::completion::REPEAT_PENALTY == 1. {
            logits
        } else {
            let start_at = tokens
                .len()
                .saturating_sub(super::completion::REPEAT_LAST_N);
            candle_transformers::utils::apply_repeat_penalty(
                &logits,
                super::completion::REPEAT_PENALTY,
                &tokens[start_at..],
            )?
        };

        let next_token = logits_processor.sample(&logits)?;
        tokens.push(next_token);
        generated_tokens += 1;
        if next_token == eos_token {
            if let Some(t) = tokenizer.decode_rest()? {
                match result_sender {
                    ResultSender::ChannelSender(sender) => {
                        crate::sse_send!(sender, t);
                    }
                    ResultSender::StrBuf(sb) => {
                        sb.push_str(&t);
                        // ResultReceiver::StrBuf(sb)
                    }
                }
            }
            break;
        }
        if let Some(t) = tokenizer.next_token(next_token)? {
            match result_sender {
                ResultSender::ChannelSender(sender) => {
                    crate::sse_send!(sender, t);
                }
                ResultSender::StrBuf(sb) => {
                    sb.push_str(&t);
                    // ResultReceiver::StrBuf(sb)
                }
            }
            // std::io::stdout().flush()?;
        }
        pos += context_size;
    }
    let dt = start_gen.elapsed();
    log::info!(
        "\n{generated_tokens} tokens generated ({:.2} token/s)",
        generated_tokens as f64 / dt.as_secs_f64(),
    );
    Ok(())
}
