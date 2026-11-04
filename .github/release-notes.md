Pre-alpha build. Expect rough edges.

Download the `.zip` below, right-click it → **Extract All**, and follow the [README](https://github.com/DarkKnightDev01/GigEyes#readme) (also inside the zip).

**What's in it**
- `gp-probe.exe`: checks that Gig Performer 5 Pro answers OSC. Start here: see "Test the Gig Performer link" in the README.
- `gigeyes.exe`: the app. Just an empty window for now.

**Known issue**
If Gig Performer says "OSC Listening Failure: port number 54344", another program holds that port. Set Gig Performer's listening port to `7344` in Options → OSC Setup and run `gp-probe.exe --gp-port 7344 check <plugin> <widget>`.

Windows may warn that the files are from an unknown publisher, because they aren't code-signed yet. Click "More info" → "Run anyway".
