// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod decode;
mod fetch;

use analysis::Tone;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, RichText, Sense, Stroke, StrokeKind, Vec2,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

const ACCENT: Color32 = Color32::from_rgb(232, 168, 56);
const AUDIO_TYPES: [&str; 9] = [
    "wav", "mp3", "flac", "ogg", "m4a", "mp4", "aac", "mkv", "mov",
];

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 760.0])
            .with_min_inner_size([480.0, 420.0]),
        ..Default::default()
    };
    // A file passed on the command line (or dropped onto gigeyes.exe in Explorer) opens right away.
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    fetch::clean_old_downloads(&std::env::temp_dir(), std::time::Duration::from_secs(3600));
    eframe::run_native(
        "GigEyes",
        options,
        Box::new(|cc| {
            let mut app = App::default();
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
        self.start(format!("Reading {}…", file_name(&path)), ctx, move || {
            read_tone(&path)
        });
    }

    fn open_link(&mut self, ctx: &egui::Context) {
        let link = match fetch::check_link(&self.link) {
            Ok(link) => link.to_string(),
            Err(e) => return self.state = State::Failed(e),
        };
        self.start(
            "Downloading the audio and reading it…".into(),
            ctx,
            move || {
                // The folder and the download are deleted when `temp` goes out of scope, even on errors.
                let temp = fetch::TempDir::new()?;
                read_tone(&fetch::download(&link, &temp.0)?)
            },
        );
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(rx) = &self.result
            && let Ok(result) = rx.try_recv()
        {
            self.state = match result {
                Ok((name, tone)) => State::Done(name, tone),
                Err(e) => State::Failed(e),
            };
            self.result = None;
        }
        if let Some(path) =
            ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()))
        {
            self.open(path, &ctx);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        egui::CentralPanel::default().frame(egui::Frame::central_panel(ui.style()).inner_margin(28.0)).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(RichText::new("GigEyes").size(26.0).strong());
                ui.label(RichText::new("Read the tone of a song or guitar track.").weak());
                ui.add_space(20.0);

                if drop_zone(ui, hovering) {
                    let picked = rfd::FileDialog::new().add_filter("Audio or video", &AUDIO_TYPES).pick_file();
                    if let Some(path) = picked {
                        self.open(path, &ctx);
                    }
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let button_width = 80.0;
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.link)
                            .hint_text("…or paste a YouTube or other video link")
                            .desired_width(ui.available_width() - button_width - ui.spacing().item_spacing.x),
                    );
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let busy = matches!(self.state, State::Reading(_));
                    let load = ui.add_enabled(!busy, egui::Button::new("Load").min_size(Vec2::new(button_width, 0.0)));
                    if (load.clicked() || enter) && !busy {
                        self.open_link(&ctx);
                    }
                });
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Works best on an isolated guitar track. On a full mix, bass, drums and vocals colour the result.",
                    )
                    .size(13.0)
                    .weak(),
                );
                ui.add_space(24.0);

                match &self.state {
                    State::Empty => {}
                    State::Reading(label) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(label);
                        });
                    }
                    State::Failed(e) => {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    State::Done(name, tone) => report(ui, name, tone),
                }
            });
        });
    }
}

/// The big "drop a file here" area. Returns true when its button was clicked.
fn drop_zone(ui: &mut egui::Ui, hovering: bool) -> bool {
    let stroke = if hovering {
        Stroke::new(2.0, ACCENT)
    } else {
        Stroke::new(1.0, ui.visuals().weak_text_color())
    };
    egui::Frame::new()
        .stroke(stroke)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(28.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Drop an audio or video file here").size(17.0));
                ui.add_space(4.0);
                ui.label(RichText::new(AUDIO_TYPES.join(", ")).size(13.0).weak());
                ui.add_space(14.0);
                ui.button(RichText::new("Choose file…").size(15.0))
                    .clicked()
            })
            .inner
        })
        .inner
}

fn report(ui: &mut egui::Ui, name: &str, tone: &Tone) {
    ui.label(RichText::new(name).size(18.0).strong());
    ui.label(RichText::new(format!("{:.0} s analysed", tone.seconds)).weak());
    ui.add_space(12.0);
    for line in &tone.summary {
        ui.label(RichText::new(format!("•  {line}")).size(15.0));
    }
    ui.add_space(20.0);
    ui.label(RichText::new("EQ curve").strong());
    eq_chart(ui, tone);
    ui.add_space(8.0);
    ui.label(
        RichText::new(format!(
            "Slope {:.1} dB per octave  ·  Peak-to-average {:.1} dB",
            tone.tilt_db_per_octave, tone.crest_db
        ))
        .size(13.0)
        .weak(),
    );
}

/// Bars around a 0 dB line: above = louder than average, below = quieter.
fn eq_chart(ui: &mut egui::Ui, tone: &Tone) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 180.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let plot = rect.shrink2(Vec2::new(0.0, 16.0));
    let visuals = ui.visuals();
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
        StrokeKind::Inside,
    );

    let range_db = tone.levels_db.iter().fold(12.0f32, |m, l| m.max(l.abs()));
    let mid_y = plot.center().y;
    let y_for = |db: f32| mid_y - db / range_db * plot.height() / 2.0;
    painter.line_segment(
        [plot.left_center(), plot.right_center()],
        Stroke::new(1.0, visuals.weak_text_color()),
    );

    let slot = plot.width() / tone.levels_db.len() as f32;
    for (i, db) in tone.levels_db.iter().enumerate() {
        let x = plot.left() + slot * (i as f32 + 0.5);
        let (top, bottom) = if *db >= 0.0 {
            (y_for(*db), mid_y)
        } else {
            (mid_y, y_for(*db))
        };
        let bar = egui::Rect::from_x_y_ranges(
            x - slot * 0.35..=x + slot * 0.35,
            top..=bottom.max(top + 1.0),
        );
        painter.rect_filled(bar, 2.0, ACCENT);
    }
    for (hz, label) in [(100.0, "100 Hz"), (1000.0, "1 kHz"), (10_000.0, "10 kHz")] {
        let first = tone.centers[0].log2();
        let last = tone.centers[tone.centers.len() - 1].log2();
        let t = (f32::log2(hz) - first) / (last - first);
        if (0.0..=1.0).contains(&t) {
            let x = plot.left() + slot * 0.5 + t * (plot.width() - slot);
            painter.text(
                egui::pos2(x, rect.bottom() - 2.0),
                Align2::CENTER_BOTTOM,
                label,
                FontId::proportional(11.0),
                visuals.weak_text_color(),
            );
        }
    }
}
