//! The local AI: the model file and the llama.cpp engine that runs it, where they're kept, whether
//! they're there, and downloading them. Shared with the setup, which offers the download (the setup
//! includes this file by path).

use crate::hardware::Model;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// llama.cpp's server, pinned to one build and checked like the model, so nothing GigEyes runs changes unseen.
pub struct Engine {
    pub url: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    /// The server program inside the unpacked archive.
    pub server: &'static str,
}

/// The Vulkan build (MIT licence): runs on NVIDIA, AMD and Intel graphics cards, and has the
/// processor backends for PCs without one. One 38 MB download instead of CUDA's 400 MB of extras.
#[cfg(windows)]
pub const ENGINE: Engine = Engine {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b12144/llama-b12144-bin-win-vulkan-x64.zip",
    bytes: 37683311,
    sha256: "4824892a8bf28726a2790c8c9041e146c4357c51f6fb22184f8ac58985ed6384",
    server: "llama-server.exe",
};
/// Development only: the same build for Linux.
#[cfg(not(windows))]
pub const ENGINE: Engine = Engine {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b12144/llama-b12144-bin-ubuntu-x64.tar.gz",
    bytes: 23084864,
    sha256: "eb003cfeda7c9bf0163bc0c9da650c86bbc28e4f178a5f7d2685ea13536163de",
    server: "llama-b12144/llama-server",
};

/// `%LOCALAPPDATA%\GigEyes\models`: local, not roaming, because the files are gigabytes.
pub fn dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("GigEyes").join("models"))
}

pub fn path(model: &Model) -> Option<PathBuf> {
    dir().map(|d| d.join(model.file))
}

/// `%LOCALAPPDATA%\GigEyes\engine-<checksum>`: a new pinned build unpacks next to the old one.
fn engine_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| {
        d.join("GigEyes")
            .join(format!("engine-{}", &ENGINE.sha256[..12]))
    })
}

/// The unpacked server program, if it's there.
pub fn server() -> Option<PathBuf> {
    engine_dir()
        .map(|d| d.join(ENGINE.server))
        .filter(|p| p.is_file())
}

fn model_complete(model: &Model) -> bool {
    path(model)
        .and_then(|p| std::fs::metadata(p).ok())
        .is_some_and(|m| m.len() == model.bytes)
}

/// Model and engine both there. Checksums were checked when downloading; the size catches a cut-off copy.
pub fn is_downloaded(model: &Model) -> bool {
    server().is_some() && model_complete(model)
}

/// What's left to download for `model`, engine included.
pub fn missing_bytes(model: &Model) -> u64 {
    let engine = if server().is_some() { 0 } else { ENGINE.bytes };
    let weights = if model_complete(model) {
        0
    } else {
        model.bytes
    };
    engine + weights
}

/// Download whatever is missing of the engine (from llama.cpp's GitHub releases) and `model` (from
/// Qwen's own Hugging Face repo), reporting bytes done across both as it goes.
/// Stops and cleans up when `cancel` is set. Kept only when size and SHA-256 match.
pub fn download(
    model: &Model,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf, String> {
    let dest = path(model).ok_or("Can't find a folder for the AI model on this PC.")?;
    let mut done_before = 0;
    if server().is_none() {
        let engine = engine_dir().ok_or("Can't find a folder for the AI engine on this PC.")?;
        install_engine(&ENGINE, &engine, cancel, &mut progress)?;
        done_before = ENGINE.bytes;
    }
    if model_complete(model) {
        return Ok(dest);
    }
    let mut progress = |done| progress(done_before + done);
    let folder = dest.parent().expect("model path has a folder");
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("Can't create {}: {e}", folder.display()))?;
    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        model.repo, model.file
    );
    let part = dest.with_extension("gguf.part");
    // shortcut: a stopped download starts over; add HTTP range resume if people on slow lines ask.
    let fetched = fetch(
        &url,
        model.bytes,
        model.sha256,
        &part,
        cancel,
        &mut progress,
    );
    if fetched.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    fetched?;
    std::fs::rename(&part, &dest).map_err(|e| format!("Couldn't save the AI model: {e}"))?;
    Ok(dest)
}

/// Download, check and unpack `engine` into `dest`. Unpacks beside it first, so a half-unpacked
/// engine never counts as installed.
fn install_engine(
    engine: &Engine,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64),
) -> Result<(), String> {
    let parent = dest.parent().expect("engine folder has a parent");
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Can't create {}: {e}", parent.display()))?;
    let archive = dest.with_extension("download");
    let fetched = fetch(
        engine.url,
        engine.bytes,
        engine.sha256,
        &archive,
        cancel,
        progress,
    );
    if fetched.is_err() {
        let _ = std::fs::remove_file(&archive);
    }
    fetched?;
    let unpacking = dest.with_extension("unpacking");
    let _ = std::fs::remove_dir_all(&unpacking);
    let unpacked = std::fs::create_dir_all(&unpacking).is_ok()
        && tar()
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&unpacking)
            .status()
            .is_ok_and(|s| s.success());
    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_dir_all(dest);
    if !unpacked || std::fs::rename(&unpacking, dest).is_err() {
        let _ = std::fs::remove_dir_all(&unpacking);
        return Err("Couldn't unpack the AI engine. Try again.".into());
    }
    Ok(())
}

/// Windows' own tar (built in since Windows 10 1803) unpacks zip files. Named by full path so no
/// other tar.exe on the PATH is used, and started without a console window.
fn tar() -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let mut tar = Command::new(Path::new(&windows).join("System32").join("tar.exe"));
        tar.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        tar
    }
    #[cfg(not(windows))]
    Command::new("tar")
}

/// Stream `url` into `part`, reporting bytes done, and keep it only if it is exactly `bytes` long with
/// SHA-256 `sha256`. Used for the AI model and for app updates.
#[allow(dead_code)]
pub fn fetch(
    url: &str,
    bytes: u64,
    sha256: &str,
    part: &Path,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64),
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        // A stalled connection fails instead of hanging forever.
        .timeout_per_read(Some(Duration::from_secs(60)))
        .build()
        .into();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("Couldn't start the download: {e}"))?;
    let mut body = response.into_body().into_reader();
    let mut file =
        std::fs::File::create(part).map_err(|e| format!("Couldn't write the download: {e}"))?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Skipped.".into());
        }
        let n = body
            .read(&mut buf)
            .map_err(|e| format!("The download stopped: {e}"))?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > bytes {
            return Err("The download was bigger than expected, so it wasn't kept.".into());
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| format!("Couldn't write the download (is the disk full?): {e}"))?;
        progress(done);
    }
    file.sync_all()
        .map_err(|e| format!("Couldn't write the download: {e}"))?;
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if done != bytes || !got.eq_ignore_ascii_case(sha256) {
        return Err(
            "The download didn't match the published file, so it wasn't kept. Try again.".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A one-request web server that answers with `body`.
    fn serve(body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.gguf", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        url
    }

    // SHA-256 of "hello".
    const HELLO: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    #[test]
    fn keeps_a_download_that_matches_and_refuses_one_that_does_not() {
        let dir = std::env::temp_dir().join(format!("gigeyes-model-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("m.part");
        let no = AtomicBool::new(false);
        let mut seen = 0;
        fetch(&serve(b"hello"), 5, HELLO, &part, &no, &mut |d| seen = d).unwrap();
        assert_eq!(
            (std::fs::read(&part).unwrap(), seen),
            (b"hello".to_vec(), 5)
        );

        let tampered = fetch(&serve(b"jello"), 5, HELLO, &part, &no, &mut |_| {});
        assert!(tampered.is_err(), "a changed file is refused");
        let skipped = fetch(
            &serve(b"hello"),
            5,
            HELLO,
            &part,
            &AtomicBool::new(true),
            &mut |_| {},
        );
        assert_eq!(skipped, Err("Skipped.".into()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unpacks_a_checked_engine_and_finds_its_server() {
        use sha2::{Digest, Sha256};
        let dir = std::env::temp_dir().join(format!("gigeyes-engine-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src").join("build")).unwrap();
        std::fs::write(dir.join("src").join("build").join("llama-server"), "x").unwrap();
        let archive = dir.join("engine.tar.gz");
        let packed = tar()
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(dir.join("src"))
            .arg("build")
            .status()
            .unwrap();
        assert!(packed.success());
        let bytes: &'static [u8] = std::fs::read(&archive).unwrap().leak();
        let sha: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let engine = Engine {
            url: serve(bytes).leak(),
            bytes: bytes.len() as u64,
            sha256: sha.leak(),
            server: "build/llama-server",
        };
        let dest = dir.join("engine");
        install_engine(&engine, &dest, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(dest.join(engine.server).is_file());
        assert!(
            !dest.with_extension("download").exists(),
            "the archive is cleaned up"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
