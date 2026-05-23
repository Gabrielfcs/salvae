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
}
