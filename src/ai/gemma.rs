use candle::{DType, Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::gemma::Model as GemmaModel;
// use crossbeam_channel::Sender;
use frand::Rand;
use tokenizers::Tokenizer;

use super::chat::ResultSender;
use crate::result::{Error, Result};

// static TEXT_GENERATION_MODEL: OnceLock<Mutex<HashMap<String, (GemmaModel, Tokenizer)>>> =
//     OnceLock::new();

// pub(super) fn replace_model_cache(robot_id: &str, info: &HuggingFaceModelInfo) -> Result<()> {
//     let device = device()?;
//     let c = load_gemma_model_files(info, &device)?;
//     if let Some(lock) = TEXT_GENERATION_MODEL.get() {
//         if let Ok(mut cache) = lock.lock() {
//             cache.insert(String::from(robot_id), c);
//         }
//     }
//     Ok(())
// }

pub(super) fn gen_text(
    device: &Device,
    model: &GemmaModel,
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
    //     let r = load_gemma_model_files(info, &device)?;
    //     model.insert(String::from(robot_id), r);
    // };
    // let (model, tokenizer) = model.get(robot_id).unwrap();
    let mut tokens = match tokenizer.encode(prompt, true) {
        Ok(t) => t.get_ids().to_vec(),
        Err(e) => return Err(Error::WithMessage(format!("{}", &e))),
    };
    let mut tokenizer = super::token_output_stream::TokenOutputStream::new(tokenizer.clone());
    let eos_token = match tokenizer.get_token("<eos>") {
        Some(token) => token,
        None => {
            return Err(Error::WithMessage(String::from(
                "cannot find the <eos> token",
            )));
        }
    };
    let mut generated_tokens = 0usize;
    let start_gen = std::time::Instant::now();
    let mut model = model.clone();
    // let rr = Rc::new(result_sender);
    for index in 0..sample_len {
        let context_size = if index > 0 { 1 } else { tokens.len() };
        let start_pos = tokens.len().saturating_sub(context_size);
        let ctxt = &tokens[start_pos..];
        let input = Tensor::new(ctxt, device)?.unsqueeze(0)?;
        let logits = model.forward(&input, start_pos)?;
        let logits = logits.squeeze(0)?.squeeze(0)?.to_dtype(DType::F32)?;
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

        let mut rng = Rand::new();
        let mut logits_processor = LogitsProcessor::new(
            rng.r#gen::<u64>(),
            Some(super::completion::TEMPERATURE),
            top_p,
        );
        let next_token = logits_processor.sample(&logits)?;
        tokens.push(next_token);
        generated_tokens += 1;
        if next_token == eos_token {
            break;
        }
        // let rr: ResultReceiver;
        if let Some(t) = tokenizer.next_token(next_token)? {
            // print!("{t}");
            // std::io::stdout().flush()?;
            // let result_sender = rr.clone();
            match result_sender {
                ResultSender::ChannelSender(sender) => {
                    if let Err(e) = sender.try_send(t) {
                        log::warn!(
                            "Sent failed, maybe receiver dropped or queue was full, err: {:?}",
                            &e
                        );
                        break;
                    }
                    // ResultReceiver::SseSender(sender)
                }
                ResultSender::StrBuf(sb) => {
                    sb.push_str(&t);
                    // ResultReceiver::StrBuf(sb)
                }
            }
        }
    }
    let dt = start_gen.elapsed();
    if let Some(rest) = tokenizer.decode_rest()? {
        // print!("{rest}");
        match result_sender {
            ResultSender::ChannelSender(sender) => {
                if let Err(e) = sender.try_send(rest) {
                    log::warn!(
                        "Sent failed, maybe receiver dropped or queue was full, err: {:?}",
                        &e
                    );
                }
            }
            ResultSender::StrBuf(sb) => sb.push_str(&rest),
        }
    }
    // std::io::stdout().flush()?;
    println!(
        "\n{generated_tokens} tokens generated ({:.2} token/s)",
        generated_tokens as f64 / dt.as_secs_f64(),
    );
    Ok(())
}
