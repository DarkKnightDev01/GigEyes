//! AI reading: the local model reads each saved research page and lists the gear and settings it
//! names. A finding is kept only when its quote is on the page word for word, so the AI can't make
//! gear up. llama.cpp's server runs only while reading, then quits and frees the memory.

use crate::hardware::Model;
use crate::model;
use crate::research::{Research, Source};
use serde::{Deserialize, Serialize};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Finding {
    /// Make and model as the page writes it, e.g. "Marshall JCM800 2203".
    pub gear: String,
    /// guitar, pickups, amp, cabinet, pedal, effect, strings, tuning or other.
    pub kind: String,
    /// Knob positions or other settings the page gives, or empty.
    #[serde(default)]
    pub settings: String,
    /// A sentence from the page that names the gear.
    pub quote: String,
    /// The page it came from; filled in by GigEyes, not the AI.
    #[serde(default)]
    pub url: String,
    /// For a plugin: the plugin's own name for this model, e.g. "Cali Rectifire"; may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
}

/// What the AI is reading the pages for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Task {
    /// The gear used on a song: `Research::song` is the song.
    Song,
    /// The real gear a plugin models: `Research::song` is the plugin's name.
    Plugin,
}

pub const STOPPED: &str = "Stopped.";

const PROMPT: &str = "You read one web page about the guitar sound of a song. List the guitar gear the page says was used on that song, its album or its tour: guitars, pickups, amps, cabinets, pedals, effects, strings and tuning.\n\
For each piece of gear give:\n\
- gear: make and model as the page writes them\n\
- kind: one of the listed kinds\n\
- settings: knob positions or other settings the page gives for it, or an empty string if it gives none\n\
- quote: one sentence copied word for word from the page that names this gear. Never reword, shorten or fix it.\n\
Leave out gear the page doesn't tie to this song, album or tour, gear used only by the bass player, singer or drummer, and anything you aren't sure the page says. If the page names no gear, return an empty list.";

const PLUGIN_PROMPT: &str = "You read one web page about an audio plugin. List the real-world amps, cabinets, pedals and effects the page says this plugin models, emulates or is based on.\n\
For each one give:\n\
- gear: the real unit's make and model as the page writes them, e.g. \"Mesa/Boogie Dual Rectifier\", not the plugin's own name for it\n\
- kind: amp, cabinet, pedal or effect\n\
- model: the plugin's own name for that model if the page gives one, otherwise an empty string\n\
- quote: one sentence copied word for word from the page that names the real unit. Never reword, shorten or fix it.\n\
Leave out gear the page doesn't say this plugin models, other products, and anything you aren't sure the page says. If the page names none, return an empty list.";

/// Starts the server, reads every page that has text, and stops the server again.
/// `page` is set to the page being read (0 while the model loads). Setting `cancel` stops at once.
pub fn read(
    model: &Model,
    gpu: bool,
    research: &Research,
    task: Task,
    page: &AtomicUsize,
    cancel: &AtomicBool,
) -> Result<Vec<Finding>, String> {
    let server = start(model, gpu)?;
    let done = AtomicBool::new(false);
    std::thread::scope(|s| {
        // Stop means stop: end the server mid-page instead of waiting minutes for a slow processor.
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                if cancel.load(Ordering::Relaxed) {
                    let _ = server.child.lock().map(|mut c| c.kill());
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        let result = wait_ready(&server, cancel)
            .and_then(|()| read_pages(&server.base, research, task, page, cancel));
        done.store(true, Ordering::Relaxed);
        result
    })
}

struct Server {
    child: Mutex<Child>,
    base: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// `%LOCALAPPDATA%\GigEyes\ai.log`: what the server printed last time, for when something goes wrong.
fn log_path() -> Option<std::path::PathBuf> {
    model::dir().and_then(|d| d.parent().map(|p| p.join("ai.log")))
}

fn start(model: &Model, gpu: bool) -> Result<Server, String> {
    let missing = "The local AI isn't downloaded yet. Get it in Settings.";
    let exe = model::server().ok_or(missing)?;
    let weights = model::path(model).filter(|p| p.is_file()).ok_or(missing)?;
    // Let the system pick a free port, then hand it to the server.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_err(|e| format!("Couldn't start the local AI: {e}"))?
        .port();
    let mut cmd = Command::new(exe);
    cmd.arg("-m").arg(weights);
    cmd.args(["--host", "127.0.0.1", "--port", &port.to_string()]);
    // Room for a 20,000-character page plus the answer. A q8_0 cache halves its memory with no
    // visible loss, which keeps 8B inside an 8 GB card next to Gig Performer.
    cmd.args([
        "-c", "12288", "-np", "1", "-fa", "on", "-ctk", "q8_0", "-ctv", "q8_0",
    ]);
    cmd.args(["--reasoning", "off"]);
    cmd.args(if gpu {
        ["-ngl", "all"]
    } else {
        ["-dev", "none"]
    });
    if let Some(log) = log_path() {
        cmd.arg("--log-file").arg(log);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    // shortcut: if GigEyes itself crashes mid-read the server keeps running until closed in Task
    // Manager; tie it to a Windows job object if that ever happens.
    let child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't start the local AI: {e}"))?;
    Ok(Server {
        child: Mutex::new(child),
        base: format!("http://127.0.0.1:{port}"),
    })
}

/// Wait for the model to load: seconds from a fast drive, a minute or two from a slow one.
fn wait_ready(server: &Server, cancel: &AtomicBool) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(2)))
        .build()
        .into();
    let until = Instant::now() + Duration::from_secs(300);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(STOPPED.into());
        }
        let exited = server
            .child
            .lock()
            .map_or(true, |mut c| !matches!(c.try_wait(), Ok(None)));
        if exited {
            return Err(
                "The local AI stopped while loading. What it said is in ai.log in %LOCALAPPDATA%\\GigEyes."
                    .into(),
            );
        }
        // /health answers 503 while loading and 200 once ready; ureq treats 503 as an error.
        if agent.get(format!("{}/health", server.base)).call().is_ok() {
            return Ok(());
        }
        if Instant::now() > until {
            return Err("The local AI took over five minutes to load, so GigEyes gave up.".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn read_pages(
    base: &str,
    research: &Research,
    task: Task,
    page: &AtomicUsize,
    cancel: &AtomicBool,
) -> Result<Vec<Finding>, String> {
    // A page on the processor can take minutes; Stop ends it sooner by ending the server.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(900)))
        .build()
        .into();
    let pages: Vec<&Source> = research
        .sources
        .iter()
        .filter(|s| !s.text.trim().is_empty())
        .collect();
    let mut found: Vec<Finding> = Vec::new();
    let mut failed = 0;
    for (i, source) in pages.iter().enumerate() {
        page.store(i + 1, Ordering::Relaxed);
        match ask(&agent, base, &research.song, task, source) {
            Ok(list) => {
                let text = norm(&source.text);
                for finding in list {
                    if let Some(f) = keep(finding, &text, &source.url)
                        && !found.iter().any(|g| same(g, &f))
                    {
                        found.push(f);
                    }
                }
            }
            Err(_) if cancel.load(Ordering::Relaxed) => return Err(STOPPED.into()),
            Err(_) => failed += 1,
        }
    }
    if failed > 0 && failed == pages.len() {
        return Err(
            "The local AI couldn't read the pages. What it said is in ai.log in %LOCALAPPDATA%\\GigEyes."
                .into(),
        );
    }
    Ok(found)
}

fn ask(
    agent: &ureq::Agent,
    base: &str,
    subject: &str,
    task: Task,
    source: &Source,
) -> Result<Vec<Finding>, String> {
    // The server only lets the model write JSON of this shape.
    let (prompt, label, kinds, detail, max, tokens) = match task {
        Task::Song => (
            PROMPT,
            "Song",
            &[
                "guitar", "pickups", "amp", "cabinet", "pedal", "effect", "strings", "tuning",
                "other",
            ][..],
            "settings",
            15,
            1500,
        ),
        // A suite's model list can be long: room for 60 models.
        Task::Plugin => (
            PLUGIN_PROMPT,
            "Plugin",
            &["amp", "cabinet", "pedal", "effect"][..],
            "model",
            60,
            4000,
        ),
    };
    let schema = serde_json::json!({
        "type": "object",
        "properties": {"found": {"type": "array", "maxItems": max, "items": {
            "type": "object",
            "properties": {
                "gear": {"type": "string"},
                "kind": {"type": "string", "enum": kinds},
                detail: {"type": "string"},
                "quote": {"type": "string"}
            },
            "required": ["gear", "kind", detail, "quote"]
        }}},
        "required": ["found"]
    });
    let body = serde_json::json!({
        "messages": [
            {"role": "system", "content": prompt},
            {"role": "user", "content": format!("{label}: {subject}\nPage: {}\n\n{}", source.title, source.text)},
        ],
        "temperature": 0.2,
        "max_tokens": tokens,
        "response_format": {"type": "json_object", "schema": schema},
        "chat_template_kwargs": {"enable_thinking": false},
    });
    let reply: serde_json::Value = agent
        .post(format!("{base}/v1/chat/completions"))
        .send_json(&body)
        .and_then(|mut r| r.body_mut().read_json())
        .map_err(|e| e.to_string())?;
    let content = reply["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("no answer")?;
    #[derive(Deserialize)]
    struct Answer {
        found: Vec<Finding>,
    }
    serde_json::from_str::<Answer>(content)
        .map(|a| a.found)
        .map_err(|e| e.to_string())
}

/// Lowercase, one space between words, and plain quote marks and dashes, so a quote still matches a
/// page that uses curly ones.
fn norm(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '`' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            '\u{2013}' | '\u{2014}' => '-',
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Kept only if the quote is on the page (`page` already normalised) and names the gear.
fn keep(mut f: Finding, page: &str, url: &str) -> Option<Finding> {
    let quote = norm(&f.quote);
    let names_it = norm(&f.gear)
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word.chars().count() >= 3 && quote.contains(word));
    if quote.chars().count() < 15 || !names_it || !page.contains(&quote) {
        return None;
    }
    f.gear = f.gear.trim().to_string();
    f.settings = f.settings.trim().to_string();
    f.url = url.to_string();
    Some(f)
}

fn same(a: &Finding, b: &Finding) -> bool {
    norm(&a.gear) == norm(&b.gear) && norm(&a.settings) == norm(&b.settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn finding(gear: &str, quote: &str) -> Finding {
        Finding {
            gear: gear.into(),
            kind: "amp".into(),
            settings: String::new(),
            quote: quote.into(),
            url: String::new(),
            model: String::new(),
        }
    }

    #[test]
    fn keeps_only_quotes_that_are_on_the_page_and_name_the_gear() {
        let page = norm(
            "He plugged into a  Marshall JCM800 \u{2014} \u{201C}it\u{2019}s the one\u{201D}, he said.",
        );
        let real = keep(
            finding(
                "Marshall JCM800",
                "plugged into a Marshall JCM800 - \"it's the one\"",
            ),
            &page,
            "https://a",
        );
        assert_eq!(
            real.map(|f| f.url),
            Some("https://a".into()),
            "curly marks and spacing still match"
        );
        assert!(
            keep(
                finding(
                    "Mesa Boogie Mark IIC+",
                    "He used a Mesa Boogie Mark IIC+ live."
                ),
                &page,
                ""
            )
            .is_none(),
            "made up"
        );
        assert!(
            keep(
                finding("Mesa Boogie", "plugged into a Marshall JCM800"),
                &page,
                ""
            )
            .is_none(),
            "quote names other gear"
        );
        assert!(
            keep(finding("Marshall", "Marshall"), &page, "").is_none(),
            "too short to be a quote"
        );
    }

    /// A fake llama-server: answers each chat request with the next reply and keeps the first request body.
    fn fake_server(replies: Vec<&'static str>) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut first = String::new();
            for reply in replies {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                if first.is_empty() {
                    first = String::from_utf8(body).unwrap();
                }
                let json =
                    serde_json::json!({"choices": [{"message": {"content": reply}}]}).to_string();
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    json.len()
                );
                let mut stream = reader.into_inner();
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(json.as_bytes()).unwrap();
            }
            first
        });
        (base, handle)
    }

    fn source(url: &str, text: &str) -> Source {
        Source {
            title: "Rig rundown".into(),
            url: url.into(),
            snippet: String::new(),
            text: text.into(),
        }
    }

    #[test]
    fn reads_each_page_and_drops_what_it_cannot_back_up() {
        let (base, server) = fake_server(vec![
            r#"{"found":[
                {"gear":"Marshall JCM800","kind":"amp","settings":"Gain 7, Bass 4","quote":"He ran a Marshall JCM800 with the gain on 7."},
                {"gear":"Klon Centaur","kind":"pedal","settings":"","quote":"A Klon Centaur pushed the amp hard."}]}"#,
            "not json at all",
        ]);
        let research = Research {
            song: "Band - Song".into(),
            sources: vec![
                source(
                    "https://one",
                    "On the album he ran a Marshall JCM800 with the gain on 7. Nothing else.",
                ),
                source("https://empty", "  "),
                source("https://two", "Some other page."),
            ],
            searched_at: 0,
            findings: None,
        };
        let page = AtomicUsize::new(0);
        let found =
            read_pages(&base, &research, Task::Song, &page, &AtomicBool::new(false)).unwrap();
        assert_eq!(found.len(), 1, "the Klon quote isn't on the page");
        assert_eq!(
            (found[0].settings.as_str(), found[0].url.as_str()),
            ("Gain 7, Bass 4", "https://one")
        );
        assert_eq!(page.load(Ordering::Relaxed), 2, "the empty page is skipped");

        let request: serde_json::Value = serde_json::from_str(&server.join().unwrap()).unwrap();
        assert_eq!(request["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(request["response_format"]["type"], "json_object");
        assert!(
            request["messages"][1]["content"]
                .as_str()
                .unwrap()
                .contains("Song: Band - Song")
        );
    }

    #[test]
    fn a_plugin_lookup_asks_for_models_and_keeps_their_plugin_names() {
        let (base, server) = fake_server(vec![
            r#"{"found":[
                {"gear":"Mesa/Boogie Dual Rectifier","kind":"amp","model":"Cali Rectifire","quote":"Cali Rectifire is based on a Mesa/Boogie Dual Rectifier."},
                {"gear":"Marshall JCM800","kind":"amp","model":"","quote":"It also has a Marshall JCM800."}]}"#,
        ]);
        let research = Research {
            song: "Helix Native".into(),
            sources: vec![source(
                "https://line6.com/helix-models/",
                "Models. Cali Rectifire is based on a Mesa/Boogie Dual Rectifier. More amps.",
            )],
            searched_at: 0,
            findings: None,
        };
        let page = AtomicUsize::new(0);
        let found = read_pages(
            &base,
            &research,
            Task::Plugin,
            &page,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(found.len(), 1, "the JCM800 sentence isn't on the page");
        assert_eq!(
            (found[0].gear.as_str(), found[0].model.as_str()),
            ("Mesa/Boogie Dual Rectifier", "Cali Rectifire")
        );
        let request: serde_json::Value = serde_json::from_str(&server.join().unwrap()).unwrap();
        let user = request["messages"][1]["content"].as_str().unwrap();
        assert!(user.starts_with("Plugin: Helix Native"), "{user}");
        let kinds = &request["response_format"]["schema"]["properties"]["found"]["items"]["properties"]
            ["kind"]["enum"];
        assert_eq!(
            kinds.as_array().unwrap().len(),
            4,
            "plugins model amps, cabs, pedals, effects"
        );
    }
}
