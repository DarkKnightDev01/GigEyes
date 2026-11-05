# GigEyes

Recreate a guitar tone from a studio album or a live recording, using the plugins you already own, in whatever host you play through (Gig Performer, a DAW, anything that loads VST3).

GigEyes listens to the song, works out the tone, and tells you which of your plugins to use and where to set the knobs. It never takes over your host.

Pre-alpha. What works today:

- **Tone analysis:** drop in a song, an isolated guitar track or a video link. GigEyes sums the tone up in one line, shows where an amp's Gain, Bass, Middle, Treble and Presence knobs would sit, and draws the EQ curve.
- **Your rig:** GigEyes finds the VST3 plugins on your PC (it reads their names from disk and never loads them) and you tick the ones you play through.
- **Song research:** with your own free search key, GigEyes finds interviews, rig rundowns and gear lists for the song, each linked so you can check it, and saves them so they open offline later.
- **Local AI choice:** GigEyes reads your graphics card and memory and picks the biggest local model that still leaves room for Gig Performer. The setup offers to download it (untick to skip).
- **Settings:** light or dark mode, your search key, the local AI, and **Remove all GigEyes files**.

Coming next, in order:

1. AI reading: the chosen local model reads the found pages and pulls out the gear and any published settings, each with a quote and a link
2. Plugin knowledge: what real amps and pedals each popular plugin models, taken from the makers' own documentation
3. Recommendations: which of your plugins for each stage and where to set each knob, saying which numbers come from a source and which from the audio
4. Automatic tuning: GigEyes runs a dry recording of your guitar through your plugins, tunes the knobs and saves standard `.vstpreset` files any host can load
5. Tone Library, fully offline

## Requirements

Windows 10 or 11.

## Install

1. Download `GigEyes-Setup-….exe` from the newest pre-release on the [Releases page](https://github.com/DarkKnightDev01/GigEyes/releases). It's one file with everything in it.
2. Run it. If Windows shows "Windows protected your PC", click **More info → Run anyway**: the setup isn't code-signed yet.
3. Click **Install GigEyes**. It installs for your Windows account only, with no admin prompt, into `%LOCALAPPDATA%\Programs\GigEyes`.

The setup also offers to download the local AI picked for your PC (Qwen3 8B is 5 GB on an 8 GB graphics card). Untick **Download the local AI** to skip it, or click **Skip the AI download** while it runs; the app is installed either way.

To update, run a newer setup: your rig, research and search key stay. To remove GigEyes, use **Settings → Remove all GigEyes files** inside the app (deletes your data and the AI, then opens the uninstaller), or **Windows Settings → Apps → Installed apps → GigEyes → Uninstall**. Both remove only what GigEyes put there.

Builds of every push, between releases, are under **Actions → Windows build → a run → Artifacts**.

## Use

Drop an audio or video file into the window, or click **Choose file…**. You can also drop a file onto `gigeyes.exe` itself.

Or paste a link (YouTube and [most other video sites](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md)) and click **Load**. GigEyes downloads only the audio into a temporary folder, reads it, and deletes the download straight away. This uses [yt-dlp](https://github.com/yt-dlp/yt-dlp), installed alongside GigEyes. If links stop working, the site has probably changed; update yt-dlp by running `.\yt-dlp.exe -U` in the GigEyes folder. Only download what you're allowed to.

- Supported files: wav, mp3, flac, ogg, m4a, mp4, mkv. Not yet: webm and opus files (links work, because GigEyes asks for a format it can read).
- An isolated guitar track gives the most accurate result. On a full mix, bass, drums and vocals colour it.
- The "how compressed" reading is a rough guess for now and will be tuned against real recordings.

### Song research

1. Get a free key at [app.tavily.com](https://app.tavily.com/): 1,000 searches a month, no card needed.
2. In GigEyes open **Settings**, paste the key under **Song research** and click **Save key**.
3. After reading a song, check the artist and title under **Research this song** and click **Find sources**.

The key is stored in `%APPDATA%\GigEyes\settings.json` on your PC and sent only to Tavily.

## Building from source

```
cargo test --workspace
cargo build --release
```

To build the one-file setup, append the app and yt-dlp to the setup program:

```
target\release\gigeyes-setup.exe --pack GigEyes-Setup.exe target\release\gigeyes.exe yt-dlp.exe
```

## Credits

- [Barlow](https://github.com/jpt/barlow) typeface, SIL Open Font License (`gigeyes/fonts/OFL.txt`)
- [Lucide](https://lucide.dev) icons, ISC License
- [yt-dlp](https://github.com/yt-dlp/yt-dlp), Unlicense, installed alongside GigEyes
- [Qwen3](https://huggingface.co/Qwen) models, Apache 2.0, for the local AI (downloaded only when AI reading arrives)
- Search by [Tavily](https://tavily.com), with your own key
