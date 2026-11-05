//! Updates: ask GitHub for the newest release, download its setup, check it, and run it.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const RELEASES: &str = "https://api.github.com/repos/DarkKnightDev01/GigEyes/releases?per_page=10";

/// A newer GigEyes, ready to download.
#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub version: String,
    pub page: String,
    pub bytes: u64,
    url: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    html_url: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    /// "sha256:…", which GitHub works out for every uploaded file.
    #[serde(default)]
    digest: Option<String>,
}

/// "v0.4.0.1" -> [0, 4, 0, 1]; "v0.4.0-prealpha.1" -> [0, 4, 0]. Missing parts count as 0.
fn numbers(version: &str) -> Vec<u32> {
    version
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|n| n.parse().unwrap_or(0))
        .collect()
}

fn newer(candidate: &str, current: &str) -> bool {
    let (mut a, mut b) = (numbers(candidate), numbers(current));
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}

/// The newest release, if it's newer than `current`.
pub fn check(current: &str) -> Result<Option<Update>, String> {
    check_at(RELEASES, current)
}

fn check_at(url: &str, current: &str) -> Result<Option<Update>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let releases: Vec<Release> = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .and_then(|mut r| r.body_mut().read_json())
        .map_err(|_| "Couldn't reach GitHub to check for updates.".to_string())?;
    let Some(newest) = releases
        .into_iter()
        .filter(|r| !r.draft)
        .max_by(|a, b| numbers(&a.tag_name).cmp(&numbers(&b.tag_name)))
    else {
        return Ok(None);
    };
    if !newer(&newest.tag_name, current) {
        return Ok(None);
    }
    let setup = newest
        .assets
        .into_iter()
        .find(|a| a.name.starts_with("GigEyes-Setup") && a.name.ends_with(".exe"))
        .ok_or("The newest release has no setup file yet. Try again in a few minutes.")?;
    // No checksum, no automatic install: GigEyes only runs a setup it could check.
    let sha256 = setup
        .digest
        .and_then(|d| d.strip_prefix("sha256:").map(str::to_string))
        .ok_or("The newest release can't be checked, so it has to be installed by hand.")?;
    Ok(Some(Update {
        version: newest.tag_name.trim_start_matches('v').to_string(),
        page: newest.html_url,
        bytes: setup.size,
        url: setup.browser_download_url,
        sha256,
    }))
}

/// Download the update's setup into a temporary folder (cleared on a later start) and check it.
pub fn download(update: &Update, mut progress: impl FnMut(u64)) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("gigeyes-update-{}", update.version));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
    let setup = dir.join(format!("GigEyes-Setup-{}.exe", update.version));
    let part = setup.with_extension("part");
    let fetched = crate::model::fetch(
        &update.url,
        update.bytes,
        &update.sha256,
        &part,
        &AtomicBool::new(false),
        &mut progress,
    );
    if fetched.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    fetched?;
    std::fs::rename(&part, &setup).map_err(|e| format!("Couldn't save the update: {e}"))?;
    Ok(setup)
}

/// Start `program` with `args` the way Explorer would, so Windows asks for administrator rights when
/// the program needs them (the setup and uninstaller do). Fails when the user says no.
pub fn run_as_admin(program: &Path, args: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{HSTRING, PCWSTR, w};
        // SAFETY: every string is a valid, NUL-terminated wide string that outlives the call.
        let code = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(program.as_os_str()),
                &HSTRING::from(args),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        }
        .0 as isize;
        // ShellExecute returns a number above 32 on success; 5 means the permission prompt was declined.
        match code {
            c if c > 32 => Ok(()),
            5 => Err("Windows needs your permission for this. Try again and click Yes.".into()),
            c => Err(format!(
                "Windows couldn't start {} (error {c}).",
                program.display()
            )),
        }
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new(program)
            .args(args.split_whitespace())
            .spawn()
            .map(drop)
            .map_err(|e| format!("Couldn't start {}: {e}", program.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn versions_compare_by_number_and_ignore_the_prerelease_tail() {
        assert!(newer("v0.4.1", "0.4.0.1"));
        assert!(newer("v0.4.0.2", "0.4.0.1"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(!newer("v0.4.0-prealpha.1", "0.4.0.1"));
        assert!(!newer("v0.4.0.1", "0.4.0.1"));
    }

    fn github(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/releases", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.read(&mut [0u8; 2048]);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body.as_bytes()).unwrap();
        });
        url
    }

    #[test]
    fn finds_the_newest_checked_setup_and_skips_older_ones() {
        let releases = r#"[
          {"tag_name":"v0.4.0.1","draft":false,"html_url":"https://x/old","assets":[]},
          {"tag_name":"v0.4.2","draft":false,"html_url":"https://x/new","assets":[
            {"name":"GigEyes-Setup-v0.4.2.exe","browser_download_url":"https://x/s.exe","size":7,"digest":"sha256:ab"}]},
          {"tag_name":"v9.0.0","draft":true,"html_url":"https://x/draft","assets":[]}
        ]"#;
        let found = check_at(&github(releases), "0.4.1").unwrap().unwrap();
        assert_eq!(
            (found.version.as_str(), found.sha256.as_str()),
            ("0.4.2", "ab")
        );
        assert_eq!(check_at(&github(releases), "0.4.2").unwrap(), None);

        let unchecked = r#"[{"tag_name":"v0.5.0","html_url":"https://x","assets":[
            {"name":"GigEyes-Setup-v0.5.0.exe","browser_download_url":"https://x/s.exe","size":7}]}]"#;
        assert!(
            check_at(&github(unchecked), "0.4.1").is_err(),
            "no checksum, no install"
        );
    }
}
