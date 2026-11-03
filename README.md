# GigEyes

A Windows app that recreates guitar tones from a song or concert video using your own plugins, running in Gig Performer 5 Pro. GigEyes changes knobs in Gig Performer over OSC, records what comes out, and keeps the settings that sound closest to the song.

Early work in progress. The plan, in order:

1. Prove the Gig Performer link (`gp-probe`, now)
2. Tone-matching engine
3. The app: song or stem import, matching, A/B, Tone Library (fully offline)
4. Hardware auto-detect
5. Local AI that builds the chain and judges the result
6. Gig Performer extension for knob names
7. Training the AI on your own runs

## Download

Every push builds Windows `.exe` files. On GitHub go to **Actions → Windows build → latest run → Artifacts → gigeyes-windows**.

## Testing the Gig Performer link with gp-probe

Use a throwaway gig, not your live one.

1. **OSC setup:** in Gig Performer go to **Options → OSC Setup**:
   - Turn on **Enable OSC**.
   - Set **Remote client IP** to `127.0.0.1` and **Remote client port** to `54345`.
   - Note the **Gig Performer listening port**. If it isn't `54344`, add `--gp-port <yours>` to every command below.
2. **Rackspace:** make one with an amp plugin and one widget mapped to one of its knobs.
   - Right-click the amp plugin, give it an OSC/GPScript handle (e.g. `Amp`) and make sure OSC is enabled for it.
   - In the widget's properties, tick **Enable OSC** and give it a handle (e.g. `Gain`).
3. Open a terminal in the folder with `gp-probe.exe` and run:

   ```
   .\gp-probe.exe check Amp Gain
   ```

   This only reads. Send back everything it prints.
4. If step 3 got replies, try one change, then put it back with the undo command it prints:

   ```
   .\gp-probe.exe --write set Amp 0 0.5
   ```
5. Recording test: tick your outputs in the recorder settings, play something, then run:

   ```
   .\gp-probe.exe --write record 3
   ```

   It records 3 seconds and lists the files Gig Performer wrote, and how long they took to appear.

`gp-probe --help` lists every command. Anything that changes your gig or writes files needs `--write`.

## Building from source

```
cargo test --workspace
cargo build --release
```
