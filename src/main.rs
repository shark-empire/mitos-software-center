//! `mitos-software-center`: a graphical front end for `mitos-pkgd`, so
//! installing, removing, and updating software doesn't require a
//! terminal. This file just wires things together — the actual daemon
//! I/O is in `daemon_worker.rs`, the actual window contents in `ui.rs`.

mod daemon_worker;
mod ui;

use daemon_worker::AppCommand;
use std::path::PathBuf;
use std::sync::mpsc;

fn main() -> eframe::Result<()> {
    // `mitos-pkgd`'s own default, unless overridden — matches the
    // env-var-free default every other client (mitos-pkg's CLI,
    // DaemonClient's own docs) assumes.
    let socket_path = std::env::var("MITOS_PKGD_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(mitos_pkg::config::Config::DEFAULT_SOCKET));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([720.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MITOS Software Center",
        native_options,
        Box::new(move |cc| {
            let (command_tx, command_rx) = mpsc::channel::<AppCommand>();
            let (event_tx, event_rx) = mpsc::channel();

            // egui_ctx is cloned (not moved) because `cc` is only
            // borrowed here — see daemon_worker::spawn's docs for why
            // the worker thread needs its own copy at all.
            daemon_worker::spawn(socket_path, cc.egui_ctx.clone(), command_rx, event_tx);

            // Kick off an initial listing so the window isn't empty on
            // first paint.
            let _ = command_tx.send(AppCommand::List);

            Ok(Box::new(ui::SoftwareCenterApp::new(command_tx, event_rx)))
        }),
    )
}
