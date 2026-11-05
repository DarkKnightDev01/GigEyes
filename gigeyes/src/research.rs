//! Song research: find sources on the gear used on a song with the user's own Tavily key.
//! Results are cached per song, so a song researched once can be read again offline.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

const TAVILY: &str = "https://api.tavily.com";
/// Where people get a free key: 1,000 searches a month, no card.
pub const KEY_PAGE: &str = "https://app.tavily.com/";
/// Pages that are about the song but never about its gear.
const SKIP_DOMAINS: [&str; 7] = [
    "youtube.com",
    "spotify.com",
    "music.apple.com",
    "genius.com",
    "azlyrics.com",
    "lyrics.com",
    "shazam.com",
];
/// Page text kept per source for the AI step; full pages can be hundreds of kilobytes.
const TEXT_LIMIT: usize = 20_000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Source {
    pub title: String,
    pub url: String,
    /// The search engine's short excerpt.
    pub snippet: String,
    /// The page's cleaned text, cut to TEXT_LIMIT, for the AI step to read.
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Research {
    pub song: String,
    pub sources: Vec<Source>,
    /// Unix seconds when it was searched.
    pub searched_at: u64,
}

/// "Metallica - Enter Sandman (Official Music Video) [HD].m4a" → "Metallica - Enter Sandman".
pub fn song_from_title(title: &str) -> String {
    let mut s = title.to_string();
    if let Some((stem, ext)) = s.rsplit_once('.')
        && (1..=4).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
    {
        s = stem.to_string();
    }
    // Drop bracketed tags like (Official Video), [HD], (Live at Wembley) stays: it names the recording.
    let noise = [
        "official",
        "video",
        "audio",
        "lyric",
        "hd",
        "4k",
        "remaster",
        "visualizer",
        "explicit",
    ];
    for (open, close) in [('(', ')'), ('[', ']')] {
        let mut from = 0;
        while let Some(start) = s[from..].find(open).map(|i| i + from) {
            let Some(len) = s[start..].find(close) else {
                break;
            };
            let inner = s[start + 1..start + len].to_lowercase();
            if noise.iter().any(|n| inner.contains(n)) {
                s.replace_range(start..=start + len, "");
            } else {
                from = start + len; // keep it and look further along
            }
        }
    }
    // Downloaded files often use underscores for spaces: "Artist_-_Song".
    s.replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn query_for(song: &str) -> String {
    format!("{song} guitar tone gear amp pedals settings")
}

#[derive(Deserialize)]
struct TavilyResponse {
    results: Vec<TavilyResult>,
}

#[derive(Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: String,
    raw_content: Option<String>,
}

pub fn search(key: &str, song: &str) -> Result<Research, String> {
    search_at(TAVILY, key, song)
}

fn search_at(base: &str, key: &str, song: &str) -> Result<Research, String> {
    let song = song.trim();
    if song.is_empty() {
        return Err("Type the artist and song first.".into());
    }
    let body = serde_json::json!({
        "query": query_for(song),
        "search_depth": "basic",
        "max_results": 8,
        "include_raw_content": "text",
        "exclude_domains": SKIP_DOMAINS,
    });
    let response = ureq::post(format!("{base}/search"))
        .config()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .header("Authorization", format!("Bearer {}", key.trim()))
        .send_json(&body)
        .map_err(|e| match e {
            ureq::Error::StatusCode(401 | 403) => "Tavily didn't accept that key. Check it on app.tavily.com.".to_string(),
            ureq::Error::StatusCode(429 | 432 | 433) => "Out of Tavily searches for now. The free plan resets each month.".to_string(),
            ureq::Error::StatusCode(code) => format!("Tavily answered with an error ({code}). Try again in a minute."),
            _ => "Couldn't reach Tavily. Check the internet connection; saved research still works offline.".to_string(),
        })?;
    let parsed: TavilyResponse = response
        .into_body()
        .read_json()
        .map_err(|_| "Tavily sent an answer GigEyes couldn't read.".to_string())?;
    let sources = parsed
        .results
        .into_iter()
        // Only web pages: a link GigEyes shows must never open a local file or another program.
        .filter(|r| r.url.starts_with("https://") || r.url.starts_with("http://"))
        .map(|r| Source {
            title: r.title,
            url: r.url,
            snippet: r.content,
            text: r
                .raw_content
                .unwrap_or_default()
                .chars()
                .take(TEXT_LIMIT)
                .collect(),
        })
        .collect();
    let searched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(Research {
        song: song.to_string(),
        sources,
        searched_at,
    })
}

/// `%APPDATA%\GigEyes`: rig, research and settings.
pub fn data_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("GigEyes"))
}

/// A safe file name for a song: letters and digits only, so no typed text can point outside the cache.
fn cache_name(song: &str) -> String {
    let slug: String = song
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let name: String = if slug.is_empty() {
        "untitled".into()
    } else {
        slug.chars().take(80).collect()
    };
    format!("{name}.json")
}

pub fn cached(song: &str) -> Option<Research> {
    let file = data_dir()?.join("research").join(cache_name(song));
    let found: Research = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    // Two long titles can share a cache file name; only return research for this exact song.
    found
        .song
        .eq_ignore_ascii_case(song.trim())
        .then_some(found)
}

/// Write a file whole or not at all: a crash mid-write leaves the old file, never half a new one.
pub fn write_atomic(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    let fresh = path.with_extension("tmp");
    std::fs::write(&fresh, contents)?;
    std::fs::rename(&fresh, path)
}

pub fn save(research: &Research) -> Result<(), String> {
    let dir = data_dir()
        .ok_or("Can't find a settings folder on this PC.")?
        .join("research");
    std::fs::create_dir_all(&dir)
        .and_then(|_| {
            write_atomic(
                &dir.join(cache_name(&research.song)),
                &serde_json::to_string(research).expect("serialises"),
            )
        })
        .map_err(|e| format!("Couldn't save the research: {e}"))
}

/// `settings.json`. Missing fields read as their defaults, so older files still load.
/// shortcut: the Tavily key is plain text in the user's own AppData; move it to Windows Credential Manager before wider release.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub tavily_key: String,
    pub light: bool,
}

pub fn load_settings() -> Settings {
    data_dir()
        .and_then(|dir| std::fs::read_to_string(dir.join("settings.json")).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let dir = data_dir().ok_or("Can't find a settings folder on this PC.")?;
    std::fs::create_dir_all(&dir)
        .and_then(|_| {
            write_atomic(
                &dir.join("settings.json"),
                &serde_json::to_string_pretty(settings).expect("serialises"),
            )
        })
        .map_err(|e| format!("Couldn't save the settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn titles_become_song_names() {
        assert_eq!(
            song_from_title("Metallica - Enter Sandman (Official Music Video) [HD].m4a"),
            "Metallica - Enter Sandman"
        );
        assert_eq!(
            song_from_title("Gojira - Stranded (Live at Wembley).mp4"),
            "Gojira - Stranded (Live at Wembley)"
        );
        assert_eq!(song_from_title("riff 2.0 demo"), "riff 2.0 demo");
        assert_eq!(song_from_title("Pantera_-_Walk_[HD].mp3"), "Pantera - Walk");
        assert_eq!(
            song_from_title("Song (Live) (Official Video)"),
            "Song (Live)"
        );
    }

    #[test]
    fn cache_names_cannot_leave_the_cache_folder() {
        assert_eq!(
            cache_name("..\\..\\Windows/System32"),
            "windows-system32.json"
        );
        assert_eq!(
            cache_name("AC/DC - Back in Black"),
            "ac-dc-back-in-black.json"
        );
        assert_eq!(cache_name("???"), "untitled.json");
    }

    /// A one-request fake Tavily: checks the key and query, answers with `status` and `body`.
    fn fake_tavily(status: &str, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let status = status.to_string();
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut request = vec![0u8; 16_384];
            let mut len = 0;
            // Read until the JSON body has arrived (it ends with '}').
            while !request[..len].ends_with(b"}") {
                len += conn.read(&mut request[len..]).unwrap();
            }
            write!(conn, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len())
                .unwrap();
            String::from_utf8_lossy(&request[..len]).into_owned()
        });
        (base, server)
    }

    #[test]
    fn search_sends_the_key_and_keeps_sources() {
        let (base, server) = fake_tavily(
            "200 OK",
            r#"{"query":"q","results":[{"title":"Rig Rundown","url":"https://example.com/rig","content":"He used a 5150.","score":0.9,"raw_content":"Full page text"},{"title":"Bad","url":"file:///C:/Windows/notepad.exe","content":"x","score":0.1}]}"#,
        );
        let research = search_at(&base, " tvly-test ", "Gojira - Stranded").unwrap();
        assert_eq!(research.sources.len(), 1, "the file: link is dropped");
        let request = server.join().unwrap();
        assert!(
            request.contains("Bearer tvly-test"),
            "key sent trimmed as a Bearer token"
        );
        assert!(request.contains("Gojira - Stranded guitar tone"));
        assert_eq!(research.sources[0].snippet, "He used a 5150.");
        assert_eq!(research.sources[0].text, "Full page text");
    }

    #[test]
    fn a_bad_key_gets_a_plain_message() {
        let (base, server) = fake_tavily("401 Unauthorized", r#"{"detail":"invalid"}"#);
        let err = search_at(&base, "nope", "Some Song").unwrap_err();
        server.join().unwrap();
        assert!(err.starts_with("Tavily didn't accept that key"), "{err}");
    }
}
