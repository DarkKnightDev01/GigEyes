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

## Requirements

- Windows 10 or 11
- Gig Performer 5 **Pro** (OSC, which GigEyes uses to talk to Gig Performer, is a Pro feature)

## Install

GigEyes is pre-alpha: expect rough edges. There's no installer yet.

1. Open the [Releases page](https://github.com/DarkKnightDev01/GigEyes/releases) and download the `.zip` from the newest pre-release. (`v0.1.0-prealpha.1` has the two `.exe` files separately instead.)
2. Right-click the zip → **Extract All**, e.g. to `C:\GigEyes`.
3. The first time you run them, Windows may show "Windows protected your PC", because the files aren't code-signed yet. Click **More info → Run anyway**.

Builds of every push, between releases, are under **Actions → Windows build → a run → Artifacts**.

## Set up Gig Performer

In Gig Performer go to **Options → OSC Setup**:

| Setting | Value |
|---|---|
| Enable OSC | on |
| Remote client IP address | `127.0.0.1` |
| Remote client port | `54345` |
| Gig Performer listening port | `7344` |

Then close the settings window. If Gig Performer shows **"OSC Listening Failure"**, another program holds that port. Pick another number, such as `7345`, and use it with `--gp-port` below.

Why `7344` and not the default `54344`: Windows can hand ports above 49152 to any program at random, so the default is sometimes taken.

## Test the Gig Performer link with gp-probe

Use a throwaway gig, not your live one.

1. **Rackspace:** add one plugin and map one widget to one of its knobs.
   - Give the plugin an OSC handle: right-click it in the Wiring view and set its OSC/GPScript handle, e.g. `Amp`.
   - In the widget's properties, tick **Enable OSC** and give it a handle, e.g. `Gain`.
2. **Check (read-only):** open PowerShell in your GigEyes folder (Shift + right-click in the folder → "Open PowerShell window here") and run:

   ```
   .\gp-probe.exe --gp-port 7344 check Amp Gain
   ```

   Use your own handles instead of `Amp` and `Gain`. This changes nothing. Send back everything it prints.
3. **Change one knob,** then put it back with the undo command it prints:

   ```
   .\gp-probe.exe --gp-port 7344 --write set Amp 0 0.5
   ```
4. **Recording:** in Gig Performer's recorder settings, tick your outputs. Play something, then run:

   ```
   .\gp-probe.exe --gp-port 7344 --write record 3
   ```

   It records 3 seconds and lists the files Gig Performer wrote, and how long they took to appear.

`.\gp-probe.exe --help` lists every command. Anything that changes your gig or writes files needs `--write`.

### If it doesn't work

| gp-probe says | What to do |
|---|---|
| "Nothing is listening on …" | Gig Performer isn't running, OSC is off, or `--gp-port` doesn't match Gig Performer's listening port. Also check Gig Performer didn't show "OSC Listening Failure". |
| "NO REPLY after 500 ms" | Gig Performer got the message but answered elsewhere. Check Remote client IP is `127.0.0.1` and Remote client port is `54345`. |
| "can't listen on port 54345" | Another program holds it. Use another number with `--reply-port` and set the same number as Remote client port in Gig Performer. |

To see which program holds a port (here 7344), run in PowerShell:

```
Get-NetUDPEndpoint -LocalPort 7344 | ForEach-Object { Get-Process -Id $_.OwningProcess } | Select-Object Id, ProcessName
```

## Building from source

```
cargo test --workspace
cargo build --release
```
