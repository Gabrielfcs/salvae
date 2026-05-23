//! Token-scoped discovery: list the guilds (servers) a bot belongs to and the
//! text channels within one, so the UI can let the owner pick a channel by
//! name instead of typing snowflake ids. Read-only; no channel id required.

use std::time::Duration;

use serde_json::Value;

use salvae_vault::VaultError;

use crate::discord::{DISCORD_API_BASE, USER_AGENT};
use crate::parse::parse_snowflake;
use crate::retry::execute_with_retry;

/// Discord channel type for a normal guild text channel (`GUILD_TEXT`).
const GUILD_TEXT: u64 = 0;
/// Additional attempts after the first when rate-limited.
const MAX_RETRIES: u32 = 5;

/// A Discord guild (server) the bot is a member of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guild {
    pub id: u64,
    pub name: String,
}

/// A text channel within a guild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChannel {
    pub id: u64,
    pub name: String,
}

/// The bot's own identity (`GET /users/@me`). The id doubles as the OAuth2
/// `client_id` used to build the "add bot to server" invite URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotIdentity {
    pub id: u64,
    pub name: String,
}

/// Parse `GET /users/@me` into the bot's id + display name.
pub fn parse_me(v: &Value) -> Result<BotIdentity, VaultError> {
    let id_str = v
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| VaultError::Transport("user JSON missing string `id`".into()))?;
    Ok(BotIdentity {
        id: parse_snowflake(id_str)?,
        name: v
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

/// Parse `GET /users/@me/guilds` (array of partial guild objects).
pub fn parse_guilds(v: &Value) -> Result<Vec<Guild>, VaultError> {
    let arr = v
        .as_array()
        .ok_or_else(|| VaultError::Transport("expected a JSON array of guilds".into()))?;
    arr.iter()
        .map(|g| {
            let id_str = g
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| VaultError::Transport("guild JSON missing string `id`".into()))?;
            Ok(Guild {
                id: parse_snowflake(id_str)?,
                name: g
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

/// Parse `GET /guilds/{id}/channels`, keeping only text channels.
pub fn parse_text_channels(v: &Value) -> Result<Vec<TextChannel>, VaultError> {
    let arr = v
        .as_array()
        .ok_or_else(|| VaultError::Transport("expected a JSON array of channels".into()))?;
    let mut out = Vec::new();
    for c in arr {
        // `type` is required; non-text channels (voice, category, …) are skipped.
        let kind = c.get("type").and_then(Value::as_u64);
        if kind != Some(GUILD_TEXT) {
            continue;
        }
        let id_str = c
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| VaultError::Transport("channel JSON missing string `id`".into()))?;
        out.push(TextChannel {
            id: parse_snowflake(id_str)?,
            name: c
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
    }
    Ok(out)
}

/// A token-scoped REST client for guild/channel discovery.
#[allow(clippy::result_large_err)]
pub struct DiscordDiscovery {
    agent: ureq::Agent,
    base_url: String,
    token: String,
    max_retries: u32,
}

#[allow(clippy::result_large_err)]
impl DiscordDiscovery {
    /// Create a discovery client authenticating with bot `token`.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(30))
                .build(),
            base_url: DISCORD_API_BASE.to_string(),
            token: token.into(),
            max_retries: MAX_RETRIES,
        }
    }

    /// Override the API base URL (used by tests to point at a mock server).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    fn authed(&self, req: ureq::Request) -> ureq::Request {
        req.set("Authorization", &format!("Bot {}", self.token))
            .set("User-Agent", USER_AGENT)
    }

    fn sleep_secs(secs: f64) {
        std::thread::sleep(Duration::from_secs_f64(secs.max(0.0)));
    }

    fn get_json(&self, url: &str) -> Result<Value, VaultError> {
        let resp = execute_with_retry(self.max_retries, Self::sleep_secs, || {
            self.authed(self.agent.get(url)).call()
        })?;
        let body = resp
            .into_string()
            .map_err(|e| VaultError::Transport(e.to_string()))?;
        serde_json::from_str(&body).map_err(|e| VaultError::Transport(e.to_string()))
    }

    /// Fetch the bot's own identity, validating the token (an invalid token
    /// → 401 surfaces as a transport error).
    pub fn me(&self) -> Result<BotIdentity, VaultError> {
        let url = format!("{}/users/@me", self.base_url);
        parse_me(&self.get_json(&url)?)
    }

    /// List the guilds (servers) this bot is a member of. A failed call (e.g.
    /// an invalid token → 401) surfaces as a transport error.
    pub fn list_guilds(&self) -> Result<Vec<Guild>, VaultError> {
        let url = format!("{}/users/@me/guilds", self.base_url);
        parse_guilds(&self.get_json(&url)?)
    }

    /// List the text channels of `guild_id`.
    pub fn list_text_channels(&self, guild_id: u64) -> Result<Vec<TextChannel>, VaultError> {
        let url = format!("{}/guilds/{}/channels", self.base_url, guild_id);
        parse_text_channels(&self.get_json(&url)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_guilds_reads_id_and_name() {
        let v = serde_json::json!([
            { "id": "111", "name": "Co-op Crew" },
            { "id": "222", "name": "Other" }
        ]);
        let guilds = parse_guilds(&v).unwrap();
        assert_eq!(guilds.len(), 2);
        assert_eq!(
            guilds[0],
            Guild {
                id: 111,
                name: "Co-op Crew".into()
            }
        );
    }

    #[test]
    fn parse_text_channels_keeps_only_text() {
        let v = serde_json::json!([
            { "id": "10", "name": "saves", "type": 0 },
            { "id": "11", "name": "General Voice", "type": 2 },
            { "id": "12", "name": "category", "type": 4 }
        ]);
        let chans = parse_text_channels(&v).unwrap();
        assert_eq!(
            chans,
            vec![TextChannel {
                id: 10,
                name: "saves".into()
            }]
        );
    }

    #[test]
    fn parse_guilds_requires_an_array() {
        let v = serde_json::json!({ "id": "1" });
        assert!(matches!(parse_guilds(&v), Err(VaultError::Transport(_))));
    }

    #[test]
    fn list_guilds_authenticates_and_parses() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/users/@me/guilds")
            .match_header("authorization", "Bot tok")
            .with_status(200)
            .with_body(r#"[{"id":"111","name":"Crew"}]"#)
            .create();

        let d = DiscordDiscovery::new("tok").with_base_url(server.url());
        let guilds = d.list_guilds().unwrap();
        m.assert();
        assert_eq!(
            guilds,
            vec![Guild {
                id: 111,
                name: "Crew".into()
            }]
        );
    }

    #[test]
    fn parse_me_reads_id_and_username() {
        let v = serde_json::json!({ "id": "999", "username": "SalvaeBot" });
        assert_eq!(
            parse_me(&v).unwrap(),
            BotIdentity {
                id: 999,
                name: "SalvaeBot".into()
            }
        );
    }

    #[test]
    fn me_authenticates_and_parses() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/users/@me")
            .match_header("authorization", "Bot tok")
            .with_status(200)
            .with_body(r#"{"id":"999","username":"SalvaeBot"}"#)
            .create();

        let d = DiscordDiscovery::new("tok").with_base_url(server.url());
        let me = d.me().unwrap();
        m.assert();
        assert_eq!(me.id, 999);
        assert_eq!(me.name, "SalvaeBot");
    }

    #[test]
    fn list_text_channels_filters_and_parses() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/guilds/111/channels")
            .match_header("authorization", "Bot tok")
            .with_status(200)
            .with_body(r#"[{"id":"10","name":"saves","type":0},{"id":"11","name":"vc","type":2}]"#)
            .create();

        let d = DiscordDiscovery::new("tok").with_base_url(server.url());
        let chans = d.list_text_channels(111).unwrap();
        m.assert();
        assert_eq!(
            chans,
            vec![TextChannel {
                id: 10,
                name: "saves".into()
            }]
        );
    }
}
