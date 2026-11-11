// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod analysis;
mod decode;
mod fetch;
mod hardware;
mod intro;
mod knowledge;
mod look;
mod model;
mod research;
mod rig;
mod update;

use analysis::{Knob, Tone};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, ImageSource, Layout, Pos2,
    RichText, Sense, Shape, Stroke, StrokeKind, Vec2,
};
use look::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// How long the knobs take to turn to a new result.
const REVEAL_SECS: f64 = 0.8;
/// Past this, lines and knob rows get too wide to read; the column centres instead.
const MAX_WIDTH: f32 = 760.0;

const AUDIO_TYPES: [&str; 9] = [
    "wav", "mp3", "flac", "ogg", "m4a", "mp4", "aac", "mkv", "mov",
];
// Lucide icons, white so they can be tinted to any colour.
const ICON_FILE: ImageSource = egui::include_image!("../icons/file-up.svg");
const ICON_LINK: ImageSource = egui::include_image!("../icons/link.svg");
const ICON_ALERT: ImageSource = egui::include_image!("../icons/alert.svg");

fn main() -> eframe::Result {
    let mut settings = research::load_settings();
    let seen = settings.intro_seen.clone();
    let intro_queued = settings.intro_now(VERSION);
    if settings.intro_seen != seen {
        let _ = research::save_settings(&settings);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // No title bar while the intro plays: just the dark window and the logo. It comes back
            // as the logo glides into the header.
            .with_decorations(!intro_queued)
            .with_inner_size([780.0, 860.0])
            .with_min_inner_size([520.0, 520.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../icons/gigeyes-256.png"))
                    .expect("the bundled icon is a valid PNG"),
            ),
        ..Default::default()
    };
    // A file passed on the command line (or dropped onto gigeyes.exe in Explorer) opens right away.
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    fetch::clean_old_downloads(&std::env::temp_dir(), std::time::Duration::from_secs(3600));
    eframe::run_native(
        "GigEyes",
        options,
        Box::new(move |cc| {
            window_icon(cc);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx.set_fonts(fonts());
            apply_theme(&cc.egui_ctx, settings.light);
            // Rescan at every start, so new plugins show up; in auto config GigEyes asks about them.
            let mut rig = rig::load();
            let mut asking = vec![];
            if let Some(old) = &rig {
                let fresh = rig::rescan(old, &rig::default_folders());
                // Not before the first-start setup: its plugins step shows them all anyway.
                if settings.onboarded && settings.new_plugins == research::NewPlugins::Auto {
                    asking = fresh
                        .iter()
                        .filter(|p| !old.iter().any(|o| o.path == p.path))
                        .map(|p| p.path.clone())
                        .collect();
                }
                let _ = rig::save(&fresh);
                rig = Some(fresh);
            }
            let mut app = App {
                rig,
                asking,
                knowledge: knowledge::Knowledge::load(),
                onboarding: (!settings.onboarded).then_some(0),
                guitar: settings.guitar,
                new_plugins: settings.new_plugins,
                tavily_key: settings.tavily_key,
                intro_queued,
                intro_choice: settings.intro,
                intro_length: settings.intro_length.clamp(1.5, 4.0),
                bare: intro_queued,
                ..App::default()
            };
            app.check_updates(&cc.egui_ctx);
            if let Some(path) = initial {
                app.open(path, &cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
}

#[derive(Default)]
enum State {
    #[default]
    Empty,
    Reading(String),
    Done(String, Tone),
    Failed(String),
}

/// What a background job sends back: the name to show, and the tone.
type Outcome = Result<(String, Tone), String>;

#[derive(Default)]
struct App {
    state: State,
    result: Option<Receiver<Outcome>>,
    link: String,
    /// When the current result arrived, so the knobs can turn into place.
    revealed_at: f64,
    screen: Screen,
    /// None until the first scan: then the tone screen asks for it.
    rig: Option<Vec<rig::Plugin>>,
    rig_filter: String,
    rig_error: Option<String>,
    /// Song research for the current result: the song name, what was found, and a search in flight.
    song: String,
    research: Option<research::Research>,
    research_job: Option<Receiver<Result<research::Research, String>>>,
    research_note: Option<String>,
    tavily_key: String,
    key_note: Option<String>,
    /// "Remove all files" asks once more before deleting.
    confirm_remove: bool,
    remove_note: Option<String>,
    confirm_at: f64,
    /// Updates: the check at start, a newer release if there is one, and its download.
    update_check: Option<Receiver<Result<Option<update::Update>, String>>>,
    update: Option<update::Update>,
    update_at: f64,
    update_note: Option<String>,
    update_job: Option<Receiver<Result<PathBuf, String>>>,
    update_done: Arc<AtomicU64>,
    /// Which screen is showing and since when, for its entrance.
    shown: (Screen, bool),
    shown_at: f64,
    /// The old background while light and dark mode crossfade.
    theme_fade: Option<(Color32, f64)>,
    /// The startup intro: queued until the first frame, then when it started; and when it plays.
    intro_queued: bool,
    intro_at: Option<f64>,
    intro_choice: research::Intro,
    intro_length: f32,
    /// The window has no title bar yet (only while the startup intro plays).
    bare: bool,
    /// What lookups found out about the user's plugins, kept offline.
    knowledge: knowledge::Knowledge,
    /// Looking plugins up: the job, the plugin it's on (and which of how many), Stop, and per-plugin
    /// errors this session. Stop pauses lookups until "Look up now".
    lookup_job: Option<Receiver<Lookup>>,
    looking_up: Option<(String, usize, usize)>,
    lookup_cancel: Arc<AtomicBool>,
    lookup_paused: bool,
    /// Why lookups stopped by themselves (no internet, a bad key, the AI not starting).
    lookup_note: Option<String>,
    lookup_failed: std::collections::HashMap<String, String>,
    /// New plugins found at start that GigEyes still has to ask about (auto config).
    asking: Vec<PathBuf>,
    new_plugins: research::NewPlugins,
    /// The first-start setup: which step, and since when it shows.
    onboarding: Option<usize>,
    onboarding_at: f64,
    guitar: String,
    /// The "In your rig" line of each finding on screen, with a fingerprint of what it was worked
    /// out from, so it's only redone when the findings, the rig or the knowledge change.
    yours: (u64, Vec<Option<String>>),
    /// Read once, the first time the AI is needed.
    hardware: Option<(hardware::Hardware, hardware::Choice)>,
    /// The local AI reading the research pages: the page it's on (0 while it loads), Stop, and any error.
    ai_job: Option<Receiver<Result<Vec<ai::Finding>, String>>>,
    ai_page: Arc<AtomicUsize>,
    ai_cancel: Arc<AtomicBool>,
    ai_note: Option<String>,
    /// Downloading the local AI from Settings: bytes done of the total, Stop, and any error.
    get_ai: Option<Receiver<Result<PathBuf, String>>>,
    get_ai_done: Arc<AtomicU64>,
    get_ai_total: u64,
    get_ai_cancel: Arc<AtomicBool>,
    get_ai_note: Option<String>,
}

/// What a lookup job reports back.
enum Lookup {
    /// Now looking up this plugin, number i of n.
    Now(String, usize, usize),
    Done(String, Result<Vec<ai::Finding>, String>),
    /// Something every lookup would fail on (the search key, the internet, the AI): pause.
    Halt(String),
    Finished,
}

const STEPS: usize = 5;

#[derive(Default, PartialEq, Clone, Copy)]
enum Screen {
    #[default]
    Tone,
    Rig,
    Settings,
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn read_tone(path: &std::path::Path) -> Outcome {
    let mut analyzer = analysis::Analyzer::new();
    let rate = decode::decode(path, |piece| analyzer.push(piece))?;
    Ok((file_name(path), analyzer.finish(rate)))
}

impl App {
    /// Run `job` off the UI thread (decoding a song takes a moment) and show `label` meanwhile.
    fn start(
        &mut self,
        label: String,
        ctx: &egui::Context,
        job: impl FnOnce() -> Outcome + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
        self.state = State::Reading(label);
        self.result = Some(rx);
    }

    fn open(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.start(
            format!("Listening to {}…", file_name(&path)),
            ctx,
            move || read_tone(&path),
        );
    }

    fn pick_file(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio or video", &AUDIO_TYPES)
            .pick_file()
        {
            self.open(path, ctx);
        }
    }

    fn open_link(&mut self, ctx: &egui::Context) {
        let link = match fetch::check_link(&self.link) {
            Ok(link) => link.to_string(),
            Err(e) => return self.state = State::Failed(e),
        };
        self.start(
            "Fetching the audio, then listening…".into(),
            ctx,
            move || {
                // The folder and the download are deleted when `temp` goes out of scope, even on errors.
                let temp = fetch::TempDir::new()?;
                read_tone(&fetch::download(&link, &temp.0)?)
            },
        );
    }

    fn tone_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, hovering: bool, reveal: f32) {
        let busy = matches!(self.state, State::Reading(_));
        let has_result = matches!(self.state, State::Done(..));
        // Before the first result the drop zone leads; afterwards the result does and inputs shrink to a row.
        if !has_result && drop_zone(ui, hovering, busy) {
            self.pick_file(ctx);
        }
        ui.add_space(12.0);
        let (pick, load) = input_row(ui, &mut self.link, has_result, busy);
        if pick {
            self.pick_file(ctx);
        }
        if load {
            self.open_link(ctx);
        }
        if !has_result {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Works best on an isolated guitar track. On a full mix, bass, drums and vocals colour the result.")
                    .size(13.0)
                    .weak(),
            );
        }
        if self.rig.is_none() {
            ui.add_space(20.0);
            card(ui, |ui| {
                ui.label(semibold("Tell GigEyes what you play through", 17.0));
                ui.label(
                    RichText::new("Pick your plugins once, and GigEyes suggests settings for your own rig instead of a generic amp.")
                        .size(13.0)
                        .weak(),
                );
                ui.add_space(10.0);
                if ui.button("Set up your rig").clicked() {
                    self.screen = Screen::Rig;
                }
            });
        }
        ui.add_space(28.0);

        match &self.state {
            State::Empty => amp_panel(ui, &waiting_knobs(), 1.0, false),
            State::Reading(label) => {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        listening(ui);
                        ui.add_space(6.0);
                        ui.label(label);
                    });
                });
                // Same panel stays put, so the knobs turn in place when the result lands.
                ui.add_space(14.0);
                amp_panel(ui, &waiting_knobs(), 1.0, false);
            }
            State::Failed(e) => card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.add(icon(ICON_ALERT, 20.0, error()));
                    ui.label(e);
                });
                ui.label(RichText::new("Try another file or link.").weak());
            }),
            State::Done(name, tone) => report(ui, name, tone, ease(reveal)),
        }
        if matches!(self.state, State::Done(..)) {
            ui.add_space(14.0);
            self.research_view(ui, ctx);
        }
    }

    fn research_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let searching = self.research_job.is_some();
        card(ui, |ui| {
            ui.label(semibold("Research this song", 17.0));
            ui.label(
                RichText::new("Find interviews, rig rundowns and gear lists for the song. Every source is linked so you can check it.")
                    .size(13.0)
                    .weak(),
            );
            ui.add_space(10.0);
            if self.tavily_key.trim().is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Research needs a free Tavily key.").weak());
                    if ui.link("Add it in Settings").clicked() {
                        self.screen = Screen::Settings;
                    }
                });
                return;
            }
            ui.horizontal(|ui| {
                let button_width = 120.0;
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.song)
                        .hint_text("Artist - Song")
                        .margin(Vec2::new(10.0, 7.0))
                        .desired_width(
                            ui.available_width() - button_width - ui.spacing().item_spacing.x,
                        ),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let label = if self.research.is_some() {
                    "Search again"
                } else {
                    "Find sources"
                };
                let go = ui.add_enabled(
                    !searching,
                    egui::Button::new(label).min_size(Vec2::new(button_width, 0.0)),
                );
                if (go.clicked() || enter) && !searching && !self.song.trim().is_empty() {
                    let (tx, rx) = mpsc::channel();
                    let (key, song, ctx) =
                        (self.tavily_key.clone(), self.song.clone(), ctx.clone());
                    thread::spawn(move || {
                        let _ = tx.send(research::search(&key, &song));
                        ctx.request_repaint();
                    });
                    self.research_job = Some(rx);
                    self.research_note = None;
                }
            });
            if searching {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    listening(ui);
                    ui.label("Searching…");
                });
            }
            if let Some(note) = &self.research_note {
                ui.add_space(8.0);
                ui.colored_label(error(), note);
            }
            if let Some(found) = &self.research {
                ui.add_space(12.0);
                let saved = if found.sources.is_empty() {
                    "Nothing found for this song. Try the artist and song name spelled out in full."
                        .to_string()
                } else {
                    format!(
                        "{} sources for {}. Saved, so they open offline next time.",
                        found.sources.len(),
                        found.song
                    )
                };
                ui.label(RichText::new(saved).size(13.0).weak());
                for source in &found.sources {
                    ui.add_space(10.0);
                    let domain = source
                        .url
                        .split('/')
                        .nth(2)
                        .unwrap_or_default()
                        .trim_start_matches("www.");
                    ui.hyperlink_to(semibold(&source.title, 15.0).color(ink()), &source.url);
                    ui.label(RichText::new(domain).size(12.0).weak());
                    ui.label(RichText::new(snippet(&source.snippet)).size(13.0));
                }
            }
            self.gear_view(ui, ctx);
        });
    }

    /// Under the sources: the local AI reads the pages and lists the gear, each with its quote.
    fn gear_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let pages = match &self.research {
            Some(r) => r
                .sources
                .iter()
                .filter(|s| !s.text.trim().is_empty())
                .count(),
            None => 0,
        };
        if pages == 0 {
            return;
        }
        let choice = self.choice();
        ui.add_space(18.0);
        ui.label(semibold("Gear on these pages", 17.0));
        if self.ai_job.is_some() {
            let page = self.ai_page.load(Ordering::Relaxed);
            let doing = match (page, &choice.model) {
                (0, Some(m)) => format!("Starting {}…", m.name),
                _ => format!("Reading page {page} of {pages}…"),
            };
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                listening(ui);
                ui.label(doing);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Stop").clicked() {
                        self.ai_cancel.store(true, Ordering::Relaxed);
                    }
                });
            });
        } else {
            let read = self.research.as_ref().is_some_and(|r| r.findings.is_some());
            match &choice.model {
                None => {
                    ui.label(
                        RichText::new("This PC can't spare the memory for the local AI, so the pages stay unread. The links above still work.")
                            .size(13.0)
                            .weak(),
                    );
                }
                Some(model) if !model::is_downloaded(model) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("The local AI can read these pages and list the gear, with the sentence each piece comes from.")
                                .size(13.0)
                                .weak(),
                        );
                        if ui.link("Download it in Settings").clicked() {
                            self.screen = Screen::Settings;
                        }
                    });
                }
                Some(model) => {
                    if !read {
                        ui.label(
                            RichText::new(format!(
                                "{} reads the pages on this PC and lists the gear and settings they name, each with the sentence it came from. Nothing is sent anywhere.",
                                model.name
                            ))
                            .size(13.0)
                            .weak(),
                        );
                    }
                    ui.add_space(8.0);
                    let label = if read { "Read again" } else { "Read the pages" };
                    let free = self.lookup_job.is_none();
                    if ui.add_enabled(free, egui::Button::new(label)).clicked() {
                        self.start_reading(ctx);
                    }
                    if !free {
                        ui.label(
                            RichText::new("The local AI is busy looking up your plugins. Stop that in Your rig to read now.")
                                .size(13.0)
                                .weak(),
                        );
                    }
                }
            }
        }
        if let Some(note) = &self.ai_note {
            ui.add_space(8.0);
            ui.colored_label(error(), note);
        }
        let Some(findings) = self.research.as_ref().and_then(|r| r.findings.as_ref()) else {
            return;
        };
        if findings.is_empty() {
            ui.add_space(8.0);
            ui.label(
                RichText::new("The AI found no gear these pages tie to the song.")
                    .size(13.0)
                    .weak(),
            );
        }
        let key = fingerprint(findings, &self.knowledge, self.rig.as_deref());
        if self.yours.0 != key || self.yours.1.len() != findings.len() {
            let lines = findings
                .iter()
                .map(|f| in_your_rig(&self.knowledge, &f.gear, &f.kind, self.rig.as_deref()))
                .collect();
            self.yours = (key, lines);
        }
        for (f, yours) in findings.iter().zip(&self.yours.1) {
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(semibold(&f.gear, 15.0));
                ui.label(RichText::new(&f.kind).size(12.0).weak());
            });
            if !f.settings.is_empty() {
                ui.label(RichText::new(&f.settings).size(13.0).color(ink()));
            }
            ui.label(
                RichText::new(format!("\u{201C}{}\u{201D}", f.quote))
                    .size(13.0)
                    .italics()
                    .weak(),
            );
            if let Some(yours) = yours {
                ui.label(RichText::new(yours).size(13.0).color(ink()));
            }
            let domain = f
                .url
                .split('/')
                .nth(2)
                .unwrap_or_default()
                .trim_start_matches("www.");
            ui.hyperlink_to(
                RichText::new(domain)
                    .size(12.0)
                    .color(ui.visuals().hyperlink_color),
                &f.url,
            );
        }
    }

    /// The local AI this PC can run, worked out once.
    fn choice(&mut self) -> hardware::Choice {
        self.hardware
            .get_or_insert_with(|| {
                let hw = hardware::detect();
                let choice = hardware::choose(&hw);
                (hw, choice)
            })
            .1
            .clone()
    }

    fn start_reading(&mut self, ctx: &egui::Context) {
        let choice = self.choice();
        let (Some(research), Some(model)) = (self.research.clone(), choice.model) else {
            return;
        };
        let gpu = choice.backend != hardware::Backend::Cpu;
        // Fresh flags per read, so a stopped read still winding down can't touch this one.
        self.ai_page = Arc::new(AtomicUsize::new(0));
        self.ai_cancel = Arc::new(AtomicBool::new(false));
        let (page, cancel, ctx) = (self.ai_page.clone(), self.ai_cancel.clone(), ctx.clone());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(ai::read(
                &model,
                gpu,
                &research,
                ai::Task::Song,
                &page,
                &cancel,
            ));
            ctx.request_repaint();
        });
        self.ai_job = Some(rx);
        self.ai_note = None;
    }

    /// Stop a read that's still going, e.g. because the research it was reading was replaced.
    fn stop_reading(&mut self) {
        self.ai_cancel.store(true, Ordering::Relaxed);
        self.ai_job = None;
        self.ai_note = None;
    }

    fn start_ai_download(&mut self, ctx: &egui::Context) {
        let Some(model) = self.choice().model else {
            return;
        };
        self.get_ai_total = model::missing_bytes(&model);
        self.get_ai_done = Arc::new(AtomicU64::new(0));
        self.get_ai_cancel = Arc::new(AtomicBool::new(false));
        let (done, cancel, ctx) = (
            self.get_ai_done.clone(),
            self.get_ai_cancel.clone(),
            ctx.clone(),
        );
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut shown = 0;
            let result = model::download(&model, &cancel, |bytes| {
                done.store(bytes, Ordering::Relaxed);
                // A repaint every 8 MB is plenty for a bar gigabytes long.
                if bytes - shown >= 8 << 20 {
                    shown = bytes;
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.get_ai = Some(rx);
        self.get_ai_note = None;
    }

    fn rig_view(&mut self, ui: &mut egui::Ui) {
        ui.label(semibold("Your rig", 22.0));
        ui.label(
            RichText::new("Tick the plugins you play through, and the ones GigEyes should look up: it searches the web for what kind of plugin each one is, what it does and what real gear it models, the local AI reads the pages, and what it finds stays on this PC. GigEyes reads plugin names from disk; it never loads or changes them.")
                .weak(),
        );
        ui.add_space(12.0);
        self.new_plugins_choice(ui);
        ui.add_space(14.0);
        let mut changed = false;
        ui.horizontal(|ui| {
            let scan = egui::Button::new(semibold("Scan for plugins", 15.0).color(ON_ACCENT))
                .fill(accent());
            if ui.add(scan).clicked() {
                let previous = self.rig.clone().unwrap_or_default();
                let fresh = rig::rescan(&previous, &rig::default_folders());
                self.ask_about_new(&previous, &fresh);
                self.rig = Some(fresh);
                changed = true;
            }
            if ui.button("Add a folder…").clicked()
                && let Some(folder) = rfd::FileDialog::new().pick_folder()
            {
                let previous = self.rig.clone().unwrap_or_default();
                let mut plugins = previous.clone();
                let new: Vec<_> = rig::scan(&[folder])
                    .into_iter()
                    .filter(|p| !plugins.iter().any(|o| o.path == p.path))
                    .collect();
                plugins.extend(new);
                plugins.sort_by_key(|p| p.name.to_lowercase());
                self.ask_about_new(&previous, &plugins);
                self.rig = Some(plugins);
                changed = true;
            }
            if let Some(plugins) = &self.rig {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let used = plugins.iter().filter(|p| p.owned).count();
                    ui.label(RichText::new(format!("{used} of {} in use", plugins.len())).weak());
                });
            }
        });
        ui.add_space(14.0);
        self.lookup_status(ui);
        changed |= self.plugin_list(ui, true);
        if changed && let Some(plugins) = &self.rig {
            self.rig_error = rig::save(plugins).err();
        }
        if let Some(e) = &self.rig_error {
            ui.add_space(10.0);
            ui.colored_label(error(), e);
        }
    }

    /// Auto config: ask about plugins in `fresh` that weren't in `previous`, as at startup.
    fn ask_about_new(&mut self, previous: &[rig::Plugin], fresh: &[rig::Plugin]) {
        if self.new_plugins != research::NewPlugins::Auto {
            return;
        }
        for p in fresh {
            if !previous.iter().any(|o| o.path == p.path) && !self.asking.contains(&p.path) {
                self.asking.push(p.path.clone());
            }
        }
    }

    /// Auto config or manual, for plugins that show up later.
    fn new_plugins_choice(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("New plugins").size(13.0));
            ui.add_space(6.0);
            let options = [
                (research::NewPlugins::Auto, "Auto config"),
                (research::NewPlugins::Manual, "Manual"),
            ];
            if segmented(ui, "new-plugins", &mut self.new_plugins, &options) {
                let mut settings = research::load_settings();
                settings.new_plugins = self.new_plugins;
                self.rig_error = research::save_settings(&settings).err();
            }
        });
        let how = match self.new_plugins {
            research::NewPlugins::Auto => {
                "When GigEyes finds a new plugin, it asks whether you use it. Yes ticks both columns and looks it up."
            }
            research::NewPlugins::Manual => {
                "New plugins appear unticked; you tick both columns yourself."
            }
        };
        ui.label(RichText::new(how).size(13.0).weak());
    }

    /// The plugins in two tick columns: "Use" and "Look up". With `details`, what each looked-up
    /// plugin models, with the sentences it came from. Returns whether a tick changed.
    fn plugin_list(&mut self, ui: &mut egui::Ui, details: bool) -> bool {
        let mut changed = false;
        let (mut remove, mut again) = (None, None);
        let auto = self.new_plugins == research::NewPlugins::Auto;
        let (knowledge, failed, now) = (&self.knowledge, &self.lookup_failed, &self.looking_up);
        match &mut self.rig {
            None => {
                let folders: Vec<String> = rig::default_folders()
                    .iter()
                    .map(|f| f.display().to_string())
                    .collect();
                ui.label(
                    RichText::new(format!(
                        "Not scanned yet. GigEyes looks in {}.",
                        folders.join(" and ")
                    ))
                    .weak(),
                );
            }
            Some(plugins) if plugins.is_empty() => {
                ui.label(
                    "No VST3 plugins found there. If yours live somewhere else, add that folder.",
                );
            }
            Some(plugins) => {
                ui.add(
                    egui::TextEdit::singleline(&mut self.rig_filter)
                        .hint_text("Filter by name or maker")
                        .margin(Vec2::new(10.0, 7.0))
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(10.0);
                let filter = self.rig_filter.to_lowercase();
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for heading in ["Use", "Look up"] {
                            ui.allocate_ui_with_layout(
                                Vec2::new(CHECK_COLUMN, 16.0),
                                Layout::top_down(Align::Center),
                                |ui| ui.label(RichText::new(heading).size(12.0).weak()),
                            );
                        }
                        let left = plugins.iter().filter(|p| p.owned && !p.lookup).count();
                        if left > 0 {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let all = ui
                                    .small_button(match left {
                                        1 => "Look up the other one in use".to_string(),
                                        n => format!("Look up the other {n} in use"),
                                    })
                                    .on_hover_text("One web search for each plugin.");
                                if all.clicked() {
                                    for plugin in plugins.iter_mut().filter(|p| p.owned) {
                                        plugin.lookup = true;
                                    }
                                    changed = true;
                                }
                            });
                        }
                    });
                    ui.add_space(4.0);
                    let mut shown = 0;
                    for plugin in plugins.iter_mut().filter(|p| {
                        filter.is_empty()
                            || p.name.to_lowercase().contains(&filter)
                            || p.vendor.to_lowercase().contains(&filter)
                    }) {
                        if shown > 0 {
                            ui.add_space(10.0);
                        }
                        shown += 1;
                        ui.horizontal_top(|ui| {
                            let gap = ui.spacing().item_spacing.x;
                            ui.spacing_mut().item_spacing.x = 0.0;
                            let was = plugin.owned;
                            changed |=
                                check(ui, &mut plugin.owned, &format!("Use {}", plugin.name));
                            if auto && plugin.owned && !was {
                                plugin.lookup = true; // auto config: using it means looking it up
                            }
                            changed |=
                                check(ui, &mut plugin.lookup, &format!("Look up {}", plugin.name));
                            ui.add_space(12.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                ui.label(semibold(&plugin.name, 15.0));
                                let about: Vec<&str> =
                                    [plugin.vendor.as_str(), plugin.kind.as_str()]
                                        .into_iter()
                                        .filter(|s| !s.is_empty())
                                        .collect();
                                if !about.is_empty() {
                                    ui.label(RichText::new(about.join(" · ")).size(13.0).weak());
                                }
                                if !(details && plugin.lookup) {
                                    return;
                                }
                                match knowledge.about(&plugin.name) {
                                    Some(known) if !known.models.is_empty() => {
                                        let mut kind = known.kind().unwrap_or_default().to_string();
                                        if let Some(first) = kind.get_mut(..1) {
                                            first.make_ascii_uppercase();
                                        }
                                        let tag = if kind.is_empty() {
                                            "Found on the web".to_string()
                                        } else {
                                            format!("{kind}  ·  found on the web")
                                        };
                                        ui.label(RichText::new(tag).size(13.0));
                                        // The first few facts; all of them are in the list below.
                                        for fact in known.facts().take(5) {
                                            ui.label(
                                                RichText::new(format!("•  {}", fact.gear))
                                                    .size(13.0),
                                            );
                                        }
                                        let models = models_line(known);
                                        if !models.is_empty() {
                                            ui.label(
                                                RichText::new(format!("•  {models}")).size(13.0),
                                            );
                                        }
                                        egui::CollapsingHeader::new(
                                            RichText::new(format!(
                                                "Where this comes from ({})",
                                                known.models.len()
                                            ))
                                            .size(13.0),
                                        )
                                        .id_salt(("models", &plugin.name))
                                        .show(ui, |ui| {
                                            for (i, m) in known.models.iter().enumerate() {
                                                if i > 0 {
                                                    ui.add_space(8.0);
                                                }
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label(semibold(&m.gear, 14.0));
                                                    if !m.model.is_empty() {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "as {}",
                                                                m.model
                                                            ))
                                                            .size(13.0),
                                                        );
                                                    }
                                                    let what = match m.kind.as_str() {
                                                        ai::TYPE => "what kind it is",
                                                        ai::FACT => "what it does",
                                                        gear => gear,
                                                    };
                                                    ui.label(RichText::new(what).size(12.0).weak());
                                                });
                                                ui.label(
                                                    RichText::new(format!(
                                                        "\u{201C}{}\u{201D}",
                                                        m.quote
                                                    ))
                                                    .size(13.0)
                                                    .italics()
                                                    .weak(),
                                                );
                                                ui.horizontal(|ui| {
                                                    let domain = m
                                                        .url
                                                        .split('/')
                                                        .nth(2)
                                                        .unwrap_or_default()
                                                        .trim_start_matches("www.");
                                                    ui.hyperlink_to(
                                                        RichText::new(domain)
                                                            .size(12.0)
                                                            .color(ui.visuals().hyperlink_color),
                                                        &m.url,
                                                    );
                                                    if ui.small_button("Remove").clicked() {
                                                        remove = Some((plugin.name.clone(), i));
                                                    }
                                                });
                                            }
                                            ui.add_space(6.0);
                                            if ui.small_button("Look it up again").clicked() {
                                                again = Some(plugin.name.clone());
                                            }
                                        });
                                    }
                                    Some(_) => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(
                                                RichText::new(
                                                    "The pages found say nothing GigEyes could check about it.",
                                                )
                                                .size(13.0)
                                                .weak(),
                                            );
                                            if ui.small_button("Look it up again").clicked() {
                                                again = Some(plugin.name.clone());
                                            }
                                        });
                                    }
                                    None if now
                                        .as_ref()
                                        .is_some_and(|(n, ..)| n == &plugin.name) =>
                                    {
                                        ui.horizontal(|ui| {
                                            listening(ui);
                                            ui.label(RichText::new("Looking it up…").size(13.0));
                                        });
                                    }
                                    None => match failed.get(&plugin.name) {
                                        Some(e) => {
                                            ui.horizontal_wrapped(|ui| {
                                                ui.colored_label(
                                                    error(),
                                                    RichText::new(e).size(13.0),
                                                );
                                                if ui.small_button("Try again").clicked() {
                                                    again = Some(plugin.name.clone());
                                                }
                                            });
                                        }
                                        None => {
                                            ui.label(
                                                RichText::new("Not looked up yet.")
                                                    .size(13.0)
                                                    .weak(),
                                            );
                                        }
                                    },
                                }
                            });
                        });
                    }
                    if shown == 0 {
                        ui.label(RichText::new("No plugin or maker matches that.").weak());
                    }
                });
            }
        }
        if let Some((plugin, i)) = remove {
            self.knowledge.remove_model(&plugin, i);
            self.rig_error = self.knowledge.save().err();
        }
        if let Some(plugin) = again {
            self.knowledge.plugins.retain(|k| k.plugin != plugin);
            self.lookup_failed.remove(&plugin);
            self.lookup_paused = false;
            self.lookup_note = None;
            self.rig_error = self.knowledge.save().err();
        }
        changed
    }

    /// Ticked to be looked up, and not looked up yet (nor failed this session).
    fn pending_lookups(&self) -> Vec<(String, String)> {
        self.rig
            .iter()
            .flatten()
            .filter(|p| {
                p.lookup
                    && self.knowledge.about(&p.name).is_none_or(|k| !k.profile)
                    && !self.lookup_failed.contains_key(&p.name)
            })
            .map(|p| (p.name.clone(), p.vendor.clone()))
            .fold(vec![], |mut todo: Vec<(String, String)>, p| {
                // The same plugin in both VST3 folders: one lookup.
                if !todo.iter().any(|(name, _)| *name == p.0) {
                    todo.push(p);
                }
                todo
            })
    }

    /// Take in what the lookup job found, and start one when plugins are waiting and the search key
    /// and the local AI are there. Never while the AI reads a song's pages: one AI at a time.
    fn run_lookups(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.lookup_job {
            let mut finished = false;
            while let Ok(message) = rx.try_recv() {
                match message {
                    Lookup::Now(name, i, n) => self.looking_up = Some((name, i, n)),
                    Lookup::Done(name, Ok(models)) => {
                        self.knowledge.set(&name, models);
                        self.rig_error = self.knowledge.save().err();
                    }
                    Lookup::Done(_, Err(e)) if e == ai::STOPPED => {}
                    Lookup::Done(name, Err(e)) => {
                        self.lookup_failed.insert(name, e);
                    }
                    Lookup::Halt(e) => {
                        self.lookup_paused = true;
                        self.lookup_note = Some(e);
                    }
                    Lookup::Finished => finished = true,
                }
            }
            if finished {
                self.lookup_job = None;
                self.looking_up = None;
            }
            return;
        }
        if self.lookup_paused
            || self.ai_job.is_some()
            || self.onboarding.is_some()
            || self.tavily_key.trim().is_empty()
        {
            return;
        }
        let todo = self.pending_lookups();
        if todo.is_empty() {
            return;
        }
        let choice = self.choice();
        let Some(model) = choice.model.filter(model::is_downloaded) else {
            return;
        };
        let gpu = choice.backend != hardware::Backend::Cpu;
        let (tx, rx) = mpsc::channel();
        self.lookup_cancel = Arc::new(AtomicBool::new(false));
        let (cancel, key, ctx) = (
            self.lookup_cancel.clone(),
            self.tavily_key.clone(),
            ctx.clone(),
        );
        thread::spawn(move || {
            let n = todo.len();
            let _ = tx.send(Lookup::Now(todo[0].0.clone(), 1, n));
            ctx.request_repaint();
            // One session for the whole batch: the model loads once.
            let ran = ai::session(&model, gpu, &cancel, |ai| {
                for (i, (name, vendor)) in todo.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let _ = tx.send(Lookup::Now(name.clone(), i + 1, n));
                    ctx.request_repaint();
                    // A failed search would fail for every plugin (and cost a search each): stop.
                    let pages = match research::search_plugin(&key, name, vendor) {
                        Ok(pages) => pages,
                        Err(e) => {
                            let _ = tx.send(Lookup::Halt(e));
                            break;
                        }
                    };
                    let found = ai.read(&pages, ai::Task::Plugin, &AtomicUsize::new(0));
                    // The AI not answering at all: end this batch; the next starts it afresh.
                    let broken = found.as_ref().err().filter(|e| *e != ai::STOPPED).cloned();
                    let _ = tx.send(Lookup::Done(name.clone(), found));
                    ctx.request_repaint();
                    if let Some(e) = broken {
                        // Pause: the next plugin would cost a search and most likely fail the same way.
                        let _ = tx.send(Lookup::Halt(e));
                        break;
                    }
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                }
            });
            if let Err(e) = ran
                && e != ai::STOPPED
            {
                let _ = tx.send(Lookup::Halt(e));
            }
            let _ = tx.send(Lookup::Finished);
            ctx.request_repaint();
        });
        self.lookup_job = Some(rx);
    }

    /// Above the plugin list: what the lookups are doing, or what they wait for.
    fn lookup_status(&mut self, ui: &mut egui::Ui) {
        let waiting = match self.pending_lookups().len() {
            1 => "1 plugin waits".to_string(),
            n => format!("{n} plugins wait"),
        };
        let line = if let Some((name, i, n)) = self.looking_up.clone() {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    listening(ui);
                    ui.label(format!("Looking up {name} ({i} of {n})…"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Stop").clicked() {
                            self.lookup_cancel.store(true, Ordering::Relaxed);
                            self.lookup_paused = true;
                        }
                    });
                });
            });
            ui.add_space(12.0);
            return;
        } else if self.pending_lookups().is_empty() {
            return;
        } else if self.tavily_key.trim().is_empty() {
            format!("{waiting} to be looked up. Add a search key in Settings to start.")
        } else if !self
            .choice()
            .model
            .is_some_and(|m| model::is_downloaded(&m))
        {
            format!("{waiting} to be looked up. Download the local AI in Settings to start.")
        } else if let Some(note) = &self.lookup_note {
            format!("{note} {waiting} to be looked up.")
        } else if self.lookup_paused {
            format!("{waiting} to be looked up.")
        } else if self.ai_job.is_some() {
            format!("{waiting} to be looked up once the AI has read the song's pages.")
        } else {
            return;
        };
        card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(line).size(13.0));
                if self.lookup_paused && ui.button("Look up now").clicked() {
                    self.lookup_paused = false;
                    self.lookup_note = None;
                }
            });
        });
        ui.add_space(12.0);
    }

    /// Auto config: "New plugin: X. Do you play through it?" for each plugin found since last time.
    fn new_plugin_banner(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.asking.first().cloned() else {
            return;
        };
        let Some(plugin) = self.rig.iter().flatten().find(|p| p.path == path).cloned() else {
            self.asking.remove(0);
            return;
        };
        let mut answer = None;
        card(ui, |ui| {
            ui.label(semibold(format!("New plugin: {}", plugin.name), 16.0));
            if !plugin.vendor.is_empty() {
                ui.label(RichText::new(&plugin.vendor).size(13.0).weak());
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new("Do you play through it? Yes ticks it in Your rig and looks it up.")
                    .size(13.0),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let yes = egui::Button::new(semibold("Yes", 15.0).color(ON_ACCENT)).fill(accent());
                if ui.add(yes).clicked() {
                    answer = Some(true);
                }
                if ui.button("No").clicked() {
                    answer = Some(false);
                }
                let more = self.asking.len() - 1;
                if more > 0 {
                    ui.label(
                        RichText::new(format!("{more} more after this"))
                            .size(13.0)
                            .weak(),
                    );
                }
            });
        });
        ui.add_space(14.0);
        if let Some(yes) = answer {
            self.asking.remove(0);
            if let Some(p) = self.rig.iter_mut().flatten().find(|p| p.path == path) {
                (p.owned, p.lookup) = (yes, yes);
            }
            if let Some(plugins) = &self.rig {
                self.rig_error = rig::save(plugins).err();
            }
        }
    }

    /// The first-start setup, one step at a time; every step can be left as it is.
    fn onboarding_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, step: usize) {
        ui.label(
            RichText::new(format!("Step {} of {STEPS}", step + 1))
                .size(13.0)
                .weak(),
        );
        ui.add_space(6.0);
        let since = self.onboarding_at;
        enter(ui, since, |ui| match step {
            0 => {
                ui.label(semibold("Welcome to GigEyes", 26.0));
                ui.label(
                    RichText::new("A few quick choices so GigEyes fits how you play. You can change all of them later in Settings.")
                        .weak(),
                );
                ui.add_space(16.0);
                card(ui, |ui| {
                    ui.label(semibold("Appearance", 17.0));
                    ui.add_space(8.0);
                    self.appearance(ui, ctx);
                });
            }
            1 => {
                ui.label(semibold("Your guitar", 22.0));
                ui.label(
                    RichText::new(
                        "What do you play? GigEyes keeps it in mind for its suggestions.",
                    )
                    .weak(),
                );
                ui.add_space(16.0);
                card(ui, |ui| self.guitar_field(ui));
            }
            2 => {
                ui.label(semibold("Your plugins", 22.0));
                ui.label(
                    RichText::new("Tick the plugins you play through, and the ones GigEyes should look up. Looking up searches the web for what each plugin is, what it does and what real gear it models, and lets the local AI read the pages; it starts once the next two steps are done, and what it finds stays on this PC.")
                        .weak(),
                );
                ui.add_space(12.0);
                self.new_plugins_choice(ui);
                ui.add_space(12.0);
                if self.rig.is_none() {
                    self.rig = Some(rig::scan(&rig::default_folders()));
                    if let Some(plugins) = &self.rig {
                        self.rig_error = rig::save(plugins).err();
                    }
                }
                if self.plugin_list(ui, false)
                    && let Some(plugins) = &self.rig
                {
                    self.rig_error = rig::save(plugins).err();
                }
            }
            3 => {
                ui.label(semibold("Search key", 22.0));
                ui.label(
                    RichText::new("Song research and plugin lookups search the web with your own Tavily key. The free plan has 1,000 searches a month and needs no card.")
                        .weak(),
                );
                ui.add_space(16.0);
                card(ui, |ui| self.key_field(ui));
            }
            _ => {
                ui.label(semibold("Local AI", 22.0));
                ui.label(
                    RichText::new("The local AI reads the pages GigEyes finds, here on this PC, and quits when it's done.")
                        .weak(),
                );
                ui.add_space(16.0);
                card(ui, |ui| self.local_ai(ui, ctx));
            }
        });
        ui.add_space(20.0);
        let now = ctx.input(|i| i.time);
        ui.horizontal(|ui| {
            if step > 0 && ui.button("Back").clicked() {
                self.onboarding = Some(step - 1);
                self.onboarding_at = now;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let last = step + 1 == STEPS;
                let label = if last { "Finish" } else { "Next" };
                let next = egui::Button::new(semibold(label, 15.0).color(ON_ACCENT))
                    .fill(accent())
                    .min_size(Vec2::new(110.0, 0.0));
                if ui.add(next).clicked() {
                    if last {
                        let mut settings = research::load_settings();
                        settings.onboarded = true;
                        self.rig_error = research::save_settings(&settings).err();
                        self.onboarding = None;
                        self.screen = Screen::Tone;
                        self.shown_at = now;
                    } else {
                        self.onboarding = Some(step + 1);
                        self.onboarding_at = now;
                    }
                }
            });
        });
    }

    /// The guitar's name, saved as it's typed.
    fn guitar_field(&mut self, ui: &mut egui::Ui) {
        let field = egui::TextEdit::singleline(&mut self.guitar)
            .hint_text("e.g. Ibanez RG550")
            .margin(Vec2::new(10.0, 7.0))
            .desired_width(320.0);
        if ui.add(field).changed() {
            let mut settings = research::load_settings();
            settings.guitar = self.guitar.trim().to_string();
            self.rig_error = research::save_settings(&settings).err();
        }
    }

    /// Dark or light, saved, with the crossfade.
    fn appearance(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut light = ctx.theme() == egui::Theme::Light;
        if segmented(
            ui,
            "appearance",
            &mut light,
            &[(false, "Dark"), (true, "Light")],
        ) {
            self.theme_fade = Some((ground(), ctx.input(|i| i.time)));
            apply_theme(ctx, light);
            let mut settings = research::load_settings();
            settings.light = light;
            self.key_note = research::save_settings(&settings).err();
        }
    }

    /// The Tavily key, saved as it's typed or pasted.
    fn key_field(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let field = egui::TextEdit::singleline(&mut self.tavily_key)
                .hint_text("tvly-…")
                .password(true)
                .margin(Vec2::new(10.0, 7.0))
                .desired_width(320.0);
            // Saved as soon as it's typed or pasted: research uses the key at once, so it has
            // to outlast a restart or an update too, with no button to forget.
            if ui.add(field).changed() {
                let mut settings = research::load_settings();
                settings.tavily_key = self.tavily_key.trim().to_string();
                self.key_note = Some(match research::save_settings(&settings) {
                    Ok(()) if self.tavily_key.trim().is_empty() => "Key removed.".into(),
                    Ok(()) => "Saved.".into(),
                    Err(e) => e,
                });
            }
            ui.hyperlink_to("Get a free key", research::KEY_PAGE);
        });
        if let Some(note) = &self.key_note {
            ui.label(RichText::new(note).size(13.0).weak());
        }
    }

    /// What this PC can run, and the download.
    fn local_ai(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let choice = self.choice();
        let hw = self.hardware.as_ref().expect("read by choice()").0.clone();
        let gpu = match &hw.gpu {
            Some(g) => format!("{} with {:.0} GB of video memory", g.name, g.vram_gb),
            None => "no separate graphics card".into(),
        };
        ui.label(
            RichText::new(format!(
                "This PC: {gpu}, and {:.0} GB of memory.",
                hw.ram_gb
            ))
            .size(13.0)
            .weak(),
        );
        ui.add_space(10.0);
        match &choice.model {
            Some(model) => {
                ui.label(semibold(
                    format!("{}  ·  {:.1} GB download", model.name, model.size_gb),
                    15.0,
                ));
            }
            None => {
                ui.label(semibold("No local AI on this PC", 15.0));
            }
        }
        ui.label(RichText::new(&choice.reason).size(13.0));
        if let Some(model) = &choice.model {
            ui.add_space(8.0);
            if self.get_ai.is_some() {
                let done = self.get_ai_done.load(Ordering::Relaxed);
                let shown = ctx.animate_value_with_time(
                    egui::Id::NULL.with("get-ai-bar"),
                    done as f32 / self.get_ai_total.max(1) as f32,
                    secs(0.3),
                );
                ui.add(
                    egui::ProgressBar::new(shown)
                        .fill(accent())
                        .desired_height(6.0)
                        .corner_radius(3),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{:.1} of {:.1} GB",
                            done as f32 / 1e9,
                            self.get_ai_total as f32 / 1e9
                        ))
                        .size(13.0)
                        .weak(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Stop").clicked() {
                            self.get_ai_cancel.store(true, Ordering::Relaxed);
                        }
                    });
                });
            } else if model::is_downloaded(model) {
                ui.label(
                    RichText::new("Downloaded. It reads your research pages when you ask, then quits to free the memory.")
                        .size(13.0)
                        .weak(),
                );
            } else {
                let missing = model::missing_bytes(model) as f32 / 1e9;
                let go = egui::Button::new(
                    semibold(format!("Download, {missing:.1} GB"), 15.0).color(ON_ACCENT),
                )
                .fill(accent());
                if ui.add(go).clicked() {
                    self.start_ai_download(ctx);
                }
            }
            if let Some(note) = &self.get_ai_note {
                ui.add_space(6.0);
                ui.colored_label(error(), note);
            }
        }
    }

    fn settings_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(semibold("Settings", 22.0));
        ui.add_space(16.0);
        card(ui, |ui| {
            ui.label(semibold("Appearance", 17.0));
            ui.add_space(8.0);
            self.appearance(ui, ctx);
        });

        ui.add_space(14.0);
        card(ui, |ui| {
            ui.label(semibold("Startup", 17.0));
            ui.label(
                RichText::new("When GigEyes opens, its logo plays a short intro. Click or press any key to skip it.")
                    .size(13.0)
                    .weak(),
            );
            ui.add_space(10.0);
            let options = [
                (research::Intro::Off, "Off"),
                (research::Intro::EveryLaunch, "Every launch"),
                (research::Intro::AfterUpdate, "After updates"),
            ];
            if segmented(ui, "intro", &mut self.intro_choice, &options) {
                let mut settings = research::load_settings();
                settings.intro = self.intro_choice;
                self.key_note = research::save_settings(&settings).err();
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Length").size(13.0));
                ui.add_space(6.0);
                ui.spacing_mut().slider_width = 220.0;
                ui.visuals_mut().selection.bg_fill = accent(); // the filled part of the rail
                let slider = ui.add(
                    egui::Slider::new(&mut self.intro_length, 1.5..=4.0)
                        .trailing_fill(true)
                        .step_by(0.1)
                        .fixed_decimals(1)
                        .suffix(" s"),
                );
                // Saved once the slider is let go (or after a keyboard step), not on every frame of a drag.
                if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                    let mut settings = research::load_settings();
                    settings.intro_length = self.intro_length;
                    self.key_note = research::save_settings(&settings).err();
                }
            });
            ui.add_space(10.0);
            if ui.button("Play it now").clicked() {
                self.intro_at = Some(ctx.input(|i| i.time));
            }
        });

        ui.add_space(14.0);
        card(ui, |ui| {
            ui.label(semibold("Your guitar", 17.0));
            ui.add_space(8.0);
            self.guitar_field(ui);
        });

        ui.add_space(14.0);
        card(ui, |ui| {
            ui.label(semibold("First-start setup", 17.0));
            ui.label(
                RichText::new("Go through appearance, your guitar, your plugins, the search key and the local AI again.")
                    .size(13.0)
                    .weak(),
            );
            ui.add_space(10.0);
            if ui.button("Run it again").clicked() {
                self.onboarding = Some(0);
                self.onboarding_at = ctx.input(|i| i.time);
            }
        });
        ui.add_space(14.0);
        card(ui, |ui| {
            ui.label(semibold("Song research", 17.0));
            ui.label(
                RichText::new("GigEyes searches the web with your own Tavily key. The free plan has 1,000 searches a month and needs no card.")
                    .size(13.0)
                    .weak(),
            );
            self.key_field(ui);
        });

        ui.add_space(14.0);
        card(ui, |ui| {
            ui.label(semibold("Local AI", 17.0));
            self.local_ai(ui, ctx);
        });

        ui.add_space(14.0);
        card(ui, |ui| {
            ui.label(semibold("Updates", 17.0));
            ui.label(
                RichText::new(format!(
                    "This is GigEyes {VERSION}. Each time it opens, GigEyes checks GitHub for a newer version."
                ))
                .size(13.0)
                .weak(),
            );
            if let Some(note) = &self.update_note {
                ui.add_space(6.0);
                ui.label(RichText::new(note).size(13.0));
            }
            ui.add_space(10.0);
            let idle = self.update_check.is_none() && self.update_job.is_none();
            if ui
                .add_enabled(idle, egui::Button::new("Check now"))
                .clicked()
            {
                self.check_updates(ctx);
            }
        });

        ui.add_space(14.0);
        let uninstaller = uninstaller();
        card(ui, |ui| {
            ui.label(semibold("Remove all GigEyes files", 17.0));
            let what = if uninstaller.is_some() {
                "Deletes your rig, plugin lookups, saved research, search key, settings and the downloaded AI, then opens the uninstaller to remove the app itself."
            } else {
                "Deletes your rig, plugin lookups, saved research, search key, settings and the downloaded AI from this PC."
            };
            ui.label(RichText::new(what).size(13.0).weak());
            ui.add_space(10.0);
            if !self.confirm_remove {
                if ui.button("Remove all files…").clicked() {
                    self.confirm_remove = true;
                    self.confirm_at = ctx.input(|i| i.time);
                    self.remove_note = None;
                }
            } else {
                enter(ui, self.confirm_at, |ui| {
                    ui.label(semibold("This can't be undone. Remove everything?", 15.0));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let remove =
                            egui::Button::new(semibold("Remove everything", 15.0).color(error()));
                        if ui.add(remove).clicked() {
                            self.remove_all(ctx, uninstaller.as_deref());
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_remove = false;
                        }
                    });
                });
            }
            if let Some(note) = &self.remove_note {
                ui.add_space(8.0);
                ui.label(RichText::new(note).size(13.0));
            }
        });
    }

    fn check_updates(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            update::clear_downloads();
            let _ = tx.send(update::check(VERSION));
            ctx.request_repaint();
        });
        self.update_check = Some(rx);
        self.update_note = Some("Checking for updates…".into());
    }

    /// Download the newer setup in the background; once it's checked, it runs and GigEyes closes.
    fn start_update(&mut self, ctx: &egui::Context) {
        let Some(found) = self.update.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let (done, ctx) = (self.update_done.clone(), ctx.clone());
        done.store(0, Ordering::Relaxed);
        thread::spawn(move || {
            let result = update::download(&found, |bytes| {
                done.store(bytes, Ordering::Relaxed);
                ctx.request_repaint();
            });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.update_job = Some(rx);
        self.update_note = None;
    }

    /// "GigEyes 0.4.2 is out", above every screen, until it's installed.
    fn update_banner(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(found) = self.update.clone() else {
            return;
        };
        enter(ui, self.update_at, |ui| {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(semibold(format!("GigEyes {} is out", found.version), 16.0));
                        ui.hyperlink_to(
                            RichText::new("What's new")
                                .size(13.0)
                                .color(ui.visuals().hyperlink_color),
                            &found.page,
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.update_job.is_some() {
                            ui.label(RichText::new("Downloading…").weak());
                        } else {
                            let go =
                                egui::Button::new(semibold("Update now", 15.0).color(ON_ACCENT))
                                    .fill(accent());
                            if ui.add(go).clicked() {
                                self.start_update(ctx);
                            }
                        }
                    });
                });
                if self.update_job.is_some() {
                    let target =
                        self.update_done.load(Ordering::Relaxed) as f32 / found.bytes.max(1) as f32;
                    let shown = ctx.animate_value_with_time(
                        egui::Id::NULL.with("update-bar"),
                        target,
                        secs(0.3),
                    );
                    ui.add_space(10.0);
                    ui.add(
                        egui::ProgressBar::new(shown)
                            .fill(accent())
                            .desired_height(6.0)
                            .corner_radius(3),
                    );
                }
                if let Some(note) = &self.update_note {
                    ui.add_space(8.0);
                    ui.colored_label(error(), note);
                }
            });
        });
        ui.add_space(14.0);
    }

    /// Delete everything GigEyes saved, then hand over to the uninstaller for the app itself.
    fn remove_all(&mut self, ctx: &egui::Context, uninstaller: Option<&std::path::Path>) {
        self.confirm_remove = false;
        // A running AI or download holds its files open, and Windows won't delete open files:
        // stop them all first.
        self.stop_reading();
        self.research_job = None; // a search finishing later would save into the removed folder
        self.lookup_cancel.store(true, Ordering::Relaxed);
        (self.lookup_job, self.looking_up) = (None, None);
        self.get_ai_cancel.store(true, Ordering::Relaxed);
        // Only GigEyes' own folders: %APPDATA%\GigEyes and %LOCALAPPDATA%\GigEyes (the AI model).
        let folders = [
            research::data_dir(),
            model::dir().and_then(|d| d.parent().map(|p| p.to_path_buf())),
        ];
        // They take a moment to let go of their files, so try for up to three seconds.
        let mut failed: Vec<String> = vec![];
        for attempt in 0..15 {
            if attempt > 0 {
                thread::sleep(std::time::Duration::from_millis(200));
            }
            failed = folders
                .iter()
                .flatten()
                .filter_map(|dir| match std::fs::remove_dir_all(dir) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        Some(format!("{}: {e}", dir.display()))
                    }
                    _ => None,
                })
                .collect();
            if failed.is_empty() {
                break;
            }
        }
        if !failed.is_empty() {
            self.remove_note = Some(format!(
                "Some files couldn't be removed. {}",
                failed.join("; ")
            ));
            return;
        }
        // Forget what was read from those files.
        (self.rig, self.research, self.key_note) = (None, None, None);
        self.knowledge = knowledge::Knowledge::default();
        self.lookup_failed.clear();
        (self.lookup_paused, self.lookup_note) = (false, None);
        self.asking.clear();
        self.tavily_key.clear();
        self.guitar.clear();
        self.remove_note = Some(match uninstaller {
            Some(path) => match update::run_as_admin(path, "--uninstall") {
                Ok(()) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    "Your files are removed. Opening the uninstaller…".into()
                }
                Err(e) => format!("Your files are removed, but the uninstaller didn't start. {e}"),
            },
            None => "All GigEyes files are removed.".into(),
        });
    }
}

/// The uninstaller next to gigeyes.exe, when GigEyes was installed with the setup.
fn uninstaller() -> Option<PathBuf> {
    let path = std::env::current_exe()
        .ok()?
        .parent()?
        .join("uninstall.exe");
    path.is_file().then_some(path)
}

/// Light or dark: switch the palette, rebuild the style for that mode and show it.
fn apply_theme(ctx: &egui::Context, light: bool) {
    set_light(light);
    let mode = if light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    };
    ctx.style_mut_of(mode, theme);
    ctx.set_theme(mode);
}

/// Search excerpts can run long; keep a readable two or three lines.
/// "Models the Mesa/Boogie Mark IIC+", or how many and a few of them, for a looked-up plugin.
fn models_line(known: &knowledge::Known) -> String {
    let mut real: Vec<&str> = vec![];
    for model in known.gear() {
        let name = model.gear.split('(').next().unwrap_or_default().trim();
        if !name.is_empty() && !real.contains(&name) {
            real.push(name);
        }
    }
    match real.as_slice() {
        [] => String::new(),
        [one] => format!("Models the {one}"),
        few if few.len() <= 4 => format!(
            "Models the {} and {}",
            few[..few.len() - 1].join(", "),
            few[few.len() - 1]
        ),
        many => format!(
            "Models {} pieces of real gear, including the {}, {} and {}",
            many.len(),
            many[0],
            many[1],
            many[2]
        ),
    }
}

/// A number that changes whenever the findings, what's known about the plugins, or the rig's ticks do.
fn fingerprint(
    findings: &[ai::Finding],
    knowledge: &knowledge::Knowledge,
    rig: Option<&[rig::Plugin]>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in findings {
        (&f.gear, &f.kind).hash(&mut h);
    }
    for known in &knowledge.plugins {
        known.plugin.hash(&mut h);
        for m in &known.models {
            (&m.gear, &m.kind, &m.model).hash(&mut h);
        }
    }
    for plugin in rig.into_iter().flatten() {
        (&plugin.name, plugin.owned).hash(&mut h);
    }
    h.finish()
}

/// "In your rig: Helix Native (Cali Rectifire), TSE808 and 1 more" when ticked plugins model this gear.
fn in_your_rig(
    knowledge: &knowledge::Knowledge,
    gear: &str,
    kind: &str,
    rig: Option<&[rig::Plugin]>,
) -> Option<String> {
    let matches = knowledge.who_models(gear, kind, rig?);
    let named: Vec<String> = matches
        .iter()
        .take(3)
        .map(|m| {
            let single = knowledge
                .about(m.plugin)
                .is_some_and(|k| k.gear().count() == 1);
            match (single, m.model.model.is_empty()) {
                (true, _) => m.plugin.to_string(), // a plugin that is that one amp
                (false, false) => format!("{} ({})", m.plugin, m.model.model),
                (false, true) => format!("{} ({})", m.plugin, m.model.gear),
            }
        })
        .collect();
    let more = matches.len().saturating_sub(named.len());
    (!named.is_empty()).then(|| {
        let more = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        format!("In your rig: {}{more}", named.join(", "))
    })
}

fn snippet(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 260 {
        return flat;
    }
    let cut: String = flat.chars().take(260).collect();
    format!(
        "{}…",
        cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head)
    )
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if let Some(rx) = &self.result
            && let Ok(result) = rx.try_recv()
        {
            self.state = match result {
                Ok((name, tone)) => {
                    self.song = research::song_from_title(&name);
                    self.research = research::cached(&self.song);
                    self.research_job = None; // a search still running was for the previous song
                    self.research_note = None;
                    self.stop_reading();
                    State::Done(name, tone)
                }
                Err(e) => State::Failed(e),
            };
            self.result = None;
            self.revealed_at = now;
        }
        if let Some(rx) = &self.research_job
            && let Ok(found) = rx.try_recv()
        {
            match found {
                Ok(found) => {
                    self.research_note = research::save(&found).err();
                    self.research = Some(found);
                    self.stop_reading(); // it was reading the sources just replaced
                }
                Err(e) => self.research_note = Some(e),
            }
            self.research_job = None;
        }
        if let Some(rx) = &self.ai_job
            && let Ok(read) = rx.try_recv()
        {
            self.ai_job = None;
            match (read, &mut self.research) {
                (Ok(findings), Some(research)) => {
                    research.findings = Some(findings);
                    self.ai_note = research::save(research).err();
                }
                (Err(e), _) if e != ai::STOPPED => self.ai_note = Some(e),
                _ => {}
            }
        }
        if let Some(rx) = &self.get_ai
            && let Ok(got) = rx.try_recv()
        {
            self.get_ai = None;
            self.get_ai_note = got.err().filter(|e| e != "Skipped.");
        }
        if let Some(path) =
            ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()))
        {
            self.screen = Screen::Tone; // the result shows there, whichever tab it was dropped on
            self.open(path, &ctx);
        }
        if let Some(rx) = &self.update_check
            && let Ok(found) = rx.try_recv()
        {
            self.update_check = None;
            match found {
                Ok(Some(newer)) => {
                    self.update = Some(newer);
                    self.update_at = now;
                    self.update_note = None;
                }
                Ok(None) => self.update_note = Some("You have the newest version.".into()),
                Err(e) => self.update_note = Some(e),
            }
        }
        if let Some(rx) = &self.update_job
            && let Ok(setup) = rx.try_recv()
        {
            self.update_job = None;
            // The setup waits for GigEyes to close, installs, and opens the new version.
            match setup.and_then(|setup| update::run_as_admin(&setup, "--update")) {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => self.update_note = Some(e),
            }
        }
        if std::mem::take(&mut self.intro_queued) {
            self.intro_at = Some(now);
        }
        // Any key or a dropped file skips the intro (a click is caught below, over the intro).
        let skip = ctx.input(|i| {
            !i.raw.dropped_files.is_empty()
                || i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
        });
        let intro = self
            .intro_at
            .filter(|_| !skip)
            .and_then(|at| intro::at(now - at, self.intro_length as f64));
        if intro.is_none() {
            self.intro_at = None;
        }
        if self.bare && intro.is_none_or(intro::gliding) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
            self.bare = false;
        }
        let (head_in, body_in) = intro.map_or((1.0, 1.0), intro::content);
        let mut home = egui::Rect::NOTHING;

        self.run_lookups(&ctx);
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let has_result = matches!(self.state, State::Done(..));
        if (self.screen, has_result) != self.shown {
            self.shown = (self.screen, has_result);
            self.shown_at = now;
        }
        let reveal = ((now - self.revealed_at) / REVEAL_SECS).clamp(0.0, 1.0) as f32;
        if has_result && reveal < 1.0 {
            ctx.request_repaint();
        }

        let margin = egui::Margin {
            top: 28,
            ..Default::default()
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(margin))
            .show(ui, |ui| {
                // One readable column, centred on wide windows; the scrollbar stays at the window's edge.
                let side = ((ui.available_width() - MAX_WIDTH) / 2.0).max(32.0);
                let width = ui.available_width() - 2.0 * side;
                column(ui, side, width, |ui| {
                    home = settle(ui, head_in, |ui| {
                        header(
                            ui,
                            &mut self.screen,
                            intro.is_none(),
                            self.onboarding.is_none(),
                        )
                    });
                });
                ui.add_space(20.0);
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        column(ui, side, width, |ui| {
                            settle(ui, body_in, |ui| {
                                ui.add_space(4.0);
                                self.update_banner(ui, &ctx);
                                if let Some(step) = self.onboarding {
                                    self.onboarding_view(ui, &ctx, step);
                                } else {
                                    self.new_plugin_banner(ui);
                                    let since = self.shown_at;
                                    enter(ui, since, |ui| match self.screen {
                                        Screen::Tone => self.tone_view(ui, &ctx, hovering, reveal),
                                        Screen::Rig => self.rig_view(ui),
                                        Screen::Settings => self.settings_view(ui, &ctx),
                                    });
                                }
                                ui.add_space(32.0);
                            });
                        });
                    });
            });

        if let Some(elapsed) = intro {
            // Over the whole window while it plays: the app underneath isn't clickable yet, and a
            // click skips to it.
            let cover = egui::Area::new(egui::Id::NULL.with("intro-cover"))
                .order(egui::Order::Foreground)
                .fixed_pos(ctx.content_rect().min)
                .show(&ctx, |ui| {
                    ui.allocate_response(ctx.content_rect().size(), Sense::click())
                });
            if cover.inner.clicked() {
                self.intro_at = None;
            }
            intro::logo(&ctx, elapsed, home);
            ctx.request_repaint();
        }

        let drop = ctx.animate_bool_with_time(egui::Id::NULL.with("drop"), hovering, secs(0.15));
        if drop > 0.0 {
            drop_overlay(&ctx, drop);
        }
        // Light and dark mode crossfade instead of flashing: the old background fades off the new one.
        if let Some((old, since)) = self.theme_fade {
            let length = secs(0.35);
            let t = if length > 0.0 {
                ((now - since) as f32 / length).min(1.0)
            } else {
                1.0
            };
            if t >= 1.0 {
                self.theme_fade = None;
            } else {
                let fade = 1.0 - egui::emath::easing::cubic_out(t);
                ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::NULL.with("theme-fade"),
                ))
                .rect_filled(ctx.content_rect(), 0.0, old.gamma_multiply(fade));
                ctx.request_repaint();
            }
        }
    }
}

/// `width` wide, `side` in from the left.
fn column(ui: &mut egui::Ui, side: f32, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        ui.add_space(side);
        ui.vertical(|ui| {
            ui.set_width(width);
            add(ui);
        });
    });
}

/// Fast start, gentle stop, no overshoot.
fn ease(t: f32) -> f32 {
    egui::emath::easing::cubic_out(t)
}

fn icon(source: ImageSource<'static>, size: f32, tint: Color32) -> egui::Image<'static> {
    egui::Image::new(source)
        .fit_to_exact_size(Vec2::splat(size))
        .tint(tint)
}

/// Returns where the mark sits, for the intro to land on. `show_mark` is false while the intro's own
/// pick is still on its way there.
fn header(ui: &mut egui::Ui, screen: &mut Screen, show_mark: bool, tabs: bool) -> egui::Rect {
    ui.horizontal(|ui| {
        let (home, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
        if show_mark {
            paint_mark(&ui.painter_at(home), home, [1.0; 5], 1.0);
        }
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(semibold("GigEyes", 24.0));
            ui.label(RichText::new("Hear a tone. See where the knobs sit.").weak());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !tabs {
                return; // the first-start setup leads through its own steps
            }
            segmented(
                ui,
                "screens",
                screen,
                &[
                    (Screen::Tone, "Tone"),
                    (Screen::Rig, "Your rig"),
                    (Screen::Settings, "Settings"),
                ],
            );
        });
        home
    })
    .inner
}

/// Five bars moving like a level meter while a file is being read.
fn listening(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(26.0, 18.0), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    for i in 0..5 {
        let level = 0.3 + 0.7 * (0.5 + 0.5 * (t * 7.0 + i as f32 * 1.3).sin());
        let x = rect.left() + 3.0 + i as f32 * 5.0;
        let half = rect.height() * level / 2.0;
        ui.painter().line_segment(
            [
                Pos2::new(x, rect.center().y - half),
                Pos2::new(x, rect.center().y + half),
            ],
            Stroke::new(3.0, accent()),
        );
    }
    ui.ctx().request_repaint();
}

/// The big "drop a file here" area. Returns true when its button was clicked.
fn drop_zone(ui: &mut egui::Ui, hovering: bool, busy: bool) -> bool {
    let stroke = if hovering {
        Stroke::new(2.0, accent())
    } else {
        Stroke::new(1.0, line())
    };
    egui::Frame::new()
        .fill(surface())
        .stroke(stroke)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(36.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add(icon(ICON_FILE, 38.0, muted()));
                ui.add_space(12.0);
                ui.label(semibold("Drop a song, a guitar stem or a live video", 19.0));
                ui.add_space(4.0);
                ui.label(RichText::new(AUDIO_TYPES.join(", ")).size(13.0).weak());
                ui.add_space(18.0);
                let button = egui::Button::new(semibold("Choose file…", 15.0).color(ON_ACCENT))
                    .fill(accent());
                ui.add_enabled(!busy, button).clicked()
            })
            .inner
        })
        .inner
}

/// Link field and Load button, plus a compact "Open file…" once a result is showing.
/// Returns (open file clicked, load link requested).
fn input_row(ui: &mut egui::Ui, link: &mut String, compact: bool, busy: bool) -> (bool, bool) {
    let mut pick = false;
    let mut load = false;
    ui.horizontal(|ui| {
        if compact {
            pick = ui
                .add_enabled(
                    !busy,
                    egui::Button::image_and_text(icon(ICON_FILE, 16.0, ink()), "Open file…"),
                )
                .clicked();
            ui.add_space(6.0);
        }
        ui.add(icon(ICON_LINK, 18.0, muted()));
        let load_width = 84.0;
        let field = ui.add(
            egui::TextEdit::singleline(link)
                .hint_text("Paste a YouTube or other video link")
                .margin(Vec2::new(10.0, 7.0))
                .desired_width(ui.available_width() - load_width - ui.spacing().item_spacing.x),
        );
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let button = ui.add_enabled(
            !busy,
            egui::Button::new("Load").min_size(Vec2::new(load_width, 0.0)),
        );
        load = (button.clicked() || enter) && !busy;
    });
    (pick, load)
}

fn report(ui: &mut egui::Ui, name: &str, tone: &Tone, reveal: f32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).weak());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let secs = tone.seconds.round() as u32;
            ui.label(RichText::new(format!("{}:{:02} listened", secs / 60, secs % 60)).weak());
        });
    });
    ui.add_space(4.0);
    ui.label(semibold(&tone.verdict, 26.0));
    ui.add_space(20.0);

    amp_panel(ui, &tone.knobs, reveal, true);
    ui.add_space(14.0);

    card(ui, |ui| {
        ui.label(semibold("EQ curve", 17.0));
        ui.label(
            RichText::new("Above the line is louder than average, below is quieter. Hover a bar for its value.")
                .size(13.0)
                .weak(),
        );
        ui.add_space(12.0);
        eq_chart(ui, tone, reveal);
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Slope {:.1} dB per octave  ·  Peak-to-average {:.1} dB",
                tone.tilt_db_per_octave, tone.crest_db
            ))
            .size(12.0)
            .weak(),
        );
    });
}

/// Five amp knobs on one panel. Before any result (`live` false) they wait at noon, greyed out.
fn amp_panel(ui: &mut egui::Ui, knobs: &[Knob], reveal: f32, live: bool) {
    card(ui, |ui| {
        ui.label(semibold("Where an amp would sit", 17.0));
        let caption = if live {
            "Starting points on an amp with its EQ flat at noon. Hover a knob for the measurement."
        } else {
            "Drop something in and the knobs turn to match it."
        };
        ui.label(RichText::new(caption).size(13.0).weak());
        ui.add_space(14.0);
        ui.columns(knobs.len(), |cols| {
            for (col, knob) in cols.iter_mut().zip(knobs) {
                dial(col, knob, reveal, live);
            }
        });
    });
}

fn waiting_knobs() -> Vec<Knob> {
    ["Gain", "Bass", "Middle", "Treble", "Presence"]
        .map(|label| Knob {
            label,
            value: 5.0,
            verdict: " ", // keeps the row's height, so nothing jumps when the result lands
            detail: "Nothing listened to yet.".into(),
        })
        .into()
}

/// One amp knob: label, a dial with 0–10 ticks turned to the knob's value, the value and its verdict.
fn dial(ui: &mut egui::Ui, knob: &Knob, reveal: f32, live: bool) {
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(knob.label).weak());
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 86.0), Sense::hover());
        let painter = ui.painter_at(rect);
        let c = rect.center();
        let r = (rect.height() / 2.0 - 10.0).min(rect.width() / 2.0 - 10.0);
        // 270° sweep from bottom-left (0) over the top to bottom-right (10); screen y points down.
        let start = 0.75 * std::f32::consts::PI;
        let angle_at = |v: f32| start + v / 10.0 * 1.5 * std::f32::consts::PI;
        let at = |a: f32, radius: f32| c + Vec2::new(a.cos(), a.sin()) * radius;
        let arc = |from: f32, to: f32, radius: f32| -> Vec<Pos2> {
            let n = ((to - from).abs() * 20.0).ceil().max(2.0) as usize;
            (0..=n)
                .map(|i| at(from + (to - from) * i as f32 / n as f32, radius))
                .collect()
        };

        for tick in 0..=10 {
            painter.circle_filled(
                at(angle_at(tick as f32), r + 7.0),
                if tick % 5 == 0 { 1.8 } else { 1.2 },
                muted(),
            );
        }
        let value = if live {
            knob.value * reveal
        } else {
            knob.value
        };
        let (arc_colour, pointer) = if live {
            (accent(), ink())
        } else {
            (line(), muted())
        };
        painter.add(Shape::line(
            arc(angle_at(0.0), angle_at(10.0), r),
            Stroke::new(3.0, raised()),
        ));
        painter.add(Shape::line(
            arc(angle_at(0.0), angle_at(value), r),
            Stroke::new(3.0, arc_colour),
        ));
        painter.circle(c, r - 7.0, raised(), Stroke::new(1.0, line()));
        painter.line_segment(
            [
                at(angle_at(value), (r - 7.0) * 0.25),
                at(angle_at(value), r - 11.0),
            ],
            Stroke::new(3.0, pointer),
        );

        response.on_hover_text(&knob.detail);
        let shown = if live {
            format!("{value:.1}")
        } else {
            "–".into()
        };
        ui.label(semibold(shown, 20.0));
        ui.label(RichText::new(knob.verdict).size(13.0).weak());
    });
}

/// Bars around a 0 dB line, with the main frequency regions marked and a value on hover.
fn eq_chart(ui: &mut egui::Ui, tone: &Tone, reveal: f32) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 210.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let plot = egui::Rect::from_min_max(
        rect.min + Vec2::new(0.0, 22.0),
        rect.max - Vec2::new(0.0, 20.0),
    );
    let range_db = tone.levels_db.iter().fold(12.0f32, |m, l| m.max(l.abs()));
    let mid_y = plot.center().y;
    let y_for = |db: f32| mid_y - db * reveal / range_db * plot.height() / 2.0;
    let slot = plot.width() / tone.levels_db.len() as f32;
    let (first, last) = (
        tone.centers[0].log2(),
        tone.centers[tone.centers.len() - 1].log2(),
    );
    let x_for_hz = |hz: f32| {
        plot.left() + slot * 0.5 + (hz.log2() - first) / (last - first) * (plot.width() - slot)
    };

    painter.rect_filled(rect, CornerRadius::same(RADIUS - 2), well());
    // Frequency regions, every other one a tone step lighter, named along the top.
    let regions = [
        (60.0, 250.0, "Lows"),
        (250.0, 800.0, "Low mids"),
        (800.0, 2000.0, "Mids"),
        (2000.0, 5000.0, "Highs"),
        (5000.0, 12_000.0, "Air"),
    ];
    for (i, (lo, hi, label)) in regions.iter().enumerate() {
        let x0 = x_for_hz(*lo).max(plot.left());
        let x1 = x_for_hz(*hi).min(plot.right());
        if x1 <= x0 {
            continue;
        }
        if i % 2 == 1 {
            painter.rect_filled(
                egui::Rect::from_x_y_ranges(x0..=x1, plot.y_range()),
                0.0,
                surface().gamma_multiply(0.7),
            );
        }
        painter.text(
            egui::pos2((x0 + x1) / 2.0, rect.top() + 4.0),
            Align2::CENTER_TOP,
            *label,
            FontId::proportional(12.0),
            muted(),
        );
    }
    painter.line_segment(
        [plot.left_center(), plot.right_center()],
        Stroke::new(1.0, line()),
    );

    let hovered = response
        .hover_pos()
        .filter(|p| plot.x_range().contains(p.x))
        .map(|p| (((p.x - plot.left()) / slot) as usize).min(tone.levels_db.len() - 1));
    for (i, db) in tone.levels_db.iter().enumerate() {
        let x = plot.left() + slot * (i as f32 + 0.5);
        let (top, bottom) = if *db >= 0.0 {
            (y_for(*db), mid_y)
        } else {
            (mid_y, y_for(*db))
        };
        let bar = egui::Rect::from_x_y_ranges(
            x - slot * 0.3..=x + slot * 0.3,
            top..=bottom.max(top + 1.0),
        );
        let colour = if hovered == Some(i) { ink() } else { accent() };
        painter.rect_filled(bar, 3.0, colour);
    }
    for (hz, label) in [(100.0, "100 Hz"), (1000.0, "1 kHz"), (10_000.0, "10 kHz")] {
        let x = x_for_hz(hz);
        if plot.x_range().contains(x) {
            painter.text(
                egui::pos2(x, rect.bottom() - 3.0),
                Align2::CENTER_BOTTOM,
                label,
                FontId::proportional(12.0),
                muted(),
            );
        }
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(RADIUS - 2),
        Stroke::new(1.0, line()),
        StrokeKind::Inside,
    );

    if let Some(i) = hovered {
        let hz = tone.centers[i];
        let freq = if hz >= 1000.0 {
            format!("{:.1} kHz", hz / 1000.0)
        } else {
            format!("{hz:.0} Hz")
        };
        response.on_hover_text_at_pointer(format!("{freq}: {:+.1} dB", tone.levels_db[i]));
    }
}

/// While a file is dragged over the window: dim everything and say what will happen.
/// `shown` runs 0 to 1 as the overlay fades in.
fn drop_overlay(ctx: &egui::Context, shown: f32) {
    let rect = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::NULL.with("drop_overlay"),
    ));
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha((170.0 * shown) as u8));
    painter.rect_stroke(
        rect.shrink(12.0),
        CornerRadius::same(RADIUS * 2),
        Stroke::new(2.0, accent().gamma_multiply(shown)),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "Drop it. GigEyes is listening.",
        FontId::new(24.0, FontFamily::Name("semibold".into())),
        Color32::WHITE.gamma_multiply(shown),
    );
}
