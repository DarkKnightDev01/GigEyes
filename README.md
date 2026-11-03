# GigEyes

Match a guitar tone from a song or concert video using **your own VST3 plugins**, then use the settings in Gig Performer.

## Setup (Windows)

```
winget install ffmpeg
pip install -r requirements.txt
pip install demucs   # optional: only for --separate
```

## Use

1. Record your dry guitar (DI) playing the same part as the reference.
2. Run:

```
python gigeyes.py "song.mp3" my_di.wav --separate
```

On first run it asks which plugins you have (it lists your `C:\Program Files\Common Files\VST3` folder and accepts other paths), then the chain order and which knobs to turn. Your list is saved; `--plugins` asks again.

Use `--separate` when the reference is a full mix, to isolate the guitar first. `--evals` sets how many settings it tries (default 400).

Output in `gigeyes_out/`: `matched.wav` (your DI through the matched chain), `settings.json` (every knob value), and a `.vstpreset` per plugin.

## Test

```
python test_gigeyes.py
```
