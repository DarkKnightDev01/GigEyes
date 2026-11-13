//! The signal chain for a song: which of your plugins to put between the guitar and the output, in
//! what order, and which model in each. The gear comes from the song research; where a source names
//! nothing (no amp, say), the recording fills the gap. Gear none of your plugins models gets the
//! closest plugin you have, marked as a stand-in.

use crate::ai::Finding;
use crate::analysis::Tone;
use crate::knowledge::Knowledge;
use crate::rig::Plugin;

/// Where a block sits in the chain. The order here is the order on the board.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Stage {
    Wah,
    Compressor,
    Pitch,
    Fuzz,
    Overdrive,
    Distortion,
    Boost,
    Gate,
    Amp,
    Cabinet,
    Eq,
    Modulation,
    Delay,
    Reverb,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Wah => "Wah",
            Stage::Compressor => "Compressor",
            Stage::Pitch => "Pitch",
            Stage::Fuzz => "Fuzz",
            Stage::Overdrive => "Overdrive",
            Stage::Distortion => "Distortion",
            Stage::Boost => "Boost",
            Stage::Gate => "Noise gate",
            Stage::Amp => "Amp",
            Stage::Cabinet => "Cabinet",
            Stage::Eq => "EQ",
            Stage::Modulation => "Modulation",
            Stage::Delay => "Delay",
            Stage::Reverb => "Reverb",
        }
    }

    /// The plugin types (as lookups name them) that can do this job, closest first.
    fn types(self) -> &'static [&'static str] {
        match self {
            Stage::Wah => &["wah or filter", "multi-effect suite"],
            // A capture copies a fixed sound: amps, cabs and drives, not a compressor's attack.
            Stage::Compressor => &["compressor", "multi-effect suite"],
            Stage::Pitch => &["pitch", "multi-effect suite"],
            Stage::Fuzz => &["fuzz", "distortion", "multi-effect suite", "capture loader"],
            Stage::Overdrive => &[
                "overdrive",
                "boost",
                "distortion",
                "multi-effect suite",
                "capture loader",
            ],
            Stage::Distortion => &[
                "distortion",
                "overdrive",
                "fuzz",
                "multi-effect suite",
                "capture loader",
            ],
            Stage::Boost => &[
                "boost",
                "overdrive",
                "EQ",
                "multi-effect suite",
                "capture loader",
            ],
            Stage::Gate => &["noise gate", "multi-effect suite"],
            Stage::Amp => &["amp sim", "preamp", "multi-effect suite", "capture loader"],
            Stage::Cabinet => &[
                "cabinet or IR loader",
                "amp sim",
                "multi-effect suite",
                "capture loader",
            ],
            Stage::Eq => &["EQ", "multi-effect suite"],
            Stage::Modulation => &["modulation", "multi-effect suite"],
            Stage::Delay => &["delay", "multi-effect suite"],
            Stage::Reverb => &["reverb", "multi-effect suite"],
        }
    }
}

/// One plugin that can fill a block.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub plugin: String,
    /// The model inside it to choose, e.g. "Brit 2203"; empty for a plugin that is one unit.
    pub model: String,
    /// It models the very gear the source names; false for a stand-in.
    pub exact: bool,
    /// A capture loader (TONE3000, NAM): get a capture of the named gear and load it.
    pub capture: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stage: Stage,
    /// The gear a source names for this spot, with its quote and page; None when the recording
    /// put the block here.
    pub real: Option<Finding>,
    /// More gear the sources name for the same spot (a second amp, say).
    pub also: Vec<String>,
    /// Your plugins that can do it, best first.
    pub picks: Vec<Pick>,
    /// What to set: the source's settings, or a starting point measured from the recording.
    pub settings: String,
    /// Why the block is here when no source named it.
    pub note: String,
}

impl Block {
    /// Stays the same when the chain is rebuilt, so a swapped plugin stays swapped.
    pub fn key(&self) -> String {
        format!(
            "{:?}:{}",
            self.stage,
            self.real.as_ref().map_or("", |r| r.gear.as_str())
        )
    }
}

/// Builds the chain from what the sources name (`findings`), what the lookups know about the ticked
/// plugins in `rig`, and the recording.
pub fn build(
    findings: &[Finding],
    knowledge: &Knowledge,
    rig: &[Plugin],
    tone: &Tone,
) -> Vec<Block> {
    let gain = gain(tone);
    let mut blocks: Vec<Block> = vec![];
    for f in findings {
        let Some(stage) = stage_of(&f.kind, &f.gear) else {
            continue;
        };
        let lower = f.gear.to_lowercase();
        let one_only = matches!(stage, Stage::Amp | Stage::Cabinet);
        if let Some(b) = blocks.iter_mut().find(|b| {
            b.stage == stage
                && b.real
                    .as_ref()
                    .is_some_and(|r| r.gear.to_lowercase() == lower || one_only)
        }) {
            let first = b
                .real
                .as_ref()
                .is_some_and(|r| r.gear.to_lowercase() == lower);
            if !first && !b.also.iter().any(|a| a.to_lowercase() == lower) {
                b.also.push(f.gear.clone());
            }
            if b.settings.is_empty() && first {
                b.settings = f.settings.clone();
            }
            continue;
        }
        let mut picks: Vec<Pick> = vec![];
        for m in knowledge.who_models(&f.gear, &f.kind, rig) {
            // A capture loader's lookup lists captures people uploaded, not models it ships with.
            let loader = rig.iter().any(|p| {
                p.name == m.plugin && plugin_type(p, knowledge).as_deref() == Some(CAPTURES)
            });
            if !loader && !picks.iter().any(|p| p.plugin == m.plugin) {
                let single = knowledge
                    .about(m.plugin)
                    .is_some_and(|k| k.gear().count() == 1);
                let model = if single {
                    String::new()
                } else if m.model.model.is_empty() {
                    m.model.gear.clone()
                } else {
                    m.model.model.clone()
                };
                picks.push(Pick {
                    plugin: m.plugin.to_string(),
                    model,
                    exact: true,
                    capture: false,
                });
            }
        }
        add_stand_ins(&mut picks, stage, Some(&f.gear), knowledge, rig, gain);
        blocks.push(Block {
            stage,
            real: Some(f.clone()),
            also: vec![],
            picks,
            settings: f.settings.clone(),
            note: String::new(),
        });
    }

    if !blocks.iter().any(|b| b.stage == Stage::Amp) {
        let mut picks = vec![];
        add_stand_ins(&mut picks, Stage::Amp, None, knowledge, rig, gain);
        let sound = match gain {
            Gain::High => "high-gain",
            Gain::Crunch => "crunchy",
            Gain::Clean => "clean",
        };
        blocks.push(Block {
            stage: Stage::Amp,
            real: None,
            also: vec![],
            picks,
            settings: String::new(),
            note: format!("No source names the amp. The recording sounds {sound}, so start with a {sound} amp."),
        });
    }
    let has_ir_loader = rig
        .iter()
        .any(|p| p.owned && plugin_type(p, knowledge).as_deref() == Some("cabinet or IR loader"));
    if has_ir_loader && !blocks.iter().any(|b| b.stage == Stage::Cabinet) {
        let mut picks = vec![];
        add_stand_ins(&mut picks, Stage::Cabinet, None, knowledge, rig, gain);
        picks.retain(|p| {
            rig.iter()
                .find(|r| r.name == p.plugin)
                .and_then(|r| plugin_type(r, knowledge))
                .as_deref()
                == Some("cabinet or IR loader")
        });
        blocks.push(Block {
            stage: Stage::Cabinet,
            real: None,
            also: vec![],
            picks,
            settings: String::new(),
            note: "No source names the cabinet. Leave this out if your amp plugin has its own."
                .into(),
        });
    }
    for b in blocks
        .iter_mut()
        .filter(|b| b.stage == Stage::Amp && b.settings.is_empty())
    {
        b.settings = start_settings(tone);
    }
    blocks.sort_by_key(|b| b.stage); // stable: within a stage, the order the sources gave
    blocks
}

/// Plugins that can do `stage` though they don't model the named gear, closest kind first.
fn add_stand_ins(
    picks: &mut Vec<Pick>,
    stage: Stage,
    gear: Option<&str>,
    knowledge: &Knowledge,
    rig: &[Plugin],
    gain: Gain,
) {
    let mut fits: Vec<(usize, Pick)> = vec![];
    for p in rig.iter().filter(|p| p.owned) {
        if picks.iter().any(|x| x.plugin == p.name) {
            continue;
        }
        let known = knowledge.about(&p.name);
        let models_it = known.is_some_and(|k| k.gear().any(|m| model_stage(m) == Some(stage)));
        let kind = plugin_type(p, knowledge);
        let capture = kind.as_deref() == Some(CAPTURES);
        let rank = kind
            .and_then(|t| stage.types().iter().position(|s| *s == t))
            .or(models_it.then_some(stage.types().len()));
        let Some(rank) = rank else {
            continue;
        };
        if capture {
            // A capture of the very unit beats a different unit modelled; with no unit named
            // ("a chorus pedal") there's nothing to search for, so it goes last.
            if let Some(gear) = gear.filter(|g| crate::knowledge::real_gear(g)) {
                fits.push((
                    0,
                    Pick {
                        plugin: p.name.clone(),
                        model: format!("{gear} capture"),
                        exact: false,
                        capture: true,
                    },
                ));
                continue;
            }
        }
        let model = known.filter(|_| !capture).map_or(String::new(), |k| {
            let fitting: Vec<&Finding> =
                k.gear().filter(|m| model_stage(m) == Some(stage)).collect();
            if fitting.len() == k.gear().count() && fitting.len() == 1 {
                return String::new(); // the plugin is that one unit
            }
            // A chorus for a chorus, not the suite's phaser; an amp with the recording's gain.
            let word = effect_word(stage, gear);
            let named = |m: &&&Finding| {
                word.is_some_and(|w| format!("{} {}", m.gear, m.model).to_lowercase().contains(w))
            };
            let best = fitting
                .iter()
                .find(|m| match stage {
                    Stage::Amp => amp_gain(&m.gear) == Some(gain),
                    _ => named(m),
                })
                .or(fitting.first());
            best.map_or(String::new(), |m| {
                if m.model.is_empty() {
                    m.gear.clone()
                } else {
                    m.model.clone()
                }
            })
        });
        fits.push((
            rank,
            Pick {
                plugin: p.name.clone(),
                model,
                exact: false,
                capture,
            },
        ));
    }
    // Among equals: a capture of the unit, then a plugin known to have a model for the job.
    fits.sort_by_key(|(rank, p)| (*rank, !p.capture, p.model.is_empty()));
    picks.extend(fits.into_iter().map(|(_, p)| p));
}

/// The plugin's type: what a lookup found, else a guess from the category in its bundle. Two
/// kinds lookups don't name are worked out here: a capture loader (TONE3000, NAM), and an amp sim
/// that is really a whole rig with pedals and effects (AmpliTube, Helix).
pub fn plugin_type(plugin: &Plugin, knowledge: &Knowledge) -> Option<String> {
    let known = knowledge.about(&plugin.name);
    let facts: String = known
        .map(|k| {
            k.facts()
                .map(|f| f.gear.to_lowercase())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let name = format!("{} {}", plugin.name, plugin.vendor).to_lowercase();
    let either = |terms: &[&str]| terms.iter().any(|t| name.contains(t) || facts.contains(t));
    // Only by name: plenty of amp sims say they capture an amp's feel, and AmpliTube's pages
    // mention ToneX.
    let compact: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
    if [
        "capture",
        "tone3000",
        "tonehunt",
        "neuralampmodeler",
        "tonex",
    ]
    .iter()
    .any(|t| compact.contains(t))
        || name
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == "nam")
    {
        return Some(CAPTURES.into());
    }
    let looked_up = known
        .and_then(|k| k.kind())
        .filter(|kind| *kind != "unknown" && *kind != "other");
    let amp_like = matches!(looked_up, None | Some("amp sim" | "preamp"));
    // shortcut: the named suites are the common ones; others count once a lookup shows pedals.
    let suite = either(&[
        "amplitube",
        "helix",
        "guitar rig",
        "bias fx",
        "tonelib",
        "effects",
        "stompbox",
        "stomps",
        "pedals",
    ]) || known.is_some_and(|k| {
        k.gear().any(|m| m.kind == "amp")
            && k.gear().any(|m| m.kind != "amp" && m.kind != "cabinet")
    });
    if amp_like && suite {
        return Some("multi-effect suite".into());
    }
    if let Some(kind) = looked_up {
        return Some(kind.to_string());
    }
    let vst = plugin.kind.to_lowercase();
    [
        ("amp", "amp sim"),
        ("distortion", "distortion"),
        ("delay", "delay"),
        ("reverb", "reverb"),
        ("dynamics", "compressor"),
        ("eq", "EQ"),
        ("modulation", "modulation"),
        ("filter", "wah or filter"),
        ("pitch", "pitch"),
    ]
    .iter()
    .find(|(word, _)| vst.contains(word))
    .map(|(_, t)| t.to_string())
}

const CAPTURES: &str = "capture loader";

/// Where a model inside a plugin goes: its real gear's name, or the plugin's own name for it
/// ("70s Chorus" for a Boss CE-1).
fn model_stage(m: &Finding) -> Option<Stage> {
    stage_of(&m.kind, &m.gear).or_else(|| stage_of(&m.kind, &format!("{} {}", m.gear, m.model)))
}

/// The one word for what a block does ("chorus", "delay"), to find a suite's models for it by.
/// None for an effect with no common name.
pub fn effect_word(stage: Stage, gear: Option<&str>) -> Option<&'static str> {
    const EFFECTS: [&str; 12] = [
        "chorus",
        "flanger",
        "phaser",
        "tremolo",
        "vibrato",
        "rotary",
        "echo",
        "octave",
        "fuzz",
        "overdrive",
        "distortion",
        "boost",
    ];
    let lower = gear.unwrap_or_default().to_lowercase();
    if let Some(word) = EFFECTS.iter().find(|w| lower.contains(*w)) {
        return Some(word);
    }
    Some(match stage {
        Stage::Wah => "wah",
        Stage::Compressor => "compressor",
        Stage::Pitch => "pitch",
        Stage::Fuzz => "fuzz",
        Stage::Overdrive => "overdrive",
        Stage::Distortion => "distortion",
        Stage::Boost => "boost",
        Stage::Gate => "noise gate",
        Stage::Amp => "amp",
        Stage::Cabinet => "cab",
        Stage::Eq => "eq",
        Stage::Modulation => "modulation",
        Stage::Delay => "delay",
        Stage::Reverb => "reverb",
    })
}

/// Where gear the sources name goes. Guitars, pickups, strings and tuning aren't plugins, and
/// neither is gear that isn't a known kind of effect.
pub fn stage_of(kind: &str, gear: &str) -> Option<Stage> {
    match kind {
        "amp" => return Some(Stage::Amp),
        "cabinet" => return Some(Stage::Cabinet),
        "pedal" | "effect" | "other" => {}
        _ => return None,
    }
    let lower = gear.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '+')
        .filter(|w| !w.is_empty())
        .collect();
    let has = |terms: &[&str]| {
        terms.iter().any(|t| {
            if t.contains([' ', '-']) {
                lower.contains(t)
            } else {
                words.contains(t)
            }
        })
    };
    let stage = if has(&["wah", "crybaby", "cry baby"]) {
        Stage::Wah
    } else if has(&[
        "whammy",
        "octave",
        "octavia",
        "octavio",
        "pitch",
        "harmonizer",
        "pog",
    ]) {
        Stage::Pitch
    } else if has(&["fuzz", "muff", "bender", "fuzzface"]) {
        Stage::Fuzz
    } else if has(&[
        "comp",
        "compressor",
        "dyna",
        "ross",
        "squeezer",
        "sustainer",
    ]) {
        Stage::Compressor
    } else if has(&["gate", "suppressor", "decimator", "ns-2", "ns2"]) {
        Stage::Gate
    } else if has(&[
        "screamer",
        "klon",
        "centaur",
        "808",
        "ts808",
        "ts9",
        "ts-9",
        "ts10",
        "overdrive",
        "od",
        "bluesbreaker",
        "ocd",
        "sd-1",
        "sd1",
        "timmy",
        "drive",
    ]) {
        Stage::Overdrive
    } else if has(&[
        "rat",
        "ds-1",
        "ds1",
        "distortion",
        "distortion+",
        "metal zone",
        "mt-2",
        "plus",
        "grind",
    ]) {
        Stage::Distortion
    } else if has(&["boost", "booster", "rangemaster", "ep-3", "spark"]) {
        Stage::Boost
    } else if has(&[
        "chorus", "flanger", "phaser", "phase", "tremolo", "vibe", "univibe", "uni-vibe", "rotary",
        "leslie", "clone", "mistress", "vibrato", "ce-2", "ce2",
    ]) {
        Stage::Modulation
    } else if has(&[
        "delay",
        "echo",
        "echoplex",
        "memory man",
        "carbon copy",
        "dd-3",
        "dd3",
        "tape",
    ]) {
        Stage::Delay
    } else if has(&[
        "reverb",
        "verb",
        "hall",
        "plate",
        "spring",
        "holy grail",
        "shimmer",
    ]) {
        Stage::Reverb
    } else if has(&["eq", "equalizer", "graphic"]) {
        Stage::Eq
    } else {
        // Gear GigEyes can't place (a talk box, a tuner, a wireless) has no plugin to stand in.
        return None;
    };
    Some(stage)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Gain {
    Clean,
    Crunch,
    High,
}

/// How driven the recording sounds, from how squashed its peaks are.
fn gain(tone: &Tone) -> Gain {
    match tone.crest_db {
        c if c < 6.0 => Gain::High,
        c if c < 10.0 => Gain::Crunch,
        _ => Gain::Clean,
    }
}

/// How much gain a well-known amp is known for, when its name says.
fn amp_gain(gear: &str) -> Option<Gain> {
    let lower = gear.to_lowercase();
    let any = |terms: &[&str]| terms.iter().any(|t| lower.contains(t));
    if any(&[
        "rectif",
        "recto",
        "5150",
        "6505",
        "evh",
        "uberschall",
        "slo",
        "soldano",
        "mark iv",
        "mark v",
        "iic",
        "triple",
        "krank",
        "diezel",
        "vh4",
        "engl",
        "powerball",
        "invader",
        "jvm",
        "randall",
        "panama",
        "petrucci",
    ]) {
        Some(Gain::High)
    } else if any(&[
        "twin",
        "deluxe",
        "jc-120",
        "jc120",
        "jazz chorus",
        "princeton",
        "vibrolux",
        "clean",
    ]) {
        Some(Gain::Clean)
    } else if any(&[
        "jcm", "800", "2203", "plexi", "1959", "jtm", "ac30", "ac15", "vox", "bassman", "hiwatt",
        "orange", "crunch", "brit",
    ]) {
        Some(Gain::Crunch)
    } else {
        None
    }
}

/// Where the amp's knobs could start, from the recording: "Gain 7, Bass 6, …".
fn start_settings(tone: &Tone) -> String {
    let knobs: Vec<String> = tone
        .knobs
        .iter()
        .map(|k| format!("{} {:.0}", k.label, k.value))
        .collect();
    if knobs.is_empty() {
        return String::new();
    }
    format!(
        "From the recording, as a start: {} (out of 10, 5 is noon).",
        knobs.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::Knob;

    fn plugin(name: &str, kind: &str) -> Plugin {
        Plugin {
            path: name.into(),
            name: name.into(),
            vendor: String::new(),
            kind: kind.into(),
            owned: true,
            lookup: true,
        }
    }

    fn found(gear: &str, kind: &str, model: &str) -> Finding {
        Finding {
            gear: gear.into(),
            kind: kind.into(),
            settings: String::new(),
            quote: format!("He played a {gear}."),
            url: "https://example.com".into(),
            model: model.into(),
        }
    }

    fn typed(kind: &str) -> Finding {
        found(kind, crate::ai::TYPE, "")
    }

    fn tone(crest_db: f32) -> Tone {
        let knob = |label, value| Knob {
            label,
            value,
            verdict: "",
            detail: String::new(),
        };
        Tone {
            centers: vec![],
            levels_db: vec![],
            tilt_db_per_octave: -5.0,
            crest_db,
            seconds: 30.0,
            verdict: String::new(),
            knobs: vec![knob("Gain", 8.0), knob("Bass", 6.0)],
            tempo_bpm: Some(120.0),
        }
    }

    fn rig_and_knowledge() -> (Vec<Plugin>, Knowledge) {
        let rig = vec![
            plugin("Helix Native", ""),
            plugin("TSE808", "Distortion"),
            plugin("ValhallaRoom", "Reverb"),
            plugin("NadIR", ""),
            Plugin {
                owned: false,
                ..plugin("Unticked Delay", "Delay")
            },
        ];
        let mut k = Knowledge::default();
        k.set(
            "Helix Native",
            vec![
                typed("multi-effect suite"),
                found("Marshall JCM-800", "amp", "Brit 2203"),
                found("Peavey 5150", "amp", "PV Panama"),
                found("Fender Twin Reverb", "amp", "US Double Nrm"),
            ],
        );
        k.set(
            "TSE808",
            vec![
                typed("overdrive"),
                found("Ibanez Tube Screamer", "pedal", ""),
            ],
        );
        k.set("NadIR", vec![typed("cabinet or IR loader")]);
        (rig, k)
    }

    #[test]
    fn named_gear_goes_in_order_with_exact_matches_first() {
        let (rig, k) = rig_and_knowledge();
        let findings = vec![
            found("Lexicon hall reverb", "effect", ""),
            found("Marshall JCM800", "amp", ""),
            found("Ibanez Tube Screamer TS808", "pedal", ""),
            found("Fender Stratocaster", "guitar", ""),
        ];
        let chain = build(&findings, &k, &rig, &tone(12.0));
        let stages: Vec<Stage> = chain.iter().map(|b| b.stage).collect();
        assert_eq!(
            stages,
            [Stage::Overdrive, Stage::Amp, Stage::Cabinet, Stage::Reverb]
        );
        assert_eq!(chain[0].picks[0].plugin, "TSE808");
        assert!(chain[0].picks[0].exact);
        assert_eq!(chain[0].picks[0].model, "", "TSE808 is that one pedal");
        assert_eq!(chain[1].picks[0].model, "Brit 2203");
        assert!(chain[1].picks[0].exact);
        assert_eq!(chain[3].picks[0].plugin, "ValhallaRoom");
        assert!(!chain[3].picks[0].exact);
        assert!(chain[2].real.is_none() && chain[2].picks[0].plugin == "NadIR");
        assert!(
            chain
                .iter()
                .all(|b| b.picks.iter().all(|p| p.plugin != "Unticked Delay"))
        );
    }

    #[test]
    fn unmodelled_gear_gets_the_closest_plugin_as_a_stand_in() {
        let (rig, k) = rig_and_knowledge();
        let chain = build(&[found("Klon Centaur", "pedal", "")], &k, &rig, &tone(12.0));
        let drive = &chain[0];
        assert_eq!(drive.stage, Stage::Overdrive);
        assert_eq!(drive.picks[0].plugin, "TSE808");
        assert!(!drive.picks[0].exact);
        assert_eq!(
            drive.picks.last().unwrap().plugin,
            "Helix Native",
            "a suite comes last"
        );
    }

    #[test]
    fn with_no_amp_named_the_recording_picks_one() {
        let (rig, k) = rig_and_knowledge();
        let chain = build(&[], &k, &rig, &tone(4.0));
        let amp = chain.iter().find(|b| b.stage == Stage::Amp).unwrap();
        assert!(amp.real.is_none());
        assert!(amp.note.contains("high-gain"));
        assert_eq!(amp.picks[0].model, "PV Panama");
        assert!(amp.settings.contains("Gain 8"));
        let clean = build(&[], &k, &rig, &tone(14.0));
        assert_eq!(clean[0].picks[0].model, "US Double Nrm");
    }

    #[test]
    fn a_second_amp_is_noted_on_the_first_not_put_in_series() {
        let (rig, k) = rig_and_knowledge();
        let findings = vec![
            found("Marshall JCM800", "amp", ""),
            found("Marshall JCM800", "amp", ""),
            found("Vox AC30", "amp", ""),
        ];
        let chain = build(&findings, &k, &rig, &tone(8.0));
        let amps: Vec<&Block> = chain.iter().filter(|b| b.stage == Stage::Amp).collect();
        assert_eq!(amps.len(), 1);
        assert_eq!(amps[0].also, ["Vox AC30"]);
    }

    #[test]
    fn amp_sims_with_effects_and_capture_loaders_fill_effect_slots() {
        // As lookups really type them: AmpliTube as an amp sim, TONE3000 with a capture as gear.
        let rig = vec![
            plugin("AmpliTube 5", ""),
            plugin("TONE3000", ""),
            plugin("Gateway", ""),
        ];
        let mut k = Knowledge::default();
        k.set(
            "AmpliTube 5",
            vec![typed("amp sim"), found("Marshall JCM800", "amp", "")],
        );
        k.set(
            "TONE3000",
            vec![
                typed("amp sim"),
                found("Jext Telez The Black Sheep", "amp", ""),
            ],
        );
        k.set(
            "Gateway",
            vec![typed("amp sim"), found("Marshall JCM800 2203", "amp", "")],
        );
        let findings = vec![
            found("Chorus pedal (model unknown)", "effect", ""),
            found("Delay pedal", "effect", ""),
            found("MXR M222 Talk Box", "pedal", ""),
        ];
        let chain = build(&findings, &k, &rig, &tone(12.0));
        let at = |stage| chain.iter().find(|b| b.stage == stage).unwrap();
        let only = |stage| {
            at(stage)
                .picks
                .iter()
                .map(|p| p.plugin.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(only(Stage::Modulation), ["AmpliTube 5"]);
        assert_eq!(only(Stage::Delay), ["AmpliTube 5"]);
        assert_eq!(chain.len(), 3, "no block for the talk box");
        let amp = only(Stage::Amp);
        assert_eq!(
            amp.last(),
            Some(&"TONE3000"),
            "with no amp named, a capture loader goes last"
        );
    }

    #[test]
    fn the_love_song_chain_names_things_you_can_do() {
        // What 0.8.0 got wrong on a real rig: AmpliTube's pages mention ToneX, TONE3000's lookup
        // lists an uploaded capture as a model, and Helix got no model for the chorus.
        let rig = vec![
            plugin("AmpliTube 5", ""),
            plugin("TONE3000", ""),
            plugin("Helix Native (x64)", ""),
        ];
        let mut k = Knowledge::default();
        k.set(
            "AmpliTube 5",
            vec![
                typed("amp sim"),
                found("Loads ToneX captures too", crate::ai::FACT, ""),
                found("Fender Twin Reverb", "amp", "American Tube Clean"),
            ],
        );
        k.set(
            "TONE3000",
            vec![
                typed("amp sim"),
                found("Marshall JCM800 2203", "amp", "V30 1960"),
            ],
        );
        k.set(
            "Helix Native (x64)",
            vec![
                typed("amp sim"),
                found("Peavey 5150", "amp", "PV Panama"),
                found("MXR Phase 90", "pedal", "Script Mod Phase"),
                found("Boss CE-1", "pedal", "70s Chorus"),
            ],
        );
        let findings = vec![
            found("MXR M222 Talk Box", "pedal", ""),
            found("Marshall JCM800 2203", "amp", ""),
            found("Chorus pedal", "effect", ""),
        ];
        let chain = build(&findings, &k, &rig, &tone(12.0));
        let first = |stage| &chain.iter().find(|b| b.stage == stage).unwrap().picks[0];
        assert!(
            chain
                .iter()
                .all(|b| !b.picks.iter().any(|p| p.model.contains("Talk Box"))),
            "a talk box isn't a guitar plugin, so nothing is suggested for it"
        );
        let amp = first(Stage::Amp);
        assert_eq!(
            (amp.plugin.as_str(), amp.model.as_str(), amp.capture),
            ("TONE3000", "Marshall JCM800 2203 capture", true),
            "an uploaded capture's name is not a model to pick"
        );
        let chorus = first(Stage::Modulation);
        assert_eq!(
            (chorus.plugin.as_str(), chorus.model.as_str()),
            ("Helix Native (x64)", "70s Chorus"),
            "the suite's chorus, found by its own name, not its phaser"
        );
        assert_eq!(
            effect_word(Stage::Modulation, Some("Chorus pedal")),
            Some("chorus")
        );
    }

    #[test]
    fn pedals_are_placed_by_their_names() {
        let at = |gear| stage_of("pedal", gear);
        assert_eq!(at("Dunlop Cry Baby"), Some(Stage::Wah));
        assert_eq!(at("Electro-Harmonix Big Muff Pi"), Some(Stage::Fuzz));
        assert_eq!(at("ProCo RAT"), Some(Stage::Distortion));
        assert_eq!(at("MXR Phase 90"), Some(Stage::Modulation));
        assert_eq!(at("Boss DD-3"), Some(Stage::Delay));
        assert_eq!(at("DigiTech Whammy"), Some(Stage::Pitch));
        assert_eq!(at("Mystery Box"), None);
        assert_eq!(at("MXR M222 Talk Box"), None);
        assert_eq!(stage_of("strings", "Ernie Ball 10-46"), None);
    }
}
