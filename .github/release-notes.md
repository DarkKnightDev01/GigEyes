Beta. Most of it works; some rough edges are left.

**Install:** download `GigEyes-Setup-….exe` below and run it. It's one file with everything in it, installs into Program Files (Windows asks for permission), and shows up in Installed apps so you can remove it from there.

Windows may say "Windows protected your PC" because the setup isn't code-signed yet. Click **More info → Run anyway**.

**New in 0.8.2: suites name the model to load**
- **Helix and other suites now get their own model names right.** Looking up a suite's chorus (or delay, reverb…) models used to return the real pedals it copies but not the suite's name for them, so the block couldn't say what to load. It now asks only for that effect's models and keeps a model only where the page pairs it with the unit it's based on. For Helix it names "70s Chorus" (BOSS CE-1).
- **About 5 times faster**, since the local AI no longer lists every amp on the page.
- Lookups that found nothing before are tried again.

**In 0.8.1: every effect says how far to turn it**
- **Starting points.** When no source gives settings for a chorus, delay, reverb, drive or compressor, GigEyes no longer leaves the knobs at "default". It lists the usual knobs for that kind of effect with where players usually start (a chorus: rate 3, depth 5, mix 5 of 10; a reverb: decay 4, mix 2.5; a delay: feedback 3, mix 3, time from the song's tempo). They're marked "starting point": dial them in by ear from there.
- **This works in AmpliTube and other suites even when GigEyes can't find which model to use.** Add any chorus (or delay, reverb) in the suite and start from the listed settings.
- **You can see it working.** While it looks up a suite's models it says whether it's searching the web or the local AI is reading pages.

**In 0.8.0.4**
- On a PC without a graphics card the local AI now gets up to an hour per page instead of 15 minutes. Long pages, such as Helix's model list, were giving up with "couldn't read the pages".

**In 0.8.0.3**
- Settings a source writes without a space ("Gain5, Bass5") are no longer lost.

**In 0.8.0.2: settings even when knobs can't be read**
- **Amps and delays always list settings.** When GigEyes can't open a plugin to read its knobs (or the AI can't look a model up), it now lists the usual knobs (gain, bass, middle, treble, presence; time on a delay) with values from the sources or the recording, instead of only saying it has no knobs. Names may differ a little from your plugin's.
- **Fewer plugins fail to open for knob reading.** Plugins that load their own DLLs from their folder now open, and a broken plugin no longer pops up a Windows error box.

**In 0.8.0.1: the chain names things you can do**
- **Suites get a model.** When Helix, AmpliTube or another suite stands in for a chorus, delay or other effect and GigEyes doesn't know which model in it does that, it now looks the suite's models for that effect up and names one ("pick 70s Chorus"), then finds that model's knobs. Before, the block said only "shows GigEyes no knobs".
- **AmpliTube is no longer told to "find a capture".** Its pages mention ToneX, which made GigEyes take it for a capture loader. Loaders are now recognised by name only (TONE3000, NAM, ToneX).
- **Capture loaders always say which capture to find.** A capture someone uploaded ("V30 1960") is no longer shown as a model to pick. The block says to find a capture of the named amp or pedal, and which settings it should be made near.
- **"A chorus pedal" isn't a model.** When a source names only a kind of pedal, the block says so ("Any chorus pedal works") instead of "None of your plugins models the Chorus pedal".
- Plugin names are searched without "(x64)".

**In 0.8: what to set on each plugin**
- **Every block lists its knobs.** Click a block and it shows the knobs to turn, with the value for each, and folds the rest under "Leave at default". Each value says where it comes from: a source, the recording, or nothing (default).
- **GigEyes reads your plugins' own knobs.** It opens each plugin in a separate background process and reads its knob names, ranges and defaults, so the list uses the plugin's own names and scale (e.g. "6 of 10" or "0.4 s"). The plugin isn't changed, and a plugin that hangs is left alone after 40 seconds.
- **Suites look the model up.** Helix, AmpliTube and other multi-effect suites don't show their knobs per model, so GigEyes searches the model's manual and lets the local AI pick out its knobs (needs the Tavily key and the local AI).
- **Values from the sources and the recording.** Settings the sources give ("gain at 6", "treble at 2 o'clock") go on the matching knobs. When nothing says, the amp's gain, bass, middle, treble and presence come from the recording, and delay time is set to a quarter note at the song's tempo, which GigEyes now detects.

**In 0.7.3.1: fixes to the chain**
- **Amp sims with effects fill effect slots.** AmpliTube, Helix, Guitar Rig and other amp sims whose lookup shows pedals or effects are now offered for chorus, delay, reverb and other effect blocks, instead of "Nothing in your rig".
- **Capture loaders get the real unit.** For gear none of your plugins models, TONE3000, NAM and ToneX now come first with "Find a capture of" the named unit (say, an MXR M222 Talk Box), instead of an unrelated capture from the lookup.
- A long file name no longer runs under the "listened" time.

**In 0.7.3: the Tone page shows a chain**
- **Your chain instead of knobs.** After a song is read, the Tone page lays out a signal chain like a Gig Performer rackspace: Guitar, then a block per plugin, then Output. Each block names which of your plugins to load and which model to pick in it.
- **One button builds it.** **Build the chain** searches the web for the song once (or uses the saved search), lets the local AI read the pages, and turns the gear they name into blocks in playing order: wah, drives, amp, cabinet, modulation, delay, reverb.
- **The recording fills the gaps.** If no source names the amp, GigEyes picks one by how driven the recording sounds, and the amp block gets starting settings measured from it.
- **Stand-ins.** Gear none of your plugins models gets the closest plugin you have, marked as a stand-in, e.g. a Klon Centaur played through your TSE808.
- **Click a block** for the sentence and page it comes from, the settings, and a menu to swap in another of your plugins that can do the job.
- The sources and the full gear list are still there, folded up under the chain.

**In 0.7.2.3: smoother, and easier to fix a lookup**
- **Switching tabs slides.** The page glides in from the side of the tab you picked.
- **Animations always play.** They used to switch off when Windows' "Animation effects" setting was off; now GigEyes animates the same on every PC, with nothing to change in Windows.
- **Look it up again is always there** under a looked-up plugin, instead of hidden in "Where this comes from".
- Sound collections ("Jimi Hendrix collection") no longer count as modelled gear.

**In 0.7.2.2: better plugin lookups**
- **Facts are about the sound.** The AI is told to leave out slogans and sales lines ("Your signal chain, your way") and only give facts the page's sentence states.
- **Only real gear counts as modelled.** A maker on its own ("Orange", "Ampeg"), a bare word ("Portaflex") or software ("Neural Amp Modeler") is no longer listed as modelled gear. This also cleans up what earlier lookups found.
- **The right product.** Lookups now search for the guitar plugin, so a plain name like "Gateway" doesn't pull in a synthesizer of the same name, and pages about another product are skipped.
- Facts from earlier lookups stay until you press **Look it up again** under a plugin.

**In 0.7.2.1: fixes**
- **A broken AI no longer burns your searches.** If the local AI fails while looking up plugins, the lookups now pause with the reason, instead of moving on to the next plugin and spending a web search on each one.
- **Scan for plugins asks about new ones.** In auto config, plugins found with **Scan for plugins** or **Add a folder** now get the "Do you play through it?" question, as they already did at startup.
- **One lookup per plugin.** A plugin installed in both VST3 folders was looked up twice.
- **Remove all files removes them all.** A song search still running when you pressed it could put the research folder back.

**In 0.7.2: lookups learn what a plugin is and does**
- A plugin lookup now finds out **what kind of plugin it is** (amp sim, overdrive, compressor, reverb and so on) and **what it does and how it behaves**, as short facts like "Tightens the low end, pushes the mids". The real gear it models is still found too.
- Every fact is backed by the sentence from the page it comes from. Facts whose sentence isn't really on the page are dropped. **Where this comes from** under each plugin shows the sentences and links, and a wrong one can be removed.
- Plugins looked up with 0.7 or 0.7.1 are looked up again by themselves, one web search each.

**In 0.7.1: fixes**
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
