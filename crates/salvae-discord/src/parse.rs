//! Pure parsing of Discord REST JSON into the vault's message types.

use serde_json::Value;

use salvae_vault::channel::{AttachmentRef, Message};
use salvae_vault::VaultError;

/// Parse a Discord snowflake (decimal string) into a `u64`.
pub fn parse_snowflake(s: &str) -> Result<u64, VaultError> {
    s.parse::<u64>()
        .map_err(|_| VaultError::Transport(format!("invalid Discord id: {s:?}")))
}

/// Parse one Discord message object into a vault [`Message`].
pub fn parse_message(v: &Value) -> Result<Message, VaultError> {
    let id_str = v
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| VaultError::Transport("message JSON missing string `id`".into()))?;
    let id = parse_snowflake(id_str)?;
    let content = v
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut attachments = Vec::new();
    if let Some(arr) = v.get("attachments").and_then(Value::as_array) {
        for a in arr {
            let aid_str = a.get("id").and_then(Value::as_str).ok_or_else(|| {
                VaultError::Transport("attachment JSON missing string `id`".into())
            })?;
            let aid = parse_snowflake(aid_str)?;
            let filename = a
                .get("filename")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            attachments.push(AttachmentRef { id: aid, filename });
        }
    }
    Ok(Message {
        id,
        content,
        attachments,
    })
}

/// Parse a Discord "get channel messages" array response.
pub fn parse_messages(v: &Value) -> Result<Vec<Message>, VaultError> {
    let arr = v
        .as_array()
        .ok_or_else(|| VaultError::Transport("expected a JSON array of messages".into()))?;
    arr.iter().map(parse_message).collect()
}

/// Look up the (expiring) CDN URL of the attachment named `filename` in a
/// freshly-fetched message object.
pub fn attachment_url(message: &Value, filename: &str) -> Option<String> {
    message
        .get("attachments")?
        .as_array()?
        .iter()
        .find(|a| a.get("filename").and_then(Value::as_str) == Some(filename))
        .and_then(|a| a.get("url").and_then(Value::as_str))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg_json() -> Value {
        serde_json::json!({
            "id": "100",
            "content": "the header",
            "attachments": [
                { "id": "9001", "filename": "chunk_0.bin", "url": "https://cdn.example/chunk_0.bin?ex=abc" }
            ]
        })
    }

    #[test]
    fn parses_a_message_with_attachments() {
        let m = parse_message(&msg_json()).unwrap();
        assert_eq!(m.id, 100);
        assert_eq!(m.content, "the header");
        assert_eq!(
            m.attachments,
            vec![AttachmentRef {
                id: 9001,
                filename: "chunk_0.bin".into()
            }]
        );
    }

    #[test]
    fn parses_an_array_of_messages() {
        let arr = serde_json::json!([
            { "id": "2", "content": "b", "attachments": [] },
            { "id": "1", "content": "a", "attachments": [] }
        ]);
        let msgs = parse_messages(&arr).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].id, 2);
        assert_eq!(msgs[1].id, 1);
    }

    #[test]
    fn missing_or_bad_id_is_a_transport_error() {
        assert!(matches!(
            parse_snowflake("not-a-number"),
            Err(VaultError::Transport(_))
        ));
        let bad = serde_json::json!({ "content": "x", "attachments": [] });
        assert!(matches!(parse_message(&bad), Err(VaultError::Transport(_))));
    }

    #[test]
    fn parse_messages_requires_an_array() {
        let not_array = serde_json::json!({ "id": "1" });
        assert!(matches!(
            parse_messages(&not_array),
            Err(VaultError::Transport(_))
        ));
    }

    #[test]
    fn attachment_url_looks_up_by_filename() {
        let v = msg_json();
        assert_eq!(
            attachment_url(&v, "chunk_0.bin").as_deref(),
            Some("https://cdn.example/chunk_0.bin?ex=abc")
        );
        assert_eq!(attachment_url(&v, "missing.bin"), None);
    }
}
