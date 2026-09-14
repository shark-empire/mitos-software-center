//! Runs all `mitos-pkgd` interaction on a background thread, so the UI
//! thread (`ui.rs`) never blocks on daemon I/O — an install can take
//! however long a download takes; the window still has to redraw at 60
//! fps while that happens. Talks to the UI via two channels:
//! [`AppCommand`] in, [`AppEvent`] out.
//!
//! Reconnects to the daemon fresh for every command rather than holding
//! one connection open for the app's whole lifetime. A long-lived
//! connection would need its own reconnect-on-broken-pipe handling for
//! whenever `mitos-pkgd` restarts; connecting per command sidesteps that
//! entirely, and installs/removals/searches are infrequent enough
//! (human-triggered, not a tight loop) that one extra `connect()` per
//! action costs nothing anyone would notice.

use mitos_pkg::daemon::client::DaemonClient;
use mitos_pkg::database::packages::InstalledPackage;
use mitos_pkg::repository::metadata::PackageMetadata;
use mitos_pkg::service::ProgressEvent;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;

/// What the UI asks the worker to do.
#[derive(Debug, Clone)]
pub enum AppCommand {
    List,
    Search(String),
    Install(String),
    Remove(String),
    Update,
}

/// What the worker reports back. `Listed`/`Searched` replace the UI's
/// corresponding list outright (not incremental updates); `Progress`
/// streams as an install/remove/update runs; `Done` closes out whichever
/// command produced it, successfully or not.
#[derive(Debug, Clone)]
pub enum AppEvent {
    Listed(Vec<(String, InstalledPackage)>),
    Searched(Vec<PackageMetadata>),
    Progress {
        action: String,
        event: ProgressEvent,
    },
    Done {
        action: String,
        /// Whether this command could have changed what's installed —
        /// if so, the UI queues a follow-up `List` once this arrives,
        /// so the installed panel doesn't go stale after an
        /// install/remove/update. `List`/`Search` themselves are not
        /// list-changing in this sense.
        refreshes_list: bool,
        result: Result<String, String>,
    },
}

/// Spawns the worker thread and returns immediately. `ctx` is used
/// purely to wake the UI up (`request_repaint`) right after an event is
/// sent, instead of the new state sitting unseen until the next OS
/// input event happens to trigger a redraw — `egui::Context` is
/// `Clone + Send + Sync` specifically so a background thread can do
/// this.
pub fn spawn(
    socket_path: PathBuf,
    ctx: egui::Context,
    commands: Receiver<AppCommand>,
    events: Sender<AppEvent>,
) {
    thread::spawn(move || {
        for command in commands {
            let action_label = describe(&command);
            let refreshes_list = matches!(
                command,
                AppCommand::Install(_) | AppCommand::Remove(_) | AppCommand::Update
            );

            let result = run_command(&socket_path, command, &events, &action_label);

            let _ = events.send(AppEvent::Done {
                action: action_label,
                refreshes_list,
                result,
            });
            ctx.request_repaint();
        }
    });
}

fn describe(command: &AppCommand) -> String {
    match command {
        AppCommand::List => "list".to_string(),
        AppCommand::Search(query) => format!("search '{query}'"),
        AppCommand::Install(name) => format!("install {name}"),
        AppCommand::Remove(name) => format!("remove {name}"),
        AppCommand::Update => "update".to_string(),
    }
}

/// Connects fresh, runs one command, and returns a short human-readable
/// summary on success or an error message on failure — either way
/// becomes the text in `AppEvent::Done`.
fn run_command(
    socket_path: &Path,
    command: AppCommand,
    events: &Sender<AppEvent>,
    action_label: &str,
) -> Result<String, String> {
    let mut client = DaemonClient::connect(socket_path).map_err(|e| {
        format!(
            "couldn't reach mitos-pkgd at {}: {e}",
            socket_path.display()
        )
    })?;

    // Shared by every command below that streams progress
    // (install/remove/update). Doesn't need `move`/`clone` — it only
    // borrows `events` and `action_label`, both `Copy` references, and
    // is used at most once per call to `run_command` (whichever single
    // match arm below actually runs).
    let emit_progress = |progress: ProgressEvent| {
        let _ = events.send(AppEvent::Progress {
            action: action_label.to_string(),
            event: progress,
        });
    };

    match command {
        AppCommand::List => {
            let packages = client.list().map_err(|e| e.to_string())?;
            let count = packages.len();
            let _ = events.send(AppEvent::Listed(packages));
            Ok(format!("{count} package(s) installed"))
        }
        AppCommand::Search(query) => {
            let matches = client.search(&query).map_err(|e| e.to_string())?;
            let count = matches.len();
            let _ = events.send(AppEvent::Searched(matches));
            Ok(format!("{count} match(es) for '{query}'"))
        }
        AppCommand::Install(name) => {
            client
                .install(&name, emit_progress)
                .map_err(|e| e.to_string())?;
            Ok(format!("installed {name}"))
        }
        AppCommand::Remove(name) => {
            let removed = client
                .remove(&name, false, false, emit_progress)
                .map_err(|e| e.to_string())?;
            Ok(format!("removed {}", removed.join(", ")))
        }
        AppCommand::Update => {
            client.update(emit_progress).map_err(|e| e.to_string())?;
            Ok("index refreshed".to_string())
        }
    }
}
