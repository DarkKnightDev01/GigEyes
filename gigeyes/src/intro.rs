//! The startup intro, so GigEyes doesn't just pop up. The amber pick fades and scales in, its five
//! bars rise one after another like a level meter and "GigEyes" fades in underneath; the name fades
//! away, then the pick glides into its place in the header while the header and the screen below
//! settle in. 2.5 seconds unless Settings says otherwise; a click, a key or a dropped file skips it.
//! It plays even when Windows animations are off: that's how the user asked for it.

use crate::look::{ink, paint_mark};
use eframe::egui::{self, Rect, Vec2};

/// Strong ease-out for things arriving, strong ease-in-out for moving across the screen
/// (Emil Kowalski's UI curves), and easeOutBack (easings.net) for the meter's peak and settle.
const EASE_OUT: [f32; 4] = [0.23, 1.0, 0.32, 1.0];
const EASE_IN_OUT: [f32; 4] = [0.77, 0.0, 0.175, 1.0];
const OVERSHOOT: [f32; 4] = [0.34, 1.56, 0.64, 1.0];

/// The base timeline: (start, length) in seconds from the first frame, with the name leaving as soon
/// as it has arrived. A longer intro holds the name on screen in between (see `at`).
const PICK: (f64, f64) = (0.05, 0.50);
const BARS_FROM: f64 = 0.25;
const BAR_STAGGER: f64 = 0.05;
const BAR_LENGTH: f64 = 0.30;
/// The name arrives as the bars finish and leaves faster than it came.
const NAME_IN: (f64, f64) = (0.65, 0.45);
const NAME_SHOWN: f64 = NAME_IN.0 + NAME_IN.1;
const NAME_OUT: (f64, f64) = (NAME_SHOWN, 0.30);
const GLIDE: (f64, f64) = (1.35, 0.50);
const HEADER: (f64, f64) = (1.50, 0.30);
const BODY: (f64, f64) = (1.60, 0.30);
/// The base timeline's length: the shortest intro that plays at full speed.
const BASE: f64 = 1.90;
/// The pick's size in the middle of the window, before it glides into its 40 px header slot.
const BIG: f32 = 120.0;
/// It starts a little small, never from nothing.
const START_SCALE: f32 = 0.9;

/// Where `elapsed` real seconds into an intro `length` seconds long fall on the base timeline, or
/// None once it's over. Extra length holds the name on screen; a length under BASE plays it all faster.
pub fn at(elapsed: f64, length: f64) -> Option<f64> {
    if elapsed >= length {
        return None;
    }
    let hold = length - BASE;
    Some(if hold < 0.0 {
        elapsed * BASE / length
    } else if elapsed < NAME_SHOWN {
        elapsed
    } else {
        (elapsed - hold).max(NAME_SHOWN)
    })
}

/// The pick has started gliding to the header (`t` on the base timeline).
pub fn gliding(t: f64) -> bool {
    t >= GLIDE.0
}

/// How far the header and the screen below are through settling in, eased, at `elapsed`.
pub fn content(elapsed: f64) -> (f32, f32) {
    (
        step(HEADER, elapsed, EASE_OUT),
        step(BODY, elapsed, EASE_OUT),
    )
}

/// Draw the pick for `elapsed` seconds into the intro, above everything. `home` is the header slot
/// it lands in; at the end it sits exactly there, so the header's own mark takes over unseen.
pub fn logo(ctx: &egui::Context, elapsed: f64, home: Rect) {
    let appear = step(PICK, elapsed, EASE_OUT);
    let glide = step(GLIDE, elapsed, EASE_IN_OUT);
    let window = ctx.content_rect();
    let centre = window.center() - Vec2::new(0.0, window.height() * 0.04);
    let size = egui::lerp(BIG..=home.width(), glide) * egui::lerp(START_SCALE..=1.0, appear);
    let rect = Rect::from_center_size(centre.lerp(home.center(), glide), Vec2::splat(size));
    let bars = std::array::from_fn(|i| {
        let start = BARS_FROM + i as f64 * BAR_STAGGER;
        step((start, BAR_LENGTH), elapsed, OVERSHOOT)
    });
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::NULL.with("intro"));
    let painter = ctx.layer_painter(layer);
    paint_mark(&painter, rect, bars, appear);

    // The name under the pick: fades in rising from 8 px below, then fades out where it stands.
    let shown = step(NAME_IN, elapsed, EASE_OUT) * (1.0 - step(NAME_OUT, elapsed, EASE_OUT));
    if shown > 0.0 {
        let rise = 8.0 * (1.0 - step(NAME_IN, elapsed, EASE_OUT));
        painter.text(
            centre + Vec2::new(0.0, BIG * 0.5 + 18.0 + rise),
            egui::Align2::CENTER_TOP,
            "GigEyes",
            egui::FontId::new(30.0, egui::FontFamily::Name("semibold".into())),
            ink().gamma_multiply(shown),
        );
    }
}

/// Progress through one step of the timeline, eased: 0 before it starts, 1 once it's done.
fn step((start, length): (f64, f64), elapsed: f64, curve: [f32; 4]) -> f32 {
    bezier(curve, ((elapsed - start) / length).clamp(0.0, 1.0) as f32)
}

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing curve at time `t`: find where the curve's x reaches
/// `t` (x only grows, so halving the interval finds it), then return the curve's y there.
fn bezier([x1, y1, x2, y2]: [f32; 4], t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let at = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if at(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(y1, y2, (lo + hi) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_match_their_css_definitions() {
        let close = |a: f32, b: f32| (a - b).abs() < 0.002;
        // linear: cubic-bezier(0, 0, 1, 1) is y = x.
        assert!(close(bezier([0.0, 0.0, 1.0, 1.0], 0.3), 0.3));
        // CSS `ease-in-out` (0.42, 0, 0.58, 1) is symmetric: halfway in time is halfway there.
        assert!(close(bezier([0.42, 0.0, 0.58, 1.0], 0.5), 0.5));
        // The strong ease-out is most of the way there early on, and ends exactly at 1.
        assert!(bezier(EASE_OUT, 0.3) > 0.8 && bezier(EASE_OUT, 1.0) == 1.0);
        // easeOutBack overshoots, then lands.
        assert!((0..=100).any(|i| bezier(OVERSHOOT, i as f32 / 100.0) > 1.05));
    }

    #[test]
    fn the_timeline_starts_empty_and_ends_settled() {
        assert_eq!(content(0.0), (0.0, 0.0));
        assert_eq!(content(BASE), (1.0, 1.0));
        assert_eq!(step(PICK, 0.0, EASE_OUT), 0.0);
        // Everything has landed by the end of the base timeline, the last bar included.
        let last_bar = BARS_FROM + 4.0 * BAR_STAGGER + BAR_LENGTH;
        for done in [
            PICK.0 + PICK.1,
            GLIDE.0 + GLIDE.1,
            last_bar,
            BODY.0 + BODY.1,
        ] {
            assert!(done <= BASE + 1e-9, "{done} ends after BASE");
        }
        // The name is fully in before it starts to leave, and nearly gone before the pick moves off.
        assert!(NAME_IN.0 + NAME_IN.1 <= NAME_OUT.0);
        assert!(step(NAME_OUT, GLIDE.0, EASE_OUT) > 0.9);
    }

    #[test]
    fn the_length_setting_holds_the_name_or_speeds_everything_up() {
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        // 2.5 s: on the base timeline until the name is in, then 0.6 s held on it, then the rest.
        assert!(close(at(0.5, 2.5), 0.5));
        assert!(close(at(1.5, 2.5), NAME_SHOWN), "holding the name");
        assert!(close(at(2.0, 2.5), 1.4));
        assert_eq!(at(2.5, 2.5), None, "over at exactly its length");
        // 1.5 s is shorter than the base timeline: everything plays faster, in the same order.
        assert!(close(at(0.75, 1.5), 0.95));
        assert_eq!(at(1.5, 1.5), None);
        // The glide always starts so the title bar comes back while the app settles in.
        assert!(at(2.49, 2.5).is_some_and(gliding) && !at(1.0, 2.5).is_some_and(gliding));
    }
}
