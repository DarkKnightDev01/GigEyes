Pre-alpha build. Expect rough edges.

**Install:** download `GigEyes-Setup-….exe` below and run it. It's one file with everything in it, installs into Program Files (Windows asks for permission), and shows up in Installed apps so you can remove it from there.

Windows may say "Windows protected your PC" because the setup isn't code-signed yet. Click **More info → Run anyway**.

**New in 0.5.0.2:** the GigEyes icon now shows in the window's title bar and on the taskbar too, not just on the files. 0.5.0.1 put the icon in the program, but the window only picked it up if it opened in front, and after an update it usually opens behind. The setup also tells Windows to refresh its icon cache, so shortcuts switch to the new icon straight away.

**In 0.5.0.1:** GigEyes got its logo, the amber pick, on the app, the setup, shortcuts and Installed apps, and each file's Properties show its version.

**New in 0.5.0: AI reading**
- After song research, click **Read the pages**. The local AI reads each page on your PC and lists the gear and any published settings: amps, pedals, guitars, pickups, strings and tuning. Each piece shows the sentence from the page that names it and links to the page. Anything the AI can't back with a sentence that's really on the page is dropped. The list is saved with the research and opens offline.
- The AI runs only while it reads, then quits to free the memory for Gig Performer. **Stop** ends it at once.
- **Settings → Local AI → Download** gets the AI if you skipped it in setup. It now includes llama.cpp's server (38 MB), which runs the model on NVIDIA, AMD and Intel graphics cards, or on the processor. If you already downloaded the model, only the 38 MB is new.

**Earlier in 0.4.1:** automatic updates, installing into Program Files, and motion. **In 0.4.0.1:** a Settings tab with light and dark mode and **Remove all GigEyes files**, and an optional local AI download during setup.

**What it does now**
- **Tone reading:** drop in a song, an isolated guitar track or a video, or paste a YouTube or other video link. GigEyes sums the tone up in one line, turns an amp's Gain, Bass, Middle, Treble and Presence knobs to where the recording sits, and draws its EQ curve. Downloads from links are deleted right after reading.
- **Your rig:** GigEyes finds the VST3 plugins on your PC (it only reads their names, it never loads them) and you tick the ones you play through.
- **Song research:** with your own free [Tavily](https://app.tavily.com/) key, GigEyes finds interviews, rig rundowns and gear lists for the song, each linked so you can check it, and saves them so they open offline later.
- **Local AI choice:** GigEyes reads your graphics card and memory and shows which local model fits next to Gig Performer. On an RTX 4070 laptop with 8 GB, that's Qwen3 8B on the graphics card.
- **AI reading:** the local AI lists the gear and settings the research pages name, each with its quote.

**Known limits**
- Nothing suggests settings for your own plugins yet. That's next.
- AI reading on the processor takes several minutes for a full set of pages. Small models (1.7B, 4B) miss more gear than 8B and 14B.
- The knob positions are rough starting points on an amp with its EQ flat at noon.
- The Gain reading is a rough guess and can call a compressed track clean.
- webm and opus files don't open yet (links work).
- Your search key is saved as plain text in `%APPDATA%\GigEyes\settings.json`.
