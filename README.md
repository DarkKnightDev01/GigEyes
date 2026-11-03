# GigEyes

Match a guitar tone from a song or concert video using **your own plugins, running in Gig Performer**. GigEyes turns your widgets over OSC, records what Gig Performer outputs, and keeps the closest-sounding settings.

Needs **Gig Performer Pro** (OSC is a Pro feature) on Windows.

## Install

```
winget install ffmpeg
pip install -r requirements.txt
pip install demucs   # optional: only for --separate
```

## One-time Gig Performer setup

1. **Options > OSC Setup:** enable OSC. Set *Remote client IP* to `127.0.0.1` and *Remote client port* to `54345`. Note the *Gig Performer listening port* (GigEyes assumes `54344`; pass `--gp-port` if yours differs).
2. **Recorder:** in the recording options, tick the outputs your guitar chain goes to.
3. **Rackspace wiring:** `Audio File Player -> your plugins -> Audio Output`. Load your DI (dry guitar playing the same part as the song) into lane 1 and turn **Loop on**.
4. **Knobs:** map each knob you want matched (gain, bass, mid, treble, presence...) to a widget. In each widget's properties tick **Enable OSC** and give it a handle with no spaces, e.g. `AmpGain`.

## Use

Press Play on the Audio File Player so the DI loops, then:

```
python gigeyes.py "song.mp3" --separate
```

GigEyes asks Gig Performer for the OSC widgets in the current rackspace and asks which to turn. Then it tries settings, recording a few seconds of output each time. Default is 150 tries at about 6 s each, so roughly 15 minutes. Ctrl+C stops early and keeps the best so far.

When it finishes, the widgets are left on the matched settings; save your gig to keep them. `gigeyes_out/` gets `matched.wav` and `settings.json`.

Options: `--separate` isolates the guitar from a full mix first. `--seconds` sets how long each try records; make it at least one full loop of your DI. `--evals` sets how many tries. `--recordings` points to Gig Performer's recordings folder if you moved it from `Documents\Gig Performer Recordings`.

Tips: pick 3–8 knobs, since more knobs need many more tries. Mute your speakers or turn them down while it runs.

## Test

```
pip install pedalboard
python test_gigeyes.py
```
