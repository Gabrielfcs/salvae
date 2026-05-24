//! The egui application: renders the ViewModel and turns user input into
//! Commands. Drains worker Events each frame and minimizes to tray on close.

use std::path::PathBuf;
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
    /// Lazily-loaded mascot logo texture for the welcome screen.
    bot_logo: Option<egui::TextureHandle>,
    /// Whether the first-run consent screen has been accepted.
    consent_accepted: bool,
    /// Marker file written when consent is accepted (gates the app on first run).
    consent_path: Option<PathBuf>,
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
            bot_logo: None,
            consent_accepted: false,
            consent_path: None,
            surfaced_conflict: None,
            tray_open_id: None,
            tray_quit_id: None,
            _tray: None,
        }
    }

    /// Gate the app behind a first-run consent screen, persisting acceptance to
    /// `path` (already accepted if the marker file exists).
    pub fn with_consent(mut self, path: PathBuf) -> Self {
        self.consent_accepted = path.exists();
        self.consent_path = Some(path);
        self
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
        ui.heading("Grupos");
        ui.add_space(4.0);
        if self.vm.groups.is_empty() {
            ui.label(egui::RichText::new("Nenhum grupo ainda.").color(theme::MUTED));
        }
        for g in &self.vm.groups {
            let selected = self.forms.selected_group.as_deref() == Some(&g.id);
            if ui.selectable_label(selected, &g.name).clicked() {
                self.forms.selected_group = Some(g.id.clone());
            }
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if theme::primary_button(ui, "+ Criar grupo").clicked() {
                self.reset_create_form();
                self.forms.show_create = true;
            }
            if ui.button("Entrar em grupo").clicked() {
                self.forms.join_password.clear();
                self.forms.join_invite.clear();
                self.forms.show_join = true;
            }
        });

        if let Some(invite) = self.vm.last_invite.clone() {
            ui.separator();
            ui.label(
                egui::RichText::new("Convite para compartilhar (envie a senha por fora):")
                    .color(theme::MUTED),
            );
            ui.add(egui::TextEdit::multiline(&mut invite.clone()).desired_rows(2));
            if ui.button("Copiar convite").clicked() {
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
        egui::Window::new("Criar grupo")
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
        wizard_header(ui, "Crie seu bot do Discord", 1);
        ui.label(
            "O Salvaê guarda seus saves em um canal privado do Discord, acessado por um bot. \
             Você configura isso só uma vez por grupo.",
        );
        ui.add_space(14.0);

        // Instructions section.
        ui.label(egui::RichText::new("No Portal de Desenvolvedores do Discord").strong());
        ui.add_space(2.0);
        ui.hyperlink_to(
            "Abrir o portal ↗",
            "https://discord.com/developers/applications",
        );
        ui.label(
            egui::RichText::new("Se aparecer um questionário de boas-vindas, clique em \"Pular\".")
                .color(theme::MUTED)
                .small(),
        );
        ui.add_space(8.0);
        theme::card_frame().show(ui, |ui| {
            for (n, text) in [
                (
                    1,
                    "Clique em \"Novo aplicativo\", dê um nome (ex.: \"Salvaê\") e crie.",
                ),
                (
                    2,
                    "Defina o ícone e a descrição (opcional) e clique em \"Salvar alterações\" \
                     — se trocar de aba sem salvar, você perde essas mudanças.",
                ),
                (3, "Abra a aba \"Bot\" no menu lateral."),
                (
                    4,
                    "Clique em \"Redefinir token\", confirme, e copie o token que aparecer.",
                ),
            ] {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(format!("{n}.")).strong().color(GREEN));
                    ui.label(text);
                });
                ui.add_space(6.0);
            }
            ui.label(
                egui::RichText::new("Mantenha o token em segredo — é a chave do grupo.")
                    .color(theme::MUTED)
                    .small(),
            );
        });

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);

        // Optional defaults section.
        ui.label(egui::RichText::new("Opcional — padrões do Salvaê").strong());
        ui.label(
            egui::RichText::new("Use se não quiser personalizar o bot.")
                .color(theme::MUTED)
                .small(),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Baixar ícone do bot…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("salvae-bot.png")
                    .add_filter("PNG", &["png"])
                    .save_file()
                {
                    let _ = std::fs::write(path, crate::icon::bot_icon_png());
                }
            }
            if ui.button("Copiar descrição").clicked() {
                ui.output_mut(|o| o.copied_text = DEFAULT_BOT_DESCRIPTION.to_string());
            }
        });

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if theme::primary_button(ui, "Avançar").clicked() {
                self.forms.create_step = 1;
            }
        });
    }

    /// Step 2: paste + validate the bot token.
    fn wizard_step_token(&mut self, ui: &mut egui::Ui) {
        wizard_header(ui, "Cole o token do bot", 2);
        ui.label("Token do bot");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.forms.new_token)
                    .password(true)
                    .desired_width(300.0),
            );
            let token = self.forms.new_token.trim().to_string();
            if ui.button("Validar").clicked() && !token.is_empty() {
                self.vm.token_validated = false;
                self.send(Command::ValidateToken { token });
            }
        });
        if self.vm.token_validated {
            if let Some(name) = self.vm.bot_name.clone() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format!("✓ Conectado como {name}")).color(GREEN));
            }
        }
        ui.add_space(10.0);
        ui.separator();
        let validated = self.vm.token_validated;
        ui.horizontal(|ui| {
            if ui.button("Voltar").clicked() {
                self.forms.create_step = 0;
            }
            ui.add_enabled_ui(validated, |ui| {
                if theme::primary_button(ui, "Avançar").clicked() {
                    self.forms.create_step = 2;
                }
            });
        });
    }

    /// Step 3: invite the bot to a server, then pick the server + channel.
    fn wizard_step_channel(&mut self, ui: &mut egui::Ui) {
        wizard_header(ui, "Adicione o bot e escolha o canal", 3);
        if let Some(bot_id) = self.vm.bot_id {
            let url = bot_invite_url(bot_id);
            ui.hyperlink_to("Adicionar o bot ao seu servidor ↗", &url);
            if ui.button("Copiar link de convite").clicked() {
                ui.output_mut(|o| o.copied_text = url);
            }
        }
        ui.label(
            egui::RichText::new("Depois de autorizar no navegador, clique em Buscar servidores.")
                .color(theme::MUTED)
                .small(),
        );
        ui.add_space(6.0);
        let token = self.forms.new_token.trim().to_string();
        if ui.button("Buscar servidores").clicked() && !token.is_empty() {
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
                        "Token OK, mas o bot ainda não está em nenhum servidor. Adicione-o e tente de novo.",
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
            if ui.button("Voltar").clicked() {
                self.forms.create_step = 1;
            }
            ui.add_enabled_ui(ready, |ui| {
                if theme::primary_button(ui, "Avançar").clicked() {
                    self.forms.create_step = 3;
                }
            });
        });
    }

    /// Step 4: name the group + set the shared password, then create.
    fn wizard_step_name(&mut self, ui: &mut egui::Ui) {
        wizard_header(ui, "Dê um nome ao grupo", 4);
        ui.label("Nome do grupo");
        ui.text_edit_singleline(&mut self.forms.new_name);
        ui.add_space(4.0);
        ui.label("Senha compartilhada");
        ui.add(egui::TextEdit::singleline(&mut self.forms.new_password).password(true));
        ui.label(
            egui::RichText::new("Todos no grupo digitam essa mesma senha (combine-a por fora).")
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
            if ui.button("Voltar").clicked() {
                self.forms.create_step = 2;
            }
            ui.add_enabled_ui(ready, |ui| {
                if theme::primary_button(ui, "Criar grupo").clicked() {
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
        ui.label("Servidor");
        let prev_guild = self.forms.create_guild;
        egui::ComboBox::from_id_salt("create_guild")
            .selected_text(label_for(
                &guilds,
                self.forms.create_guild,
                "Selecione um servidor",
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
            ui.label("Canal");
            egui::ComboBox::from_id_salt("create_channel")
                .selected_text(label_for(
                    &channels,
                    self.forms.create_channel,
                    "Selecione um canal",
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
        egui::Window::new("Entrar em grupo")
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.label("Convite");
                ui.add(egui::TextEdit::multiline(&mut self.forms.join_invite).desired_rows(3));
                ui.add_space(4.0);
                ui.label("Senha compartilhada");
                ui.add(egui::TextEdit::singleline(&mut self.forms.join_password).password(true));
                ui.add_space(8.0);
                let ready = !self.forms.join_invite.trim().is_empty()
                    && !self.forms.join_password.is_empty();
                ui.add_enabled_ui(ready, |ui| {
                    if theme::primary_button(ui, "Entrar em grupo").clicked() {
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
            ui.label("Selecione um grupo à esquerda.");
            return;
        };
        let Some(group) = self.vm.groups.iter().find(|g| g.id == group_id).cloned() else {
            return;
        };

        ui.heading(format!("{} — jogos", group.name));
        if ui.button("Remover este grupo").clicked() {
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
                                egui::RichText::new(format!("Pasta: {}", m.folder))
                                    .color(theme::MUTED),
                            );
                        }
                        None => {
                            ui.label(
                                egui::RichText::new("Pasta: não definida").color(theme::MUTED),
                            );
                        }
                    }

                    ui.horizontal(|ui| {
                        if ui.button("Escolher pasta…").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.send(Command::SetGamePath {
                                    group_id: group_id.clone(),
                                    game_id: game.id.clone(),
                                    folder: path.display().to_string(),
                                });
                            }
                        }
                        if self.vm.scan_armed.contains(&game.id) {
                            if ui.button("Fechei o jogo — encontrar save").clicked() {
                                self.send(Command::CollectScan {
                                    game_id: game.id.clone(),
                                });
                            }
                            ui.label(
                                egui::RichText::new("varredura armada — abra e feche o jogo")
                                    .color(theme::MUTED),
                            );
                        } else if ui.button("Encontrar save automaticamente").clicked() {
                            self.send(Command::ArmScan {
                                game_id: game.id.clone(),
                            });
                        }
                        if ui.button("Histórico").clicked() {
                            self.send(Command::LoadHistory {
                                game_id: game.id.clone(),
                            });
                        }
                    });

                    if let Some(cands) = self.vm.scan_results.get(&game.id) {
                        ui.add_space(4.0);
                        ui.strong("Pastas candidatas a save");
                        for c in cands {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "{} ({} arquivos alterados)",
                                    c.folder.display(),
                                    c.changed_files
                                ));
                                if ui.button("Usar esta").clicked() {
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
                        ui.strong("Versões");
                        for v in versions.iter().rev() {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "v{} — {} — {}",
                                    v.number,
                                    v.author,
                                    human_size(v.size)
                                ));
                                if ui.button("Restaurar").clicked() {
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
        egui::Window::new("Conflito de save")
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!(
                    "Existe um save mais novo para {} (versão {} por {}).",
                    conflict.game_id, conflict.remote.number, conflict.remote.author
                ));
                ui.label(
                    egui::RichText::new("Sobrescrever pode perder progresso.")
                        .color(egui::Color32::from_rgb(245, 158, 11)),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, "Manter o save remoto mais novo").clicked() {
                        self.send(Command::Resolve {
                            game_id: conflict.game_id.clone(),
                            take_remote: true,
                        });
                    }
                    if ui.button("Enviar o meu como nova versão").clicked() {
                        self.send(Command::Resolve {
                            game_id: conflict.game_id.clone(),
                            take_remote: false,
                        });
                    }
                });
            });
    }

    /// Load (once) and return the mascot logo texture.
    fn bot_logo(&mut self, ctx: &egui::Context) -> egui::TextureHandle {
        self.bot_logo
            .get_or_insert_with(|| {
                // High-quality CPU downscale (Lanczos3) so it stays crisp when
                // drawn small — egui has no mipmaps, so a raw 1254px→200px GPU
                // minification would alias badly.
                let img = image::load_from_memory(crate::icon::bot_logo_png())
                    .expect("decode logo")
                    .resize(400, 400, image::imageops::FilterType::Lanczos3)
                    .to_rgba8();
                let (w, h) = img.dimensions();
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [w as usize, h as usize],
                    img.as_raw(),
                );
                ctx.load_texture("bot-logo", color, egui::TextureOptions::LINEAR)
            })
            .clone()
    }

    /// First-run consent / transparency screen. Gates the app until accepted.
    fn consent_screen(&mut self, ctx: &egui::Context) {
        let logo = self.bot_logo(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            // Roughly centre the fixed-width card vertically.
            let top = ((ui.available_height() - 520.0) * 0.32).max(12.0);
            ui.add_space(top);
            ui.vertical_centered(|ui| {
                ui.set_max_width(460.0);

                ui.image((logo.id(), egui::vec2(200.0, 200.0)));
                ui.add_space(6.0);
                ui.heading("Bem-vindo ao Salvaê");
                ui.add_space(10.0);
                ui.label(
                    "O Salvaê sincroniza os saves de jogos co-op do seu grupo por um canal \
                     privado e cifrado do Discord. Para isso, ele precisa:",
                );
                ui.add_space(10.0);
                theme::card_frame().show(ui, |ui| {
                    ui.set_max_width(460.0);
                    for line in [
                        "Detectar quando seus jogos abrem e fecham (lendo a lista de processos).",
                        "Ler e gravar apenas as pastas de save que você escolher.",
                        "Enviar os saves (cifrados) ao canal do Discord do seu grupo, pela internet.",
                        "Guardar segredos (token e chave) protegidos pela DPAPI do Windows.",
                    ] {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new("•").color(GREEN).strong());
                            ui.label(line);
                        });
                        ui.add_space(4.0);
                    }
                });
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "O Salvaê só mexe nos jogos que você configurar, e nada é enviado sem a \
                         senha do grupo.",
                    )
                    .color(theme::MUTED)
                    .small(),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, "Aceitar e continuar").clicked() {
                        self.accept_consent();
                    }
                    if ui.button("Sair").clicked() {
                        self.send(Command::Shutdown);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });
    }

    /// Record consent (in memory + a marker file) so it is asked only once.
    fn accept_consent(&mut self) {
        self.consent_accepted = true;
        if let Some(path) = self.consent_path.clone() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, b"accepted");
        }
    }

    fn activity_panel(&self, ui: &mut egui::Ui) {
        ui.heading("Atividade");
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

/// Default description offered for the bot (the user can paste it in the portal).
const DEFAULT_BOT_DESCRIPTION: &str =
    "Bot do Salvaê — sincroniza os saves de jogos co-op do nosso grupo por um \
     canal privado e cifrado do Discord.";

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
        egui::RichText::new(format!("Passo {step} de 4"))
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

        // Minimize-to-tray: intercept the window close button (always).
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // First-run consent gate: nothing else is shown until accepted.
        if !self.consent_accepted {
            self.consent_screen(ctx);
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }

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

        if let Some(err) = self.vm.last_error.clone() {
            egui::TopBottomPanel::top("error").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                    if ui.button("Dispensar").clicked() {
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
