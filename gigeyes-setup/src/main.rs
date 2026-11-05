// No console window behind the setup on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! GigEyes Setup: one window in the app's own look. Install, update or remove GigEyes for the current user.
//!
//! `GigEyes-Setup.exe`                         the window
//! `GigEyes-Setup.exe --silent [--dir <path>]`  install with defaults, no window (used by the build to test it)
//! `uninstall.exe --uninstall [--silent]`       remove (Windows' Installed apps runs this)
//! `gigeyes-setup --pack <out> <files…>`        build a setup by appending files to this program

mod install;
#[path = "../../gigeyes/src/look.rs"]
mod look;
mod payload;

use eframe::egui::{self, Align, Layout, RichText, Vec2};
use install::Options;
use look::*;
use std::path::{Path, PathBuf};
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
            } else {
                "GigEyes Setup"
            })
            .with_inner_size([500.0, 600.0])
            .with_resizable(false),
        ..Default::default()
    };
    let app = Setup {
        me,
        uninstalling,
        opts: Options {
            dir,
            start_menu: true,
            desktop: false,
        },
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
    Done,
    Failed(String),
}

enum Message {
    Step(String),
    Finished(Result<(), String>),
}

struct Setup {
    me: PathBuf,
    uninstalling: bool,
    opts: Options,
    open_after: bool,
    remove_data: bool,
    /// The version already installed, if any: the button then says Update.
    previous: Option<String>,
    size_mb: Option<f32>,
    phase: Phase,
    work: Option<Receiver<Message>>,
}

impl Setup {
    fn start(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let (me, opts, remove_data, uninstalling, ctx) = (
            self.me.clone(),
            self.opts.clone(),
            self.remove_data,
            self.uninstalling,
            ctx.clone(),
        );
        let total = payload::read(&me)
            .ok()
            .flatten()
            .map_or(1, |(entries, _)| entries.len() + 2);
        std::thread::spawn(move || {
            let result = if uninstalling {
                install::uninstall(&opts.dir, remove_data)
            } else {
                install::install(&me, &opts, |step| {
                    let _ = tx.send(Message::Step(step.to_string()));
                    ctx.request_repaint();
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
                Message::Finished(result) => {
                    self.work = None;
                    self.phase = match result {
                        Ok(()) => Phase::Done,
                        Err(e) => Phase::Failed(e),
                    };
                }
            }
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
                match &self.phase {
                    Phase::Ready if self.uninstalling => self.confirm_removal(ui, &ctx),
                    Phase::Ready => self.choose(ui, &ctx),
                    Phase::Working(done, total, now) => {
                        let label = if self.uninstalling {
                            "Removing GigEyes…"
                        } else {
                            now.as_str()
                        };
                        ui.label(semibold(label, 16.0));
                        ui.add_space(12.0);
                        let fraction = (*done as f32 / *total as f32).clamp(0.05, 1.0);
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .fill(ACCENT)
                                .desired_height(8.0)
                                .corner_radius(4),
                        );
                        ctx.request_repaint();
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
                }
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
        });
        ui.add_space(10.0);
        ui.label(
            RichText::new("For your Windows account only: no admin rights, nothing else on the PC is changed.")
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
                RichText::new("Also delete my rig, saved research and search key"),
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
        semibold(text, 16.0).color(GROUND)
    } else {
        semibold(text, 16.0)
    };
    let mut button = egui::Button::new(label).min_size(Vec2::new(0.0, 44.0));
    if primary {
        button = button.fill(ACCENT);
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
