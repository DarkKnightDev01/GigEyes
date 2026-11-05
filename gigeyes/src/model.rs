//! The local AI model file: where it's kept, whether it's there, and downloading it.
//! Shared with the setup, which offers the download (the setup includes this file by path).

use crate::hardware::Model;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// `%LOCALAPPDATA%\GigEyes\models`: local, not roaming, because the files are gigabytes.
pub fn dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("GigEyes").join("models"))
}

pub fn path(model: &Model) -> Option<PathBuf> {
    dir().map(|d| d.join(model.file))
}

/// There and complete. The checksum was checked when it was downloaded; the size catches a cut-off copy.
// The setup only downloads; the app only checks.
#[allow(dead_code)]
pub fn is_downloaded(model: &Model) -> bool {
    path(model)
        .and_then(|p| std::fs::metadata(p).ok())
        .is_some_and(|m| m.len() == model.bytes)
}

/// Download `model` from Qwen's own Hugging Face repo, reporting bytes done as it goes.
/// Stops and cleans up when `cancel` is set. Kept only when size and SHA-256 match.
#[allow(dead_code)]
pub fn download(
    model: &Model,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf, String> {
    let dest = path(model).ok_or("Can't find a folder for the AI model on this PC.")?;
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
}
