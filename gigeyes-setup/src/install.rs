//! Installing and removing GigEyes for everyone on the PC, in Program Files. The setup runs as administrator
//! (its manifest asks Windows for that); each user's own data stays in their AppData.

use crate::payload;
use std::path::{Path, PathBuf};

pub const APP: &str = "GigEyes";
pub const VERSION: &str = crate::look::VERSION;
const UNINSTALLER: &str = "uninstall.exe";
/// Windows' list of installed apps (machine-wide now; per-user before 0.4.1).
#[cfg(windows)]
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\GigEyes";

pub fn default_dir() -> PathBuf {
    std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| r"C:\Program Files".into())
        .join(APP)
}

/// A folder the user picked gets a GigEyes folder inside it, so removal never touches what was already there.
pub fn dir_inside(picked: &Path) -> PathBuf {
    if picked
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case(APP))
    {
        picked.to_path_buf()
    } else {
        picked.join(APP)
    }
}

#[derive(Clone)]
pub struct Options {
    pub dir: PathBuf,
    pub start_menu: bool,
    pub desktop: bool,
}

/// An earlier install, from Windows' installed-apps list: its folder and version.
#[cfg(windows)]
pub fn existing() -> Option<(PathBuf, String)> {
    use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(UNINSTALL_KEY)
        .ok()?;
    let dir: String = key.get_value("InstallLocation").ok()?;
    Some((
        PathBuf::from(dir),
        key.get_value("DisplayVersion").unwrap_or_default(),
    ))
}

#[cfg(not(windows))]
pub fn existing() -> Option<(PathBuf, String)> {
    None
}

/// Put every packed file into `opts.dir`, write the uninstaller and hook into Windows.
/// `step` hears a short sentence before each step, for the progress line.
pub fn install(setup: &Path, opts: &Options, mut step: impl FnMut(&str)) -> Result<(), String> {
    let (entries, stub_len) = payload::read(setup).map_err(|e| e.to_string())?.ok_or(
        "This setup file carries no app. Download GigEyes-Setup.exe from the Releases page.",
    )?;
    std::fs::create_dir_all(&opts.dir)
        .map_err(|e| format!("Can't create {}: {e}", opts.dir.display()))?;

    for entry in &entries {
        step(&format!("Copying {}…", entry.name));
        let target = opts.dir.join(&entry.name);
        let fresh = opts.dir.join(format!("{}.new", entry.name));
        payload::extract(setup, entry, &fresh).map_err(|e| {
            let _ = std::fs::remove_file(&fresh);
            format!("Couldn't write {}: {e}", entry.name)
        })?;
        // Swap in place: if the old file is in use (GigEyes still open), nothing is half-replaced.
        std::fs::rename(&fresh, &target).map_err(|_| {
            let _ = std::fs::remove_file(&fresh);
            format!("{} is in use. Close GigEyes, then try again.", entry.name)
        })?;
    }
    step("Adding the uninstaller…");
    payload::write_stub(setup, stub_len, &opts.dir.join(UNINSTALLER))
        .map_err(|e| format!("Couldn't write the uninstaller: {e}"))?;

    step("Adding shortcuts…");
    system::add(opts, entries.iter().map(|e| e.len).sum::<u64>() + stub_len)?;
    system::remove_old_per_user_install(&opts.dir);
    Ok(())
}

/// Remove GigEyes from `dir`: its own files, shortcuts and the installed-apps entry, and the user's
/// GigEyes settings and research too when `remove_data` is set. Other files in `dir` are left alone.
pub fn uninstall(dir: &Path, remove_data: bool) -> Result<(), String> {
    // Files first: if GigEyes is still open, the Installed-apps entry and shortcuts stay so "Try again" works.
    for name in ["gigeyes.exe", "yt-dlp.exe"] {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(format!("{name} is in use. Close GigEyes, then try again.")),
        }
    }
    system::remove();
    if remove_data {
        // %APPDATA%\GigEyes (rig, research, key) and %LOCALAPPDATA%\GigEyes (the AI model).
        for data in [dirs::config_dir(), dirs::data_local_dir()]
            .into_iter()
            .flatten()
            .map(|d| d.join(APP))
        {
            let _ = std::fs::remove_dir_all(data);
        }
    }
    system::remove_self_and_folder(dir);
    Ok(())
}

/// Start the installed app, detached from the setup. Through Explorer on Windows, so the app runs as
/// the signed-in user and not as administrator: Windows blocks drag and drop into elevated windows.
pub fn launch(dir: &Path) {
    let app = dir.join("gigeyes.exe");
    let _ = if cfg!(windows) {
        std::process::Command::new("explorer.exe").arg(app).spawn()
    } else {
        std::process::Command::new(app).current_dir(dir).spawn()
    };
}

/// Which shortcuts exist now, so an update keeps the user's choice.
pub fn shortcuts() -> (bool, bool) {
    system::shortcuts()
}

#[cfg(windows)]
mod system {
    use super::*;
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
    };

    /// The shortcuts every user sees: the shared Start menu and the public desktop.
    fn start_menu_link() -> Option<PathBuf> {
        std::env::var_os("ProgramData").map(|d| {
            PathBuf::from(d)
                .join(r"Microsoft\Windows\Start Menu\Programs")
                .join("GigEyes.lnk")
        })
    }

    fn desktop_link() -> Option<PathBuf> {
        std::env::var_os("PUBLIC").map(|d| PathBuf::from(d).join(r"Desktop\GigEyes.lnk"))
    }

    pub fn shortcuts() -> (bool, bool) {
        let exists = |link: Option<PathBuf>| link.is_some_and(|l| l.exists());
        (exists(start_menu_link()), exists(desktop_link()))
    }

    /// Before 0.4.1 GigEyes installed per user under %LOCALAPPDATA%\Programs. Take that copy away once
    /// the Program Files one is in place, so there's one GigEyes in the Start menu and Installed apps.
    pub fn remove_old_per_user_install(new_dir: &Path) {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let Some(old) = user
            .open_subkey(UNINSTALL_KEY)
            .ok()
            .and_then(|k| k.get_value::<String, _>("InstallLocation").ok())
            .map(PathBuf::from)
        else {
            return;
        };
        if old != new_dir {
            for name in ["gigeyes.exe", "yt-dlp.exe", UNINSTALLER] {
                let _ = std::fs::remove_file(old.join(name));
            }
            let _ = std::fs::remove_dir(&old); // only if empty
        }
        let links = [
            dirs::data_dir().map(|d| d.join(r"Microsoft\Windows\Start Menu\Programs\GigEyes.lnk")),
            dirs::desktop_dir().map(|d| d.join("GigEyes.lnk")),
        ];
        for link in links.into_iter().flatten() {
            let _ = std::fs::remove_file(link);
        }
        let _ = user.delete_subkey_all(UNINSTALL_KEY);
    }

    pub fn add(opts: &Options, bytes: u64) -> Result<(), String> {
        let app = opts.dir.join("gigeyes.exe");
        for (wanted, link) in [
            (opts.start_menu, start_menu_link()),
            (opts.desktop, desktop_link()),
        ] {
            let Some(link) = link else { continue };
            if wanted {
                let mut shortcut = mslnk::ShellLink::new(&app)
                    .map_err(|e| format!("Couldn't make a shortcut: {e}"))?;
                shortcut.set_working_dir(Some(opts.dir.display().to_string()));
                shortcut
                    .create_lnk(&link)
                    .map_err(|e| format!("Couldn't make a shortcut: {e}"))?;
            } else {
                let _ = std::fs::remove_file(link); // unticked on an update: take the old one away
            }
        }
        let uninstall = format!("\"{}\" --uninstall", opts.dir.join(UNINSTALLER).display());
        let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .create_subkey(UNINSTALL_KEY)
            .map_err(|e| format!("Couldn't add GigEyes to Installed apps: {e}"))?;
        let set = |name: &str, value: &str| key.set_value(name, &value.to_string());
        set("DisplayName", APP)
            .and_then(|_| set("DisplayVersion", VERSION))
            .and_then(|_| set("Publisher", "GigEyes"))
            .and_then(|_| set("InstallLocation", &opts.dir.display().to_string()))
            .and_then(|_| set("DisplayIcon", &app.display().to_string()))
            .and_then(|_| set("UninstallString", &uninstall))
            .and_then(|_| key.set_value("EstimatedSize", &((bytes / 1024) as u32)))
            .and_then(|_| key.set_value("NoModify", &1u32))
            .and_then(|_| key.set_value("NoRepair", &1u32))
            .map_err(|e| format!("Couldn't add GigEyes to Installed apps: {e}"))
    }

    pub fn remove() {
        for link in [start_menu_link(), desktop_link()].into_iter().flatten() {
            let _ = std::fs::remove_file(link);
        }
        let _ = RegKey::predef(HKEY_LOCAL_MACHINE).delete_subkey_all(UNINSTALL_KEY);
    }

    /// A running program can't delete itself on Windows, so a hidden cmd does it a moment after we exit.
    /// `rmdir` without /s only removes the folder if it's empty: nothing the user put there is touched.
    pub fn remove_self_and_folder(dir: &Path) {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let uninstaller = dir.join(UNINSTALLER);
        let script = format!(
            "ping 127.0.0.1 -n 3 > nul & del /f /q \"{}\" & rmdir \"{}\"",
            uninstaller.display(),
            dir.display()
        );
        // raw_arg: cmd has its own quoting rules, so the line goes over exactly as written.
        // /S /C "…" strips only the outer quotes and keeps the ones around each path.
        // Run from the temp folder: a cmd sitting in the install folder would keep rmdir from removing it.
        let _ = std::process::Command::new("cmd")
            .current_dir(std::env::temp_dir())
            .raw_arg(format!("/S /C \"{script}\""))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

/// Outside Windows there are no shortcuts or installed-apps list; files only (development and tests).
#[cfg(not(windows))]
mod system {
    use super::*;

    pub fn add(_: &Options, _: u64) -> Result<(), String> {
        Ok(())
    }

    pub fn remove() {}

    pub fn shortcuts() -> (bool, bool) {
        (true, false)
    }

    pub fn remove_old_per_user_install(_: &Path) {}

    pub fn remove_self_and_folder(dir: &Path) {
        let _ = std::fs::remove_file(dir.join(UNINSTALLER));
        let _ = std::fs::remove_dir(dir); // only if empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_then_uninstall_leaves_other_files_alone() {
        let root =
            std::env::temp_dir().join(format!("gigeyes-install-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for (name, bytes) in [
            ("stub", &b"stub"[..]),
            ("gigeyes.exe", b"app"),
            ("yt-dlp.exe", b"dl"),
        ] {
            std::fs::write(root.join(name), bytes).unwrap();
        }
        let setup = root.join("setup.exe");
        payload::pack(
            &root.join("stub"),
            &[&root.join("gigeyes.exe"), &root.join("yt-dlp.exe")],
            &setup,
        )
        .unwrap();

        let dir = dir_inside(&root.join("Apps"));
        assert_eq!(dir, root.join("Apps").join("GigEyes"));
        let opts = Options {
            dir: dir.clone(),
            start_menu: true,
            desktop: false,
        };
        let mut steps = vec![];
        install(&setup, &opts, |s| steps.push(s.to_string())).unwrap();
        assert_eq!(std::fs::read(dir.join("gigeyes.exe")).unwrap(), b"app");
        assert_eq!(std::fs::read(dir.join(UNINSTALLER)).unwrap(), b"stub");
        assert_eq!(
            steps.first().map(String::as_str),
            Some("Copying gigeyes.exe…")
        );

        // Installing again over the top (an update) works.
        install(&setup, &opts, |_| {}).unwrap();

        std::fs::write(dir.join("my-notes.txt"), b"mine").unwrap();
        uninstall(&dir, false).unwrap();
        assert!(!dir.join("gigeyes.exe").exists());
        // On Windows the uninstaller deletes itself a moment after it finishes.
        for _ in 0..50 {
            if !dir.join(UNINSTALLER).exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        assert!(
            !dir.join(UNINSTALLER).exists(),
            "the uninstaller removed itself"
        );
        assert!(
            dir.join("my-notes.txt").exists(),
            "a file GigEyes didn't put there survives"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_setup_without_payload_refuses_to_install() {
        let root = std::env::temp_dir().join(format!("gigeyes-empty-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("setup.exe"), b"just a program").unwrap();
        let opts = Options {
            dir: root.join("GigEyes"),
            start_menu: false,
            desktop: false,
        };
        let err = install(&root.join("setup.exe"), &opts, |_| {}).unwrap_err();
        assert!(err.contains("carries no app"), "{err}");
        std::fs::remove_dir_all(root).unwrap();
    }
}
