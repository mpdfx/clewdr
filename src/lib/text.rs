use itertools::Itertools;
use rand::{Rng, rng};
use serde_json::Value;
use std::fmt::Write;

use crate::{
    config::PromptPolyfill,
    messages::{Attachment, ClientRequestBody, RequestBody},
    state::AppState,
    types::message::{ContentBlock, ImageSource, Message, MessageContent, Role},
    utils::{TIME_ZONE, print_out_text},
};

/// Merged messages and images
#[derive(Default, Debug)]
pub struct Merged {
    pub paste: String,
    pub prompt: String,
    pub images: Vec<ImageSource>,
}

impl AppState {
    /// Transform the request body from Claude API to Claude web
    pub fn transform(&self, value: ClientRequestBody) -> Option<RequestBody> {
        let system = merge_system(value.system);
        let merged = self.merge_messages(value.messages, system)?;
        Some(RequestBody {
            max_tokens_to_sample: value.max_tokens,
            attachments: vec![Attachment::new(merged.paste)],
            files: vec![],
            model: value.model,
            rendering_mode: "messages".to_string(),
            prompt: merged.prompt,
            timezone: TIME_ZONE.to_string(),
            images: merged.images,
        })
    }

    /// Merge messages into strings and extract images
    fn merge_messages(&self, msgs: Vec<Message>, system: String) -> Option<Merged> {
        if msgs.is_empty() {
            return None;
        }
        let h = self
            .config
            .read()
            .custom_h
            .clone()
            .unwrap_or("Human".to_string());
        let a = self
            .config
            .read()
            .custom_a
            .clone()
            .unwrap_or("Assistant".to_string());

        let user_real_roles = self.config.read().user_real_roles;
        let line_breaks = if user_real_roles { "\n\n\x08" } else { "\n\n" };
        let system = system.trim().to_string();
        let size = size_of_val(&msgs);
        // preallocate string to avoid reallocations
        let mut w = String::with_capacity(size);
        let mut imgs: Vec<ImageSource> = vec![];

        let chunks = msgs
            .into_iter()
            .map_while(|m| match m.content {
                MessageContent::Blocks { content } => {
                    // collect all text blocks, join them with new line
                    let blocks = content
                        .into_iter()
                        .map_while(|b| match b {
                            ContentBlock::Text { text } => Some(text.trim().to_string()),
                            ContentBlock::Image { source } => {
                                // push image to the list
                                imgs.push(source);
                                None
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if blocks.is_empty() {
                        None
                    } else {
                        Some((m.role, blocks))
                    }
                }
                MessageContent::Text { content } => {
                    // plain text
                    let content = content.trim().to_string();
                    if content.is_empty() {
                        None
                    } else {
                        Some((m.role, content))
                    }
                }
            })
            // chunk by role
            .chunk_by(|m| m.0.clone());
        // join same role with new line
        let mut msgs = chunks.into_iter().map(|(role, grp)| {
            let txt = grp.into_iter().map(|m| m.1).collect::<Vec<_>>().join("\n");
            (role, txt)
        });
        // first message does not need prefix
        if !system.is_empty() {
            w += system.as_str();
        } else {
            let first = msgs.next()?;
            w += first.1.as_str();
        }
        for (role, text) in msgs {
            let prefix = match role {
                Role::User => format!("{}: ", h),
                Role::Assistant => format!("{}: ", a),
            };
            write!(w, "{}{}{}", line_breaks, prefix, text).unwrap();
        }
        print_out_text(w.as_str(), "paste.txt");

        // prompt polyfill
        let prompt_polyfill = self.config.read().prompt_polyfill.clone();
        let polyfill = match prompt_polyfill {
            PromptPolyfill::CustomPrompt(p) => p,
            PromptPolyfill::PadTxt(_) => self.generate_padding(),
        };

        Some(Merged {
            paste: w,
            prompt: polyfill,
            images: imgs,
        })
    }

    /// Generate padding text
    fn generate_padding(&self) -> String {
        let conf = &self.config.read();
        let tokens = conf.padtxt.iter().map(|s| s.as_str()).collect::<Vec<_>>();
        assert!(tokens.len() >= 4096, "Padding tokens too short");

        let mut result = String::with_capacity(4096 * 8);
        let mut rng = rng();
        let mut pushed = 0;
        loop {
            let slice_len = rng.random_range(8..64);
            let slice_start = rng.random_range(0..tokens.len() - slice_len);
            let slice = &tokens[slice_start..slice_start + slice_len];
            result.push_str(slice.join(" ").as_str());
            pushed += slice_len;
            result.push('\n');
            if rng.random_range(0..100) < 5 {
                result.push('\n');
            }
            if pushed > 4000 {
                break;
            }
        }
        print_out_text(result.as_str(), "padding.txt");
        result
    }
}

fn merge_system(sys: Value) -> String {
    if let Some(str) = sys.as_str() {
        return str.to_string();
    }
    let Some(arr) = sys.as_array() else {
        return String::new();
    };
    arr.iter()
        .map_while(|v| v["text"].as_str())
        .map(|v| v.trim())
        .to_owned()
        .collect::<Vec<_>>()
        .join("\n")
}
