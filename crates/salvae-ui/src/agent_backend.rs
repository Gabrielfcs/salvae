//! The real `Backend`: a `ConfigStore` + a per-group `Agent` over Discord.
//! Rebuilds the agent whenever group config changes so
//! history/restore/resolve/tick see fresh state.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use salvae_agent::agent::{Agent, GroupRuntime};
use salvae_agent::outcome::AgentOutcome;
use salvae_config::dpapi::DpapiSecretStore;
use salvae_config::store::ConfigStore;
use salvae_core::version::SaveVersion;
use salvae_detect::game::InstalledGame;
use salvae_detect::manifest::{Manifest, Placeholders};
use salvae_detect::roots::save_search_roots;
use salvae_detect::{epic, resolve, steam};
use salvae_discord::discord::DiscordChannel;
use salvae_discord::discover::DiscordDiscovery;
use salvae_sync::engine::{PushOutcome, Resolution, SyncEngine};
use salvae_sync::state::SyncState;
use salvae_watch::detector::{Detector, GameEvent};
use salvae_watch::process::Watcher;
use salvae_watch::system::SystemProcessLister;

use crate::backend::Backend;
use crate::command::Event;
use crate::view::{
    ActivityView, ChannelView, GameMapping, GameView, GroupView, GuildView, VersionView,
};

type DiscordAgent = Agent<DiscordChannel, SystemProcessLister>;

/// Production backend.
pub struct AgentBackend {
    store: ConfigStore<DpapiSecretStore>,
    games: Vec<InstalledGame>,
    agent: DiscordAgent,
    app_dir: PathBuf,
    manifest: Manifest,
}

impl AgentBackend {
    /// Load config from `app_dir`, enumerate games, and build the agent.
    pub fn load(app_dir: PathBuf) -> Result<Self, String> {
        let secrets = DpapiSecretStore::new(app_dir.join("secrets.dat"));
        let store = ConfigStore::load_or_default(app_dir.join("config.toml"), secrets)
            .map_err(|e| e.to_string())?;
        let games = enumerate_games();
        let agent = build_agent(&store, &games, &app_dir)?;
        Ok(Self {
            store,
            games,
            agent,
            app_dir,
            manifest: Manifest::embedded(),
        })
    }

    /// The installed game with this id, if known.
    fn game(&self, game_id: &str) -> Option<&InstalledGame> {
        self.games.iter().find(|g| g.id == game_id)
    }

    /// Apply a config change by swapping in fresh per-group runtimes, keeping
    /// the agent's watcher/detector (and their live process/open-game state).
    fn rebuild_agent(&mut self) -> Result<(), String> {
        let groups = build_groups(&self.store, &self.app_dir)?;
        self.agent.set_groups(groups);
        Ok(())
    }

    /// Find an installed game's display name by id (falls back to the id).
    fn game_name(&self, game_id: &str) -> String {
        self.game(game_id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| game_id.to_string())
    }
}

/// Parse the Steam AppID out of an id like `steam:892970`.
fn steam_id_of(game_id: &str) -> Option<u64> {
    game_id.strip_prefix("steam:").and_then(|n| n.parse().ok())
}

/// Turn a raw sync error into a clear, actionable message for common cases.
fn friendly_sync_error(raw: &str) -> String {
    if raw.contains("403") {
        "Sem acesso ao canal do Discord (403). Confira se o bot pode ver o canal, \
         ver o histórico, enviar mensagens e anexar arquivos nele."
            .to_string()
    } else if raw.contains("401") {
        "Token do bot inválido (401). Refaça o grupo com um token válido.".to_string()
    } else if raw.contains("404") {
        "Canal do Discord não encontrado (404). Verifique o canal escolhido.".to_string()
    } else {
        format!("Falha na sincronização: {raw}")
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn to_version_view(v: &SaveVersion) -> VersionView {
    VersionView {
        number: v.number,
        author: v.author.clone(),
        size: v.size_bytes,
        created_at_ms: v.created_at_ms,
    }
}

/// Enumerate Steam + Epic games from the standard locations (mirrors the agent
/// binary).
fn enumerate_games() -> Vec<InstalledGame> {
    let mut games = Vec::new();
    let steam_root = PathBuf::from(r"C:\Program Files (x86)\Steam");
    if steam_root.exists() {
        if let Ok(s) = steam::enumerate(&steam_root) {
            games.extend(s);
        }
    }
    let program_data =
        std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".to_string());
    let epic_manifests = PathBuf::from(program_data)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");
    if let Ok(e) = epic::enumerate(&epic_manifests) {
        games.extend(e);
    }
    games
}

/// Build the per-group runtimes for the current config (mirrors the agent
/// binary). The watcher/detector are built once at load and reused.
fn build_groups(
    store: &ConfigStore<DpapiSecretStore>,
    app_dir: &Path,
) -> Result<Vec<GroupRuntime<DiscordChannel>>, String> {
    let device_id = store.device_id().to_string();
    let backups_dir = app_dir.join("backups");
    let mut groups = Vec::new();
    for group in store.groups() {
        let secret = store.group_secret(&group.id).map_err(|e| e.to_string())?;
        let channel = DiscordChannel::new(secret.token, group.channel_id);
        let state_path = app_dir.join("state").join(format!("{}.json", group.id));
        let state = SyncState::load(&state_path).map_err(|e| e.to_string())?;
        let engine = SyncEngine::new(
            channel,
            secret.key,
            device_id.clone(),
            device_id.clone(),
            group.max_versions,
            backups_dir.clone(),
        )
        .with_state(state);
        groups.push(GroupRuntime::new(group.clone(), engine, state_path));
    }
    Ok(groups)
}

/// Build a fresh agent for the current config (mirrors the agent binary).
fn build_agent(
    store: &ConfigStore<DpapiSecretStore>,
    games: &[InstalledGame],
    app_dir: &Path,
) -> Result<DiscordAgent, String> {
    let groups = build_groups(store, app_dir)?;
    let watcher = Watcher::new(SystemProcessLister);
    let detector = Detector::new(games.to_vec());
    Ok(Agent::new(watcher, detector, groups))
}

impl Backend for AgentBackend {
    fn refresh_groups(&self) -> Vec<GroupView> {
        self.store
            .groups()
            .iter()
            .map(|g| GroupView {
                id: g.id.clone(),
                name: g.name.clone(),
                games: g
                    .game_paths
                    .iter()
                    .map(|(game_id, folder)| GameMapping {
                        game_id: game_id.clone(),
                        folder: folder.clone(),
                    })
                    .collect(),
            })
            .collect()
    }

    fn installed_games(&self) -> Vec<GameView> {
        self.games
            .iter()
            .map(|g| GameView {
                id: g.id.clone(),
                name: g.name.clone(),
            })
            .collect()
    }

    fn validate_token(&self, token: &str) -> Result<(u64, String), String> {
        let me = DiscordDiscovery::new(token)
            .me()
            .map_err(|e| e.to_string())?;
        Ok((me.id, me.name))
    }

    fn fetch_guilds(&self, token: &str) -> Result<Vec<GuildView>, String> {
        let guilds = DiscordDiscovery::new(token)
            .list_guilds()
            .map_err(|e| e.to_string())?;
        Ok(guilds
            .into_iter()
            .map(|g| GuildView {
                id: g.id,
                name: g.name,
            })
            .collect())
    }

    fn fetch_channels(&self, token: &str, guild_id: u64) -> Result<Vec<ChannelView>, String> {
        let channels = DiscordDiscovery::new(token)
            .list_text_channels(guild_id)
            .map_err(|e| e.to_string())?;
        Ok(channels
            .into_iter()
            .map(|c| ChannelView {
                id: c.id,
                name: c.name,
            })
            .collect())
    }

    fn create_group(
        &mut self,
        name: &str,
        password: &str,
        token: &str,
        guild_id: u64,
        channel_id: u64,
    ) -> Result<String, String> {
        let (_g, invite) = self
            .store
            .create_group(name, password, token, guild_id, channel_id)
            .map_err(|e| e.to_string())?;
        self.rebuild_agent()?;
        Ok(invite)
    }

    fn join_group(&mut self, password: &str, invite: &str) -> Result<(), String> {
        self.store
            .join_group(password, invite)
            .map_err(|e| e.to_string())?;
        self.rebuild_agent()
    }

    fn remove_group(&mut self, group_id: &str) -> Result<(), String> {
        self.store
            .remove_group(group_id)
            .map_err(|e| e.to_string())?;
        self.rebuild_agent()
    }

    fn set_game_path(&mut self, group_id: &str, game_id: &str, folder: &str) -> Result<(), String> {
        self.store
            .set_game_path(group_id, game_id, folder)
            .map_err(|e| e.to_string())?;
        self.rebuild_agent()
    }

    fn enable_sync(&mut self, group_id: &str, game_id: &str) -> Result<Option<String>, String> {
        let name = self.game_name(game_id);
        let install_dir = self.game(game_id).map(|g| g.install_dir.clone());
        // Search the standard save roots plus the game's install dir.
        let mut roots = save_search_roots();
        if let Some(dir) = &install_dir {
            roots.push(dir.clone());
        }
        let placeholders = Placeholders::live(install_dir);
        let resolved = resolve::find_save_dir(
            &name,
            steam_id_of(game_id),
            &self.manifest,
            &placeholders,
            &roots,
        );
        match resolved {
            Some(folder) => {
                let folder = folder.display().to_string();
                self.store
                    .set_game_path(group_id, game_id, &folder)
                    .map_err(|e| e.to_string())?;
                self.rebuild_agent()?;
                Ok(Some(folder))
            }
            None => Ok(None),
        }
    }

    fn disable_sync(&mut self, group_id: &str, game_id: &str) -> Result<(), String> {
        self.store
            .remove_game_path(group_id, game_id)
            .map_err(|e| e.to_string())?;
        self.rebuild_agent()
    }

    fn history(&mut self, game_id: &str) -> Result<Vec<VersionView>, String> {
        let versions = self.agent.history(game_id).map_err(|e| e.to_string())?;
        Ok(versions.iter().map(to_version_view).collect())
    }

    fn restore(&mut self, game_id: &str, version: u64) -> Result<(), String> {
        self.agent
            .restore(game_id, version, now_ms())
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn resolve(&mut self, game_id: &str, take_remote: bool) -> Result<(), String> {
        let resolution = if take_remote {
            Resolution::TakeRemote
        } else {
            Resolution::PushLocal
        };
        self.agent
            .handle_resolve(game_id, resolution, now_ms())
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn tick(&mut self) -> Vec<Event> {
        let now = now_ms();
        let results = match self.agent.tick(now) {
            Ok(r) => r,
            Err(e) => return vec![Event::Error(friendly_sync_error(&e.to_string()))],
        };
        let mut events = Vec::new();
        for (game_event, outcome) in results {
            let game_id = match &game_event {
                GameEvent::Opened { game_id } | GameEvent::Closed { game_id } => game_id.clone(),
            };
            let name = self.game_name(&game_id);
            match outcome {
                AgentOutcome::Opened {
                    pull,
                    others_playing,
                } => {
                    events.push(Event::Activity(ActivityView::info(format!(
                        "{name} aberto — baixou a versão mais recente ({pull:?})"
                    ))));
                    if !others_playing.is_empty() {
                        events.push(Event::Activity(ActivityView::warning(format!(
                            "Também jogando {name} agora: {}",
                            others_playing.join(", ")
                        ))));
                    }
                }
                AgentOutcome::Closed { push } => match push {
                    PushOutcome::Conflict { remote } => events.push(Event::Conflict {
                        game_id: game_id.clone(),
                        remote: to_version_view(&remote),
                    }),
                    PushOutcome::Pushed(v) => events.push(Event::Activity(ActivityView::info(
                        format!("{name} fechado — enviou a versão {}", v.number),
                    ))),
                    PushOutcome::NoChange(n) => events.push(Event::Activity(ActivityView::info(
                        format!("{name} fechado — já está atualizado (v{n})"),
                    ))),
                },
                AgentOutcome::Restored { version } => {
                    events.push(Event::Activity(ActivityView::info(format!(
                        "{name} restaurado para a versão {}",
                        version.number
                    ))))
                }
                AgentOutcome::NotConfigured => {}
                AgentOutcome::NoFolder => events.push(Event::Activity(ActivityView::warning(
                    format!("{name} fechado, mas a pasta de save não existe"),
                ))),
            }
        }
        events
    }
}
