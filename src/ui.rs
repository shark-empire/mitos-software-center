//! The window contents. All state here is driven by [`AppEvent`]s
//! arriving from [`crate::daemon_worker`] — this module never talks to
//! `mitos-pkgd` directly, it only sends [`AppCommand`]s and reacts to
//! whatever comes back.

use crate::daemon_worker::{AppCommand, AppEvent};
use mitos_pkg::database::packages::InstalledPackage;
use mitos_pkg::repository::metadata::PackageMetadata;
use mitos_pkg::service::ProgressEvent;
use std::sync::mpsc::{Receiver, Sender};

const MAX_LOG_LINES: usize = 200;

pub struct SoftwareCenterApp {
    commands: Sender<AppCommand>,
    events: Receiver<AppEvent>,

    search_query: String,
    installed: Vec<(String, InstalledPackage)>,
    search_results: Vec<PackageMetadata>,
    log: Vec<String>,
    /// True between sending a command and its `Done` arriving — disables
    /// the buttons that would start another command, since every
    /// command right now runs one at a time (matching `mitos-pkgd`
    /// itself, which serializes mutating operations through one lock —
    /// see that daemon's own docs).
    busy: bool,
}

impl SoftwareCenterApp {
    pub fn new(commands: Sender<AppCommand>, events: Receiver<AppEvent>) -> Self {
        Self {
            commands,
            events,
            search_query: String::new(),
            installed: Vec::new(),
            search_results: Vec::new(),
            log: vec!["Connecting to mitos-pkgd...".to_string()],
            busy: false,
        }
    }

    fn send(&mut self, command: AppCommand) {
        self.busy = true;
        let _ = self.commands.send(command);
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                AppEvent::Listed(packages) => self.installed = packages,
                AppEvent::Searched(matches) => self.search_results = matches,
                AppEvent::Progress { action, event } => {
                    self.log
                        .push(format!("[{action}] {}", describe_progress(&event)));
                }
                AppEvent::Done {
                    action,
                    refreshes_list,
                    result,
                } => {
                    self.busy = false;
                    match result {
                        Ok(message) => self.log.push(format!("[{action}] done — {message}")),
                        Err(message) => self.log.push(format!("[{action}] failed — {message}")),
                    }
                    if refreshes_list {
                        let _ = self.commands.send(AppCommand::List);
                    }
                }
            }
        }

        if self.log.len() > MAX_LOG_LINES {
            let overflow = self.log.len() - MAX_LOG_LINES;
            self.log.drain(0..overflow);
        }
    }
}

impl eframe::App for SoftwareCenterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("MITOS Software Center");
                ui.add_space(16.0);
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Refresh index"))
                    .clicked()
                {
                    self.send(AppCommand::Update);
                }
                if self.busy {
                    ui.spinner();
                }
            });
        });

        egui::SidePanel::left("installed_panel")
            .resizable(true)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.heading("Installed");
                egui::ScrollArea::vertical()
                    .id_salt("installed_scroll")
                    .show(ui, |ui| {
                        let mut to_remove = None;
                        for (name, pkg) in &self.installed {
                            ui.horizontal(|ui| {
                                ui.label(format!("{name} {}", pkg.version));
                                if ui
                                    .add_enabled(!self.busy, egui::Button::new("Remove"))
                                    .clicked()
                                {
                                    to_remove = Some(name.clone());
                                }
                            });
                        }
                        if let Some(name) = to_remove {
                            self.send(AppCommand::Remove(name));
                        }
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Find software");
            ui.horizontal(|ui| {
                let response = ui.text_edit_singleline(&mut self.search_query);
                let search_clicked = ui
                    .add_enabled(!self.busy, egui::Button::new("Search"))
                    .clicked();
                let enter_pressed =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if !self.busy && (search_clicked || enter_pressed) {
                    let query = self.search_query.clone();
                    self.send(AppCommand::Search(query));
                }
            });

            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("search_scroll")
                .max_height(220.0)
                .show(ui, |ui| {
                    let mut to_install = None;
                    for meta in &self.search_results {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(format!("{} {}", meta.name, meta.version));
                                ui.small(meta.description.as_str());
                            });
                            if ui
                                .add_enabled(!self.busy, egui::Button::new("Install"))
                                .clicked()
                            {
                                to_install = Some(meta.name.clone());
                            }
                        });
                    }
                    if let Some(name) = to_install {
                        self.send(AppCommand::Install(name));
                    }
                });

            ui.separator();
            ui.heading("Activity");
            egui::ScrollArea::vertical()
                .id_salt("log_scroll")
                .max_height(160.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &self.log {
                        ui.label(line.as_str());
                    }
                });
        });

        // Keep polling for worker events even with no user input, so
        // progress lines show up as they arrive instead of only after
        // the next click/keypress. This just schedules a wake-up — it
        // doesn't spin a busy-loop between them.
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}

/// Turns one `ProgressEvent` into a short line for the activity log —
/// `{event:?}`'s raw `Fetching { name: "x", version: "1.0.0" }` shape
/// works but reads like debug output, which undercuts the entire point
/// of this app (something a non-terminal user can read comfortably).
fn describe_progress(event: &ProgressEvent) -> String {
    match event {
        ProgressEvent::Resolving { spec } => format!("Resolving {spec}..."),
        ProgressEvent::Fetching { name, version } => format!("Downloading {name} {version}..."),
        ProgressEvent::Verifying { name, version } => format!("Verifying {name} {version}..."),
        ProgressEvent::Installing { name, version } => format!("Installing {name} {version}..."),
        ProgressEvent::Removing { name } => format!("Removing {name}..."),
        ProgressEvent::Updating { repository } => format!("Refreshing {repository}..."),
    }
}
