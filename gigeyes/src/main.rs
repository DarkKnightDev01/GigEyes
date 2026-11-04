// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod decode;
mod fetch;

use analysis::{Knob, Tone};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, ImageSource, Layout, Pos2,
    RichText, Sense, Shape, Stroke, StrokeKind, Vec2,
};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

// Amp black and tube amber: a warm near-black ground, one accent on data and the main action.
const GROUND: Color32 = Color32::from_rgb(0x14, 0x13, 0x11);
const SURFACE: Color32 = Color32::from_rgb(0x1c, 0x1a, 0x17);
const WELL: Color32 = Color32::from_rgb(0x0e, 0x0d, 0x0c);
const RAISED: Color32 = Color32::from_rgb(0x2a, 0x27, 0x23);
const LINE: Color32 = Color32::from_rgb(0x34, 0x30, 0x2b);
const INK: Color32 = Color32::from_rgb(0xef, 0xe9, 0xdf);
const MUTED: Color32 = Color32::from_rgb(0xa3, 0x9d, 0x93);
const ACCENT: Color32 = Color32::from_rgb(0xe8, 0xa8, 0x38);
const ERROR: Color32 = Color32::from_rgb(0xef, 0x7a, 0x6a);
const RADIUS: u8 = 8;
/// How long the knobs take to turn to a new result.
const REVEAL_SECS: f64 = 0.8;

const AUDIO_TYPES: [&str; 9] = [
    "wav", "mp3", "flac", "ogg", "m4a", "mp4", "aac", "mkv", "mov",
];
// Lucide icons, white so they can be tinted to any colour.
const ICON_FILE: ImageSource = egui::include_image!("../icons/file-up.svg");
const ICON_LINK: ImageSource = egui::include_image!("../icons/link.svg");
const ICON_ALERT: ImageSource = egui::include_image!("../icons/alert.svg");

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([780.0, 860.0])
            .with_min_inner_size([520.0, 520.0]),
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
            cc.egui_ctx.set_fonts(fonts());
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            cc.egui_ctx.style_mut_of(egui::Theme::Dark, theme);
            let mut app = App::default();
            if let Some(path) = initial {
                app.open(path, &cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
}

/// Barlow (SIL Open Font License, see fonts/OFL.txt), with egui's own fonts behind it for missing glyphs.
fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        ("barlow", &include_bytes!("../fonts/Barlow-Regular.ttf")[..]),
        (
            "barlow-semibold",
            &include_bytes!("../fonts/Barlow-SemiBold.ttf")[..],
        ),
    ] {
        fonts.font_data.insert(
            name.into(),
            std::sync::Arc::new(egui::FontData::from_static(bytes)),
        );
    }
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "barlow".into());
    fonts.families.insert(
        FontFamily::Name("semibold".into()),
        [vec!["barlow-semibold".into()], fallback].concat(),
    );
    fonts
}

fn theme(style: &mut egui::Style) {
    let v = &mut style.visuals;
    v.panel_fill = GROUND;
    v.window_fill = SURFACE;
    v.faint_bg_color = SURFACE;
    v.extreme_bg_color = WELL;
    v.text_edit_bg_color = Some(WELL);
    v.override_text_color = Some(INK);
    v.weak_text_color = Some(MUTED);
    v.error_fg_color = ERROR;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.widgets.noninteractive.bg_stroke.color = LINE;
    for (w, fill) in [
        (&mut v.widgets.inactive, RAISED),
        (&mut v.widgets.hovered, Color32::from_rgb(0x36, 0x32, 0x2d)),
        (&mut v.widgets.active, Color32::from_rgb(0x42, 0x3d, 0x37)),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
        w.corner_radius = CornerRadius::same(6);
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, LINE);
    // A solid scrollbar takes its own lane instead of floating over the cards' right edge.
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    style.spacing.button_padding = Vec2::new(14.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    for (text, size) in [
        (egui::TextStyle::Body, 15.0),
        (egui::TextStyle::Button, 15.0),
        (egui::TextStyle::Small, 13.0),
    ] {
        if let Some(font) = style.text_styles.get_mut(&text) {
            font.size = size;
        }
    }
}

fn semibold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text)
        .size(size)
        .family(FontFamily::Name("semibold".into()))
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
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if let Some(rx) = &self.result
            && let Ok(result) = rx.try_recv()
        {
            self.state = match result {
                Ok((name, tone)) => State::Done(name, tone),
                Err(e) => State::Failed(e),
            };
            self.result = None;
            self.revealed_at = now;
        }
        if let Some(path) =
            ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()))
        {
            self.open(path, &ctx);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let busy = matches!(self.state, State::Reading(_));
        let has_result = matches!(self.state, State::Done(..));
        let reveal = ((now - self.revealed_at) / REVEAL_SECS).clamp(0.0, 1.0) as f32;
        if has_result && reveal < 1.0 {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().frame(egui::Frame::central_panel(ui.style()).inner_margin(32.0)).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                header(ui);
                ui.add_space(24.0);

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
                if !has_result {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Works best on an isolated guitar track. On a full mix, bass, drums and vocals colour the result.")
                            .size(13.0)
                            .weak(),
                    );
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
                            ui.add(icon(ICON_ALERT, 20.0, ERROR));
                            ui.label(e);
                        });
                        ui.label(RichText::new("Try another file or link.").weak());
                    }),
                    State::Done(name, tone) => report(ui, name, tone, ease(reveal)),
                }
            });
        });

        if hovering {
            drop_overlay(&ctx);
        }
    }
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

/// The GigEyes mark: a guitar pick with a waveform cut into it.
fn mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter_at(rect);
    let (c, r) = (rect.center() - Vec2::new(0.0, size * 0.04), size * 0.42);
    // r(θ) = 1 + k·cos 3θ stays convex for k < 1/8; the point faces down like a pick.
    let outline = (0..60)
        .map(|i| {
            let a = i as f32 / 60.0 * TAU;
            let rr = r * (1.0 + 0.12 * (3.0 * (a - FRAC_PI_2)).cos());
            c + Vec2::new(rr * a.cos(), rr * a.sin())
        })
        .collect();
    painter.add(Shape::convex_polygon(outline, ACCENT, Stroke::NONE));
    for (i, h) in [0.30f32, 0.62, 0.92, 0.55, 0.28].iter().enumerate() {
        let x = c.x + (i as f32 - 2.0) * r * 0.26;
        let half = h * r * 0.42;
        let y = c.y - r * 0.05;
        painter.line_segment(
            [Pos2::new(x, y - half), Pos2::new(x, y + half)],
            Stroke::new(r * 0.13, GROUND),
        );
    }
}

fn header(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        mark(ui, 40.0);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(semibold("GigEyes", 24.0));
            ui.label(RichText::new("Hear a tone. See where the knobs sit.").weak());
        });
    });
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
            Stroke::new(3.0, ACCENT),
        );
    }
    ui.ctx().request_repaint();
}

/// A rounded panel one tone step above the ground, used for every block of content.
fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(18.0)
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
        Stroke::new(1.0, LINE)
    };
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(36.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add(icon(ICON_FILE, 38.0, MUTED));
                ui.add_space(12.0);
                ui.label(semibold("Drop a song, a guitar stem or a live video", 19.0));
                ui.add_space(4.0);
                ui.label(RichText::new(AUDIO_TYPES.join(", ")).size(13.0).weak());
                ui.add_space(18.0);
                let button =
                    egui::Button::new(semibold("Choose file…", 15.0).color(GROUND)).fill(ACCENT);
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
                    egui::Button::image_and_text(icon(ICON_FILE, 16.0, INK), "Open file…"),
                )
                .clicked();
            ui.add_space(6.0);
        }
        ui.add(icon(ICON_LINK, 18.0, MUTED));
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
            verdict: "Waiting",
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
                MUTED,
            );
        }
        let value = if live {
            knob.value * reveal
        } else {
            knob.value
        };
        let (arc_colour, pointer) = if live { (ACCENT, INK) } else { (LINE, MUTED) };
        painter.add(Shape::line(
            arc(angle_at(0.0), angle_at(10.0), r),
            Stroke::new(3.0, RAISED),
        ));
        painter.add(Shape::line(
            arc(angle_at(0.0), angle_at(value), r),
            Stroke::new(3.0, arc_colour),
        ));
        painter.circle(c, r - 7.0, RAISED, Stroke::new(1.0, LINE));
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

    painter.rect_filled(rect, CornerRadius::same(RADIUS - 2), WELL);
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
                SURFACE.gamma_multiply(0.7),
            );
        }
        painter.text(
            egui::pos2((x0 + x1) / 2.0, rect.top() + 4.0),
            Align2::CENTER_TOP,
            *label,
            FontId::proportional(12.0),
            MUTED,
        );
    }
    painter.line_segment(
        [plot.left_center(), plot.right_center()],
        Stroke::new(1.0, LINE),
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
        let colour = if hovered == Some(i) { INK } else { ACCENT };
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
                MUTED,
            );
        }
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(RADIUS - 2),
        Stroke::new(1.0, LINE),
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
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(170));
    painter.rect_stroke(
        rect.shrink(12.0),
        CornerRadius::same(RADIUS * 2),
        Stroke::new(2.0, ACCENT),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "Drop it. GigEyes is listening.",
        FontId::new(24.0, FontFamily::Name("semibold".into())),
        INK,
    );
}
