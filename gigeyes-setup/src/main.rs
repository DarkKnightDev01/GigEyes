// No console window behind the setup on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! GigEyes Setup: one window in the app's own look. Install, update or remove GigEyes for the current user.
//!
//! `GigEyes-Setup.exe`                         the window
//! `GigEyes-Setup.exe --silent [--dir <path>]`  install with defaults, no window (used by the build to test it)
//! `GigEyes-Setup.exe --update`                install over the current version, then reopen GigEyes (the app's updater)
//! `uninstall.exe --uninstall [--silent]`       remove (Windows' Installed apps runs this)
//! `gigeyes-setup --pack <out> <files…>`        build a setup by appending files to this program

#[path = "../../gigeyes/src/hardware.rs"]
mod hardware;
mod install;
#[path = "../../gigeyes/src/look.rs"]
#[allow(dead_code)] // shared with the app, which uses the tabs and springs
mod look;
#[path = "../../gigeyes/src/model.rs"]
mod model;
mod payload;

use eframe::egui::{self, Align, Layout, RichText, Vec2};
use install::Options;
use look::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let me = std::env::current_exe().expect("the setup knows where it is");

    if flag("--pack") {
        let (Some(out), files) = (value("--pack"), &args[2.min(args.len())..]) else {
            eprintln!("usage: gigeyes-setup --pack <out> <files…>");
            std::process::exit(2);
        };
        let files: Vec<&Path> = files.iter().map(Path::new).collect();
        if let Err(e) = payload::pack(&me, &files, &out) {
            eprintln!("pack failed: {e}");
            std::process::exit(1);
        }
        return;
    }

    let uninstalling = flag("--uninstall");
    let updating = flag("--update");
    // Both install and removal target the recorded install. Removal falls back to the program's own
    // folder only when that folder really is an install (it holds the uninstaller), so a stray
    // `GigEyes-Setup.exe --uninstall` in Downloads can't delete files that happen to share a name.
    let here = me
        .parent()
        .map(Path::to_path_buf)
        .filter(|d| d.join("uninstall.exe").is_file());
    let dir = match (value("--dir"), install::existing()) {
        (Some(picked), _) => install::dir_inside(&picked),
        (None, Some((recorded, _))) => recorded,
        (None, None) if uninstalling => match here {
            Some(dir) => dir,
            None => {
                eprintln!("GigEyes isn't installed here.");
                std::process::exit(1);
            }
        },
        (None, None) => install::default_dir(),
    };

    if flag("--silent") {
        let result = if uninstalling {
            install::uninstall(&dir, false)
        } else {
            install::install(
                &me,
                &Options {
                    dir,
                    start_menu: true,
                    desktop: false,
                },
                |_| {},
            )
        };
        if let Err(e) = result {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }

    let size_mb = payload::read(&me).ok().flatten().map(|(entries, stub)| {
        (entries.iter().map(|e| e.len).sum::<u64>() + stub) as f32 / 1_048_576.0
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(if uninstalling {
                "Remove GigEyes"
            } else if updating {
                "Updating GigEyes"
            } else {
                "GigEyes Setup"
            })
            .with_inner_size([500.0, 650.0])
            .with_resizable(false)
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../../gigeyes/icons/gigeyes-256.png"
                ))
                .expect("the bundled icon is a valid PNG"),
            ),
        ..Default::default()
    };
    // The local AI this PC can run, unless it's already downloaded. Silent installs never download it.
    let ai = if uninstalling || updating {
        None
    } else {
        hardware::choose(&hardware::detect())
            .model
            .filter(|m| !model::is_downloaded(m))
    };
    let app = Setup {
        get_ai: ai.is_some(),
        ai,
        ai_bytes: ai.map_or(0, |m| model::missing_bytes(&m)),
        ai_note: None,
        cancel: Arc::new(AtomicBool::new(false)),
        me,
        uninstalling,
        updating,
        opts: {
            // An update keeps the shortcuts the way the user left them.
            let (start_menu, desktop) = if updating {
                install::shortcuts()
            } else {
                (true, false)
            };
            Options {
                dir,
                start_menu,
                desktop,
            }
        },
        phase_key: None,
        phase_at: 0.0,
        open_after: true,
        remove_data: false,
        previous: install::existing().map(|(_, version)| version),
        size_mb,
        phase: Phase::Ready,
        work: None,
    };
    let _ = eframe::run_native(
        "GigEyes Setup",
        options,
        Box::new(|cc| {
            window_icon(cc);
            cc.egui_ctx.set_fonts(fonts());
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            cc.egui_ctx.style_mut_of(egui::Theme::Dark, theme);
            Ok(Box::new(app))
        }),
    );
}

enum Phase {
    Ready,
    /// (steps finished, steps in all, what's happening now)
    Working(usize, usize, String),
    /// Bytes of the local AI downloaded so far.
    Downloading(u64),
    Done,
    Failed(String),
}

enum Message {
    Step(String),
    Bytes(u64),
    /// Ok carries why the local AI wasn't downloaded, if it wasn't: the install itself still worked.
    Finished(Result<Option<String>, String>),
}

struct Setup {
    me: PathBuf,
    uninstalling: bool,
    /// Started by the app's updater: install straight away, then reopen GigEyes.
    updating: bool,
    /// Which phase is showing and since when, for its entrance.
    phase_key: Option<std::mem::Discriminant<Phase>>,
    phase_at: f64,
    opts: Options,
    open_after: bool,
    remove_data: bool,
    /// The model picked for this PC (None when it doesn't fit or is already there), and whether to get it.
    ai: Option<hardware::Model>,
    get_ai: bool,
    /// What's left to download of the AI: the engine and the model, whichever is missing.
    ai_bytes: u64,
    ai_note: Option<String>,
    cancel: Arc<AtomicBool>,
    /// The version already installed, if any: the button then says Update.
    previous: Option<String>,
    size_mb: Option<f32>,
    phase: Phase,
    work: Option<Receiver<Message>>,
}

impl Setup {
    fn start(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let (me, opts, remove_data, uninstalling, updating, ctx) = (
            self.me.clone(),
            self.opts.clone(),
            self.remove_data,
            self.uninstalling,
            self.updating,
            ctx.clone(),
        );
        let ai = self.ai.filter(|_| self.get_ai && !uninstalling);
        let cancel = self.cancel.clone();
        let total = payload::read(&me)
            .ok()
            .flatten()
            .map_or(1, |(entries, _)| entries.len() + 2);
        std::thread::spawn(move || {
            let result = if uninstalling {
                install::uninstall(&opts.dir, remove_data).map(|()| None)
            } else {
                let attempt = || {
                    install::install(&me, &opts, |step| {
                        let _ = tx.send(Message::Step(step.to_string()));
                        ctx.request_repaint();
                    })
                };
                let mut result = attempt();
                // The updater starts us while GigEyes is still closing: give it up to 10 s to let go.
                for _ in 0..40 {
                    if !(updating && result.as_ref().is_err_and(|e| e.contains("in use"))) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    result = attempt();
                }
                result.map(|()| {
                    let model = ai?;
                    let _ = tx.send(Message::Bytes(0));
                    let mut shown = 0;
                    model::download(&model, &cancel, |done| {
                        // A repaint every 8 MB is plenty for a bar gigabytes long.
                        if done - shown >= 8 << 20 {
                            shown = done;
                            let _ = tx.send(Message::Bytes(done));
                            ctx.request_repaint();
                        }
                    })
                    .err()
                })
            };
            let _ = tx.send(Message::Finished(result));
            ctx.request_repaint();
        });
        self.phase = Phase::Working(0, total, String::new());
        self.work = Some(rx);
    }
}

impl eframe::App for Setup {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        while let Some(message) = self.work.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match message {
                Message::Step(step) => {
                    if let Phase::Working(done, _, now) = &mut self.phase {
                        *done += 1;
                        *now = step;
                    }
                }
                Message::Bytes(done) => self.phase = Phase::Downloading(done),
                Message::Finished(result) => {
                    self.work = None;
                    self.phase = match result {
                        Ok(_) if self.updating => {
                            install::launch(&self.opts.dir);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            Phase::Done
                        }
                        Ok(note) => {
                            self.ai_note = note;
                            Phase::Done
                        }
                        Err(e) => Phase::Failed(e),
                    };
                }
            }
        }

        if self.updating && matches!(self.phase, Phase::Ready) {
            self.start(&ctx);
        }
        let key = std::mem::discriminant(&self.phase);
        if self.phase_key != Some(key) {
            self.phase_key = Some(key);
            self.phase_at = ctx.input(|i| i.time);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(36.0))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    mark(ui, 72.0);
                    ui.add_space(10.0);
                    ui.label(semibold("GigEyes", 30.0));
                    ui.label(RichText::new("Hear a tone. See where the knobs sit.").weak());
                });
                ui.add_space(32.0);
                let since = self.phase_at;
                enter(ui, since, |ui| match &self.phase {
                    Phase::Ready if self.uninstalling => self.confirm_removal(ui, &ctx),
                    Phase::Ready => self.choose(ui, &ctx),
                    Phase::Working(done, total, now) => {
                        let label = if self.uninstalling {
                            "Removing GigEyes…"
                        } else if self.updating && now.is_empty() {
                            "Updating GigEyes…"
                        } else {
                            now.as_str()
                        };
                        ui.label(semibold(label, 16.0));
                        ui.add_space(12.0);
                        let fraction = ctx.animate_value_with_time(
                            egui::Id::NULL.with("install-bar"),
                            (*done as f32 / *total as f32).clamp(0.05, 1.0),
                            secs(0.3),
                        );
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .fill(accent())
                                .desired_height(8.0)
                                .corner_radius(4),
                        );
                        ctx.request_repaint();
                    }
                    Phase::Downloading(done) => {
                        let model = self.ai.expect("downloading a model");
                        let gb = |bytes: u64| bytes as f32 / 1e9;
                        ui.label(semibold(format!("Downloading {}…", model.name), 16.0));
                        ui.add_space(12.0);
                        ui.add(
                            egui::ProgressBar::new(ctx.animate_value_with_time(
                                egui::Id::NULL.with("ai-bar"),
                                *done as f32 / self.ai_bytes.max(1) as f32,
                                secs(0.3),
                            ))
                                .fill(accent())
                                .desired_height(8.0)
                                .corner_radius(4),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!(
                                "{:.1} of {:.1} GB. GigEyes is installed already; this is the local AI.",
                                gb(*done),
                                gb(self.ai_bytes)
                            ))
                            .size(13.0)
                            .weak(),
                        );
                        ui.add_space(18.0);
                        if wide_button(ui, "Skip the AI download", false).clicked() {
                            self.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    Phase::Done => self.finished(ui, &ctx),
                    Phase::Failed(e) => {
                        let e = e.clone();
                        card(ui, |ui| {
                            ui.label(semibold("That didn't work", 17.0));
                            ui.add_space(4.0);
                            ui.label(e);
                        });
                        ui.add_space(18.0);
                        if wide_button(ui, "Try again", true).clicked() {
                            self.phase = Phase::Ready;
                        }
                    }
                });
            });
    }
}

impl Setup {
    fn choose(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            ui.label(RichText::new("Installs to").size(13.0).weak());
            ui.horizontal(|ui| {
                ui.label(RichText::new(short_path(&self.opts.dir)).size(14.0))
                    .on_hover_text(self.opts.dir.display().to_string());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Change…").clicked()
                        && let Some(picked) = rfd::FileDialog::new()
                            .set_title("Install GigEyes in…")
                            .pick_folder()
                    {
                        self.opts.dir = install::dir_inside(&picked);
                    }
                });
            });
            ui.add_space(14.0);
            for (on, text) in [
                (&mut self.opts.start_menu, "Start menu shortcut"),
                (&mut self.opts.desktop, "Desktop shortcut"),
                (&mut self.open_after, "Open GigEyes when it's done"),
            ] {
                tick(ui, on, RichText::new(text), None);
                ui.add_space(6.0);
            }
            if let Some(model) = &self.ai {
                tick(
                    ui,
                    &mut self.get_ai,
                    RichText::new("Download the local AI"),
                    Some(
                        RichText::new(format!(
                            "{}, {:.1} GB, picked for this PC. Untick to skip it.",
                            model.name,
                            self.ai_bytes as f32 / 1e9
                        ))
                        .size(13.0)
                        .weak(),
                    ),
                );
            }
        });
        ui.add_space(10.0);
        ui.label(
            RichText::new("Installs in Program Files for everyone on this PC, so Windows asks for permission first. Your rig and settings stay in your own account.")
                .size(13.0)
                .weak(),
        );
        ui.add_space(24.0);
        let action = match &self.previous {
            Some(old) if old != install::VERSION => {
                format!("Update GigEyes {old} to {}", install::VERSION)
            }
            Some(_) => "Reinstall GigEyes".to_string(),
            None => "Install GigEyes".to_string(),
        };
        if wide_button(ui, &action, true).clicked() {
            self.start(ctx);
        }
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            let size = self
                .size_mb
                .map(|mb| format!("  ·  {mb:.0} MB"))
                .unwrap_or_default();
            let note = if self.previous.is_some() {
                "  ·  your rig and research stay"
            } else {
                ""
            };
            ui.label(
                RichText::new(format!("Version {}{size}{note}", install::VERSION))
                    .size(12.0)
                    .weak(),
            );
        });
    }

    fn confirm_removal(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            ui.label(semibold("Remove GigEyes from this PC?", 17.0));
            ui.add_space(4.0);
            ui.label(
                RichText::new("Takes away the app, its shortcuts and its entry in Installed apps.")
                    .size(13.0)
                    .weak(),
            );
            ui.add_space(12.0);
            tick(
                ui,
                &mut self.remove_data,
                RichText::new(
                    "Also delete my rig, saved research, search key and the downloaded AI",
                ),
                None,
            );
        });
        ui.add_space(24.0);
        // Removing is the one action here, but it's destructive: neutral button, not the accent.
        if wide_button(ui, "Remove GigEyes", false).clicked() {
            self.start(ctx);
        }
    }

    fn finished(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.vertical_centered(|ui| {
            let line = if self.uninstalling {
                "GigEyes is removed."
            } else {
                "GigEyes is ready."
            };
            ui.label(semibold(line, 22.0));
            if !self.uninstalling && self.opts.start_menu {
                ui.label(RichText::new("You'll find it in the Start menu.").weak());
            }
            if let Some(note) = &self.ai_note {
                let why = if note == "Skipped." {
                    "The local AI wasn't downloaded. Run this setup again to get it.".to_string()
                } else {
                    format!("The local AI wasn't downloaded: {note} Run this setup again to retry.")
                };
                ui.add_space(8.0);
                ui.label(RichText::new(why).size(13.0).weak());
            }
        });
        ui.add_space(28.0);
        if self.uninstalling {
            if wide_button(ui, "Close", false).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        let open = if self.open_after {
            "Open GigEyes"
        } else {
            "Close"
        };
        if wide_button(ui, open, true).clicked() {
            if self.open_after {
                install::launch(&self.opts.dir);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// A full-width button, 44px tall, label centred: amber for the main action, neutral otherwise.
fn wide_button(ui: &mut egui::Ui, text: &str, primary: bool) -> egui::Response {
    let label = if primary {
        semibold(text, 16.0).color(ON_ACCENT)
    } else {
        semibold(text, 16.0)
    };
    let mut button = egui::Button::new(label).min_size(Vec2::new(0.0, 44.0));
    if primary {
        button = button.fill(accent());
    }
    // A justified layout stretches the button to the full width and centres its text.
    ui.with_layout(Layout::top_down_justified(Align::Center), |ui| {
        ui.add(button)
    })
    .inner
}

/// `C:\Users\Name\AppData\Local\Programs\GigEyes` → `…\AppData\Local\Programs\GigEyes`, so it fits one line.
fn short_path(path: &Path) -> String {
    let full = path.display().to_string();
    let parts: Vec<&str> = full.split(['\\', '/']).collect();
    if full.chars().count() <= 46 || parts.len() <= 4 {
        return full;
    }
    let sep = if full.contains('\\') { "\\" } else { "/" };
    format!("…{sep}{}", parts[parts.len() - 4..].join(sep))
}
