Pre-alpha build. Expect rough edges.

**Install:** download `GigEyes-Setup-….exe` below and run it. It's one file with everything in it, installs into Program Files (Windows asks for permission), and shows up in Installed apps so you can remove it from there.

Windows may say "Windows protected your PC" because the setup isn't code-signed yet. Click **More info → Run anyway**.

**New in 0.4.1**
- **Automatic updates.** GigEyes checks GitHub each time it opens. When a newer version is out, a banner offers **Update now**: GigEyes downloads the setup, checks it against GitHub's published checksum, installs it and reopens. Your rig, research and key stay. **Settings → Updates → Check now** checks on demand. Updates are automatic from this version on, so this one has to be installed by hand.
- **Installs in Program Files** (`C:\Program Files\GigEyes`) for everyone on the PC, so Windows asks for permission when you install, update or remove it. A copy from an older version in `%LOCALAPPDATA%\Programs\GigEyes` is removed automatically.
- **Motion:** the tab switch slides, screens fade and settle in, light and dark mode crossfade, the drop overlay and highlights ease in and out. Everything follows Windows' "Show animations" setting.

**Earlier in 0.4.0.1:** a Settings tab with light and dark mode and **Remove all GigEyes files**, and an optional local AI download during setup.

**What it does now**
- **Tone reading:** drop in a song, an isolated guitar track or a video, or paste a YouTube or other video link. GigEyes sums the tone up in one line, turns an amp's Gain, Bass, Middle, Treble and Presence knobs to where the recording sits, and draws its EQ curve. Downloads from links are deleted right after reading.
- **Your rig:** GigEyes finds the VST3 plugins on your PC (it only reads their names, it never loads them) and you tick the ones you play through.
- **Song research:** with your own free [Tavily](https://app.tavily.com/) key, GigEyes finds interviews, rig rundowns and gear lists for the song, each linked so you can check it, and saves them so they open offline later.
- **Local AI choice:** GigEyes reads your graphics card and memory and shows which local model fits next to Gig Performer. On an RTX 4070 laptop with 8 GB, that's Qwen3 8B on the graphics card.

**Known limits**
- The AI doesn't read the found pages yet, and nothing suggests settings for your own plugins yet. That's next.
- The knob positions are rough starting points on an amp with its EQ flat at noon.
- The Gain reading is a rough guess and can call a compressed track clean.
- webm and opus files don't open yet (links work).
- The downloaded AI isn't used yet; AI reading comes next.
- Your search key is saved as plain text in `%APPDATA%\GigEyes\settings.json`.
