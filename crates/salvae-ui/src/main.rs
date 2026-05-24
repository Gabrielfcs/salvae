//! Salvaê desktop UI entry point: load the backend, spawn the worker thread,
//! and run the egui app.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use eframe::egui;

use salvae_ui::agent_backend::AgentBackend;
use salvae_ui::app::SalvaeApp;
use salvae_ui::backend::Backend;
use salvae_ui::{theme, worker};

/// Per-user Salvaê app directory (`%AppData%\salvae`).
fn app_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("salvae")
}

/// How often the worker polls the process list / sync loop.
const TICK_INTERVAL: Duration = Duration::from_secs(4);

/// Decode the embedded mascot logo into the window/taskbar icon.
fn load_window_icon() -> egui::IconData {
    let image = image::load_from_memory(salvae_ui::icon::app_icon_png())
        .expect("decode window icon")
        .to_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn main() -> eframe::Result<()> {
    let backend = match AgentBackend::load(app_dir()) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("salvae-ui: failed to load backend: {e}");
            std::process::exit(1);
        }
    };

    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (ev_tx, ev_rx) = mpsc::channel();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Salvaê")
            .with_icon(std::sync::Arc::new(load_window_icon()))
            .with_inner_size([900.0, 600.0])
            // Don't let the window shrink below the default opening size.
            .with_min_inner_size([900.0, 600.0]),
        // Restore the last window position/size on the next run (requires the
        // eframe `persistence` feature). On by default, listed for clarity.
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        // App name → on-disk storage key for the persisted window geometry.
        "salvae",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);

            // Read the name state before the worker takes ownership of backend.
            let name_set = !backend.display_name().is_empty();

            // Spawn the worker thread, waking the UI via the egui context.
            let ctx = cc.egui_ctx.clone();
            let ev_tx_worker = ev_tx;
            std::thread::spawn(move || {
                worker::run(
                    backend,
                    cmd_rx,
                    ev_tx_worker,
                    move || ctx.request_repaint(),
                    TICK_INTERVAL,
                );
            });

            let app = SalvaeApp::new(cmd_tx, ev_rx).with_name_state(name_set);
            Ok(Box::new(app))
        }),
    )
}
