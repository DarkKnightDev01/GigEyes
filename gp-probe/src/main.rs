//! gp-probe: talks to Gig Performer Pro over OSC and prints exactly what goes out and comes back.
//! Used to confirm how GP behaves on a real machine before GigEyes relies on it.

use rosc::{OscMessage, OscPacket, OscType, decoder, encoder};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use std::{env, fs, process, thread};

const USAGE: &str = "\
gp-probe: check how Gig Performer Pro answers OSC.

Usage: gp-probe [options] <command>

Read-only commands (safe):
  check [plugin] [widget]   run the whole checklist; paste this output back
  widgets                   list OSC-enabled widgets   (/RackSpace/ListOSCWidgets)
  plugins                   list OSC-enabled plugins   (/RackSpace/ListOSCPlugins)
  params <plugin> [max]     parameter count and the first [max] values (default 16)
  get <plugin> <index>      one parameter value
  value <widget>            one widget value

Commands that change your rackspace or write files (need --write):
  set <plugin> <index> <0..1>   set a plugin parameter
  setvalue <widget> <0..1>      set a widget
  bypass <plugin> <0|1>         bypass (1) or enable (0) a plugin
  record <seconds>              record with GP's recorder and report the files it writes
  send <address> [args...]      send any OSC message (123 = int, 0.5 = float, text = string)

Options:
  --gp-ip <addr>     GP's address (default 127.0.0.1; if GP doesn't answer, try the
                     \"Gig Performer IP address\" shown in GP's OSC Setup)
  --gp-port <n>      GP's OSC listening port (default 54344)
  --reply-port <n>   port GP sends replies to: GP's \"Remote client port\" (default 54345)
  --wait <ms>        how long to wait after the last reply (default 500)
  --rec-dir <path>   GP's recordings folder (default: Documents\\Gig Performer Recordings)
  --write            allow commands that change things
";

struct Opts {
    gp_ip: IpAddr,
    gp_port: u16,
    reply_port: u16,
    wait: Duration,
    rec_dir: PathBuf,
    write: bool,
    command: Vec<String>,
}

fn main() {
    let sha = option_env!("GITHUB_SHA").map_or("local build", |s| &s[..7.min(s.len())]);
    println!("gp-probe {} ({sha})", env!("CARGO_PKG_VERSION"));
    if let Err(e) = parse_opts(env::args().skip(1).collect()).and_then(|o| run(&o)) {
        eprintln!("\nerror: {e}");
        process::exit(1);
    }
}

fn parse_opts(args: Vec<String>) -> Result<Opts, String> {
    let mut o = Opts {
        gp_ip: Ipv4Addr::LOCALHOST.into(),
        gp_port: 54344,
        reply_port: 54345,
        wait: Duration::from_millis(500),
        rec_dir: dirs::document_dir()
            .unwrap_or_default()
            .join("Gig Performer Recordings"),
        write: false,
        command: vec![],
    };
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--gp-ip" => {
                let v = value(&a)?;
                o.gp_ip = v
                    .parse()
                    .map_err(|_| format!("'{v}' is not an IP address"))?
            }
            "--gp-port" => o.gp_port = number(&value(&a)?)?,
            "--reply-port" => o.reply_port = number(&value(&a)?)?,
            "--wait" => o.wait = Duration::from_millis(number(&value(&a)?)?),
            "--rec-dir" => o.rec_dir = PathBuf::from(value(&a)?),
            "--write" => o.write = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                process::exit(0)
            }
            _ => o.command.push(a),
        }
    }
    if o.command.is_empty() {
        return Err(format!("no command given\n\n{USAGE}"));
    }
    Ok(o)
}

fn number<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse()
        .map_err(|_| format!("'{s}' is not a valid number"))
}

/// A knob value: GP expects 0..1, so anything outside is clamped.
fn unit(s: &str) -> Result<f32, String> {
    let v: f32 = number(s)?;
    if v.is_nan() {
        return Err(format!("'{s}' is not a valid number"));
    }
    Ok(v.clamp(0.0, 1.0))
}

/// Text from the command line to an OSC argument: 123 -> int, 0.5 -> float, anything else -> string.
fn parse_arg(s: &str) -> OscType {
    if let Ok(i) = s.parse::<i32>() {
        OscType::Int(i)
    } else if let Some(f) = s.contains('.').then(|| s.parse::<f32>().ok()).flatten() {
        OscType::Float(f)
    } else {
        OscType::String(s.to_string())
    }
}

fn show(args: &[OscType]) -> String {
    let parts: Vec<String> = args
        .iter()
        .map(|a| match a {
            OscType::Int(v) => format!("int {v}"),
            OscType::Float(v) => format!("float {v}"),
            OscType::Double(v) => format!("double {v}"),
            OscType::String(v) => format!("string {v:?}"),
            other => format!("{other:?}"),
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

struct Gp {
    sock: UdpSocket,
    target: SocketAddr,
    wait: Duration,
}

impl Gp {
    fn connect(target: SocketAddr, reply_port: u16, wait: Duration) -> Result<Gp, String> {
        // GP sends replies to its configured "remote client" port, not back to the sender,
        // so we must own that port and send from it too.
        // Loopback when GP is on loopback: no firewall prompt, no "replies" injected from the
        // network. A loopback-bound socket can't send to GP's network address, so then use all.
        let bind_ip = if target.ip().is_loopback() {
            Ipv4Addr::LOCALHOST
        } else {
            Ipv4Addr::UNSPECIFIED
        };
        let sock = UdpSocket::bind((bind_ip, reply_port)).map_err(|e| {
            format!(
                "can't listen on port {reply_port} ({e}). Another program (or another gp-probe) is \
                 using it. Close it, or pick another port with --reply-port and set the same \
                 number as \"Remote client port\" in GP's OSC Setup."
            )
        })?;
        Ok(Gp { sock, target, wait })
    }

    /// Send one message and print every reply that arrives until GP goes quiet.
    fn ask(&self, addr: &str, args: Vec<OscType>) -> Result<Vec<OscMessage>, String> {
        self.drain();
        println!("\n-> {} {addr} {}", self.target, show(&args));
        let packet = OscPacket::Message(OscMessage {
            addr: addr.to_string(),
            args,
        });
        let bytes = encoder::encode(&packet).map_err(|e| format!("can't encode message: {e:?}"))?;
        let sent = Instant::now();
        self.sock
            .send_to(&bytes, self.target)
            .map_err(|e| format!("send failed: {e}"))?;

        let mut replies = vec![];
        let mut buf = [0u8; 65536];
        self.sock
            .set_read_timeout(Some(self.wait))
            .map_err(|e| e.to_string())?;
        // Hard cap: widgets that push updates nonstop (e.g. meters) would otherwise never let GP go quiet.
        let deadline = self.wait + Duration::from_secs(5);
        while sent.elapsed() < deadline {
            let (n, from) = match self.sock.recv_from(&mut buf) {
                Ok(r) => r,
                // Windows reports this right away when nothing listens on the GP port.
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                    println!(
                        "   Nothing is listening on {}: Gig Performer isn't running, OSC is off, \
                         or GP listens elsewhere (try --gp-ip with the \"Gig Performer IP \
                         address\" from GP's OSC Setup, or check --gp-port).",
                        self.target
                    );
                    return Ok(replies);
                }
                Err(_) => break,
            };
            let ms = sent.elapsed().as_millis();
            match decoder::decode_udp(&buf[..n]) {
                Ok((_, p)) => {
                    let mut msgs = vec![];
                    flatten(p, &mut msgs);
                    for m in msgs {
                        println!("<- {ms:>5} ms from {from}  {} {}", m.addr, show(&m.args));
                        replies.push(m);
                    }
                }
                Err(e) => println!(
                    "<- {ms:>5} ms from {from}  undecodable ({e:?}): {:02x?}",
                    &buf[..n.min(64)]
                ),
            }
        }
        if replies.is_empty() {
            println!(
                "   NO REPLY after {} ms. Check in GP > Options > OSC Setup: OSC enabled, listening \
                 port {}, remote client 127.0.0.1 port {}.",
                self.wait.as_millis(),
                self.target.port(),
                self.sock.local_addr().map(|a| a.port()).unwrap_or(0)
            );
        }
        Ok(replies)
    }

    /// Throw away anything left over (e.g. GP broadcasting widget moves) so replies line up.
    fn drain(&self) {
        let _ = self.sock.set_read_timeout(Some(Duration::from_millis(1)));
        let mut buf = [0u8; 65536];
        while self.sock.recv_from(&mut buf).is_ok() {}
    }
}

fn flatten(p: OscPacket, out: &mut Vec<OscMessage>) {
    match p {
        OscPacket::Message(m) => out.push(m),
        OscPacket::Bundle(b) => b.content.into_iter().for_each(|p| flatten(p, out)),
    }
}

fn first_int(replies: &[OscMessage]) -> Option<i32> {
    replies.iter().flat_map(|m| &m.args).find_map(|a| match a {
        OscType::Int(v) => Some(*v),
        _ => None,
    })
}

fn first_number(replies: &[OscMessage]) -> Option<f64> {
    replies.iter().flat_map(|m| &m.args).find_map(|a| match a {
        OscType::Float(v) => Some(*v as f64),
        OscType::Double(v) => Some(*v),
        _ => None,
    })
}

fn arg<'a>(cmd: &'a [String], i: usize, what: &str) -> Result<&'a str, String> {
    cmd.get(i)
        .map(String::as_str)
        .ok_or(format!("missing {what}. Run gp-probe --help"))
}

fn run(o: &Opts) -> Result<(), String> {
    let cmd = &o.command;
    let writes = ["set", "setvalue", "bypass", "record", "send"];
    if writes.contains(&cmd[0].as_str()) && !o.write {
        return Err(format!(
            "'{}' changes your rackspace or writes files. Add --write to allow it \
             (try it in a throwaway gig first).",
            cmd[0]
        ));
    }
    println!(
        "GP at {}:{}, reply port {}",
        o.gp_ip, o.gp_port, o.reply_port
    );
    let gp = Gp::connect(SocketAddr::new(o.gp_ip, o.gp_port), o.reply_port, o.wait)?;

    match cmd[0].as_str() {
        "check" => check(&gp, cmd.get(1), cmd.get(2))?,
        "widgets" => drop(gp.ask("/RackSpace/ListOSCWidgets", vec![])?),
        "plugins" => drop(gp.ask("/RackSpace/ListOSCPlugins", vec![])?),
        "params" => {
            let max = cmd.get(2).map(|s| number(s)).transpose()?.unwrap_or(16);
            params(&gp, arg(cmd, 1, "plugin handle")?, max)?
        }
        "get" => {
            let index: i32 = number(arg(cmd, 2, "parameter index")?)?;
            drop(gp.ask(
                &format!("/{}/GetParam", arg(cmd, 1, "plugin handle")?),
                vec![OscType::Int(index)],
            )?)
        }
        "value" => drop(gp.ask(
            &format!("/{}/GetValue", arg(cmd, 1, "widget handle")?),
            vec![],
        )?),
        "set" => {
            let (h, index) = (
                arg(cmd, 1, "plugin handle")?,
                number::<i32>(arg(cmd, 2, "parameter index")?)?,
            );
            let value = unit(arg(cmd, 3, "value 0..1")?)?;
            let before =
                first_number(&gp.ask(&format!("/{h}/GetParam"), vec![OscType::Int(index)])?);
            undo_hint(before, &format!("set {h} {index}"));
            gp.ask(
                &format!("/{h}/SetParam"),
                vec![OscType::Int(index), OscType::Float(value)],
            )?;
            gp.ask(&format!("/{h}/GetParam"), vec![OscType::Int(index)])?;
        }
        "setvalue" => {
            let h = arg(cmd, 1, "widget handle")?;
            let value = unit(arg(cmd, 2, "value 0..1")?)?;
            undo_hint(
                first_number(&gp.ask(&format!("/{h}/GetValue"), vec![])?),
                &format!("setvalue {h}"),
            );
            gp.ask(&format!("/{h}/SetValue"), vec![OscType::Float(value)])?;
            gp.ask(&format!("/{h}/GetValue"), vec![])?;
        }
        "bypass" => {
            let h = arg(cmd, 1, "plugin handle")?;
            let on: i32 = number(arg(cmd, 2, "0 or 1")?)?;
            // GP has no OSC command to read bypass state, so the undo can only be a guess.
            println!(
                "To undo, if it was {} before: gp-probe --write bypass {h} {}",
                if on == 0 { "bypassed" } else { "enabled" },
                1 - on.clamp(0, 1)
            );
            gp.ask(&format!("/{h}/Bypass"), vec![OscType::Int(on)])?;
        }
        "record" => {
            let s: f64 = number(arg(cmd, 1, "seconds")?)?;
            let secs = Duration::try_from_secs_f64(s)
                .ok()
                .filter(|d| !d.is_zero())
                .ok_or(format!(
                    "'{s}' isn't a usable number of seconds; use something like 3"
                ))?;
            record(&gp, secs, &o.rec_dir)?
        }
        "send" => drop(gp.ask(
            arg(cmd, 1, "OSC address")?,
            cmd[2..].iter().map(|s| parse_arg(s)).collect(),
        )?),
        other => return Err(format!("unknown command '{other}'. Run gp-probe --help")),
    }
    Ok(())
}

fn undo_hint(before: Option<f64>, cmd: &str) {
    match before {
        Some(v) => println!("Current value {v}. To undo: gp-probe --write {cmd} {v}"),
        None => println!(
            "Couldn't read the current value, so there's no undo command. Note it in GP first."
        ),
    }
}

fn check(gp: &Gp, plugin: Option<&String>, widget: Option<&String>) -> Result<(), String> {
    println!("Read-only checklist. Nothing in your gig is changed.");
    gp.ask("/RackSpace/ListOSCWidgets", vec![])?;
    gp.ask("/RackSpace/ListOSCPlugins", vec![])?;
    match plugin {
        Some(h) => params(gp, h, 8)?,
        None => println!(
            "\n(skipped plugin checks: pass a plugin handle, e.g. gp-probe check Amp Gain)"
        ),
    }
    match widget {
        Some(w) => drop(gp.ask(&format!("/{w}/GetValue"), vec![])?),
        None => println!("\n(skipped widget check: pass a widget handle after the plugin handle)"),
    }
    println!("\nDone. Copy everything above and send it back.");
    Ok(())
}

fn params(gp: &Gp, handle: &str, max: i32) -> Result<(), String> {
    let count = first_int(&gp.ask(&format!("/{handle}/GetParamCount"), vec![])?);
    let Some(count) = count else {
        println!("   No parameter count came back, so skipping the values.");
        return Ok(());
    };
    println!(
        "   {handle} has {count} parameters; reading the first {}",
        count.min(max)
    );
    for i in 0..count.min(max) {
        gp.ask(&format!("/{handle}/GetParam"), vec![OscType::Int(i)])?;
    }
    Ok(())
}

type Snapshot = HashMap<PathBuf, (u64, SystemTime)>;

fn wav_files(dir: &Path, out: &mut Snapshot) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            wav_files(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav"))
            && let Ok(m) = e.metadata()
        {
            out.insert(p, (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
    }
}

/// Files that are new or changed since `before`.
fn changed(before: &Snapshot, now: &Snapshot) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = now
        .iter()
        .filter(|(p, s)| before.get(*p) != Some(s))
        .map(|(p, _)| p.clone())
        .collect();
    v.sort();
    v
}

fn record(gp: &Gp, seconds: Duration, dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        println!(
            "Warning: {} doesn't exist yet. If GP saves elsewhere, pass --rec-dir.",
            dir.display()
        );
    }
    let mut before = Snapshot::new();
    wav_files(dir, &mut before);
    gp.ask("/RecordInputs", vec![OscType::Int(1)])?;
    thread::sleep(seconds);
    gp.ask("/RecordInputs", vec![OscType::Int(0)])?;
    let stopped = Instant::now();

    // Wait for new files to appear, then for their sizes to stop growing.
    let mut last = Snapshot::new();
    while stopped.elapsed() < Duration::from_secs(15) {
        let mut now = Snapshot::new();
        wav_files(dir, &mut now);
        let new = changed(&before, &now);
        if !new.is_empty() && now == last {
            println!(
                "\nRecorder wrote {} file(s), finished {} ms after stop:",
                new.len(),
                stopped.elapsed().as_millis()
            );
            for p in new {
                println!("   {}  ({} bytes)", p.display(), now[&p].0);
            }
            return Ok(());
        }
        last = now;
        thread::sleep(Duration::from_millis(300));
    }
    println!(
        "\nNo new WAV files in {} after 15 s. Check the recorder has inputs/outputs ticked.",
        dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_are_typed_from_text() {
        assert_eq!(parse_arg("3"), OscType::Int(3));
        assert_eq!(parse_arg("0.5"), OscType::Float(0.5));
        assert_eq!(parse_arg("Amp"), OscType::String("Amp".into()));
        assert_eq!(parse_arg("inf"), OscType::String("inf".into()));
        assert_eq!(unit("5").unwrap(), 1.0);
        assert!(unit("nan").is_err());
    }

    #[test]
    fn ask_gives_up_when_gp_never_goes_quiet() {
        let chatty_gp = UdpSocket::bind("127.0.0.1:0").unwrap();
        let gp = Gp::connect(
            chatty_gp.local_addr().unwrap(),
            0,
            Duration::from_millis(100),
        )
        .unwrap();
        let reply_to = gp.sock.local_addr().unwrap();
        let meter = encoder::encode(&OscPacket::Message(OscMessage {
            addr: "/Meter/Value".into(),
            args: vec![OscType::Float(0.5)],
        }))
        .unwrap();
        thread::spawn(move || {
            while chatty_gp.send_to(&meter, reply_to).is_ok() {
                thread::sleep(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        gp.ask("/RackSpace/ListOSCWidgets", vec![]).unwrap();
        assert!(started.elapsed() < Duration::from_secs(7));
    }

    #[test]
    fn ask_collects_every_reply_including_bundles() {
        let fake_gp = UdpSocket::bind("127.0.0.1:0").unwrap();
        let gp = Gp::connect(fake_gp.local_addr().unwrap(), 0, Duration::from_millis(300)).unwrap();
        let reply_to = SocketAddr::from(([127, 0, 0, 1], gp.sock.local_addr().unwrap().port()));

        let server = thread::spawn(move || {
            let mut buf = [0u8; 1024];
            let (n, _) = fake_gp.recv_from(&mut buf).unwrap();
            let (_, p) = decoder::decode_udp(&buf[..n]).unwrap();
            assert!(
                matches!(p, OscPacket::Message(ref m) if m.addr == "/RackSpace/ListOSCWidgets")
            );
            let msg = |name: &str| {
                OscPacket::Message(OscMessage {
                    addr: "/Widget".into(),
                    args: vec![OscType::String(name.into())],
                })
            };
            fake_gp
                .send_to(&encoder::encode(&msg("Gain")).unwrap(), reply_to)
                .unwrap();
            let bundle = OscPacket::Bundle(rosc::OscBundle {
                timetag: (0, 1).into(),
                content: vec![msg("Bass"), msg("Treble")],
            });
            fake_gp
                .send_to(&encoder::encode(&bundle).unwrap(), reply_to)
                .unwrap();
        });

        let replies = gp.ask("/RackSpace/ListOSCWidgets", vec![]).unwrap();
        server.join().unwrap();
        let names: Vec<_> = replies
            .iter()
            .map(|m| m.args[0].clone().string().unwrap())
            .collect();
        assert_eq!(names, ["Gain", "Bass", "Treble"]);
    }

    #[test]
    fn changed_finds_new_and_grown_files() {
        let t = SystemTime::UNIX_EPOCH;
        let before: Snapshot = [(PathBuf::from("a.wav"), (10, t))].into();
        let now: Snapshot = [
            (PathBuf::from("a.wav"), (10, t)),
            (PathBuf::from("O_Out.wav"), (5, t)),
        ]
        .into();
        assert_eq!(changed(&before, &now), [PathBuf::from("O_Out.wav")]);
        let grown: Snapshot = [(PathBuf::from("a.wav"), (20, t))].into();
        assert_eq!(changed(&before, &grown), [PathBuf::from("a.wav")]);
    }
}
