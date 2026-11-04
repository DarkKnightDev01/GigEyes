//! Your rig: the VST3 plugins on this PC and which of them you play through.
//! Plugins are never loaded; names, makers and types come from each bundle's moduleinfo.json.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Plugin {
    pub path: PathBuf,
    pub name: String,
    /// The maker, when the bundle says so.
    pub vendor: String,
    /// The plugin's own category, e.g. "Distortion" or "Delay"; empty when unknown.
    pub kind: String,
    /// Ticked by the user: GigEyes only suggests plugins they own and use.
    pub owned: bool,
}

/// Where Windows installs VST3 plugins: for all users, and for this user only.
pub fn default_folders() -> Vec<PathBuf> {
    let common = std::env::var_os("CommonProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| r"C:\Program Files\Common Files".into());
    let mut folders = vec![common.join("VST3")];
    if let Some(local) = dirs::data_local_dir() {
        folders.push(local.join("Programs").join("Common").join("VST3"));
    }
    folders
}

/// Every .vst3 under `folders`, sorted by name. Instruments are left out: GigEyes builds guitar effect chains.
pub fn scan(folders: &[PathBuf]) -> Vec<Plugin> {
    let mut found = vec![];
    for folder in folders {
        collect(folder, &mut found);
    }
    found.sort_by_key(|p| p.name.to_lowercase());
    found.dedup_by(|a, b| a.path == b.path);
    found
}

fn collect(dir: &Path, found: &mut Vec<Plugin>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("vst3"))
        {
            // A .vst3 is a bundle folder or a single file; never look inside it for more plugins.
            if let Some(plugin) = describe(&path) {
                found.push(plugin);
            }
        } else if path.is_dir() {
            collect(&path, found);
        }
    }
}

/// Name, maker and type from the bundle's moduleinfo.json, or just the file name when it has none.
fn describe(path: &Path) -> Option<Plugin> {
    let fallback = path.file_stem()?.to_string_lossy().into_owned();
    let mut plugin = Plugin {
        path: path.to_path_buf(),
        name: fallback,
        vendor: String::new(),
        kind: String::new(),
        owned: true,
    };
    let info = std::fs::read_to_string(
        path.join("Contents")
            .join("Resources")
            .join("moduleinfo.json"),
    )
    .ok();
    // moduleinfo.json is JSON5 (comments and trailing commas allowed).
    let Some(info) = info.and_then(|text| json5::from_str::<serde_json::Value>(&text).ok()) else {
        return Some(plugin);
    };
    let audio_classes: Vec<&serde_json::Value> = info["Classes"]
        .as_array()
        .map(|classes| {
            classes
                .iter()
                .filter(|c| c["Category"] == "Audio Module Class")
                .collect()
        })
        .unwrap_or_default();
    let subcategories = |c: &serde_json::Value| -> Vec<String> {
        c["Sub Categories"]
            .as_array()
            .map(|s| {
                s.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    if audio_classes
        .iter()
        .any(|c| subcategories(c).iter().any(|s| s.starts_with("Instrument")))
    {
        return None;
    }
    if let Some(class) = audio_classes.first() {
        if let Some(name) = class["Name"].as_str() {
            plugin.name = name.to_owned();
        }
        // "Fx|Distortion" → "Distortion"; a bare "Fx" says nothing.
        plugin.kind = subcategories(class)
            .iter()
            .flat_map(|s| s.split('|'))
            .find(|part| *part != "Fx" && !part.is_empty())
            .unwrap_or_default()
            .to_owned();
        plugin.vendor = class["Vendor"].as_str().unwrap_or_default().to_owned();
    }
    if plugin.vendor.is_empty() {
        plugin.vendor = info["Factory Info"]["Vendor"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
    }
    Some(plugin)
}

/// A fresh scan, keeping the tick the user set on plugins seen before.
pub fn merge(previous: &[Plugin], scanned: Vec<Plugin>) -> Vec<Plugin> {
    scanned
        .into_iter()
        .map(|mut p| {
            if let Some(old) = previous.iter().find(|old| old.path == p.path) {
                p.owned = old.owned;
            }
            p
        })
        .collect()
}

/// Scan `folders` again: new plugins appear, uninstalled ones go, plugins added from
/// other folders stay while they still exist, and every tick is kept.
pub fn rescan(previous: &[Plugin], folders: &[PathBuf]) -> Vec<Plugin> {
    let mut fresh = scan(folders);
    let added_elsewhere: Vec<Plugin> = previous
        .iter()
        .filter(|old| old.path.exists() && !fresh.iter().any(|p| p.path == old.path))
        .cloned()
        .collect();
    fresh.extend(added_elsewhere);
    fresh.sort_by_key(|p| p.name.to_lowercase());
    merge(previous, fresh)
}

fn rig_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("GigEyes").join("rig.json"))
}

/// The saved rig, or None before the first scan.
pub fn load() -> Option<Vec<Plugin>> {
    serde_json::from_str(&std::fs::read_to_string(rig_file()?).ok()?).ok()
}

pub fn save(plugins: &[Plugin]) -> Result<(), String> {
    let file = rig_file().ok_or("Can't find a settings folder on this PC.")?;
    std::fs::create_dir_all(file.parent().expect("rig file has a parent folder"))
        .and_then(|_| {
            crate::research::write_atomic(
                &file,
                &serde_json::to_string_pretty(plugins).expect("plugins serialise"),
            )
        })
        .map_err(|e| format!("Couldn't save your rig to {}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(root: &Path, file: &str, moduleinfo: Option<&str>) {
        let resources = root.join(file).join("Contents").join("Resources");
        std::fs::create_dir_all(&resources).unwrap();
        if let Some(text) = moduleinfo {
            std::fs::write(resources.join("moduleinfo.json"), text).unwrap();
        }
    }

    #[test]
    fn scan_reads_bundles_and_skips_instruments() {
        let root = crate::fetch::TempDir::new().unwrap();
        let vendor_dir = root.0.join("Neural DSP");
        // JSON5 with a trailing comma, as real moduleinfo files may have.
        bundle(
            &vendor_dir,
            "Archetype Gojira X.vst3",
            Some(
                r#"{
            "Name": "Archetype Gojira X",
            "Factory Info": { "Vendor": "Neural DSP Technologies", },
            "Classes": [{ "Category": "Audio Module Class", "Name": "Archetype Gojira X",
                          "Sub Categories": ["Fx", "Distortion"], "Vendor": "Neural DSP Technologies" }],
        }"#,
            ),
        );
        bundle(&root.0, "Old Delay.vst3", None);
        bundle(
            &root.0,
            "Big Synth.vst3",
            Some(
                r#"{"Classes": [{"Category": "Audio Module Class",
            "Name": "Big Synth", "Sub Categories": ["Instrument|Synth"]}]}"#,
            ),
        );
        std::fs::write(root.0.join("readme.txt"), "not a plugin").unwrap();

        let plugins = scan(std::slice::from_ref(&root.0));
        let names: Vec<_> = plugins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Archetype Gojira X", "Old Delay"]);
        assert_eq!(
            (plugins[0].vendor.as_str(), plugins[0].kind.as_str()),
            ("Neural DSP Technologies", "Distortion")
        );
        assert_eq!(
            (plugins[1].vendor.as_str(), plugins[1].kind.as_str()),
            ("", "")
        );
    }

    #[test]
    fn rescan_keeps_added_folders_and_drops_uninstalled_plugins() {
        let (main, other) = (
            crate::fetch::TempDir::new().unwrap(),
            crate::fetch::TempDir::new().unwrap(),
        );
        bundle(&main.0, "Amp.vst3", None);
        bundle(&other.0, "Pedal.vst3", None);
        let mut before = scan(&[main.0.clone(), other.0.clone()]);
        before[0].owned = false; // the user unticked Amp
        before.push(Plugin {
            path: main.0.join("Gone.vst3"),
            name: "Gone".into(),
            vendor: String::new(),
            kind: String::new(),
            owned: true,
        });

        let after = rescan(&before, std::slice::from_ref(&main.0));
        let names: Vec<_> = after.iter().map(|p| (p.name.as_str(), p.owned)).collect();
        assert_eq!(names, [("Amp", false), ("Pedal", true)]);
    }

    #[test]
    fn rescan_keeps_the_users_ticks() {
        let p = |name: &str, owned| Plugin {
            path: name.into(),
            name: name.into(),
            vendor: String::new(),
            kind: String::new(),
            owned,
        };
        let merged = merge(
            &[p("a.vst3", false)],
            vec![p("a.vst3", true), p("b.vst3", true)],
        );
        assert_eq!(merged, [p("a.vst3", false), p("b.vst3", true)]);
    }
}
