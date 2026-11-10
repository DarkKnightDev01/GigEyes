Pre-alpha build. Expect rough edges.

**Install:** download `GigEyes-Setup-….exe` below and run it. It's one file with everything in it, installs into Program Files (Windows asks for permission), and shows up in Installed apps so you can remove it from there.

Windows may say "Windows protected your PC" because the setup isn't code-signed yet. Click **More info → Run anyway**.

**New in 0.7.1: fixes**
- **The AI can't be left running.** Closing GigEyes, or updating it, while the AI was reading or looking plugins up used to leave the AI server running in the background, holding memory. Windows now ends it together with GigEyes.
- **Plugin lookups are faster and more complete.** The AI now loads once for a whole batch of plugins, not once per plugin, and reads long pages (a suite's full model list) in parts, where it used to stop after the first 20,000 characters.
- **Lookups pause on a problem.** With no internet, a rejected key or no searches left, lookups used to fail for every plugin, using a search each. Now they pause with one message and a **Look up now** button.
- **Look up the other N in use:** one button ticks **Look up** for every plugin you use.
- **Remove all GigEyes files** now stops lookups and downloads first and waits for them to let go of their files, so it no longer fails when the AI was busy.
- Dropping a file on the **Your rig** or **Settings** tab now switches to **Tone**, where the result shows.
- The gear list scrolls smoothly again with large plugins in your rig.
- If you're updating from 0.6 or older, the first-start setup now shows plugins installed since your last scan.

**In 0.7: GigEyes learns your own plugins**
- **First-start setup.** GigEyes opens with five short steps: appearance, your guitar, your plugins, the search key and the local AI. Every step can be skipped, and **Settings → First-start setup → Run it again** goes through it again. If you're updating, it opens once now.
- **Your rig has two columns:** **Use** (you play through it) and **Look up** (find out what real gear it models). Looking up searches the web with your Tavily key and the local AI reads the pages; what it finds stays on your PC, marked "found on the web", each model with the sentence it came from and a link. A wrong one can be removed, and any plugin can be looked up again. Lookups run in the background (one search per plugin) and **Stop** pauses them.
- **New plugins.** GigEyes rescans at every start. In **Auto config** it asks about each new plugin: Yes ticks both columns and looks it up. In **Manual** new plugins appear unticked.
- The built-in list from 0.6 is gone: every plugin is now looked up the same way, including ones the list didn't know.

**In 0.6: plugin knowledge**
- **Your rig** now shows, for each plugin GigEyes knows, which real amps, cabinets and pedals it models, with a link to the maker's own list it comes from.
- **Gear on these pages:** each piece of gear the AI finds now shows which of your ticked plugins model it, e.g. "In your rig: Helix Native (Cali Rectifire), Mesa Boogie Mark IIC+ Suite".
- Known so far: Line 6 Helix Native (330 models), IK AmpliTube 5's licensed models (Fender, Mesa/Boogie, Orange, Fulltone, T-Rex, Dr. Z, Jet City), the Universal Audio and Plugin Alliance amps (Marshall, Fender, Ampeg, ENGL, Diezel, Friedman, Fuchs, Suhr and more), Softube Amp Room's Marshall suite, Neural DSP's licensed suites (Mesa/Boogie Mark IIC+, Soldano SLO-100, Fortin, Morgan, Tone King, Omega Granophyre) and TSE 808. Plugins whose makers don't say what they model aren't guessed at.

**In 0.5.2:**
- **No title bar during the intro.** GigEyes opens as a plain dark window with just the logo; the title bar comes back as the logo glides into the header.
- **Intro settings.** **Settings → Startup** now has **Off**, **Every launch** or **After updates**, and a **Length** slider from 1.5 to 4 seconds (2.5 s by default). Longer keeps "GigEyes" on screen longer; shorter than 1.9 s plays the whole intro faster. **Play it now** tries your choice.

**In 0.5.1.2:** the intro is 3 seconds. The pick and its bars arrive as quickly as before; "GigEyes" now stays under it for about a second longer, then the pick glides into the header at a calmer pace. Click or press any key to skip it, or set it to play only after updates in **Settings → Startup**.

**In 0.5.1.1:** the intro takes a moment longer (1.6 seconds): once the pick has formed, "GigEyes" fades in underneath it, holds, and fades away before the pick glides into the header.

**In 0.5.1: a startup intro.** GigEyes no longer just pops up. The amber pick springs in, its bars rise like a level meter, then it glides into its place in the header while the app settles in. Click or press any key to skip it. **Settings → Startup** chooses whether it plays every launch or only after an update, and **Play it now** replays it. It plays even when Windows' "Show animations" is off.

**In 0.5.0.4:** your Tavily key stays saved. It used to be saved only when you clicked **Save key**, but research worked straight after pasting it, so it was easy to skip the button and lose the key at the next restart or update. Now it saves the moment you paste or type it, and the button is gone. Every build also checks on Windows that the key survives updating.

**In 0.5.0.3:** updates clean up after themselves. Each update used to leave its downloaded setup (about 60 MB) in your temp folder; GigEyes now deletes those when it starts. Every build is now also tested updating from 0.4.0.1 through the previous release on a real Windows machine, and must end with exactly one GigEyes in Installed apps at the new version.

**In 0.5.0.2:** the GigEyes icon now shows in the window's title bar and on the taskbar too, not just on the files. 0.5.0.1 put the icon in the program, but the window only picked it up if it opened in front, and after an update it usually opens behind. The setup also tells Windows to refresh its icon cache, so shortcuts switch to the new icon straight away.

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
