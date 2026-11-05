Pre-alpha build. Expect rough edges.

**Install:** download `GigEyes-Setup-….exe` below and run it. It's one file with everything in it, installs for your Windows account only (no admin prompt), and shows up in Installed apps so you can remove it from there. To update, run a newer setup: your rig, research and search key stay.

Windows may say "Windows protected your PC" because the setup isn't code-signed yet. Click **More info → Run anyway**.

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
- Your search key is saved as plain text in `%APPDATA%\GigEyes\settings.json`.
