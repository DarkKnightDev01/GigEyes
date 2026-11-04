//! Link support: download just the audio of a video link with yt-dlp into a temporary folder.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A temporary folder that deletes itself, with everything in it, when dropped, even after an error.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> Result<Self, String> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let dir = std::env::temp_dir().join(format!("gigeyes-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create a temporary folder: {e}"))?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Only plain web links, so nothing typed into the box can reach yt-dlp as an option.
pub fn check_link(link: &str) -> Result<&str, String> {
    let link = link.trim();
    let lower = link.to_ascii_lowercase();
    if (lower.starts_with("https://") || lower.starts_with("http://")) && !link.contains(char::is_whitespace) {
        Ok(link)
    } else {
        Err("Paste a full web link starting with https://".into())
    }
}

/// yt-dlp next to gigeyes.exe (shipped in the release zip), otherwise the one on PATH.
fn yt_dlp() -> PathBuf {
    let name = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Download the audio of `link` into `dir` and return the file's path.
pub fn download(link: &str, dir: &Path) -> Result<PathBuf, String> {
    let link = check_link(link)?;
    let mut cmd = Command::new(yt_dlp());
    cmd.args(["--no-playlist", "--no-warnings", "--no-simulate", "--print", "after_move:filepath"])
        // m4a (AAC) first: GigEyes can't decode Opus, YouTube's other audio format.
        .args(["-f", "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/best[ext=mp4]/bestaudio"])
        .arg("-o")
        .arg(dir.join("%(title).80B.%(ext)s"))
        .arg("--")
        .arg(link);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
    }
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "Link support needs yt-dlp.exe in the GigEyes folder. It comes in the release zip.".to_string()
        } else {
            format!("Couldn't start yt-dlp: {e}")
        }
    })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let reason = err.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        return Err(format!("Couldn't download that link: {}", reason.trim_start_matches("ERROR: ")));
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .rev()
        .map(|l| PathBuf::from(l.trim()))
        .find(|p| p.is_file())
        .ok_or_else(|| "yt-dlp finished but no audio file came out.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_links_get_through() {
        assert_eq!(check_link("  https://youtu.be/abc ").unwrap(), "https://youtu.be/abc");
        assert!(check_link("--exec calc.exe").is_err());
        assert!(check_link("https://x.com/a --exec calc").is_err());
        assert!(check_link("C:\\song.mp3").is_err());
    }

    #[test]
    fn temp_folder_is_deleted_with_its_files() {
        let dir = TempDir::new().unwrap();
        let path = dir.0.clone();
        std::fs::write(path.join("song.m4a"), b"x").unwrap();
        drop(dir);
        assert!(!path.exists());
    }
}
