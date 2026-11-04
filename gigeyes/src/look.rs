//! The GigEyes look, shared by the app and its installer: amp black and tube amber, Barlow, the pick mark.

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, Layout, Pos2, RichText, Sense, Shape, Stroke,
    Vec2,
};
use std::f32::consts::{FRAC_PI_2, TAU};

// Amp black and tube amber: a warm near-black ground, one accent on data and the main action.
pub const GROUND: Color32 = Color32::from_rgb(0x14, 0x13, 0x11);
pub const SURFACE: Color32 = Color32::from_rgb(0x1c, 0x1a, 0x17);
pub const WELL: Color32 = Color32::from_rgb(0x0e, 0x0d, 0x0c);
pub const RAISED: Color32 = Color32::from_rgb(0x2a, 0x27, 0x23);
pub const LINE: Color32 = Color32::from_rgb(0x34, 0x30, 0x2b);
pub const INK: Color32 = Color32::from_rgb(0xef, 0xe9, 0xdf);
pub const MUTED: Color32 = Color32::from_rgb(0xa3, 0x9d, 0x93);
pub const ACCENT: Color32 = Color32::from_rgb(0xe8, 0xa8, 0x38);
pub const ERROR: Color32 = Color32::from_rgb(0xef, 0x7a, 0x6a);
pub const RADIUS: u8 = 8;
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
    v.panel_fill = GROUND;
    v.window_fill = SURFACE;
    v.faint_bg_color = SURFACE;
    v.extreme_bg_color = WELL;
    v.text_edit_bg_color = Some(WELL);
    v.override_text_color = Some(INK);
    v.weak_text_color = Some(MUTED);
    v.error_fg_color = ERROR;
    v.hyperlink_color = ACCENT;
    // Neutral selection (tabs, highlighted text): the accent stays on data and the main action.
    v.selection.bg_fill = Color32::from_rgb(0x3a, 0x36, 0x30);
    v.selection.stroke = Stroke::new(1.0, INK);
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

/// A rounded panel one tone step above the ground, used for every block of content.
pub fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
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
