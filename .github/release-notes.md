Pre-alpha build. Expect rough edges.

**What's in it**
- `gp-probe.exe`: checks that Gig Performer 5 Pro answers OSC. Start here: see "Testing the Gig Performer link" in the [README](https://github.com/DarkKnightDev01/GigEyes#readme).
- `gigeyes.exe`: the app. Just an empty window for now.

**Known issue**
If Gig Performer says "OSC Listening Failure: port number 54344", another program holds that port. Set Gig Performer's listening port to `7344` in Options → OSC Setup and run `gp-probe.exe --gp-port 7344 check <plugin> <widget>`.

Windows may warn that the files are from an unknown publisher, because they aren't code-signed yet. Click "More info" → "Run anyway".
