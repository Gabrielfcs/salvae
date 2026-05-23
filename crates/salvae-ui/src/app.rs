//! The egui application: renders the ViewModel and turns user input into
//! Commands. Drains worker Events each frame and minimizes to tray on close.

use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;

use crate::command::{Command, Event};
use crate::theme;
use crate::view::{human_size, ActivityKind, ChannelView, GuildView};
use crate::viewmodel::ViewModel;

/// Transient form state for the create/join modals and selections.
#[derive(Default)]
struct Forms {
    selected_group: Option<String>,
    show_create: bool,
    show_join: bool,
    /// Current step of the create-group wizard (0..=3).
    create_step: u8,
    new_name: String,
    new_password: String,
    new_token: String,
    /// Server/channel chosen from the token-discovered lists.
    create_guild: Option<u64>,
    create_channel: Option<u64>,
    join_password: String,
    join_invite: String,
}

/// The eframe application.
pub struct SalvaeApp {
    vm: ViewModel,
    tx: Sender<Command>,
    rx: Receiver<Event>,
    forms: Forms,
    /// The game id of the conflict we have already forced the window open for,
    /// so a single conflict surfaces the window exactly once (the user can
    /// re-minimize while deciding).
    surfaced_conflict: Option<String>,
    /// Tray menu item ids (set by Task 9 via `with_tray`).
    tray_open_id: Option<tray_icon::menu::MenuId>,
    tray_quit_id: Option<tray_icon::menu::MenuId>,
    /// Kept alive so the tray icon is not dropped (set by Task 9).
    _tray: Option<tray_icon::TrayIcon>,
}

impl SalvaeApp {
    pub fn new(tx: Sender<Command>, rx: Receiver<Event>) -> Self {
        Self {
            vm: ViewModel::default(),
            tx,
            rx,
            forms: Forms::default(),
            surfaced_conflict: None,
            tray_open_id: None,
            tray_quit_id: None,
            _tray: None,
        }
    }

    /// Attach the tray icon + menu ids (called from Task 9).
    pub fn with_tray(
        mut self,
        tray: tray_icon::TrayIcon,
        open_id: tray_icon::menu::MenuId,
        quit_id: tray_icon::menu::MenuId,
    ) -> Self {
        self._tray = Some(tray);
        self.tray_open_id = Some(open_id);
        self.tray_quit_id = Some(quit_id);
        self
    }

    fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    /// Drain all pending worker events into the view model.
    fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            self.vm.apply(ev);
        }
    }

    /// Handle tray menu clicks: show the window or quit.
    fn poll_tray(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if Some(&ev.id) == self.tray_open_id.as_ref() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if Some(&ev.id) == self.tray_quit_id.as_ref() {
                self.send(Command::Shutdown);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn groups_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Groups");
        ui.add_space(4.0);
        if self.vm.groups.is_empty() {
            ui.label(egui::RichText::new("No groups yet.").color(theme::MUTED));
        }
        for g in &self.vm.groups {
            let selected = self.forms.selected_group.as_deref() == Some(&g.id);
            if ui.selectable_label(selected, &g.name).clicked() {
                self.forms.selected_group = Some(g.id.clone());
            }
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if theme::primary_button(ui, "+ Create group").clicked() {
                self.reset_create_form();
                self.forms.show_create = true;
            }
            if ui.button("Join group").clicked() {
                self.forms.join_password.clear();
                self.forms.join_invite.clear();
                self.forms.show_join = true;
            }
        });

        if let Some(invite) = self.vm.last_invite.clone() {
            ui.separator();
            ui.label(
                egui::RichText::new("Invite to share (send the password out-of-band):")
                    .color(theme::MUTED),
            );
            ui.add(egui::TextEdit::multiline(&mut invite.clone()).desired_rows(2));
            if ui.button("Copy invite").clicked() {
                ui.output_mut(|o| o.copied_text = invite);
            }
        }
    }

    /// Clear the create-group form and any discovered servers/channels.
    fn reset_create_form(&mut self) {
        self.forms.new_name.clear();
        self.forms.new_password.clear();
        self.forms.new_token.clear();
        self.forms.create_step = 0;
        self.forms.create_guild = None;
        self.forms.create_channel = None;
        self.vm.discovered_guilds.clear();
        self.vm.discovered_channels.clear();
        self.vm.guilds_loaded = false;
        self.vm.token_validated = false;
        self.vm.bot_id = None;
        self.vm.bot_name = None;
    }

    /// The create-group wizard: guides the owner through making a Discord bot,
    /// validating its token, adding it to a server, and picking a channel.
    fn create_modal(&mut self, ctx: &egui::Context) {
        if !self.forms.show_create {
            return;
        }
        let mut open = true;
        egui::Window::new("Create group")
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                match self.forms.create_step {
                    0 => self.wizard_step_bot(ui),
                    1 => self.wizard_step_token(ui),
                    2 => self.wizard_step_channel(ui),
                    _ => self.wizard_step_name(ui),
                }
            });
        // Honour the window's close (X) button.
        if !open {
            self.forms.show_create = false;
        }
    }

    /// Step 1: create the bot in the Discord Developer Portal.
    fn wizard_step_bot(&mut self, ui: &mut egui::Ui) {
        wizard_header(ui, "Create your Discord bot", 1);
        ui.label(
            "Salvaê keeps your saves in a private Discord channel, accessed by a bot. \
             You only set this up once per group.",
        );
        ui.add_space(8.0);
        ui.hyperlink_to(
            "Open the Discord Developer Portal ↗",
            "https://discord.com/developers/applications",
        );
        ui.add_space(8.0);
        for line in [
            "1. New Application → give it a name (e.g. \"Salvaê\").",
            "2. Open the Bot tab → Reset Token → Copy.",
            "3. Keep the token secret — it's the group's key to the channel.",
        ] {
            ui.label(egui::RichText::new(line).color(theme::MUTED));
        }
        ui.add_space(10.0);
        ui.separator();
        ui.horizontal(|ui| {
            if theme::primary_button(ui, "Next").clicked() {
                self.forms.create_step = 1;
            }
        });
    }

    /// Step 2: paste + validate the bot token.
    fn wizard_step_token(&mut self, ui: &mut egui::Ui) {
        wizard_header(ui, "Paste the bot token", 2);
        ui.label("Bot token");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.forms.new_token)
                    .password(true)
                    .desired_width(300.0),
            );
            let token = self.forms.new_token.trim().to_string();
            if ui.button("Validate").clicked() && !token.is_empty() {
                self.vm.token_validated = false;
                self.send(Command::ValidateToken { token });
            }
        });
        if self.vm.token_validated {
            if let Some(name) = self.vm.bot_name.clone() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format!("✓ Connected as {name}")).color(GREEN));
            }
        }
        ui.add_space(10.0);
        ui.separator();
        let validated = self.vm.token_validated;
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.forms.create_step = 0;
            }
            ui.add_enabled_ui(validated, |ui| {
                if theme::primary_button(ui, "Next").clicked() {
                    self.forms.create_step = 2;
                }
            });
        });
    }

    /// Step 3: invite the bot to a server, then pick the server + channel.
    fn wizard_step_channel(&mut self, ui: &mut egui::Ui) {
        wizard_header(ui, "Add the bot & choose the channel", 3);
        if let Some(bot_id) = self.vm.bot_id {
            let url = bot_invite_url(bot_id);
            ui.hyperlink_to("Add the bot to your server ↗", &url);
            if ui.button("Copy invite link").clicked() {
                ui.output_mut(|o| o.copied_text = url);
            }
        }
        ui.label(
            egui::RichText::new("After authorizing in the browser, click Find servers.")
                .color(theme::MUTED)
                .small(),
        );
        ui.add_space(6.0);
        let token = self.forms.new_token.trim().to_string();
        if ui.button("Find servers").clicked() && !token.is_empty() {
            self.forms.create_guild = None;
            self.forms.create_channel = None;
            self.vm.guilds_loaded = false;
            self.vm.discovered_guilds.clear();
            self.vm.discovered_channels.clear();
            self.send(Command::FetchGuilds { token });
        }
        if self.vm.guilds_loaded {
            ui.add_space(4.0);
            if self.vm.discovered_guilds.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Token OK, but the bot isn't in any server yet. Add it, then retry.",
                    )
                    .color(theme::MUTED),
                );
            } else {
                self.channel_pickers_ui(ui);
            }
        }
        ui.add_space(10.0);
        ui.separator();
        let ready = self.forms.create_channel.is_some();
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.forms.create_step = 1;
            }
            ui.add_enabled_ui(ready, |ui| {
                if theme::primary_button(ui, "Next").clicked() {
                    self.forms.create_step = 3;
                }
            });
        });
    }

    /// Step 4: name the group + set the shared password, then create.
    fn wizard_step_name(&mut self, ui: &mut egui::Ui) {
        wizard_header(ui, "Name your group", 4);
        ui.label("Group name");
        ui.text_edit_singleline(&mut self.forms.new_name);
        ui.add_space(4.0);
        ui.label("Shared password");
        ui.add(egui::TextEdit::singleline(&mut self.forms.new_password).password(true));
        ui.label(
            egui::RichText::new(
                "Everyone in the group types this same password (share it out-of-band).",
            )
            .color(theme::MUTED)
            .small(),
        );
        ui.add_space(10.0);
        ui.separator();
        let ready = !self.forms.new_name.trim().is_empty()
            && !self.forms.new_password.is_empty()
            && self.forms.create_guild.is_some()
            && self.forms.create_channel.is_some();
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.forms.create_step = 2;
            }
            ui.add_enabled_ui(ready, |ui| {
                if theme::primary_button(ui, "Create group").clicked() {
                    self.send(Command::CreateGroup {
                        name: self.forms.new_name.clone(),
                        password: self.forms.new_password.clone(),
                        token: self.forms.new_token.clone(),
                        guild_id: self.forms.create_guild.unwrap(),
                        channel_id: self.forms.create_channel.unwrap(),
                    });
                    self.forms.show_create = false;
                }
            });
        });
    }

    /// The server + channel dropdowns (shared by the wizard's channel step).
    fn channel_pickers_ui(&mut self, ui: &mut egui::Ui) {
        let guilds = self.vm.discovered_guilds.clone();
        let token = self.forms.new_token.trim().to_string();
        ui.label("Server");
        let prev_guild = self.forms.create_guild;
        egui::ComboBox::from_id_salt("create_guild")
            .selected_text(label_for(
                &guilds,
                self.forms.create_guild,
                "Select a server",
            ))
            .show_ui(ui, |ui| {
                for g in &guilds {
                    ui.selectable_value(&mut self.forms.create_guild, Some(g.id), &g.name);
                }
            });
        if self.forms.create_guild != prev_guild {
            self.forms.create_channel = None;
            if let Some(gid) = self.forms.create_guild {
                self.send(Command::FetchChannels {
                    token: token.clone(),
                    guild_id: gid,
                });
            }
        }
        if self.forms.create_guild.is_some() {
            let channels = self.vm.discovered_channels.clone();
            ui.add_space(4.0);
            ui.label("Channel");
            egui::ComboBox::from_id_salt("create_channel")
                .selected_text(label_for(
                    &channels,
                    self.forms.create_channel,
                    "Select a channel",
                ))
                .show_ui(ui, |ui| {
                    for c in &channels {
                        ui.selectable_value(
                            &mut self.forms.create_channel,
                            Some(c.id),
                            format!("# {}", c.name),
                        );
                    }
                });
        }
    }

    /// The join-group dialog: paste an invite + the shared password.
    fn join_modal(&mut self, ctx: &egui::Context) {
        if !self.forms.show_join {
            return;
        }
        let mut open = true;
        egui::Window::new("Join group")
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.label("Invite");
                ui.add(egui::TextEdit::multiline(&mut self.forms.join_invite).desired_rows(3));
                ui.add_space(4.0);
                ui.label("Shared password");
                ui.add(egui::TextEdit::singleline(&mut self.forms.join_password).password(true));
                ui.add_space(8.0);
                let ready = !self.forms.join_invite.trim().is_empty()
                    && !self.forms.join_password.is_empty();
                ui.add_enabled_ui(ready, |ui| {
                    if theme::primary_button(ui, "Join group").clicked() {
                        self.send(Command::JoinGroup {
                            password: self.forms.join_password.clone(),
                            invite: self.forms.join_invite.clone(),
                        });
                        self.forms.show_join = false;
                    }
                });
            });
        if !open {
            self.forms.show_join = false;
        }
    }

    fn games_panel(&mut self, ui: &mut egui::Ui) {
        let Some(group_id) = self.forms.selected_group.clone() else {
            ui.label("Select a group on the left.");
            return;
        };
        let Some(group) = self.vm.groups.iter().find(|g| g.id == group_id).cloned() else {
            return;
        };

        ui.heading(format!("{} — games", group.name));
        if ui.button("Remove this group").clicked() {
            self.send(Command::RemoveGroup {
                group_id: group_id.clone(),
            });
            self.forms.selected_group = None;
            return;
        }
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for game in &self.vm.installed_games {
                let mapping = group.games.iter().find(|m| m.game_id == game.id);
                theme::card_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&game.name);
                        ui.label(egui::RichText::new(format!("({})", game.id)).color(theme::MUTED));
                    });
                    match mapping {
                        Some(m) => {
                            ui.label(
                                egui::RichText::new(format!("Folder: {}", m.folder))
                                    .color(theme::MUTED),
                            );
                        }
                        None => {
                            ui.label(egui::RichText::new("Folder: not set").color(theme::MUTED));
                        }
                    }

                    ui.horizontal(|ui| {
                        if ui.button("Choose folder…").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.send(Command::SetGamePath {
                                    group_id: group_id.clone(),
                                    game_id: game.id.clone(),
                                    folder: path.display().to_string(),
                                });
                            }
                        }
                        if self.vm.scan_armed.contains(&game.id) {
                            if ui.button("I've closed the game — find save").clicked() {
                                self.send(Command::CollectScan {
                                    game_id: game.id.clone(),
                                });
                            }
                            ui.label(
                                egui::RichText::new("scan armed — launch & close the game")
                                    .color(theme::MUTED),
                            );
                        } else if ui.button("Auto-find save (scan)").clicked() {
                            self.send(Command::ArmScan {
                                game_id: game.id.clone(),
                            });
                        }
                        if ui.button("History").clicked() {
                            self.send(Command::LoadHistory {
                                game_id: game.id.clone(),
                            });
                        }
                    });

                    if let Some(cands) = self.vm.scan_results.get(&game.id) {
                        ui.add_space(4.0);
                        ui.strong("Candidate save folders");
                        for c in cands {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "{} ({} files changed)",
                                    c.folder.display(),
                                    c.changed_files
                                ));
                                if ui.button("Use this").clicked() {
                                    self.send(Command::SetGamePath {
                                        group_id: group_id.clone(),
                                        game_id: game.id.clone(),
                                        folder: c.folder.display().to_string(),
                                    });
                                }
                            });
                        }
                    }

                    if let Some(versions) = self.vm.history.get(&game.id) {
                        ui.add_space(4.0);
                        ui.strong("Versions");
                        for v in versions.iter().rev() {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "v{} — {} — {}",
                                    v.number,
                                    v.author,
                                    human_size(v.size)
                                ));
                                if ui.button("Restore").clicked() {
                                    self.send(Command::Restore {
                                        game_id: game.id.clone(),
                                        version: v.number,
                                    });
                                }
                            });
                        }
                    }
                });
                ui.add_space(8.0);
            }
        });
    }

    fn conflict_modal(&mut self, ctx: &egui::Context) {
        let Some(conflict) = self.vm.pending_conflicts.first().cloned() else {
            return;
        };
        egui::Window::new("Save conflict")
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!(
                    "A newer save exists for {} (version {} by {}).",
                    conflict.game_id, conflict.remote.number, conflict.remote.author
                ));
                ui.label(
                    egui::RichText::new("Overwriting it may lose progress.")
                        .color(egui::Color32::from_rgb(245, 158, 11)),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, "Keep the newer remote save").clicked() {
                        self.send(Command::Resolve {
                            game_id: conflict.game_id.clone(),
                            take_remote: true,
                        });
                    }
                    if ui.button("Upload mine as a new version").clicked() {
                        self.send(Command::Resolve {
                            game_id: conflict.game_id.clone(),
                            take_remote: false,
                        });
                    }
                });
            });
    }

    fn activity_panel(&self, ui: &mut egui::Ui) {
        ui.heading("Activity");
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for a in &self.vm.activity {
                    let color = match a.kind {
                        ActivityKind::Info => theme::MUTED,
                        ActivityKind::Warning => egui::Color32::from_rgb(245, 158, 11), // amber-500
                        ActivityKind::Error => egui::Color32::from_rgb(239, 68, 68),    // red-500
                    };
                    ui.colored_label(color, &a.message);
                }
            });
    }
}

/// Paint a full-window dim layer behind an open dialog so the panels can't be
/// interacted with (egui 0.29 has no built-in modal). Sits on `Middle` order —
/// strictly below the dialog windows (`Foreground`) so it never covers them.
/// Returns `true` if the backdrop itself was clicked (dismiss the dialog).
fn modal_shield(ctx: &egui::Context) -> bool {
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("modal_shield"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(160));
            // Swallow input meant for the panels; report a click for dismissal.
            ui.allocate_rect(screen, egui::Sense::click()).clicked()
        })
        .inner
}

/// Confirmation green (Tailwind green-500) for the "connected" check.
const GREEN: egui::Color32 = egui::Color32::from_rgb(34, 197, 94);

/// Minimal bot permissions for Salvaê: View Channel, Send Messages, Manage
/// Messages (prune old versions), Read Message History, Attach Files.
const BOT_PERMISSIONS: u64 = 1024 + 2048 + 8192 + 65536 + 32768;

/// The OAuth2 "add bot to server" URL for a bot/application id.
fn bot_invite_url(bot_id: u64) -> String {
    format!(
        "https://discord.com/oauth2/authorize?client_id={bot_id}&scope=bot&permissions={BOT_PERMISSIONS}"
    )
}

/// A wizard step's heading + "Step N of 4" subtitle.
fn wizard_header(ui: &mut egui::Ui, title: &str, step: u8) {
    ui.heading(title);
    ui.label(
        egui::RichText::new(format!("Step {step} of 4"))
            .color(theme::MUTED)
            .small(),
    );
    ui.add_space(8.0);
}

/// A picker item with a numeric id and a display name (server or channel).
trait IdName {
    fn item_id(&self) -> u64;
    fn item_name(&self) -> &str;
}
impl IdName for GuildView {
    fn item_id(&self) -> u64 {
        self.id
    }
    fn item_name(&self) -> &str {
        &self.name
    }
}
impl IdName for ChannelView {
    fn item_id(&self) -> u64 {
        self.id
    }
    fn item_name(&self) -> &str {
        &self.name
    }
}

/// The display name of the `selected` id within `items`, or `placeholder`.
fn label_for<T: IdName>(items: &[T], selected: Option<u64>, placeholder: &str) -> String {
    match selected {
        Some(id) => items
            .iter()
            .find(|i| i.item_id() == id)
            .map(|i| i.item_name().to_string())
            .unwrap_or_else(|| placeholder.to_string()),
        None => placeholder.to_string(),
    }
}

impl eframe::App for SalvaeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        self.poll_tray(ctx);

        // A conflict is the core safety prompt: force the window visible once
        // when a new one arrives, even if minimized to tray.
        match self.vm.pending_conflicts.first() {
            Some(c) if self.surfaced_conflict.as_deref() != Some(&c.game_id) => {
                self.surfaced_conflict = Some(c.game_id.clone());
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            None => self.surfaced_conflict = None,
            _ => {}
        }

        // Minimize-to-tray: intercept the window close button.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        if let Some(err) = self.vm.last_error.clone() {
            egui::TopBottomPanel::top("error").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                    if ui.button("Dismiss").clicked() {
                        self.vm.last_error = None;
                    }
                });
            });
        }

        let side_frame =
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::same(16.0));
        let central_frame =
            egui::Frame::central_panel(&ctx.style()).inner_margin(egui::Margin::same(16.0));

        egui::SidePanel::left("groups")
            .resizable(false)
            .exact_width(264.0)
            .frame(side_frame)
            .show(ctx, |ui| {
                self.groups_panel(ui);
            });
        egui::TopBottomPanel::bottom("activity")
            .resizable(false)
            .exact_height(150.0)
            .frame(side_frame)
            .show(ctx, |ui| {
                self.activity_panel(ui);
            });
        egui::CentralPanel::default()
            .frame(central_frame)
            .show(ctx, |ui| {
                self.games_panel(ui);
            });

        // Dim + block the panels behind the create/join dialogs; a click on the
        // backdrop dismisses them. (The conflict prompt requires an explicit
        // choice, so it has no dismissable backdrop.)
        if (self.forms.show_create || self.forms.show_join) && modal_shield(ctx) {
            self.forms.show_create = false;
            self.forms.show_join = false;
        }
        self.create_modal(ctx);
        self.join_modal(ctx);
        self.conflict_modal(ctx);

        // Tray menu clicks arrive on a global channel that eframe does not
        // observe, so schedule a low-frequency wake (also a fallback for worker
        // events). This is a timer, not a busy loop — idle stays cheap.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
