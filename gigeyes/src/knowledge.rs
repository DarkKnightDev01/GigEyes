//! Plugin knowledge: which real amps, cabinets and pedals each known plugin models, taken from the
//! makers' own pages (data/plugins.json, built into the app, every entry with its source). Used to
//! show what a plugin in your rig models, and which of your plugins model the gear a song used.

use crate::rig::Plugin;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Entry {
    /// Names the plugin goes by; a plugin on disk matches when its name contains one of them.
    pub names: Vec<String>,
    pub maker: String,
    /// The maker's page the models come from.
    pub source: String,
    pub models: Vec<Model>,
}

#[derive(Deserialize)]
pub struct Model {
    /// What the plugin calls it, e.g. "Cali Rectifire".
    pub model: String,
    /// amp, cabinet, pedal or effect.
    pub kind: String,
    /// The real gear, as the maker names it, e.g. "MESA/Boogie Dual Rectifier".
    pub real: String,
}

#[derive(Deserialize)]
struct File {
    plugins: Vec<Entry>,
}

struct Db {
    entries: Vec<Entry>,
    /// Words that name a maker (the first word of each real name), left out when comparing models.
    brands: HashSet<String>,
}

fn db() -> &'static Db {
    static DB: OnceLock<Db> = OnceLock::new();
    DB.get_or_init(|| {
        let file: File = serde_json::from_str(include_str!("../data/plugins.json"))
            .expect("data/plugins.json is checked by the tests");
        let brands = file
            .plugins
            .iter()
            .flat_map(|e| &e.models)
            .filter_map(|m| words(&m.real).into_iter().next())
            .filter(|w| !w.chars().any(|c| c.is_ascii_digit()) && !GENERIC.contains(&w.as_str()))
            .collect();
        Db {
            entries: file.plugins,
            brands,
        }
    })
}

/// What GigEyes knows about `plugin`: every entry whose name it carries.
pub fn about(plugin: &str) -> Vec<&'static Entry> {
    let name = squash(plugin);
    db().entries
        .iter()
        .filter(|e| e.names.iter().any(|n| name.contains(&squash(n))))
        .collect()
}

/// A model in one of your plugins that matches a piece of gear.
pub struct Match<'a> {
    pub plugin: &'a str,
    pub model: &'static Model,
}

/// Which of `plugins` (only the ticked ones) model `gear`, a name from a page, of `kind` (the AI's
/// guitar/pickups/amp/cabinet/pedal/effect/strings/tuning/other).
pub fn who_models<'a>(gear: &str, kind: &str, plugins: &'a [Plugin]) -> Vec<Match<'a>> {
    let mut found: Vec<Match> = vec![];
    for plugin in plugins.iter().filter(|p| p.owned) {
        for entry in about(&plugin.name) {
            for model in &entry.models {
                if kinds_fit(kind, &model.kind)
                    && same_gear(gear, &model.real)
                    && !found
                        .iter()
                        .any(|f| f.plugin == plugin.name && f.model.real == model.real)
                {
                    found.push(Match {
                        plugin: &plugin.name,
                        model,
                    });
                }
            }
        }
    }
    found
}

fn kinds_fit(found: &str, model: &str) -> bool {
    match found {
        "amp" => model == "amp",
        "cabinet" => model == "cabinet",
        "pedal" | "effect" | "other" => model == "pedal" || model == "effect",
        _ => false, // guitars, pickups, strings and tuning aren't plugins
    }
}

/// Words that say what kind of thing it is, not which one: a page saying "a Marshall amp" names no model.
const GENERIC: [&str; 40] = [
    "amp",
    "amps",
    "amplifier",
    "head",
    "combo",
    "cab",
    "cabinet",
    "pedal",
    "pedals",
    "guitar",
    "bass",
    "overdrive",
    "distortion",
    "fuzz",
    "delay",
    "reverb",
    "chorus",
    "flanger",
    "phaser",
    "compressor",
    "boost",
    "booster",
    "wah",
    "eq",
    "tremolo",
    "vibrato",
    "the",
    "a",
    "an",
    "and",
    "with",
    "his",
    "her",
    "channel",
    "lead",
    "clean",
    "tube",
    "preamp",
    "vintage",
    "modern",
];

/// Whether a name from a page (`gear`) and a plugin model's real gear (`real`) are the same thing:
/// the model words of one are named in the other, e.g. "Dual Rectifier" in "Mesa/Boogie Dual
/// Rectifier 3-channel", or "Tube Screamer" in "Ibanez TS808 Tube Screamer". Two different makers
/// in front never match: a Peavey 5150 isn't an EVH 5150III.
fn same_gear(gear: &str, real: &str) -> bool {
    let brands = &db().brands;
    let real = real.split('(').next().unwrap_or_default(); // "(bright channel)" and the like
    let (real_words, gear_words) = (words(real), words(gear));
    let maker = |w: &[String]| w.iter().take_while(|w| brands.contains(*w)).count();
    let (real_maker, gear_maker) = (maker(&real_words), maker(&gear_words));
    if real_maker > 0
        && gear_maker > 0
        && !real_words[..real_maker]
            .iter()
            .any(|w| gear_words[..gear_maker].contains(w))
    {
        return false;
    }
    // The model's own words: without the maker in front, or words like "distortion".
    let model = |w: &[String], skip: usize| -> Vec<String> {
        w[skip..]
            .iter()
            .filter(|w| !GENERIC.contains(&w.as_str()))
            .cloned()
            .collect()
    };
    named_in(&model(&real_words, real_maker), &gear_words)
        || named_in(&model(&gear_words, gear_maker), &real_words)
}

/// Whether the model words `model` are all among `words` (a short one may start a model number:
/// "SLO" for "SLO100"), or appear run together ("JCM 800" in "JCM800").
fn named_in(model: &[String], words: &[String]) -> bool {
    if model.is_empty() {
        return false;
    }
    let has = |m: &String| {
        words.iter().any(|w| {
            w == m
                || (m.len() >= 2
                    && w.starts_with(m.as_str())
                    && w[m.len()..].starts_with(|c: char| c.is_ascii_digit()))
        })
    };
    let joined = model.concat();
    model.iter().all(has) || (joined.len() >= 4 && words.concat().contains(&joined))
}

/// Lowercase words, with hyphens joined ("DS-1" is "ds1"), "Mesa/Boogie" as one maker, and other
/// punctuation and trademark signs as gaps.
fn words(s: &str) -> Vec<String> {
    let s = s
        .to_lowercase()
        .replace("mesa/boogie", "mesa")
        .replace("mesa boogie", "mesa")
        .replace("electro harmonix", "electroharmonix")
        .replace("pro co ", "proco ")
        .replace(['-', '\'', '’', '‘', '.'], "");
    s.split(|c: char| !(c.is_alphanumeric() || c == '+'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// A name with only its letters and digits, for comparing plugin names written different ways.
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(name: &str) -> Plugin {
        Plugin {
            path: name.into(),
            name: name.into(),
            vendor: String::new(),
            kind: String::new(),
            owned: true,
        }
    }

    #[test]
    fn every_entry_is_complete_and_sourced() {
        let kinds = ["amp", "cabinet", "pedal", "effect"];
        for e in &db().entries {
            assert!(!e.names.is_empty() && !e.maker.is_empty(), "{:?}", e.names);
            assert!(
                e.source.starts_with("https://"),
                "{} needs a source",
                e.names[0]
            );
            for m in &e.models {
                assert!(
                    kinds.contains(&m.kind.as_str()),
                    "{}: kind {}",
                    m.model,
                    m.kind
                );
                assert!(!m.real.trim().is_empty(), "{} has no real gear", m.model);
            }
        }
    }

    #[test]
    fn plugin_names_match_however_they_are_written() {
        assert_eq!(about("Helix Native").len(), 1);
        assert_eq!(
            about("UADx Marshall Plexi Super Lead 1959 Amp")[0].maker,
            "Universal Audio"
        );
        assert_eq!(about("Fortin Nameless Suite X")[0].maker, "Neural DSP");
        assert!(
            about("Archetype: Gojira X").is_empty(),
            "not in the list: its page names no amps"
        );
    }

    #[test]
    fn gear_from_a_page_finds_the_plugins_that_model_it() {
        let rig = [
            plugin("Helix Native"),
            plugin("Mesa Boogie Mark IIC+ Suite"),
            plugin("TSE808"),
            Plugin {
                owned: false,
                ..plugin("AmpliTube 5")
            },
        ];
        let names = |gear: &str, kind: &str| -> Vec<String> {
            who_models(gear, kind, &rig)
                .iter()
                .map(|m| format!("{}: {}", m.plugin, m.model.model))
                .collect()
        };
        let iic = names("Mesa/Boogie Mark IIC+", "amp");
        assert!(
            iic.contains(&"Mesa Boogie Mark IIC+ Suite: Mark IIC+".to_string()),
            "{iic:?}"
        );
        assert!(
            !iic.iter().any(|n| n.contains("AmpliTube")),
            "unticked plugins stay out"
        );
        assert!(
            !iic.iter().any(|n| n.contains("Cali IV")),
            "a Mark IV isn't a Mark IIC+: {iic:?}"
        );

        let ts = names("Ibanez Tube Screamer", "pedal");
        assert!(ts.contains(&"TSE808: TSE 808".to_string()), "{ts:?}");
        assert!(ts.iter().any(|n| n.starts_with("Helix Native")), "{ts:?}");

        assert!(
            !names("Boss DS-1", "pedal").is_empty(),
            "short model names still match"
        );
        assert!(
            names("Marshall", "amp").is_empty(),
            "a maker alone names no model"
        );
        assert!(names("Marshall amp", "amp").is_empty());
        assert!(
            names("Mesa/Boogie Dual Rectifier", "cabinet").is_empty() || {
                who_models("Mesa/Boogie Dual Rectifier", "cabinet", &rig)
                    .iter()
                    .all(|m| m.model.kind == "cabinet")
            }
        );
        assert!(
            names("Gibson Les Paul", "guitar").is_empty(),
            "guitars aren't plugins"
        );
    }
}
