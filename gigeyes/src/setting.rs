//! What to set each knob of a block's plugin to. The knob names come from the plugin itself, or for
//! one model inside a suite, from its manual on the web. Each value comes from a source that names
//! it, else from the recording where it can tell, else the knob stays at its default.

use crate::analysis::Tone;
use crate::chain::{Pick, Stage};
use crate::knobs::Knob;
use crate::knowledge::{Knowledge, ModelKnobs};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum From {
    Source,
    Recording,
    Default,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Setting {
    pub knob: String,
    /// What to turn it to, as the plugin shows it where GigEyes knows the scale; empty for a
    /// default nobody knows.
    pub value: String,
    pub from: From,
    /// Where the value comes from, in a sentence.
    pub why: String,
}

/// Where a block's knob names are, or what has to happen first.
pub enum Names<'a> {
    Plugin(Vec<&'a Knob>),
    Page(&'a ModelKnobs),
    /// The plugin's knobs haven't been read yet.
    Read,
    /// The plugin's knobs don't show this model's: its manual has to be looked up.
    Web,
}

/// Words in a plugin's section or knob names that mark the part of it for a stage.
fn section_words(stage: Stage) -> &'static [&'static str] {
    match stage {
        Stage::Amp => &["amp", "amplifier", "preamp", "head"],
        Stage::Cabinet => &["cab", "cabinet", "ir", "speaker", "mic"],
        Stage::Delay => &["delay", "echo"],
        Stage::Reverb => &["reverb", "verb", "room", "hall", "plate"],
        Stage::Modulation => &[
            "chorus",
            "flanger",
            "phaser",
            "mod",
            "modulation",
            "tremolo",
            "vibe",
        ],
        Stage::Overdrive | Stage::Distortion | Stage::Fuzz | Stage::Boost => &[
            "drive",
            "od",
            "overdrive",
            "dist",
            "distortion",
            "fuzz",
            "boost",
            "pedal",
            "stomp",
        ],
        Stage::Compressor => &["comp", "compressor"],
        Stage::Gate => &["gate"],
        Stage::Eq => &["eq"],
        Stage::Wah => &["wah"],
        Stage::Pitch => &["pitch", "octave", "whammy"],
        Stage::Other => &[],
    }
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Automation slots and other names that aren't a knob on the unit.
fn generic(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower
        .chars()
        .all(|c| c.is_ascii_digit() || c.is_whitespace())
        || [
            "midi cc",
            "cc ",
            "param",
            "macro",
            "automation",
            "unused",
            "reserved",
            "bypass",
            "preset",
            "program",
            "assign",
        ]
        .iter()
        .any(|g| lower.starts_with(g))
}

/// Only the knobs of the plugin's section for `stage`, when its sections or knob names show it.
fn in_section(knobs: Vec<&Knob>, stage: Stage) -> Vec<&Knob> {
    let marks = section_words(stage);
    let marked = |s: &str| words(s).iter().any(|w| marks.contains(&w.as_str()));
    let by_group: Vec<&Knob> = knobs.iter().copied().filter(|k| marked(&k.group)).collect();
    if !by_group.is_empty() {
        return by_group;
    }
    let by_name: Vec<&Knob> = knobs.iter().copied().filter(|k| marked(&k.name)).collect();
    if by_name.len() >= 2 { by_name } else { knobs }
}

/// The knob names for `pick` in a `stage` block.
pub fn names<'a>(stage: Stage, pick: &Pick, knowledge: &'a Knowledge) -> Names<'a> {
    let Some(read) = knowledge.knobs.get(&pick.plugin) else {
        return Names::Read;
    };
    let useful: Vec<&Knob> = read.iter().filter(|k| !generic(&k.name)).collect();
    let models = knowledge
        .about(&pick.plugin)
        .map_or(0, |k| k.gear().count());
    if pick.capture || pick.model.is_empty() || models <= 1 {
        return Names::Plugin(in_section(useful, stage));
    }
    // A suite: the model's own section, if the plugin names it.
    let model = words(&pick.model).join(" ");
    let own: Vec<&Knob> = useful
        .iter()
        .copied()
        .filter(|k| {
            words(&format!("{} {}", k.group, k.name))
                .join(" ")
                .contains(&model)
        })
        .collect();
    if own.len() >= 2 {
        return Names::Plugin(own);
    }
    match knowledge.model_knobs(&pick.plugin, &pick.model) {
        Some(found) if !found.knobs.is_empty() => Names::Page(found),
        Some(_) => Names::Plugin(vec![]),
        None => Names::Web,
    }
}

/// What a knob does, so "Drive" on one plugin and "Gain" on another are the same knob.
fn role(name: &str) -> Option<&'static str> {
    const ROLES: [(&str, &[&str]); 15] = [
        (
            "gain",
            &[
                "gain",
                "drive",
                "overdrive",
                "pre-amp",
                "preamp",
                "distortion",
                "dist",
                "fuzz",
                "sustain",
            ],
        ),
        ("bass", &["bass", "low", "lows"]),
        ("middle", &["middle", "mid", "mids"]),
        ("treble", &["treble", "high", "highs"]),
        ("presence", &["presence"]),
        ("master", &["master", "volume", "vol"]),
        ("level", &["level", "output"]),
        ("tone", &["tone"]),
        ("time", &["time"]),
        (
            "feedback",
            &["feedback", "repeats", "regen", "regeneration"],
        ),
        ("mix", &["mix", "blend", "wet"]),
        ("rate", &["rate", "speed"]),
        ("depth", &["depth", "intensity"]),
        ("decay", &["decay"]),
        ("threshold", &["threshold", "thresh", "sensitivity"]),
    ];
    let w = words(name);
    // "Low Cut" and "Mid Freq" set a frequency, not the bass or the mids.
    if w.iter().any(|w| {
        [
            "cut",
            "freq",
            "frequency",
            "hz",
            "q",
            "mode",
            "type",
            "sync",
            "switch",
        ]
        .contains(&w.as_str())
    }) {
        return None;
    }
    let joined = w.join(" ");
    ROLES.iter().find_map(|(role, names)| {
        names
            .iter()
            .any(|n| w.iter().any(|x| x == n) || (n.contains('-') && joined.contains(n)))
            .then_some(*role)
    })
}

/// A setting a source gives: "Gain 7", "bass on 4", "treble at 2 o'clock", "presence maxed".
struct Said {
    label: String,
    /// On a 0 to 10 scale, when it reads as a knob position.
    value: Option<f32>,
    text: String,
}

fn said(settings: &str) -> Vec<Said> {
    settings
        .replace(" and ", ",")
        .replace(". ", ",")
        .split([',', ';', '\n'])
        .map(|s| s.trim().trim_end_matches('.'))
        .filter(|s| !s.is_empty())
        .map(|text| {
            let lower = text.to_lowercase();
            let number = lower
                .split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .find_map(|n| n.trim_matches('.').parse::<f32>().ok());
            let clock = lower.contains("o'clock") || lower.contains("oclock");
            let ws = words(&lower);
            let has = |w: &str| {
                if w.contains(' ') {
                    lower.contains(w)
                } else {
                    ws.iter().any(|x| x == w)
                }
            };
            let worded = [
                (&["noon", "half", "halfway"][..], 5.0),
                (
                    &["max", "maxed", "full", "dimed", "cranked", "all the way up"][..],
                    10.0,
                ),
                (&["off", "zero", "min", "all the way down"][..], 0.0),
            ]
            .iter()
            .find(|(names, _)| names.iter().any(|w| has(w)))
            .map(|(_, v)| *v);
            // "400ms" or "400 ms": a number with a unit isn't a knob position.
            let unit = lower.contains('%')
                || ws.iter().any(|w| {
                    let u = w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
                    ["ms", "db", "hz", "khz", "bpm"].contains(&u)
                });
            let value = match number {
                // 7 o'clock is all the way down, noon is half, 5 o'clock is all the way up.
                Some(h) if clock && (1.0..=12.0).contains(&h) => Some(((h as i32 + 5) % 12) as f32),
                Some(_) if unit => None,
                Some(n) if n <= 10.0 => Some(n),
                Some(_) => None,
                None => worded,
            };
            let label = lower
                .split_whitespace()
                .filter(|w| !w.chars().any(|c| c.is_ascii_digit()))
                .collect::<Vec<_>>()
                .join(" ");
            Said {
                label,
                value,
                text: text.to_string(),
            }
        })
        .collect()
}

/// A 0 to 10 position as the knob shows it: its own numbers where its range is a plain number.
fn on_knob(position: f32, knob: Option<&Knob>) -> String {
    let plain = format!("{} of 10", round(position));
    let Some(knob) = knob else {
        return plain;
    };
    if knob.steps > 0 {
        return plain;
    }
    let (Ok(low), Ok(high)) = (knob.low.parse::<f32>(), knob.high.parse::<f32>()) else {
        return plain;
    };
    if low == 0.0 && high == 10.0 {
        return round(position);
    }
    let at = low + position / 10.0 * (high - low);
    let units = if knob.units.is_empty() {
        String::new()
    } else {
        format!(" {}", knob.units)
    };
    format!("{}{units} ({plain})", round(at))
}

fn round(n: f32) -> String {
    let r = if n.abs() >= 10.0 {
        n.round()
    } else {
        (n * 10.0).round() / 10.0
    };
    if r == 0.0 { "0".into() } else { r.to_string() }
}

/// A delay time for the song's tempo, as the knob shows it.
fn delay_time(bpm: f32, knob: Option<&Knob>) -> (String, String) {
    let quarter = 60_000.0 / bpm;
    let why = format!(
        "A quarter note at {bpm:.0} BPM, the recording's tempo. A dotted eighth ({:.0} ms) is the other common choice: go by ear.",
        quarter * 0.75
    );
    let synced = knob.is_some_and(|k| k.steps > 0 && (k.low.contains('/') || k.high.contains('/')));
    let value = if synced {
        "1/4".to_string()
    } else if knob.is_some_and(|k| k.units == "s" || k.high.ends_with(" s")) {
        format!("{:.2} s", quarter / 1000.0)
    } else {
        format!("{quarter:.0} ms")
    };
    (value, why)
}

/// Every knob in `names`, with what to set it to for a `stage` block. `source` is the settings
/// text of the gear the block stands for. Knobs to change come first.
pub fn dial(
    stage: Stage,
    knobs: &[(String, Option<&Knob>)],
    source: &str,
    tone: &Tone,
) -> Vec<Setting> {
    let said = said(source);
    let mut out: Vec<Setting> = knobs
        .iter()
        .map(|(name, knob)| {
            let role = role(name);
            let lower = name.to_lowercase();
            let from_source = said.iter().find(|s| {
                (role.is_some() && role == role_of_label(&s.label)) || s.label.contains(&lower)
            });
            if let Some(s) = from_source {
                let value = s
                    .value
                    .map_or_else(|| s.text.clone(), |v| on_knob(v, *knob));
                return Setting {
                    knob: name.clone(),
                    value,
                    from: From::Source,
                    why: format!("The source: \u{201C}{}\u{201D}", s.text),
                };
            }
            let measured = match (stage, role) {
                (Stage::Amp, Some(r @ ("gain" | "bass" | "middle" | "treble" | "presence"))) => {
                    tone.knobs
                        .iter()
                        .find(|k| k.label.eq_ignore_ascii_case(r))
                        .map(|k| {
                            (
                                on_knob(k.value, *knob),
                                format!("From the recording: {}", k.detail),
                            )
                        })
                }
                (Stage::Delay, Some("time")) => tone.tempo_bpm.map(|bpm| delay_time(bpm, *knob)),
                _ => None,
            };
            if let Some((value, why)) = measured {
                return Setting {
                    knob: name.clone(),
                    value,
                    from: From::Recording,
                    why,
                };
            }
            let value = knob.map_or(String::new(), |k| {
                if k.default.is_empty() || k.units.is_empty() || k.default.parse::<f32>().is_err() {
                    k.default.clone()
                } else {
                    format!("{} {}", k.default, k.units)
                }
            });
            Setting {
                knob: name.clone(),
                value,
                from: From::Default,
                why: String::new(),
            }
        })
        .collect();
    out.sort_by_key(|s| s.from == From::Default);
    out
}

/// When a plugin's own knob names can't be found: the knobs the source gives settings for, plus
/// the ones every amp (or delay) has, under their usual names.
pub fn usual(stage: Stage, source: &str) -> Vec<String> {
    const FILLER: [&str; 22] = [
        "at", "on", "to", "of", "the", "set", "around", "about", "o'clock", "oclock", "is",
        "turned", "up", "down", "all", "way", "maxed", "max", "full", "off", "noon", "halfway",
    ];
    let mut names: Vec<String> = vec![];
    let mut add = |name: &str| {
        let mut letters = name.chars();
        let Some(first) = letters.next() else {
            return;
        };
        let name: String = first.to_uppercase().chain(letters).collect();
        if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            names.push(name);
        }
    };
    for s in said(source) {
        let label = s
            .label
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\''))
            .filter(|w| !FILLER.contains(w))
            .collect::<Vec<_>>()
            .join(" ");
        add(role_of_label(&label).unwrap_or(&label));
    }
    let always: &[&str] = match stage {
        Stage::Amp => &["gain", "bass", "middle", "treble", "presence"],
        Stage::Delay => &["time"],
        _ => &[],
    };
    always.iter().for_each(|a| add(a));
    names
}

/// The role a source's label names: "gain on" → gain.
fn role_of_label(label: &str) -> Option<&'static str> {
    words(label).iter().find_map(|w| role(w))
}

/// The knobs of `names` as `dial` takes them.
pub fn list<'a>(names: &Names<'a>) -> Vec<(String, Option<&'a Knob>)> {
    match names {
        Names::Plugin(knobs) => knobs.iter().map(|k| (k.name.clone(), Some(*k))).collect(),
        Names::Page(found) => found.knobs.iter().map(|n| (n.clone(), None)).collect(),
        Names::Read | Names::Web => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::Knob as Measured;

    fn knob(name: &str, low: &str, high: &str, default: &str, units: &str) -> Knob {
        Knob {
            name: name.into(),
            group: String::new(),
            default: default.into(),
            low: low.into(),
            high: high.into(),
            units: units.into(),
            steps: 0,
        }
    }

    fn tone() -> Tone {
        let m = |label, value| Measured {
            label,
            value,
            verdict: "",
            detail: "measured".into(),
        };
        Tone {
            centers: vec![],
            levels_db: vec![],
            tilt_db_per_octave: -5.0,
            crest_db: 8.0,
            seconds: 30.0,
            verdict: String::new(),
            knobs: vec![
                m("Gain", 8.0),
                m("Bass", 6.0),
                m("Middle", 4.0),
                m("Treble", 6.0),
                m("Presence", 5.0),
            ],
            tempo_bpm: Some(120.0),
        }
    }

    fn by<'a>(set: &'a [Setting], knob: &str) -> &'a Setting {
        set.iter().find(|s| s.knob == knob).unwrap()
    }

    #[test]
    fn sources_beat_the_recording_and_the_rest_stays_at_default() {
        let knobs = [
            knob("Gain", "0", "10", "5", ""),
            knob("Bass", "0", "10", "5", ""),
            knob("Middle", "0", "10", "5", ""),
            knob("Master", "0", "10", "5", ""),
            knob("Low Cut", "20", "200", "80", "Hz"),
        ];
        let list: Vec<(String, Option<&Knob>)> =
            knobs.iter().map(|k| (k.name.clone(), Some(k))).collect();
        let set = dial(Stage::Amp, &list, "Gain at 2 o'clock, bass on 4", &tone());
        assert_eq!(
            (by(&set, "Gain").value.as_str(), by(&set, "Gain").from),
            ("7", From::Source)
        );
        assert_eq!(
            (by(&set, "Bass").value.as_str(), by(&set, "Bass").from),
            ("4", From::Source)
        );
        assert_eq!(
            (by(&set, "Middle").value.as_str(), by(&set, "Middle").from),
            ("4", From::Recording)
        );
        assert_eq!(by(&set, "Master").from, From::Default);
        // "Low Cut" isn't the bass knob.
        assert_eq!(
            (by(&set, "Low Cut").value.as_str(), by(&set, "Low Cut").from),
            ("80 Hz", From::Default)
        );
        assert_eq!(set.last().unwrap().from, From::Default);
    }

    #[test]
    fn without_knob_names_the_sources_own_and_the_usual_ones_are_listed() {
        let names = usual(Stage::Amp, "Mids 7, Bass: 6, Gain at 2 o'clock, Reverb 3");
        assert_eq!(
            names,
            ["Middle", "Bass", "Gain", "Reverb", "Treble", "Presence"]
        );
        let list: Vec<(String, Option<&Knob>)> = names.into_iter().map(|n| (n, None)).collect();
        let set = dial(
            Stage::Amp,
            &list,
            "Mids 7, Bass: 6, Gain at 2 o'clock, Reverb 3",
            &tone(),
        );
        assert_eq!(by(&set, "Middle").value, "7 of 10");
        assert_eq!(by(&set, "Reverb").value, "3 of 10");
        assert_eq!(by(&set, "Gain").value, "7 of 10");
        assert_eq!(by(&set, "Treble").from, From::Recording);
        assert_eq!(usual(Stage::Delay, ""), ["Time"]);
        assert!(usual(Stage::Modulation, "").is_empty());
    }

    #[test]
    fn sources_are_read_in_players_words() {
        let got: Vec<(String, Option<f32>)> =
            said("Gain at 2 o'clock, treble maxed and delay 400ms; mids on 6.5")
                .into_iter()
                .map(|s| (s.label, s.value))
                .collect();
        assert_eq!(got[0], ("gain at o'clock".into(), Some(7.0)));
        assert_eq!(got[1].1, Some(10.0));
        assert_eq!(got[2].1, None);
        assert_eq!(got[3].1, Some(6.5));
    }

    #[test]
    fn positions_use_the_knobs_own_scale() {
        let percent = knob("Drive", "0", "100", "50", "%");
        assert_eq!(on_knob(7.0, Some(&percent)), "70 % (7 of 10)");
        let db = knob("Treble", "-12", "12", "0", "dB");
        assert_eq!(on_knob(5.0, Some(&db)), "0 dB (5 of 10)");
        assert_eq!(on_knob(6.5, None), "6.5 of 10");
    }

    #[test]
    fn delay_time_follows_the_tempo() {
        let time = knob("Time", "1", "2000", "400", "ms");
        let set = dial(
            Stage::Delay,
            &[("Time".into(), Some(&time)), ("Mix".into(), None)],
            "",
            &tone(),
        );
        assert_eq!(
            (by(&set, "Time").value.as_str(), by(&set, "Time").from),
            ("500 ms", From::Recording)
        );
        assert_eq!(by(&set, "Mix").from, From::Default);
    }

    #[test]
    fn suites_need_the_models_own_knobs() {
        use crate::ai::Finding;
        let mut k = Knowledge::default();
        let amp = |gear: &str, model: &str| Finding {
            gear: gear.into(),
            kind: "amp".into(),
            settings: String::new(),
            quote: String::new(),
            url: String::new(),
            model: model.into(),
        };
        k.set(
            "AmpliTube 5",
            vec![
                amp("Marshall JCM800", "Brit 8000"),
                amp("Fender Twin", "American Clean"),
            ],
        );
        let pick = Pick {
            plugin: "AmpliTube 5".into(),
            model: "Brit 8000".into(),
            exact: true,
            capture: false,
        };
        assert!(matches!(names(Stage::Amp, &pick, &k), Names::Read));
        k.knobs.insert(
            "AmpliTube 5".into(),
            vec![
                knob("Parameter 1", "0", "1", "0", ""),
                knob("Volume", "0", "10", "5", ""),
            ],
        );
        assert!(matches!(names(Stage::Amp, &pick, &k), Names::Web));
        k.set_model_knobs(ModelKnobs {
            plugin: "AmpliTube 5".into(),
            model: "Brit 8000".into(),
            knobs: vec!["Pre Amp".into(), "Presence".into()],
            url: String::new(),
        });
        let Names::Page(found) = names(Stage::Amp, &pick, &k) else {
            panic!("the manual's knobs")
        };
        assert_eq!(found.knobs, ["Pre Amp", "Presence"]);
        // A plugin that is one unit uses its own knobs, minus automation slots.
        let one = Pick {
            plugin: "Gateway".into(),
            model: String::new(),
            exact: false,
            capture: false,
        };
        k.knobs.insert(
            "Gateway".into(),
            vec![
                knob("Gain", "0", "10", "5", ""),
                knob("MIDI CC 1", "0", "1", "0", ""),
            ],
        );
        let Names::Plugin(own) = names(Stage::Amp, &one, &k) else {
            panic!("its own knobs")
        };
        assert_eq!(own.len(), 1);
    }

    #[test]
    fn a_suites_section_for_the_stage_is_picked_out() {
        let mut delay = knob("Mix", "0", "100", "30", "%");
        delay.group = "Delay".into();
        let mut amp = knob("Gain", "0", "10", "5", "");
        amp.group = "Amp".into();
        let only = in_section(vec![&delay, &amp], Stage::Delay);
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].name, "Mix");
    }
}
