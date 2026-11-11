<img src="gigeyes/icons/gigeyes.svg" width="88" alt="GigEyes logo: an amber guitar pick with a sound wave cut into it">

# GigEyes

Recreate a guitar tone from a studio album or a live recording, using the plugins you already own, in whatever host you play through (Gig Performer, a DAW, anything that loads VST3).

GigEyes listens to the song, works out the tone, and tells you which of your plugins to use and where to set the knobs. It never takes over your host.

Beta. What works today:

- **Tone analysis:** drop in a song, an isolated guitar track or a video link. GigEyes sums the tone up in one line, shows where an amp's Gain, Bass, Middle, Treble and Presence knobs would sit, and draws the EQ curve.
- **Your rig:** GigEyes finds the VST3 plugins on your PC (it reads their names from disk and never loads them) and you tick the ones you play through.
- **First-start setup:** on first start GigEyes walks you through appearance, your guitar, your plugins, the search key and the local AI. **Settings → First-start setup → Run it again** repeats it.
- **Plugin knowledge:** in **Your rig** each plugin has two ticks: **Use** (you play through it) and **Look up** (find out what kind of plugin it is, what it does and how it behaves, and what real gear it models). Looking up searches the web with your key, the local AI reads the pages, and every fact it finds is kept on your PC with the sentence it came from and a link, marked "found on the web"; a wrong one can be removed. When the AI finds gear on a song's pages, each piece shows which of your plugins model it, e.g. "In your rig: Helix Native (Cali Rectifire)". GigEyes rescans your plugins at every start: in **Auto config** it asks about each new one (Yes ticks both and looks it up); in **Manual** you tick them yourself.
- **Song research:** with your own free search key, GigEyes finds interviews, rig rundowns and gear lists for the song, each linked so you can check it, and saves them so they open offline later.
- **AI reading:** a local AI reads the pages research found and lists the gear and any published settings, each with the sentence it came from and a link to the page. It runs on your PC, nothing is sent anywhere, and anything it can't back with a sentence that's really on the page is dropped.
- **Local AI choice:** GigEyes reads your graphics card and memory and picks the biggest local model that still leaves room for Gig Performer. The setup offers to download it (untick to skip), or get it later in **Settings → Local AI**.
- **Settings:** light or dark mode, the startup intro (off, every launch or only after updates, 1.5 to 4 seconds), your guitar, your search key, the local AI, and **Remove all GigEyes files**.

Coming next, in order:

1. Recommendations: which of your plugins for each stage and where to set each knob, saying which numbers come from a source and which from the audio
2. Automatic tuning: GigEyes runs a dry recording of your guitar through your plugins, tunes the knobs and saves standard `.vstpreset` files any host can load
3. Tone Library, fully offline

## Requirements

Windows 10 or 11.

## Install

1. Download `GigEyes-Setup-….exe` from the newest pre-release on the [Releases page](https://github.com/DarkKnightDev01/GigEyes/releases). It's one file with everything in it.
2. Run it. If Windows shows "Windows protected your PC", click **More info → Run anyway**: the setup isn't code-signed yet.
3. Click **Install GigEyes** and say **Yes** when Windows asks for permission. GigEyes installs into `C:\Program Files\GigEyes` for everyone on the PC; your rig, research and settings stay in your own account. An older copy in `%LOCALAPPDATA%\Programs\GigEyes` is removed.

The setup also offers to download the local AI picked for your PC (Qwen3 8B is 5 GB on an 8 GB graphics card). Untick **Download the local AI** to skip it, or click **Skip the AI download** while it runs; the app is installed either way.

**Updates are automatic from 0.4.1 on:** each time GigEyes opens it checks GitHub, and when a newer version is out a banner offers **Update now**. GigEyes downloads the new setup, checks it against GitHub's published checksum, and installs it (Windows asks for permission once), then reopens. Your rig, research and search key stay. **Settings → Updates → Check now** checks on demand. To remove GigEyes, use **Settings → Remove all GigEyes files** inside the app (deletes your data and the AI, then opens the uninstaller), or **Windows Settings → Apps → Installed apps → GigEyes → Uninstall**. Both remove only what GigEyes put there.

Builds of every push, between releases, are under **Actions → Windows build → a run → Artifacts**.

## Use

Drop an audio or video file into the window, or click **Choose file…**. You can also drop a file onto `gigeyes.exe` itself.

Or paste a link (YouTube and [most other video sites](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md)) and click **Load**. GigEyes downloads only the audio into a temporary folder, reads it, and deletes the download straight away. This uses [yt-dlp](https://github.com/yt-dlp/yt-dlp), installed alongside GigEyes. If links stop working, the site has probably changed; update yt-dlp by running `.\yt-dlp.exe -U` in the GigEyes folder. Only download what you're allowed to.

- Supported files: wav, mp3, flac, ogg, m4a, mp4, mkv. Not yet: webm and opus files (links work, because GigEyes asks for a format it can read).
- An isolated guitar track gives the most accurate result. On a full mix, bass, drums and vocals colour it.
- The "how compressed" reading is a rough guess for now and will be tuned against real recordings.

### Song research

1. Get a free key at [app.tavily.com](https://app.tavily.com/): 1,000 searches a month, no card needed.
2. In GigEyes open **Settings**, paste the key under **Song research**. It saves as you paste it.
3. After reading a song, check the artist and title under **Research this song** and click **Find sources**.

The key is stored in `%APPDATA%\GigEyes\settings.json` on your PC and sent only to Tavily.

Then click **Read the pages** under **Gear on these pages**. The local AI starts, reads each page, and quits again to free the memory; **Stop** ends it at once. On a graphics card it takes about a minute, on the processor several. Each piece of gear shows the sentence from the page that names it, so you can check it. The list is saved with the research.

The AI runs in [llama.cpp](https://github.com/ggml-org/llama.cpp)'s server, which GigEyes downloads with the model (38 MB, checked against a pinned checksum) into `%LOCALAPPDATA%\GigEyes`. It listens only on your own PC. If it won't start, what it said is in `%LOCALAPPDATA%\GigEyes\ai.log`.

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
- The GigEyes logo (`gigeyes/icons/gigeyes.svg`) is drawn from the same shape as the mark in the app header.
- [yt-dlp](https://github.com/yt-dlp/yt-dlp), Unlicense, installed alongside GigEyes
- [Qwen3](https://huggingface.co/Qwen) models, Apache 2.0, for the local AI
- [llama.cpp](https://github.com/ggml-org/llama.cpp), MIT License, runs the local AI (downloaded with it)
- Search by [Tavily](https://tavily.com), with your own key
