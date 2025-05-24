use candle::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::llama::{Cache, Llama, LlamaEosToks};
// use crossbeam_channel::Sender;
use frand::Rand;
use tokenizers::Tokenizer;

use super::chat::ResultSender;
use crate::result::{Error, Result};

// static TEXT_GENERATION_MODEL: OnceLock<
//     Mutex<HashMap<String, (Llama, Cache, Tokenizer, Option<u32>)>>,
// > = OnceLock::new();

// pub(super) fn replace_model_cache(robot_id: &str, info: &HuggingFaceModelInfo) -> Result<()> {
//     let device = device()?;
//     let c = load_llama_model_files(info, &device)?;
//     if let Some(lock) = TEXT_GENERATION_MODEL.get() {
//         if let Ok(mut cache) = lock.lock() {
//             cache.insert(String::from(robot_id), c);
//         }
//     }
//     Ok(())
// }

pub(super) fn gen_text(
    device: &Device,
    model: &Llama,
    cache: &Cache,
    tokenizer: &Tokenizer,
    eos_token_id: &Option<LlamaEosToks>,
    prompt: &str,
    sample_len: usize,
    top_k: Option<usize>,
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
    //     let r = load_llama_model_files(info, &device)?;
    //     model.insert(String::from(robot_id), r);
    // };
    // let (model, ref cache, tokenizer, eos_token_id) = model.get(robot_id).unwrap();

    // let (model, mut cache, tokenizer, eos_token_id) = load_llama_model_files(info,&device)?;
    // let mut tokens = tokenizer
    //     .encode(prompt, true)
    //     .map_err(|e| Err(Error::ErrorWithMessage(format!("{}", &e))))?
    //     .get_ids()
    //     .to_vec();
    let mut tokens = match tokenizer.encode(prompt, true) {
        Ok(t) => t.get_ids().to_vec(),
        Err(e) => return Err(Error::WithMessage(format!("{}", &e))),
    };
    log::info!("tokens len={}", tokens.len());
    let mut tokenizer = super::token_output_stream::TokenOutputStream::new(tokenizer.clone());
    // log::info!("starting the inference loop");
    // log::info!("{prompt}");
    let mut logits_processor = {
        let sampling = if super::completion::TEMPERATURE <= 0. {
            Sampling::ArgMax
        } else {
            match (top_k, top_p) {
                (None, None) => Sampling::All {
                    temperature: super::completion::TEMPERATURE,
                },
                (Some(k), None) => Sampling::TopK {
                    k,
                    temperature: super::completion::TEMPERATURE,
                },
                (None, Some(p)) => Sampling::TopP {
                    p,
                    temperature: super::completion::TEMPERATURE,
                },
                (Some(k), Some(p)) => Sampling::TopKThenTopP {
                    k,
                    p,
                    temperature: super::completion::TEMPERATURE,
                },
            }
        };
        let mut rng = Rand::new();
        LogitsProcessor::from_sampling(rng.r#gen::<u64>(), sampling)
    };
    // log::info!("logits_processor finished");
    let start_gen = std::time::Instant::now();
    let mut index_pos = 0;
    let mut token_generated = 0;
    // let model = model.clone();
    let mut cache = cache.clone();
    for index in 0..sample_len {
        let (context_size, context_index) = if cache.use_kv_cache && index > 0 {
            (1, index_pos)
        } else {
            (tokens.len(), 0)
        };
        let ctxt = &tokens[tokens.len().saturating_sub(context_size)..];
        let input = Tensor::new(ctxt, device)?.unsqueeze(0)?;
        let logits = model.forward(&input, context_index, &mut cache)?;
        let logits = logits.squeeze(0)?;
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
        index_pos += ctxt.len();

        let next_token = logits_processor.sample(&logits)?;
        token_generated += 1;
        tokens.push(next_token);

        match eos_token_id {
            Some(LlamaEosToks::Single(eos_tok_id)) if next_token == *eos_tok_id => {
                break;
            }
            Some(LlamaEosToks::Multiple(eos_ids)) if eos_ids.contains(&next_token) => {
                break;
            }
            _ => (),
        }
        if let Some(t) = tokenizer.next_token(next_token)? {
            // log::info!("{&t}");
            // std::io::stdout().flush()?;
            // if let Err(e) = sender.try_send(t) {
            //     log::warn!(
            //         "Sent failed, maybe receiver dropped or queue was full, err: {:?}",
            //         &e
            //     );
            //     break;
            // }
            log::info!("Llama {}", &t);
            match result_sender {
                ResultSender::ChannelSender(sender) => {
                    if sender.is_closed() {
                        log::info!("Sender closed, break");
                        break;
                    }
                    crate::sse_send!(sender, t);
                }
                ResultSender::StrBuf(sb) => {
                    sb.push_str(&t);
                    // ResultReceiver::StrBuf(sb)
                }
            }
        }
    }
    if let Some(rest) = tokenizer.decode_rest()? {
        match result_sender {
            ResultSender::ChannelSender(sender) => {
                crate::sse_send!(sender, rest);
            }
            ResultSender::StrBuf(sb) => {
                sb.push_str(&rest);
                // ResultReceiver::StrBuf(sb)
            }
        }
    }
    let dt = start_gen.elapsed();
    log::info!(
        "\n\n{} tokens generated ({} token/s)\n",
        token_generated,
        (token_generated - 1) as f64 / dt.as_secs_f64(),
    );
    Ok(())
}
