//! Hosted (provider-executed) tool items on the Responses stream (port of
//! `src/protocols/utils/responses-hosted-tools.ts`).
//!
//! When a hosted tool call item completes (`response.output_item.done`), the
//! protocol synthesizes the provider-executed `tool-call` **and** its
//! `tool-result` in one step: the provider already ran the tool.

use serde_json::{json, Value};

use crate::protocols::open_responses::{self, ParserState};
use crate::protocols::utils::lifecycle;
use crate::schema::{AiError, LLMEvent, ToolResultValue};

/// A hosted tool output item: the typed fields protocols read, with the raw
/// item preserved for the `json` result fallback (port of `Item`).
#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub raw: Value,
}

/// Field accessors over the raw item (mirrors the optional `Item` fields).
impl Item {
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&Value> {
        self.raw.get(name).filter(|value| !value.is_null())
    }
    #[must_use]
    pub fn string_field(&self, name: &str) -> Option<&str> {
        self.field(name).and_then(Value::as_str)
    }
}

/// One hosted tool definition: the common tool name and the input payload
/// synthesized from the completed item (port of `Definition`). `result`
/// customizes the tool-result payload; the default records the whole item as
/// JSON (or the item's `error`).
pub struct Definition {
    pub name: &'static str,
    pub input: fn(&Item) -> Value,
    pub result: Option<fn(&Item) -> Result<ToolResultValue, AiError>>,
}

/// Hosted tool definitions keyed by item `type` (port of `Definitions`).
pub type Definitions<'a> = &'a [(&'static str, Definition)];

/// Whether the output item's `type` has a hosted tool definition.
#[must_use]
pub fn is_item(kind: &str, tools: Definitions<'_>) -> bool {
    tools.iter().any(|(item_type, _)| *item_type == kind)
}

fn item_result(item: &Item, definition: &Definition) -> Result<ToolResultValue, AiError> {
    if let Some(result) = definition.result {
        return result(item);
    }
    // `error: null` is not an error; only a present, non-null value is.
    if let Some(error) = item.field("error") {
        return Ok(ToolResultValue::Error { value: error.clone() });
    }
    Ok(ToolResultValue::Json { value: item.raw.clone() })
}

/// Emit the provider-executed tool call + result for a completed hosted item
/// (port of `ResponsesHostedTools.onDone`).
pub fn on_done(
    state: &ParserState,
    item: &Item,
    tools: Definitions<'_>,
) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    let Some((_, definition)) = tools.iter().find(|(kind, _)| *kind == item.kind) else {
        return Ok((state.clone(), Vec::new()));
    };
    let provider_metadata = open_responses::provider_metadata(state, json!({ "itemId": item.id }));
    let mut events = Vec::new();
    let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
    events.push(LLMEvent::ToolCall {
        id: item.id.clone(),
        name: definition.name.to_string(),
        namespace: None,
        input: (definition.input)(item),
        provider_executed: Some(true),
        provider_metadata: Some(provider_metadata.clone()),
    });
    events.push(LLMEvent::ToolResult {
        id: item.id.clone(),
        name: definition.name.to_string(),
        namespace: None,
        result: item_result(item, definition)?,
        output: None,
        provider_executed: Some(true),
        provider_metadata: Some(provider_metadata),
    });
    Ok((ParserState { lifecycle, ..state.clone() }, events))
}

/// Validate base64 image results (port of `Encoding.decodeBase64` gating) and
/// build the file content item for an `image_generation_call` result.
pub fn image_generation_result(item: &Item) -> Result<ToolResultValue, AiError> {
    let Some(result) = item.string_field("result") else {
        // no image data: fall through to the json/error default
        return Ok(if let Some(error) = item.field("error") {
            ToolResultValue::Error { value: error.clone() }
        } else {
            ToolResultValue::Json { value: item.raw.clone() }
        });
    };
    if decode_base64(result).is_none() {
        return Err(crate::protocols::shared::event_error(
            "openai-responses",
            "OpenAI Responses returned invalid image base64",
        ));
    }
    let format = item.string_field("output_format").unwrap_or("png");
    Ok(ToolResultValue::Content {
        value: crate::schema::ToolContentItems(vec![json!({
            "type": "file",
            "uri": format!("data:image/{format};base64,{result}"),
            "mime": format!("image/{format}"),
        })]),
    })
}

/// Strict base64 validation (standard alphabet, length % 4 != 1), mirroring
/// `Encoding.decodeBase64`'s rejection of invalid payloads.
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    if value.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(value.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for (index, ch) in value.bytes().enumerate() {
        if ch == b'=' {
            let remainder = value.len() - index;
            if remainder > 2 {
                return None;
            }
            break;
        }
        let digit = ALPHABET.iter().position(|&candidate| candidate == ch)? as u32;
        buffer = (buffer << 6) | digit;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosted_tools_decode_base64_like_the_ts_encoder() {
        assert_eq!(decode_base64("AQID").as_deref(), Some(&[1, 2, 3][..]));
        assert_eq!(decode_base64("QQ==").as_deref(), Some(&[65][..]));
        assert_eq!(decode_base64("QUI=").as_deref(), Some(&[65, 66][..]));
        assert_eq!(decode_base64("!!!!"), None);
        assert_eq!(decode_base64("A"), None);
        // output padding longer than the alphabet allows
        assert_eq!(decode_base64("A==="), None);
    }
}
