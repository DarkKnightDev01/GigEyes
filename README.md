# GigEyes

Recreate a guitar tone from a studio album or a live recording, using the plugins you already own, in whatever host you play through (Gig Performer, a DAW, anything that loads VST3).

GigEyes listens to the song, works out the tone, and tells you which of your plugins to use and where to set the knobs. It never takes over your host.

Pre-alpha. What works today:

- **Tone analysis:** drop in a song or an isolated guitar track and get its EQ curve, brightness, mid scoop, low end and how compressed it sounds.

Coming next, in order:

1. Plugin scanner: GigEyes reads the knobs of your VST3 plugins
2. Your rig: tell GigEyes what each plugin covers (drive, amp, cab, EQ, delay, reverb…); all-in-one plugins like Helix Native just cover several
3. Recommendations: which plugin for each stage and where to set each knob
4. Automatic tuning: GigEyes runs a dry recording of your guitar through your plugins, tunes the knobs and saves standard `.vstpreset` files any host can load. Hand-dialled settings stay as the fallback for plugins that won't load outside a host
5. Tone Library, fully offline
6. Local AI that picks the chain and explains the result

## Requirements

Windows 10 or 11.

## Install

1. Open the [Releases page](https://github.com/DarkKnightDev01/GigEyes/releases) and download the `.zip` from the newest pre-release.
2. Right-click the zip → **Extract All**, e.g. to `C:\GigEyes`.
3. Run `gigeyes.exe`. If Windows shows "Windows protected your PC", click **More info → Run anyway**. The app isn't code-signed yet.

Builds of every push, between releases, are under **Actions → Windows build → a run → Artifacts**.

## Use

Drop an audio or video file into the window, or click **Choose file…**. You can also drop a file onto `gigeyes.exe` itself.

- Supported: wav, mp3, flac, ogg, m4a, mp4, mkv. Not yet: webm and opus.
- An isolated guitar track gives the most accurate result. On a full mix, bass, drums and vocals colour it.
- The "how compressed" reading is a rough guess for now and will be tuned against real recordings.

## Building from source

```
cargo test --workspace
cargo build --release
```
