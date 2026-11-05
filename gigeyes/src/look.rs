//! The GigEyes look, shared by the app and its installer: amp black and tube amber, Barlow, the pick mark.

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, Layout, Pos2, RichText, Sense, Shape, Stroke,
    Vec2,
};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::atomic::{AtomicBool, Ordering};

// Amp black and tube amber, or warm paper for light mode: one accent on data and the main action.
static LIGHT: AtomicBool = AtomicBool::new(false);

/// Switch every colour below to light mode (or back). Call `theme` again afterwards.
// The setup is always dark; only the app switches.
#[allow(dead_code)]
pub fn set_light(on: bool) {
    LIGHT.store(on, Ordering::Relaxed);
}

fn pick(dark: [u8; 3], light: [u8; 3]) -> Color32 {
    let [r, g, b] = if LIGHT.load(Ordering::Relaxed) {
        light
    } else {
        dark
    };
    Color32::from_rgb(r, g, b)
}

pub fn ground() -> Color32 {
    pick([0x14, 0x13, 0x11], [0xf3, 0xef, 0xe7])
}
pub fn surface() -> Color32 {
    pick([0x1c, 0x1a, 0x17], [0xfb, 0xf9, 0xf4])
}
pub fn well() -> Color32 {
    pick([0x0e, 0x0d, 0x0c], [0xe9, 0xe4, 0xda])
}
pub fn raised() -> Color32 {
    pick([0x2a, 0x27, 0x23], [0xe6, 0xe0, 0xd5])
}
pub fn line() -> Color32 {
    pick([0x34, 0x30, 0x2b], [0xd3, 0xcb, 0xbe])
}
pub fn ink() -> Color32 {
    pick([0xef, 0xe9, 0xdf], [0x1f, 0x1c, 0x18])
}
pub fn muted() -> Color32 {
    pick([0xa3, 0x9d, 0x93], [0x66, 0x5f, 0x55])
}
/// Deeper amber on light paper, so bars and arcs keep their contrast.
pub fn accent() -> Color32 {
    pick([0xe8, 0xa8, 0x38], [0xc2, 0x7c, 0x0e])
}
pub fn error() -> Color32 {
    pick([0xef, 0x7a, 0x6a], [0xb0, 0x3a, 0x2a])
}
/// Text and marks drawn on an accent fill: dark in both modes.
pub const ON_ACCENT: Color32 = Color32::from_rgb(0x14, 0x13, 0x11);
pub const RADIUS: u8 = 8;
/// The version people see. Four parts, Windows style; Cargo's own version only allows three.
pub const VERSION: &str = "0.4.0.1";
/// Barlow (SIL Open Font License, see fonts/OFL.txt), with egui's own fonts behind it for missing glyphs.
pub fn fonts() -> egui::FontDefinitions {
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

pub fn theme(style: &mut egui::Style) {
    let v = &mut style.visuals;
    v.panel_fill = ground();
    v.window_fill = surface();
    v.faint_bg_color = surface();
    v.extreme_bg_color = well();
    v.text_edit_bg_color = Some(well());
    v.override_text_color = Some(ink());
    v.weak_text_color = Some(muted());
    v.error_fg_color = error();
    v.hyperlink_color = pick([0xe8, 0xa8, 0x38], [0x8a, 0x5a, 0x06]);
    // Neutral selection (tabs, highlighted text): the accent stays on data and the main action.
    v.selection.bg_fill = pick([0x3a, 0x36, 0x30], [0xdd, 0xd5, 0xc8]);
    v.selection.stroke = Stroke::new(1.0, ink());
    v.widgets.noninteractive.bg_stroke.color = line();
    for (w, fill) in [
        (&mut v.widgets.inactive, raised()),
        (
            &mut v.widgets.hovered,
            pick([0x36, 0x32, 0x2d], [0xdc, 0xd5, 0xc9]),
        ),
        (
            &mut v.widgets.active,
            pick([0x42, 0x3d, 0x37], [0xd0, 0xc8, 0xba]),
        ),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
        w.corner_radius = CornerRadius::same(6);
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, line());
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

pub fn semibold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text)
        .size(size)
        .family(FontFamily::Name("semibold".into()))
}

/// The GigEyes mark: a guitar pick with a waveform cut into it.
pub fn mark(ui: &mut egui::Ui, size: f32) {
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
    painter.add(Shape::convex_polygon(outline, accent(), Stroke::NONE));
    for (i, h) in [0.30f32, 0.62, 0.92, 0.55, 0.28].iter().enumerate() {
        let x = c.x + (i as f32 - 2.0) * r * 0.26;
        let half = h * r * 0.42;
        let y = c.y - r * 0.05;
        painter.line_segment(
            [Pos2::new(x, y - half), Pos2::new(x, y + half)],
            Stroke::new(r * 0.13, ground()),
        );
    }
}

/// A checkbox row that reads at a glance: amber when ticked, outlined when not. The whole row toggles it,
/// by mouse or by Space on keyboard focus. Returns true when it was toggled.
pub fn tick(ui: &mut egui::Ui, on: &mut bool, title: RichText, detail: Option<RichText>) -> bool {
    let name = title.text().to_string();
    let backdrop = ui.painter().add(Shape::Noop);
    let row = ui.horizontal_top(|ui| {
        // Plain labels, so a click on the name reaches the row instead of starting a text selection.
        ui.style_mut().interaction.selectable_labels = false;
        let (mark, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(title);
            if let Some(detail) = detail {
                ui.label(detail);
            }
        });
        mark
    });
    let rect = row.response.rect;
    let area = egui::Rect::from_min_max(rect.min, Pos2::new(ui.max_rect().right(), rect.max.y))
        .expand2(Vec2::new(8.0, 4.0));
    let mut response = ui.interact(area, row.response.id.with("tick"), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, *on, &name));

    let corner = CornerRadius::same(6);
    if response.hovered() || response.has_focus() {
        ui.painter().set(
            backdrop,
            Shape::rect_filled(area, corner, raised().gamma_multiply(0.7)),
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            area,
            corner,
            Stroke::new(1.0, muted()),
            egui::StrokeKind::Inside,
        );
    }
    let b = egui::Rect::from_center_size(row.inner.center(), Vec2::splat(16.0));
    if *on {
        ui.painter().rect_filled(b, CornerRadius::same(4), accent());
        let check = vec![
            Pos2::new(b.left() + 3.5, b.center().y),
            Pos2::new(b.left() + 6.5, b.bottom() - 4.0),
            Pos2::new(b.right() - 3.5, b.top() + 4.0),
        ];
        ui.painter()
            .add(Shape::line(check, Stroke::new(2.0, ON_ACCENT)));
    } else {
        let edge = if response.hovered() { ink() } else { muted() };
        ui.painter().rect(
            b,
            CornerRadius::same(4),
            well(),
            Stroke::new(1.5, edge),
            egui::StrokeKind::Inside,
        );
    }
    response.changed()
}

/// A rounded panel one tone step above the ground, used for every block of content.
pub fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(surface())
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(18.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Plain left-aligned flow: inside ui.columns text would otherwise be stretched to full width.
            ui.with_layout(Layout::top_down(Align::LEFT), add);
        });
}
