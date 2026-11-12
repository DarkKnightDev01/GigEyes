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

const PLUGIN_PROMPT: &str = "You read one web page about an audio plugin. Report what the page says about this plugin.\n\
- type: what kind of plugin it is, from the list; \"unknown\" if the page doesn't say\n\
- type_quote: one sentence copied word for word from the page that shows its type, or an empty string\n\
- facts: up to 8 short facts about how this plugin sounds, what it does to the signal and how it behaves, each at most 12 words, in plain words, with quote: the sentence from the page it comes from, copied word for word. Only facts that sentence states. No slogans or sales lines, nothing about the price, the company, presets or how easy it is to use.\n\
- found: the real-world hardware amps, cabinets, pedals and effects the page says this plugin models, emulates or is based on. Not software, plugins or capture formats. gear: the real unit's full make and model as the page writes them, not the plugin's own name for it and never a maker name alone; kind: amp, cabinet, pedal or effect; model: the plugin's own name for that model if the page gives one, otherwise an empty string; quote: one sentence copied word for word from the page that names the real unit.\n\
Never reword, shorten or fix a quote. Leave out prices, system requirements, other products, and anything you aren't sure the page says about this plugin. If the page is about a different product with the same name, or says nothing about this plugin, return unknown and empty lists.";

/// The kinds of plugin the AI may name.
const PLUGIN_TYPES: [&str; 20] = [
    "amp sim",
    "preamp",
    "cabinet or IR loader",
    "overdrive",
    "distortion",
    "fuzz",
    "boost",
    "compressor",
    "EQ",
    "noise gate",
    "delay",
    "reverb",
    "modulation",
    "pitch",
    "wah or filter",
    "multi-effect suite",
    "tuner",
    "utility",
    "other",
    "unknown",
];
/// `Finding::kind` of a plugin's type (its `gear` is one of PLUGIN_TYPES) and of a fact about
/// what it does (its `gear` is the fact).
pub const TYPE: &str = "type";
pub const FACT: &str = "fact";

/// The local AI, loaded and ready, for as long as `session` runs its work.
pub struct Session<'a> {
    base: &'a str,
    cancel: &'a AtomicBool,
}

impl Session<'_> {
    /// Read every page of `research` that has text. `page` is set to the page being read.
    pub fn read(
        &self,
        research: &Research,
        task: Task,
        page: &AtomicUsize,
    ) -> Result<Vec<Finding>, String> {
        read_pages(self.base, research, task, page, self.cancel)
    }
}

/// Starts the server, runs `work` once the model is loaded, and stops the server again: several
/// reads in one `work` load the model once. Setting `cancel` stops at once.
pub fn session<R>(
    model: &Model,
    gpu: bool,
    cancel: &AtomicBool,
    work: impl FnOnce(&Session) -> R,
) -> Result<R, String> {
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
        let result = wait_ready(&server, cancel).map(|()| {
            work(&Session {
                base: &server.base,
                cancel,
            })
        });
        done.store(true, Ordering::Relaxed);
        result
    })
}

/// One read with its own session. `page` is 0 while the model loads.
pub fn read(
    model: &Model,
    gpu: bool,
    research: &Research,
    task: Task,
    page: &AtomicUsize,
    cancel: &AtomicBool,
) -> Result<Vec<Finding>, String> {
    session(model, gpu, cancel, |ai| ai.read(research, task, page))?
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
    let child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't start the local AI: {e}"))?;
    #[cfg(windows)]
    end_with_gigeyes(&child);
    Ok(Server {
        child: Mutex::new(child),
        base: format!("http://127.0.0.1:{port}"),
    })
}

/// Put the server in a Windows job that ends its processes when GigEyes exits, however it exits:
/// closed mid-read, updated or crashed, the AI never stays behind holding memory.
#[cfg(windows)]
pub(crate) fn end_with_gigeyes(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    // One job for the app's lifetime. Its handle is never closed, so Windows closes it when
    // GigEyes exits, and that is what ends the server.
    static JOB: OnceLock<Option<usize>> = OnceLock::new();
    let job = JOB.get_or_init(|| {
        // SAFETY: plain Win32 calls; `info` is a correctly sized, zeroed struct that outlives the call.
        unsafe {
            let job = CreateJobObjectW(None, windows::core::PCWSTR::null()).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .ok()?;
            Some(job.0 as usize)
        }
    });
    if let Some(job) = *job {
        // SAFETY: both handles are valid: the job lives for the whole run, the child is ours.
        // If it fails the server just isn't tied to GigEyes; reading still works.
        let _ = unsafe {
            AssignProcessToJobObject(HANDLE(job as *mut _), HANDLE(child.as_raw_handle()))
        };
    }
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
    let (mut asked, mut failed) = (0, 0);
    for (i, source) in pages.iter().enumerate() {
        page.store(i + 1, Ordering::Relaxed);
        let text = norm(&source.text);
        // A long page (a plugin's full model list) is read part by part.
        for part in parts(&source.text) {
            asked += 1;
            match ask(&agent, base, &research.song, task, &source.title, part) {
                Ok(list) => {
                    for finding in list {
                        if let Some(f) = keep(finding, &text, &source.url)
                            && (task == Task::Song
                                || matches!(f.kind.as_str(), TYPE | FACT)
                                || crate::knowledge::real_gear(&f.gear))
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
    }
    if failed > 0 && failed == asked {
        return Err(
            "The local AI couldn't read the pages. What it said is in ai.log in %LOCALAPPDATA%\\GigEyes."
                .into(),
        );
    }
    Ok(found)
}

/// As much of a page as the model reads in one go: with the prompt and the answer it fits the
/// 12,288-token context.
const PART: usize = 20_000;

/// `text` in pieces of at most PART characters, cut at a line end where there is one.
fn parts(text: &str) -> Vec<&str> {
    let mut parts = vec![];
    let mut rest = text;
    while let Some((end, _)) = rest.char_indices().nth(PART) {
        let cut = rest[..end]
            .rfind('\n')
            .filter(|&at| at > end / 2)
            .map_or(end, |at| at + 1);
        parts.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    if !rest.trim().is_empty() {
        parts.push(rest);
    }
    parts
}

fn ask(
    agent: &ureq::Agent,
    base: &str,
    subject: &str,
    task: Task,
    title: &str,
    text: &str,
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
    let mut schema = serde_json::json!({
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
    if task == Task::Plugin {
        schema["properties"]["type"] = serde_json::json!({"type": "string", "enum": PLUGIN_TYPES});
        schema["properties"]["type_quote"] = serde_json::json!({"type": "string"});
        schema["properties"]["facts"] = serde_json::json!({"type": "array", "maxItems": 8, "items": {
            "type": "object",
            "properties": {"fact": {"type": "string"}, "quote": {"type": "string"}},
            "required": ["fact", "quote"]
        }});
        schema["required"] = serde_json::json!(["type", "type_quote", "facts", "found"]);
    }
    let content = chat(
        agent,
        base,
        prompt,
        &format!("{label}: {subject}\nPage: {title}\n\n{text}"),
        schema,
        tokens,
    )?;
    let content = content.as_str();
    #[derive(Deserialize)]
    struct Fact {
        fact: String,
        quote: String,
    }
    #[derive(Deserialize)]
    struct Answer {
        #[serde(default, rename = "type")]
        kind: String,
        #[serde(default)]
        type_quote: String,
        #[serde(default)]
        facts: Vec<Fact>,
        found: Vec<Finding>,
    }
    let answer: Answer = serde_json::from_str(content).map_err(|e| e.to_string())?;
    // The type and the facts travel as findings too, so the same quote check covers them.
    let note = |gear: String, kind: &str, quote: String| Finding {
        gear,
        kind: kind.into(),
        settings: String::new(),
        quote,
        url: String::new(),
        model: String::new(),
    };
    let mut all = vec![];
    if PLUGIN_TYPES.contains(&answer.kind.as_str()) && answer.kind != "unknown" {
        all.push(note(answer.kind, TYPE, answer.type_quote));
    }
    all.extend(
        answer
            .facts
            .into_iter()
            .map(|f| note(f.fact, FACT, f.quote)),
    );
    all.extend(answer.found);
    Ok(all)
}

/// One question to the model; the answer is JSON of `schema`'s shape.
fn chat(
    agent: &ureq::Agent,
    base: &str,
    system: &str,
    user: &str,
    schema: serde_json::Value,
    tokens: usize,
) -> Result<String, String> {
    let body = serde_json::json!({
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
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
    reply["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "no answer".into())
}

const KNOBS_PROMPT: &str = "You read part of a web page about an audio plugin. List the controls of one model in it: the knobs, switches and sliders a player sets, named as the page names them.\n\
- name: the control's name as the page writes it, e.g. \"Presence\"\n\
- quote: the sentence or line copied word for word from the page that names this control of this model\n\
Only this model's controls, not those of other models or of the plugin as a whole (no presets, tuner, input or output menus). If this part of the page doesn't list them, return an empty list.";

/// The page around each mention of `name`, up to one part's worth: a manual is long and the
/// model's controls are listed next to its name.
fn around<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    const REACH: usize = 3000;
    let (lower, name) = (text.to_ascii_lowercase(), name.to_ascii_lowercase());
    if name.trim().is_empty() {
        return vec![];
    }
    let edge = |mut at: usize| {
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    let mut spans: Vec<(usize, usize)> = vec![];
    for (at, _) in lower.match_indices(&name) {
        let (start, end) = (
            edge(at.saturating_sub(REACH / 3)),
            edge((at + REACH).min(text.len())),
        );
        match spans.last_mut() {
            Some(last) if start <= last.1 => last.1 = end,
            _ => spans.push((start, end)),
        }
    }
    let mut room = PART;
    spans
        .into_iter()
        .map_while(|(start, end)| {
            if room == 0 {
                return None;
            }
            let end = edge(end.min(start + room));
            room -= end - start;
            Some(&text[start..end])
        })
        .collect()
}

impl Session<'_> {
    /// The controls of `model` in `plugin`, from the first of `pages` that lists at least two,
    /// with that page's address. Each name must be on the page in its quote.
    pub fn knobs(
        &self,
        pages: &Research,
        plugin: &str,
        model: &str,
    ) -> Result<(Vec<String>, String), String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(900)))
            .build()
            .into();
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"knobs": {"type": "array", "maxItems": 30, "items": {
                "type": "object",
                "properties": {"name": {"type": "string"}, "quote": {"type": "string"}},
                "required": ["name", "quote"]
            }}},
            "required": ["knobs"]
        });
        #[derive(Deserialize)]
        struct Control {
            name: String,
            quote: String,
        }
        #[derive(Deserialize)]
        struct Answer {
            knobs: Vec<Control>,
        }
        let (mut asked, mut failed) = (0, 0);
        for source in &pages.sources {
            let page = norm(&source.text);
            let mut names: Vec<String> = vec![];
            for part in around(&source.text, model) {
                if self.cancel.load(Ordering::Relaxed) {
                    return Err(STOPPED.into());
                }
                asked += 1;
                let user = format!(
                    "Plugin: {plugin}\nModel: {model}\nPage: {}\n\n{part}",
                    source.title
                );
                let answer = chat(&agent, self.base, KNOBS_PROMPT, &user, schema.clone(), 800)
                    .and_then(|a| serde_json::from_str::<Answer>(&a).map_err(|e| e.to_string()));
                let Ok(answer) = answer else {
                    failed += 1;
                    continue;
                };
                for c in answer.knobs {
                    let (name, quote) = (c.name.trim().to_string(), norm(&c.quote));
                    if !name.is_empty()
                        && name.chars().count() <= 30
                        && quote.contains(&norm(&name))
                        && page.contains(&quote)
                        && !names.iter().any(|n| n.eq_ignore_ascii_case(&name))
                    {
                        names.push(name);
                    }
                }
            }
            if names.len() >= 2 {
                return Ok((names, source.url.clone()));
            }
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(STOPPED.into());
        }
        if failed > 0 && failed == asked {
            return Err("The local AI couldn't read the pages.".into());
        }
        Ok((vec![], String::new()))
    }
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
    // A type or a fact is the AI's wording, so its quote needn't repeat it; gear must be named.
    let worded = f.kind == TYPE || f.kind == FACT;
    if quote.chars().count() < 15
        || !(names_it || worded)
        || !page.contains(&quote)
        || f.gear.trim().is_empty()
    {
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
    fn around_keeps_the_text_near_the_model() {
        let text = format!(
            "{}Brit 8000: Pre Amp, Bass, Middle.{}",
            "x".repeat(5000),
            "y".repeat(9000)
        );
        let near = around(&text, "brit 8000");
        assert_eq!(near.len(), 1);
        assert!(near[0].contains("Pre Amp, Bass"));
        assert!(near[0].len() < 4500);
        assert!(around(&text, "Missing").is_empty());
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
            r#"{"type":"amp sim","type_quote":"Helix Native is an amp and effects plugin.",
              "facts":[{"fact":"Runs whole signal chains","quote":"Helix Native is an amp and effects plugin."},
                       {"fact":"Sounds like a tube amp","quote":"This sentence is nowhere on the page."}],
              "found":[
                {"gear":"Mesa/Boogie Dual Rectifier","kind":"amp","model":"Cali Rectifire","quote":"Cali Rectifire is based on a Mesa/Boogie Dual Rectifier."},
                {"gear":"Marshall JCM800","kind":"amp","model":"","quote":"It also has a Marshall JCM800."}]}"#,
        ]);
        let research = Research {
            song: "Helix Native".into(),
            sources: vec![source(
                "https://line6.com/helix-models/",
                "Helix Native is an amp and effects plugin. Cali Rectifire is based on a Mesa/Boogie Dual Rectifier. More amps.",
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
        let got: Vec<(&str, &str)> = found
            .iter()
            .map(|f| (f.kind.as_str(), f.gear.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (TYPE, "amp sim"),
                (FACT, "Runs whole signal chains"),
                ("amp", "Mesa/Boogie Dual Rectifier")
            ],
            "the made-up fact and the JCM800 have no sentence on the page"
        );
        assert_eq!(found[2].model, "Cali Rectifire");
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

    #[test]
    fn long_pages_are_read_in_parts_cut_at_line_ends() {
        assert_eq!(parts("short page"), ["short page"]);
        assert!(parts("  \n ").is_empty());
        let line = "ä model line, forty characters long ok.\n"; // 40 characters, 41 bytes
        let page = line.repeat(1200); // 48,000 characters
        let cut = parts(&page);
        assert_eq!(cut.len(), 3);
        assert!(
            cut.iter()
                .all(|p| p.chars().count() <= PART && p.ends_with('\n'))
        );
        assert_eq!(cut.concat(), page, "nothing lost or doubled");
        // No line ends at all: still cut, on a character boundary.
        let wall = "ä".repeat(PART + 5);
        assert_eq!(
            parts(&wall)
                .iter()
                .map(|p| p.chars().count())
                .collect::<Vec<_>>(),
            [PART, 5]
        );
    }

    #[test]
    fn every_part_of_a_long_page_is_asked_about() {
        let (base, server) = fake_server(vec![
            r#"{"found":[{"gear":"Marshall JCM800","kind":"amp","model":"Brit 2203","quote":"Brit 2203 is based on the Marshall JCM800."}]}"#,
            r#"{"found":[{"gear":"Klon Centaur","kind":"pedal","model":"Minotaur","quote":"Minotaur is based on the Klon Centaur."}]}"#,
        ]);
        let filler = "A line of the page that names nothing.\n".repeat(600);
        let text = format!(
            "Brit 2203 is based on the Marshall JCM800.\n{filler}Minotaur is based on the Klon Centaur.\n"
        );
        let research = Research {
            song: "Helix Native".into(),
            sources: vec![source("https://line6.com/helix-models/", &text)],
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
        let gear: Vec<&str> = found.iter().map(|f| f.gear.as_str()).collect();
        assert_eq!(
            gear,
            ["Marshall JCM800", "Klon Centaur"],
            "one from each part"
        );
        server.join().unwrap();
    }
}
