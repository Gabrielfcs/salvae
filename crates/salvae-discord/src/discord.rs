//! `DiscordChannel`: a live `Channel` over the Discord REST API.

use std::io::Read;
use std::time::Duration;

use salvae_vault::channel::{AttachmentRef, Channel, Message, MessageId};
use salvae_vault::{multipart, VaultError};

use crate::parse;
use crate::retry::execute_with_retry;

/// Discord REST API base (v10).
pub const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
/// Discord requires a descriptive User-Agent on every request.
const USER_AGENT: &str = "DiscordBot (https://github.com/salvae/salvae, 0.1)";
/// Additional attempts after the first when rate-limited.
const MAX_RETRIES: u32 = 5;

/// A `Channel` backed by one private Discord channel, accessed with a bot token.
pub struct DiscordChannel {
    agent: ureq::Agent,
    base_url: String,
    token: String,
    channel_id: u64,
    max_retries: u32,
}

impl DiscordChannel {
    /// Create a channel client for `channel_id` authenticating with bot `token`.
    pub fn new(token: impl Into<String>, channel_id: u64) -> Self {
        Self {
            agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build(),
            base_url: DISCORD_API_BASE.to_string(),
            token: token.into(),
            channel_id,
            max_retries: MAX_RETRIES,
        }
    }

    /// Override the API base URL (used by tests to point at a mock server).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Attach the bot auth + user-agent headers to a request.
    fn authed(&self, req: ureq::Request) -> ureq::Request {
        req.set("Authorization", &format!("Bot {}", self.token))
            .set("User-Agent", USER_AGENT)
    }

    fn sleep_secs(secs: f64) {
        std::thread::sleep(Duration::from_secs_f64(secs.max(0.0)));
    }

    /// GET channel messages (newest-first), optionally before a message id.
    pub fn fetch_messages(
        &self,
        before: Option<u64>,
        limit: u16,
    ) -> Result<Vec<Message>, VaultError> {
        let mut url =
            format!("{}/channels/{}/messages?limit={}", self.base_url, self.channel_id, limit);
        if let Some(b) = before {
            url.push_str(&format!("&before={b}"));
        }
        let resp = execute_with_retry(self.max_retries, Self::sleep_secs, || {
            self.authed(self.agent.get(&url)).call()
        })?;
        let body = resp.into_string().map_err(|e| VaultError::Transport(e.to_string()))?;
        let value: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| VaultError::Transport(e.to_string()))?;
        parse::parse_messages(&value)
    }

    /// POST a new message with `content` and the given binary attachments,
    /// using a multipart body. Returns the created message.
    pub fn create_message(
        &self,
        content: &str,
        attachments: &[(String, Vec<u8>)],
    ) -> Result<Message, VaultError> {
        let meta: Vec<serde_json::Value> = attachments
            .iter()
            .enumerate()
            .map(|(i, (name, _))| serde_json::json!({ "id": i, "filename": name }))
            .collect();
        let payload = serde_json::json!({ "content": content, "attachments": meta }).to_string();

        let boundary = random_boundary();
        let body = multipart::build_form_data(&boundary, &payload, attachments);
        let content_type = multipart::content_type(&boundary);
        let url = format!("{}/channels/{}/messages", self.base_url, self.channel_id);

        let resp = execute_with_retry(self.max_retries, Self::sleep_secs, || {
            self.authed(self.agent.post(&url))
                .set("Content-Type", &content_type)
                .send_bytes(&body)
        })?;
        let resp_body = resp.into_string().map_err(|e| VaultError::Transport(e.to_string()))?;
        let value: serde_json::Value =
            serde_json::from_str(&resp_body).map_err(|e| VaultError::Transport(e.to_string()))?;
        parse::parse_message(&value)
    }

    /// Download the bytes of the attachment named `filename` on `message_id`.
    /// Discord CDN URLs expire, so this re-fetches the message to obtain a
    /// fresh URL, then downloads it.
    pub fn fetch_attachment(&self, message_id: u64, filename: &str) -> Result<Vec<u8>, VaultError> {
        let url = format!("{}/channels/{}/messages/{}", self.base_url, self.channel_id, message_id);
        let resp = execute_with_retry(self.max_retries, Self::sleep_secs, || {
            self.authed(self.agent.get(&url)).call()
        })?;
        let body = resp.into_string().map_err(|e| VaultError::Transport(e.to_string()))?;
        let value: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| VaultError::Transport(e.to_string()))?;
        let cdn_url = parse::attachment_url(&value, filename).ok_or(VaultError::NotFound)?;

        let dl = execute_with_retry(self.max_retries, Self::sleep_secs, || {
            self.agent.get(&cdn_url).call()
        })?;
        let mut buf = Vec::new();
        dl.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| VaultError::Transport(e.to_string()))?;
        Ok(buf)
    }

    /// DELETE a message by id.
    pub fn remove_message(&self, message_id: u64) -> Result<(), VaultError> {
        let url = format!("{}/channels/{}/messages/{}", self.base_url, self.channel_id, message_id);
        execute_with_retry(self.max_retries, Self::sleep_secs, || {
            self.authed(self.agent.delete(&url)).call()
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_messages_parses_array_and_authenticates() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", mockito::Matcher::Regex(r"/channels/123/messages.*".to_string()))
            .match_header("authorization", "Bot tok")
            .with_status(200)
            .with_body(
                r#"[{"id":"10","content":"a","attachments":[]},
                    {"id":"9","content":"b","attachments":[{"id":"77","filename":"chunk_0.bin","url":"http://x/y"}]}]"#,
            )
            .create();

        let ch = DiscordChannel::new("tok", 123).with_base_url(server.url());
        let msgs = ch.fetch_messages(None, 100).unwrap();
        m.assert();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].id, 10);
        assert_eq!(msgs[1].attachments[0].filename, "chunk_0.bin");
    }

    #[test]
    fn fetch_messages_sends_before_cursor() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", mockito::Matcher::Regex(r"/channels/123/messages".to_string()))
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("limit".into(), "2".into()),
                mockito::Matcher::UrlEncoded("before".into(), "50".into()),
            ]))
            .with_status(200)
            .with_body("[]")
            .create();
        let ch = DiscordChannel::new("tok", 123).with_base_url(server.url());
        let msgs = ch.fetch_messages(Some(50), 2).unwrap();
        m.assert();
        assert!(msgs.is_empty());
    }

    #[test]
    fn create_message_sends_multipart_and_parses_response() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("POST", "/channels/123/messages")
            .match_header("authorization", "Bot tok")
            .match_header("content-type", mockito::Matcher::Regex("multipart/form-data".to_string()))
            .with_status(200)
            .with_body(r#"{"id":"55","content":"hdr","attachments":[{"id":"1","filename":"chunk_0.bin","url":"http://x"}]}"#)
            .create();

        let ch = DiscordChannel::new("tok", 123).with_base_url(server.url());
        let msg = ch
            .create_message("hdr", &[("chunk_0.bin".to_string(), vec![1u8, 2, 3])])
            .unwrap();
        m.assert();
        assert_eq!(msg.id, 55);
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].filename, "chunk_0.bin");
    }

    #[test]
    fn fetch_attachment_refetches_message_then_downloads_fresh_url() {
        let mut server = mockito::Server::new();
        let base = server.url();
        // The message's attachment URL points back at the mock "CDN".
        let msg_body = format!(
            r#"{{"id":"55","content":"hdr","attachments":[{{"id":"1","filename":"chunk_0.bin","url":"{base}/cdn/chunk_0.bin"}}]}}"#
        );
        let m_msg = server
            .mock("GET", "/channels/123/messages/55")
            .with_status(200)
            .with_body(msg_body)
            .create();
        let m_cdn = server
            .mock("GET", "/cdn/chunk_0.bin")
            .with_status(200)
            .with_body(vec![9u8, 8, 7, 6])
            .create();

        let ch = DiscordChannel::new("tok", 123).with_base_url(base);
        let bytes = ch.fetch_attachment(55, "chunk_0.bin").unwrap();
        m_msg.assert();
        m_cdn.assert();
        assert_eq!(bytes, vec![9u8, 8, 7, 6]);
    }

    #[test]
    fn fetch_attachment_missing_filename_is_not_found() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/channels/123/messages/55")
            .with_status(200)
            .with_body(r#"{"id":"55","content":"h","attachments":[]}"#)
            .create();
        let ch = DiscordChannel::new("tok", 123).with_base_url(server.url());
        let r = ch.fetch_attachment(55, "chunk_0.bin");
        m.assert();
        assert!(matches!(r, Err(VaultError::NotFound)));
    }

    #[test]
    fn remove_message_succeeds_on_204() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("DELETE", "/channels/123/messages/55")
            .match_header("authorization", "Bot tok")
            .with_status(204)
            .create();
        let ch = DiscordChannel::new("tok", 123).with_base_url(server.url());
        ch.remove_message(55).unwrap();
        m.assert();
    }

    #[test]
    fn remove_missing_message_is_not_found() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("DELETE", "/channels/123/messages/999")
            .with_status(404)
            .with_body(r#"{"message":"Unknown Message","code":10008}"#)
            .create();
        let ch = DiscordChannel::new("tok", 123).with_base_url(server.url());
        let r = ch.remove_message(999);
        m.assert();
        assert!(matches!(r, Err(VaultError::NotFound)));
    }
}

/// A random multipart boundary unlikely to appear in any attachment bytes.
fn random_boundary() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("OS RNG failure");
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("salvaeboundary{hex}")
}
