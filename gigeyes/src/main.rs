// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod decode;
mod fetch;

use analysis::Tone;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, ImageSource, Layout, RichText, Sense,
    Stroke, StrokeKind, Vec2,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

const ACCENT: Color32 = Color32::from_rgb(232, 168, 56);
const AUDIO_TYPES: [&str; 9] = [
    "wav", "mp3", "flac", "ogg", "m4a", "mp4", "aac", "mkv", "mov",
];
// Lucide icons, white so they can be tinted to any colour.
const ICON_FILE: ImageSource = egui::include_image!("../icons/file-up.svg");
const ICON_LINK: ImageSource = egui::include_image!("../icons/link.svg");
const ICON_WAVE: ImageSource = egui::include_image!("../icons/wave.svg");
const ICON_ALERT: ImageSource = egui::include_image!("../icons/alert.svg");

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 820.0])
            .with_min_inner_size([480.0, 480.0]),
        ..Default::default()
    };
    // A file passed on the command line (or dropped onto gigeyes.exe in Explorer) opens right away.
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    fetch::clean_old_downloads(&std::env::temp_dir(), std::time::Duration::from_secs(3600));
    eframe::run_native(
        "GigEyes",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx.all_styles_mut(|style| {
                style.spacing.button_padding = Vec2::new(12.0, 6.0);
                style.spacing.interact_size.y = 30.0;
                for (text, size) in [
                    (egui::TextStyle::Body, 15.0),
                    (egui::TextStyle::Button, 15.0),
                ] {
                    if let Some(font) = style.text_styles.get_mut(&text) {
                        font.size = size;
                    }
                }
            });
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
        let busy = matches!(self.state, State::Reading(_));
        let has_result = matches!(self.state, State::Done(..));

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(28.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    header(ui);
                    ui.add_space(20.0);

                    // Before the first result the drop zone leads; afterwards the result does and inputs shrink to a row.
                    if !has_result && drop_zone(ui, hovering) {
                        self.pick_file(&ctx);
                    }
                    ui.add_space(12.0);
                    let (pick, load) = input_row(ui, &mut self.link, has_result, busy);
                    if pick {
                        self.pick_file(&ctx);
                    }
                    if load {
                        self.open_link(&ctx);
                    }
                    ui.add_space(24.0);

                    match &self.state {
                        State::Empty => {}
                        State::Reading(label) => card(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(label);
                            });
                        }),
                        State::Failed(e) => card(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let red = ui.visuals().error_fg_color;
                                ui.add(icon(ICON_ALERT, 20.0, red));
                                ui.label(RichText::new(e).color(red));
                            });
                            ui.label(RichText::new("Try another file or link.").weak());
                        }),
                        State::Done(name, tone) => report(ui, name, tone),
                    }
                });
            });

        if hovering {
            drop_overlay(&ctx);
        }
    }
}

fn icon(source: ImageSource<'static>, size: f32, tint: Color32) -> egui::Image<'static> {
    egui::Image::new(source)
        .fit_to_exact_size(Vec2::splat(size))
        .tint(tint)
}

fn header(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add(icon(ICON_WAVE, 30.0, ACCENT));
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("GigEyes").size(26.0).strong());
            ui.label(RichText::new("Read the tone of a song, stem or video.").weak());
        });
    });
}

/// A rounded panel on the faint background colour, used for every block of content.
fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Plain left-aligned flow: inside ui.columns text would otherwise be stretched to full width.
            ui.with_layout(Layout::top_down(Align::LEFT), add);
        });
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
        .corner_radius(CornerRadius::same(14))
        .inner_margin(32.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add(icon(ICON_FILE, 40.0, ui.visuals().weak_text_color()));
                ui.add_space(10.0);
                ui.label(RichText::new("Drop a song, guitar stem or video here").size(18.0));
                ui.add_space(4.0);
                ui.label(RichText::new(AUDIO_TYPES.join(", ")).size(13.0).weak());
                ui.add_space(16.0);
                let button = egui::Button::new(RichText::new("Choose file…").color(Color32::BLACK))
                    .fill(ACCENT);
                ui.add(button).clicked()
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
                    egui::Button::image_and_text(
                        icon(ICON_FILE, 16.0, ui.visuals().text_color()),
                        "Open file…",
                    ),
                )
                .clicked();
        }
        ui.add(icon(ICON_LINK, 18.0, ui.visuals().weak_text_color()));
        let load_width = 80.0;
        let field = ui.add(
            egui::TextEdit::singleline(link)
                .hint_text("Paste a YouTube or other video link")
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

fn report(ui: &mut egui::Ui, name: &str, tone: &Tone) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).size(20.0).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let secs = tone.seconds.round() as u32;
            ui.label(RichText::new(format!("{}:{:02} analysed", secs / 60, secs % 60)).weak());
        });
    });
    ui.add_space(12.0);

    // Four reading tiles: one row when there's room, two by two when the window is narrow.
    let per_row = if ui.available_width() > 620.0 { 4 } else { 2 };
    for row in tone.summary.chunks(per_row) {
        ui.columns(per_row, |cols| {
            for (col, reading) in cols.iter_mut().zip(row) {
                card(col, |ui| {
                    ui.label(
                        RichText::new(reading.label.to_uppercase())
                            .size(12.0)
                            .weak(),
                    );
                    ui.label(
                        RichText::new(reading.verdict)
                            .size(21.0)
                            .strong()
                            .color(ui.visuals().strong_text_color()),
                    );
                    ui.add_space(2.0);
                    ui.label(RichText::new(&reading.detail).size(13.0).weak());
                });
            }
        });
        ui.add_space(8.0);
    }
    ui.add_space(12.0);

    card(ui, |ui| {
        ui.label(RichText::new("EQ curve").size(16.0).strong());
        ui.label(RichText::new("Above the line is louder than average, below is quieter. Hover a bar for its value.").size(13.0).weak());
        ui.add_space(10.0);
        eq_chart(ui, tone);
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

/// Bars around a 0 dB line, with the main frequency regions marked and a value on hover.
fn eq_chart(ui: &mut egui::Ui, tone: &Tone) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 200.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals().clone();
    let plot = egui::Rect::from_min_max(
        rect.min + Vec2::new(0.0, 20.0),
        rect.max - Vec2::new(0.0, 18.0),
    );
    let range_db = tone.levels_db.iter().fold(12.0f32, |m, l| m.max(l.abs()));
    let mid_y = plot.center().y;
    let y_for = |db: f32| mid_y - db / range_db * plot.height() / 2.0;
    let slot = plot.width() / tone.levels_db.len() as f32;
    let (first, last) = (
        tone.centers[0].log2(),
        tone.centers[tone.centers.len() - 1].log2(),
    );
    let x_for_hz = |hz: f32| {
        plot.left() + slot * 0.5 + (hz.log2() - first) / (last - first) * (plot.width() - slot)
    };

    // Region bands, alternately shaded, labelled along the top.
    let regions = [
        (60.0, 250.0, "LOWS"),
        (250.0, 800.0, "LOW MIDS"),
        (800.0, 2000.0, "MIDS"),
        (2000.0, 5000.0, "HIGHS"),
        (5000.0, 12_000.0, "AIR"),
    ];
    for (i, (lo, hi, label)) in regions.iter().enumerate() {
        let x0 = x_for_hz(*lo).max(plot.left());
        let x1 = x_for_hz(*hi).min(plot.right());
        if x1 <= x0 {
            continue;
        }
        if i % 2 == 0 {
            painter.rect_filled(
                egui::Rect::from_x_y_ranges(x0..=x1, plot.y_range()),
                0.0,
                visuals.extreme_bg_color.gamma_multiply(0.6),
            );
        }
        painter.text(
            egui::pos2((x0 + x1) / 2.0, rect.top() + 2.0),
            Align2::CENTER_TOP,
            *label,
            FontId::proportional(11.0),
            visuals.weak_text_color(),
        );
    }
    painter.line_segment(
        [plot.left_center(), plot.right_center()],
        Stroke::new(1.0, visuals.weak_text_color()),
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
            x - slot * 0.32..=x + slot * 0.32,
            top..=bottom.max(top + 1.0),
        );
        let colour = if hovered == Some(i) {
            visuals.strong_text_color()
        } else {
            ACCENT
        };
        painter.rect_filled(bar, 3.0, colour);
    }
    for (hz, label) in [(100.0, "100 Hz"), (1000.0, "1 kHz"), (10_000.0, "10 kHz")] {
        let x = x_for_hz(hz);
        if plot.x_range().contains(x) {
            painter.text(
                egui::pos2(x, rect.bottom()),
                Align2::CENTER_BOTTOM,
                label,
                FontId::proportional(11.0),
                visuals.weak_text_color(),
            );
        }
    }
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
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
fn drop_overlay(ctx: &egui::Context) {
    let rect = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::NULL.with("drop_overlay"),
    ));
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(150));
    painter.rect_stroke(
        rect.shrink(10.0),
        16.0,
        Stroke::new(2.0, ACCENT),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "Drop to read the tone",
        FontId::proportional(22.0),
        Color32::WHITE,
    );
}
