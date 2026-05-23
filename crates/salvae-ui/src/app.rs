//! The egui application: renders the ViewModel and turns user input into
//! Commands. Drains worker Events each frame and minimizes to tray on close.

use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;

use crate::command::{Command, Event};
use crate::view::{human_size, ActivityKind};
use crate::viewmodel::ViewModel;

/// Transient form state for the create/join group panels and selections.
#[derive(Default)]
struct Forms {
    selected_group: Option<String>,
    new_name: String,
    new_password: String,
    new_token: String,
    new_guild: String,
    new_channel: String,
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
        for g in &self.vm.groups {
            let selected = self.forms.selected_group.as_deref() == Some(&g.id);
            if ui.selectable_label(selected, &g.name).clicked() {
                self.forms.selected_group = Some(g.id.clone());
            }
        }
        ui.separator();

        ui.collapsing("Create group", |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut self.forms.new_name);
            ui.label("Password");
            ui.add(egui::TextEdit::singleline(&mut self.forms.new_password).password(true));
            ui.label("Bot token");
            ui.add(egui::TextEdit::singleline(&mut self.forms.new_token).password(true));
            ui.label("Guild id");
            ui.text_edit_singleline(&mut self.forms.new_guild);
            ui.label("Channel id");
            ui.text_edit_singleline(&mut self.forms.new_channel);
            if ui.button("Create").clicked() {
                if let (Ok(guild_id), Ok(channel_id)) = (
                    self.forms.new_guild.trim().parse::<u64>(),
                    self.forms.new_channel.trim().parse::<u64>(),
                ) {
                    self.send(Command::CreateGroup {
                        name: self.forms.new_name.clone(),
                        password: self.forms.new_password.clone(),
                        token: self.forms.new_token.clone(),
                        guild_id,
                        channel_id,
                    });
                } else {
                    // Surface in both the banner and the activity log.
                    self.vm
                        .apply(Event::Error("Guild/Channel id must be numbers".into()));
                }
            }
        });

        ui.collapsing("Join group", |ui| {
            ui.label("Invite");
            ui.text_edit_multiline(&mut self.forms.join_invite);
            ui.label("Password");
            ui.add(egui::TextEdit::singleline(&mut self.forms.join_password).password(true));
            if ui.button("Join").clicked() {
                self.send(Command::JoinGroup {
                    password: self.forms.join_password.clone(),
                    invite: self.forms.join_invite.clone(),
                });
            }
        });

        if let Some(invite) = self.vm.last_invite.clone() {
            ui.separator();
            ui.label("Invite to share (also send the password out-of-band):");
            ui.add(egui::TextEdit::multiline(&mut invite.clone()).desired_rows(2));
            if ui.button("Copy invite").clicked() {
                ui.output_mut(|o| o.copied_text = invite);
            }
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
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&game.name);
                        ui.label(format!("({})", game.id));
                    });
                    match mapping {
                        Some(m) => {
                            ui.label(format!("Folder: {}", m.folder));
                        }
                        None => {
                            ui.label("Folder: not set");
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
                            ui.label("scan armed: launch & close the game");
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
                        ui.label("Candidate save folders:");
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
                        ui.label("Versions:");
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
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!(
                    "A newer save exists for {} (version {} by {}).",
                    conflict.game_id, conflict.remote.number, conflict.remote.author
                ));
                ui.label("Overwriting it may lose progress.");
                ui.horizontal(|ui| {
                    if ui.button("Keep the newer remote save").clicked() {
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
                        ActivityKind::Info => egui::Color32::GRAY,
                        ActivityKind::Warning => egui::Color32::YELLOW,
                        ActivityKind::Error => egui::Color32::LIGHT_RED,
                    };
                    ui.colored_label(color, &a.message);
                }
            });
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

        egui::SidePanel::left("groups")
            .default_width(240.0)
            .show(ctx, |ui| {
                self.groups_panel(ui);
            });
        egui::TopBottomPanel::bottom("activity")
            .default_height(140.0)
            .show(ctx, |ui| {
                self.activity_panel(ui);
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.games_panel(ui);
        });
        self.conflict_modal(ctx);

        // Tray menu clicks arrive on a global channel that eframe does not
        // observe, so schedule a low-frequency wake (also a fallback for worker
        // events). This is a timer, not a busy loop — idle stays cheap.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
