//! Plugin knowledge: which real amps, cabinets and pedals each of your plugins models. GigEyes looks
//! a plugin up on the web and the local AI reads the pages (`ai::Task::Plugin`); what it finds is kept
//! in `%APPDATA%\GigEyes\knowledge.json`, so it works offline from then on. Every model keeps the
//! sentence and page it came from, and a wrong one can be removed.

use crate::ai::Finding;
use crate::rig::Plugin;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Knowledge {
    pub plugins: Vec<Known>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Known {
    /// The plugin's name as the scan found it.
    pub plugin: String,
    /// Unix seconds when it was looked up.
    pub looked_up_at: u64,
    /// Everything found, each with its `quote` and `url` as proof: the plugin's type (kind
    /// `ai::TYPE`), facts about what it does (`ai::FACT`), and the real gear it models (`gear` is
    /// the real unit, `model` the plugin's own name for it).
    pub models: Vec<Finding>,
    /// Looked up for its type and what it does too. False for lookups from 0.7 and 0.7.1, which
    /// only asked what it models: those are looked up again.
    #[serde(default)]
    pub profile: bool,
}

impl Known {
    /// What kind of plugin it is, e.g. "overdrive", if a page said.
    pub fn kind(&self) -> Option<&str> {
        self.models
            .iter()
            .find(|m| m.kind == crate::ai::TYPE)
            .map(|m| m.gear.as_str())
    }

    /// Short facts about what it does and how it behaves.
    pub fn facts(&self) -> impl Iterator<Item = &Finding> {
        self.models.iter().filter(|m| m.kind == crate::ai::FACT)
    }

    /// The real gear it models. Also hides what lookups before 0.7.2.2 let through: a maker
    /// alone, a bare model word, software.
    pub fn gear(&self) -> impl Iterator<Item = &Finding> {
        self.models.iter().filter(|m| {
            m.kind != crate::ai::TYPE && m.kind != crate::ai::FACT && real_gear(&m.gear)
        })
    }
}

/// Gear makers, so "Marshall" in front of a model is read as the maker, not as part of the model.
/// Makers named in looked-up models are added to these.
const MAKERS: [&str; 64] = [
    "marshall",
    "fender",
    "mesa",
    "vox",
    "orange",
    "peavey",
    "evh",
    "engl",
    "diezel",
    "friedman",
    "soldano",
    "bogner",
    "hiwatt",
    "laney",
    "randall",
    "ampeg",
    "gibson",
    "supro",
    "dumble",
    "matchless",
    "dr",
    "victoria",
    "suhr",
    "fuchs",
    "revv",
    "kemper",
    "krank",
    "splawn",
    "jet",
    "carvin",
    "rivera",
    "budda",
    "morgan",
    "fortin",
    "omega",
    "darkglass",
    "boss",
    "ibanez",
    "maxon",
    "mxr",
    "dunlop",
    "electroharmonix",
    "ehx",
    "proco",
    "fulltone",
    "klon",
    "tc",
    "strymon",
    "eventide",
    "walrus",
    "jhs",
    "keeley",
    "analogman",
    "zvex",
    "digitech",
    "roland",
    "yamaha",
    "korg",
    "tech",
    "line",
    "celestion",
    "eminence",
    "jensen",
    "electrovoice",
];

fn file() -> Option<PathBuf> {
    crate::research::data_dir().map(|d| d.join("knowledge.json"))
}

impl Knowledge {
    /// What was looked up before, or nothing yet.
    pub fn load() -> Knowledge {
        file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let file = file().ok_or("Can't find a settings folder on this PC.")?;
        std::fs::create_dir_all(file.parent().expect("knowledge file has a folder"))
            .and_then(|_| {
                crate::research::write_atomic(
                    &file,
                    &serde_json::to_string_pretty(self).expect("knowledge serialises"),
                )
            })
            .map_err(|e| format!("Couldn't save what GigEyes found out: {e}"))
    }

    /// What was found out about `plugin`, if it was looked up.
    pub fn about(&self, plugin: &str) -> Option<&Known> {
        self.plugins.iter().find(|k| k.plugin == plugin)
    }

    /// Keep what a lookup of `plugin` found, replacing an earlier lookup.
    pub fn set(&mut self, plugin: &str, models: Vec<Finding>) {
        let looked_up_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.plugins.retain(|k| k.plugin != plugin);
        self.plugins.push(Known {
            plugin: plugin.to_string(),
            looked_up_at,
            models,
            profile: true,
        });
    }

    /// Take a model the lookup got wrong off `plugin`.
    pub fn remove_model(&mut self, plugin: &str, index: usize) {
        if let Some(known) = self.plugins.iter_mut().find(|k| k.plugin == plugin)
            && index < known.models.len()
        {
            known.models.remove(index);
        }
    }

    fn makers(&self) -> HashSet<String> {
        let found = self
            .plugins
            .iter()
            .flat_map(|k| k.gear())
            .filter_map(|m| words(&m.gear).into_iter().next())
            .filter(|w| !w.chars().any(|c| c.is_ascii_digit()) && !GENERIC.contains(&w.as_str()));
        MAKERS.iter().map(|m| m.to_string()).chain(found).collect()
    }

    /// Which of `plugins` (only the ticked ones) model `gear`, a name from a page, of `kind` (the
    /// AI's guitar/pickups/amp/cabinet/pedal/effect/strings/tuning/other).
    pub fn who_models<'a>(
        &'a self,
        gear: &str,
        kind: &str,
        plugins: &'a [Plugin],
    ) -> Vec<Match<'a>> {
        let makers = self.makers();
        let mut found: Vec<Match> = vec![];
        for plugin in plugins.iter().filter(|p| p.owned) {
            let Some(known) = self.about(&plugin.name) else {
                continue;
            };
            for model in known.gear() {
                if kinds_fit(kind, &model.kind)
                    && same_gear(&makers, gear, &model.gear)
                    && !found
                        .iter()
                        .any(|f| f.plugin == plugin.name && f.model.gear == model.gear)
                {
                    found.push(Match {
                        plugin: &plugin.name,
                        model,
                    });
                }
            }
        }
        found
    }
}

/// A model in one of your plugins that matches a piece of gear.
pub struct Match<'a> {
    pub plugin: &'a str,
    pub model: &'a Finding,
}

fn kinds_fit(found: &str, model: &str) -> bool {
    match found {
        "amp" => model == "amp",
        "cabinet" => model == "cabinet",
        "pedal" | "effect" | "other" => model == "pedal" || model == "effect",
        _ => false, // guitars, pickups, strings and tuning aren't plugins
    }
}

/// Whether `gear` names one real unit: "Ampeg SVT", "SH-101" or "JCM800" do; a maker alone
/// ("Orange", "MESA/Boogie"), a bare word ("Portaflex") or software ("Neural Amp Modeler") don't.
pub fn real_gear(gear: &str) -> bool {
    let lower = gear.to_lowercase();
    if [
        "modeler",
        "plugin",
        "software",
        "capture",
        "collection",
        "bundle",
    ]
    .iter()
    .any(|w| lower.contains(w))
    {
        return false;
    }
    let all = words(gear);
    let model: Vec<&String> = all
        .iter()
        .filter(|w| !MAKERS.contains(&w.as_str()) && !GENERIC.contains(&w.as_str()))
        .collect();
    match (all.len(), model.as_slice()) {
        (_, []) => false,
        (1, [w]) => w.chars().any(|c| c.is_ascii_digit()),
        _ => true,
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
fn same_gear(brands: &HashSet<String>, gear: &str, real: &str) -> bool {
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
            lookup: true,
        }
    }

    fn model(gear: &str, kind: &str, name: &str) -> Finding {
        Finding {
            gear: gear.into(),
            kind: kind.into(),
            settings: String::new(),
            quote: format!("It models the {gear}."),
            url: "https://example.com".into(),
            model: name.into(),
        }
    }

    /// What lookups of a few plugins might have found.
    fn looked_up() -> Knowledge {
        let mut k = Knowledge::default();
        k.set(
            "Helix Native",
            vec![
                model("MESA/Boogie Dual Rectifier", "amp", "Cali Rectifire"),
                model("MESA/Boogie Mark IV (lead channel)", "amp", "Cali IV Lead"),
                model("Peavey 5150", "amp", "PV Panama"),
                model("EVH 5150III 100 Blue channel", "amp", "EV Panama Blue"),
                model("Ibanez TS808 Tube Screamer", "pedal", "Scream 808"),
                model("BOSS DS-1 Distortion", "pedal", "Deez One Vintage"),
                model(
                    "Soldano SLO-100 (crunch channel)",
                    "amp",
                    "Solo Lead Crunch",
                ),
                model("Marshall JCM-800 (100 watt)", "amp", "Brit 2203"),
            ],
        );
        k.set(
            "Mesa Boogie Mark IIC+ Suite",
            vec![model("Mesa/Boogie Mark IIC+", "amp", "")],
        );
        k.set("TSE808", vec![model("Ibanez Tube Screamer", "pedal", "")]);
        k.set(
            "AmpliTube 5",
            vec![model("Mesa/Boogie Mark IIC+", "amp", "Mark IIC+")],
        );
        k
    }

    #[test]
    fn only_named_units_count_as_real_gear() {
        for yes in [
            "Ampeg SVT",
            "SH-101",
            "Marshall JCM800",
            "Ibanez TS808 Tube Screamer",
            "Eventide H949",
            "JCM800",
        ] {
            assert!(real_gear(yes), "{yes}");
        }
        for no in [
            "Ampeg",
            "MESA/Boogie",
            "Orange",
            "SVT",
            "Portaflex",
            "Neural Amp Modeler",
            "NeuralAmpModelerCore",
            "Marshall amp",
        ] {
            assert!(!real_gear(no), "{no}");
        }
    }

    #[test]
    fn gear_from_a_page_finds_the_plugins_that_model_it() {
        let k = looked_up();
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
            k.who_models(gear, kind, &rig)
                .iter()
                .map(|m| format!("{}: {}", m.plugin, m.model.gear))
                .collect()
        };
        let iic = names("Mesa/Boogie Mark IIC+", "amp");
        assert_eq!(
            iic,
            ["Mesa Boogie Mark IIC+ Suite: Mesa/Boogie Mark IIC+"],
            "unticked AmpliTube and the Mark IV stay out"
        );

        let ts = names("Ibanez Tube Screamer", "pedal");
        assert!(ts.iter().any(|n| n.starts_with("TSE808")), "{ts:?}");
        assert!(ts.iter().any(|n| n.starts_with("Helix Native")), "{ts:?}");

        assert!(
            !names("Boss DS-1", "pedal").is_empty(),
            "short model names still match"
        );
        assert_eq!(
            names("Marshall JCM 800", "amp"),
            ["Helix Native: Marshall JCM-800 (100 watt)"]
        );
        assert_eq!(
            names("Peavey 5150", "amp"),
            ["Helix Native: Peavey 5150"],
            "not the EVH"
        );
        assert!(
            !names("Soldano SLO", "amp").is_empty(),
            "SLO is the start of SLO-100"
        );
        assert!(
            names("Marshall", "amp").is_empty(),
            "a maker alone names no model"
        );
        assert!(names("Marshall amp", "amp").is_empty());
        assert!(
            names("Mesa/Boogie Dual Rectifier", "cabinet").is_empty(),
            "an amp isn't a cab"
        );
        assert!(
            names("Gibson Les Paul", "guitar").is_empty(),
            "guitars aren't plugins"
        );
    }

    #[test]
    fn lookups_replace_earlier_ones_and_wrong_models_can_be_removed() {
        let mut k = looked_up();
        k.set("TSE808", vec![]);
        assert_eq!(k.about("TSE808").unwrap().models.len(), 0);
        assert_eq!(k.plugins.iter().filter(|p| p.plugin == "TSE808").count(), 1);
        k.remove_model("Helix Native", 0);
        assert_eq!(
            k.about("Helix Native").unwrap().models[0].model,
            "Cali IV Lead"
        );
        k.remove_model("Helix Native", 99); // out of range: nothing happens
        assert!(k.about("Nothing").is_none());
        // A file from before still loads; a missing field doesn't lose the rest.
        let back: Knowledge = serde_json::from_str(&serde_json::to_string(&k).unwrap()).unwrap();
        assert_eq!(back, k);
    }
}
