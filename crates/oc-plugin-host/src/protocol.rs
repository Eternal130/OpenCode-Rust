//! NDJSON JSON-RPC wire protocol (the spike's framing, hardened).
//!
//! Both directions speak newline-delimited JSON objects on stdio:
//!
//! * host → guest **request**: `{"jsonrpc":"2.0","id":1,"method":"load","params":{…}}`
//! * guest → host **request**: same shape, ids from a disjoint space
//!   (the guest numbers from `1 << 40` so the two spaces can never
//!   collide even though they are matched independently)
//! * **response**: `{"jsonrpc":"2.0","id":1,"result":{…}}` or
//!   `{"jsonrpc":"2.0","id":1,"error":{"message":"…"}}`
//! * **notification** (no id, both directions): fire-and-forget
//!   (`log`, `plugin.busy`, `plugin.idle`, `plugin.status`)
//!
//! Framing invariants (see the deadlock protocol in [`crate`]):
//! exactly one JSON value per `\n`-terminated line, writers only emit
//! complete lines, readers never assume partial state.

use serde_json::{json, Value};

/// Guest request ids start here (host ids count from 1) — the two
/// pending maps are independent, but disjoint numbering makes logs
/// unambiguous.
pub const GUEST_ID_BASE: u64 = 1 << 40;

/// Build a request frame.
#[must_use]
pub fn request(id: u64, method: &str, params: Value) -> String {
    let mut line = json!({"jsonrpc": "2.0", "id": id, "method": method});
    if !params.is_null() {
        line["params"] = params;
    }
    format!("{line}\n")
}

/// Build a success response frame.
#[must_use]
pub fn response(id: u64, result: Value) -> String {
    format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": id, "result": if result.is_null() { json!({ "ok": true }) } else { result }})
    )
}

/// Build an error response frame.
#[must_use]
pub fn error_response(id: u64, message: &str) -> String {
    format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": message}})
    )
}

/// Build a notification frame.
#[must_use]
pub fn notification(method: &str, params: Value) -> String {
    let mut line = json!({"jsonrpc": "2.0", "method": method});
    if !params.is_null() {
        line["params"] = params;
    }
    format!("{line}\n")
}

/// One incoming frame, classified.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A request from the peer (has both `id` and `method`).
    Request { id: u64, method: String, params: Value },
    /// A response to one of our requests.
    Response { id: u64, result: Result<Value, String> },
    /// A notification (no `id`).
    Notification { method: String, params: Value },
}

/// Parse one line into an [`Incoming`] frame. Unparseable lines are
/// logged by the caller and skipped — a buggy or hostile guest can
/// never wedge the reader.
#[must_use]
pub fn parse_line(line: &str) -> Option<Incoming> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    let object = value.as_object()?;
    let method = object.get("method").and_then(Value::as_str);
    let id = object.get("id").and_then(Value::as_u64);
    match (id, method) {
        (Some(id), Some(method)) => Some(Incoming::Request {
            id,
            method: method.to_owned(),
            params: object.get("params").cloned().unwrap_or(Value::Null),
        }),
        (Some(id), None) => {
            let result = match object.get("error") {
                Some(error) => Err(error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown guest error")
                    .to_owned()),
                None => Ok(object.get("result").cloned().unwrap_or(Value::Null)),
            };
            Some(Incoming::Response { id, result })
        }
        (None, Some(method)) => Some(Incoming::Notification {
            method: method.to_owned(),
            params: object.get("params").cloned().unwrap_or(Value::Null),
        }),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let request =
            parse_line(&request(7, "load", json!({"entry": "/tmp/x.ts"}))).expect("request parses");
        assert_eq!(
            request,
            Incoming::Request {
                id: 7,
                method: "load".into(),
                params: json!({"entry": "/tmp/x.ts"})
            }
        );
        let response = parse_line(&response(7, json!({"ok": true}))).expect("response parses");
        assert_eq!(response, Incoming::Response { id: 7, result: Ok(json!({"ok": true})) });
        let error = parse_line(&error_response(GUEST_ID_BASE + 1, "boom")).expect("error parses");
        assert_eq!(error, Incoming::Response { id: GUEST_ID_BASE + 1, result: Err("boom".into()) });
        let note = parse_line(&notification("log", json!({"message": "hi"}))).expect("note parses");
        assert_eq!(
            note,
            Incoming::Notification { method: "log".into(), params: json!({"message": "hi"}) }
        );
    }

    #[test]
    fn garbage_lines_parse_to_none() {
        assert!(parse_line("").is_none());
        assert!(parse_line("not json").is_none());
        assert!(parse_line("{\"jsonrpc\":\"2.0\"}").is_none());
    }
}
