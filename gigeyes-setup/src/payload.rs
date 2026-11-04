//! The files the installer carries, appended after the setup program itself:
//!
//! `[setup program][file bytes…][index JSON][index length: u64 LE][stub length: u64 LE][MAGIC]`
//!
//! Windows ignores data after a program's last section, so the setup still runs. The uninstaller
//! is the first `stub length` bytes alone: the same program without the 40 MB of app it installed.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

const MAGIC: &[u8; 16] = b"GIGEYES-PAYLOAD1";
const TRAILER: u64 = 8 + 8 + 16;

/// One packed file: its name and where its bytes sit in the setup program.
#[derive(Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub offset: u64,
    pub len: u64,
}

/// What's packed into `setup`, plus the length of the program in front of it. None when nothing is packed.
pub fn read(setup: &Path) -> io::Result<Option<(Vec<Entry>, u64)>> {
    let mut file = File::open(setup)?;
    let size = file.metadata()?.len();
    if size < TRAILER {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(size - TRAILER))?;
    let mut trailer = [0u8; TRAILER as usize];
    file.read_exact(&mut trailer)?;
    if &trailer[16..] != MAGIC {
        return Ok(None);
    }
    let index_len = u64::from_le_bytes(trailer[..8].try_into().expect("8 bytes"));
    let stub_len = u64::from_le_bytes(trailer[8..16].try_into().expect("8 bytes"));
    let index_start = (size - TRAILER)
        .checked_sub(index_len)
        .ok_or_else(|| bad("index longer than the file"))?;
    file.seek(SeekFrom::Start(index_start))?;
    let mut index = vec![0u8; index_len as usize];
    file.read_exact(&mut index)?;
    let pairs = parse_index(&index).ok_or_else(|| bad("unreadable index"))?;
    let mut offset = stub_len;
    let mut entries = vec![];
    for (name, len) in pairs {
        // Plain file names only, so a tampered setup can't write outside the install folder.
        if name.is_empty() || name.contains(['/', '\\', ':']) || name.starts_with('.') {
            return Err(bad("unsafe file name in payload"));
        }
        entries.push(Entry { name, offset, len });
        offset = offset
            .checked_add(len)
            .ok_or_else(|| bad("payload sizes overflow"))?;
    }
    if offset != index_start {
        return Err(bad("payload sizes don't add up"));
    }
    Ok(Some((entries, stub_len)))
}

/// Copy one packed file out of `setup` into `to`.
pub fn extract(setup: &Path, entry: &Entry, to: &Path) -> io::Result<()> {
    let mut file = File::open(setup)?;
    file.seek(SeekFrom::Start(entry.offset))?;
    let mut out = File::create(to)?;
    let copied = io::copy(&mut file.take(entry.len), &mut out)?;
    if copied != entry.len {
        return Err(bad("setup file is cut short"));
    }
    out.sync_all()
}

/// Write the setup program alone (no payload) to `to`: the uninstaller.
pub fn write_stub(setup: &Path, stub_len: u64, to: &Path) -> io::Result<()> {
    let mut out = File::create(to)?;
    io::copy(&mut File::open(setup)?.take(stub_len), &mut out)?;
    out.sync_all()
}

/// Build a setup: `stub` (the compiled setup program) followed by `files`, written to `out`.
pub fn pack(stub: &Path, files: &[&Path], out: &Path) -> io::Result<()> {
    let mut writer = File::create(out)?;
    let stub_len = io::copy(&mut File::open(stub)?, &mut writer)?;
    let mut index = vec![];
    for path in files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| bad("file without a name"))?;
        let len = io::copy(&mut File::open(path)?, &mut writer)?;
        index.push(format!("[\"{name}\",{len}]"));
    }
    let index = format!("[{}]", index.join(","));
    writer.write_all(index.as_bytes())?;
    writer.write_all(&(index.len() as u64).to_le_bytes())?;
    writer.write_all(&stub_len.to_le_bytes())?;
    writer.write_all(MAGIC)?;
    writer.sync_all()
}

fn bad(why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("This setup file is damaged ({why}). Download it again."),
    )
}

/// The index is a tiny JSON list of `["name",length]` pairs written by `pack`; names are plain file names.
fn parse_index(bytes: &[u8]) -> Option<Vec<(String, u64)>> {
    let inner = std::str::from_utf8(bytes)
        .ok()?
        .trim()
        .strip_prefix('[')?
        .strip_suffix(']')?;
    if inner.is_empty() {
        return Some(vec![]);
    }
    inner
        .split("],")
        .map(|item| {
            let (name, len) = item
                .trim_start_matches('[')
                .trim_end_matches(']')
                .rsplit_once(',')?;
            Some((
                name.trim()
                    .strip_prefix('"')?
                    .strip_suffix('"')?
                    .to_string(),
                len.trim().parse().ok()?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gigeyes-payload-test-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn pack_then_read_back_every_file_and_the_bare_stub() {
        let dir = temp("roundtrip");
        std::fs::write(dir.join("setup-stub.exe"), b"MZ-stub-program").unwrap();
        std::fs::write(dir.join("gigeyes.exe"), b"the app").unwrap();
        std::fs::write(dir.join("yt-dlp.exe"), b"downloader, a bit longer").unwrap();
        let setup = dir.join("GigEyes-Setup.exe");
        pack(
            &dir.join("setup-stub.exe"),
            &[&dir.join("gigeyes.exe"), &dir.join("yt-dlp.exe")],
            &setup,
        )
        .unwrap();

        let (entries, stub_len) = read(&setup).unwrap().expect("payload found");
        assert_eq!(stub_len, 15);
        assert_eq!(
            entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["gigeyes.exe", "yt-dlp.exe"]
        );
        extract(&setup, &entries[1], &dir.join("out.exe")).unwrap();
        assert_eq!(
            std::fs::read(dir.join("out.exe")).unwrap(),
            b"downloader, a bit longer"
        );
        write_stub(&setup, stub_len, &dir.join("uninstall.exe")).unwrap();
        assert_eq!(
            std::fs::read(dir.join("uninstall.exe")).unwrap(),
            b"MZ-stub-program"
        );
        assert_eq!(
            read(&dir.join("uninstall.exe")).unwrap(),
            None,
            "the uninstaller carries nothing"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_cut_short_download_is_reported_not_installed() {
        let dir = temp("damaged");
        std::fs::write(dir.join("stub"), b"stub").unwrap();
        std::fs::write(dir.join("app.exe"), vec![7u8; 1000]).unwrap();
        pack(
            &dir.join("stub"),
            &[&dir.join("app.exe")],
            &dir.join("setup"),
        )
        .unwrap();
        let mut bytes = std::fs::read(dir.join("setup")).unwrap();
        bytes.drain(10..400); // lose part of the app, keep the trailer
        std::fs::write(dir.join("setup"), bytes).unwrap();
        assert!(read(&dir.join("setup")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_tampered_name_cannot_escape_the_install_folder() {
        assert_eq!(
            parse_index(br#"[["..\\evil.exe",3]]"#).unwrap()[0].0,
            "..\\\\evil.exe"
        );
        let dir = temp("tamper");
        let mut setup = b"stub".to_vec();
        setup.extend(b"abc");
        let index = br#"[["../evil.exe",3]]"#;
        setup.extend(index);
        setup.extend((index.len() as u64).to_le_bytes());
        setup.extend(4u64.to_le_bytes());
        setup.extend(MAGIC);
        std::fs::write(dir.join("setup"), setup).unwrap();
        assert!(read(&dir.join("setup")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
