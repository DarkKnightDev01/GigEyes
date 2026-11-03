"""GigEyes: match your VST plugin chain to a guitar tone from a recording.

Usage:
    python gigeyes.py REFERENCE YOUR_DI [--separate] [--evals 400] [--plugins]

REFERENCE  song or concert video/audio (anything ffmpeg can read)
YOUR_DI    your dry guitar playing the same part, recorded straight from the interface
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
from scipy.optimize import differential_evolution
from scipy.signal import stft

SR = 48000
HERE = Path(__file__).resolve().parent
PLUGIN_LIST = HERE / "plugins.json"
VST3_DIR = Path(os.environ.get("CommonProgramFiles", r"C:\Program Files\Common Files")) / "VST3"
BAND_EDGES = np.geomspace(60, 12000, 41)


# ---------- audio in ----------

def load_audio(path, seconds=None):
    """Decode any audio/video file to mono float32 at SR using ffmpeg."""
    cmd = ["ffmpeg", "-v", "error", "-i", str(path), "-vn", "-ac", "1", "-ar", str(SR)]
    if seconds:
        cmd += ["-t", str(seconds)]
    cmd += ["-f", "f32le", "-"]
    try:
        raw = subprocess.run(cmd, capture_output=True, check=True).stdout
    except FileNotFoundError:
        sys.exit("ffmpeg not found. Install it (winget install ffmpeg) and reopen the terminal.")
    except subprocess.CalledProcessError as e:
        sys.exit(f"ffmpeg could not read {path}:\n{e.stderr.decode(errors='ignore')}")
    audio = np.frombuffer(raw, dtype=np.float32)
    if audio.size == 0:
        sys.exit(f"No audio found in {path}")
    return audio


def isolate_guitar(path, workdir):
    """Pull the guitar stem out of a full mix with Demucs (optional install)."""
    print("Isolating guitar with Demucs (first run downloads the model)...")
    try:
        subprocess.run([sys.executable, "-m", "demucs", "-n", "htdemucs_6s", "--two-stems", "guitar",
                        "-o", str(workdir), str(path)], check=True)
    except subprocess.CalledProcessError:
        sys.exit("Demucs failed. Install it with: pip install demucs")
    return Path(workdir) / "htdemucs_6s" / Path(path).stem / "guitar.wav"


# ---------- tone fingerprint ----------

def profile(audio):
    """Level-independent tone fingerprint: spectral shape in 40 log bands + crest factor."""
    f, _, z = stft(audio, SR, nperseg=4096)
    power = (np.abs(z) ** 2).mean(axis=1)
    # interpolate cumulative energy so low bands narrower than one FFT bin still get a value
    bands = np.diff(np.interp(BAND_EDGES, f, np.cumsum(power))) / np.diff(BAND_EDGES)
    shape = 10 * np.log10(bands + 1e-12)
    shape -= shape.mean()
    rms = np.sqrt(np.mean(audio ** 2)) + 1e-12
    crest = 20 * np.log10(np.max(np.abs(audio)) / rms + 1e-12)  # lower = more compressed/saturated
    return shape, crest


def distance(a, b):
    return float(np.mean(np.abs(a[0] - b[0])) + 0.5 * abs(a[1] - b[1]))


def to_mono(audio):
    return audio if audio.ndim == 1 else audio.mean(axis=0)


# ---------- search ----------

def search(render, set_knobs, n_knobs, target, evals=400, start=None, seed=0):
    """Turn n_knobs (each 0..1) until render() sounds closest to target. Returns (best values, score)."""
    def loss(v):
        set_knobs(v)
        return distance(profile(to_mono(render())), target)

    popsize = 8
    maxiter = max(1, evals // (popsize * n_knobs) - 1)
    result = differential_evolution(loss, [(0.0, 1.0)] * n_knobs, popsize=popsize, maxiter=maxiter,
                                    polish=False, seed=seed, x0=start, tol=1e-6)
    set_knobs(result.x)
    return result.x, result.fun


# ---------- asking the user ----------

def ask_numbers(prompt, count):
    while True:
        text = input(prompt).strip()
        if not text:
            return []
        try:
            picks = [int(t) for t in text.replace(" ", "").split(",") if t]
        except ValueError:
            picks = [0]
        if all(1 <= p <= count for p in picks):
            return picks
        print(f"  Enter numbers between 1 and {count}, separated by commas.")


def ask_plugin_list():
    """Ask which plugins the user owns. Offers what's in the standard VST3 folder, accepts extra paths."""
    found = sorted(p for p in VST3_DIR.rglob("*.vst3")
                   if not any(parent.suffix == ".vst3" for parent in p.parents))
    print("\nWhich plugins do you have available for tone matching?")
    if found:
        print(f"Found in {VST3_DIR}:")
        for i, p in enumerate(found, 1):
            print(f"  {i:3}. {p.stem}")
        chosen = [str(found[i - 1]) for i in ask_numbers("Numbers of the ones to use (e.g. 1,4,7): ", len(found))]
    else:
        print(f"(No plugins found in {VST3_DIR})")
        chosen = []
    print("Paste paths to any other .vst3 plugins, one per line. Blank line when done.")
    while extra := input("  path: ").strip().strip('"'):
        if Path(extra).exists():
            chosen.append(extra)
        else:
            print("  Not found, skipped.")
    if not chosen:
        sys.exit("No plugins listed, nothing to match with.")
    PLUGIN_LIST.write_text(json.dumps(chosen, indent=2))
    print(f"Saved your plugin list to {PLUGIN_LIST.name}. Run with --plugins to change it.")
    return chosen


def load_chain(paths):
    from pedalboard import load_plugin, VST3Plugin
    print("\nYour plugins:")
    for i, p in enumerate(paths, 1):
        print(f"  {i}. {Path(p).stem}")
    order = ask_numbers("Signal chain in order, first to last (e.g. 2,1,3): ", len(paths))
    if not order:
        sys.exit("Empty chain.")
    chain = []
    for i in order:
        path = paths[i - 1]
        names = VST3Plugin.get_plugin_names_for_file(path)
        name = names[0]
        if len(names) > 1:  # shell plugins like Waves hold several
            for j, n in enumerate(names, 1):
                print(f"    {j}. {n}")
            name = names[(ask_numbers(f"  Which one from {Path(path).stem}? ", len(names)) or [1])[0] - 1]
        print(f"  Loading {name}...")
        chain.append((path, load_plugin(path, plugin_name=name)))
    return chain


def pick_knobs(chain):
    knobs = []
    for _, plugin in chain:
        params = [(n, p) for n, p in plugin.parameters.items() if p.type is not bool]
        print(f"\n{plugin.name} knobs:")
        for i, (n, p) in enumerate(params, 1):
            print(f"  {i:3}. {n} = {p.string_value}")
        picks = ask_numbers("Which should GigEyes turn? (numbers, blank = all): ", len(params))
        knobs += [(plugin, *params[i - 1]) for i in (picks or range(1, len(params) + 1))]
    return knobs


# ---------- main ----------

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("reference")
    ap.add_argument("di")
    ap.add_argument("--separate", action="store_true", help="isolate the guitar from a full mix first (needs demucs)")
    ap.add_argument("--evals", type=int, default=400, help="how many settings to try (more = slower, closer)")
    ap.add_argument("--seconds", type=float, default=20, help="max seconds of your DI to use per try")
    ap.add_argument("--plugins", action="store_true", help="ask for your plugin list again")
    ap.add_argument("--out", default="gigeyes_out")
    args = ap.parse_args()

    out = Path(args.out)
    out.mkdir(exist_ok=True)

    paths = ask_plugin_list() if args.plugins or not PLUGIN_LIST.exists() else json.loads(PLUGIN_LIST.read_text())
    chain = load_chain(paths)
    knobs = pick_knobs(chain)

    ref_path = args.reference
    if args.separate:
        ref_path = isolate_guitar(ref_path, tempfile.mkdtemp(prefix="gigeyes_"))
    target = profile(load_audio(ref_path))
    di = load_audio(args.di, args.seconds)

    from pedalboard import Pedalboard
    board = Pedalboard([p for _, p in chain])

    def set_knobs(values):
        for (_, _, param), v in zip(knobs, values):
            param.raw_value = float(v)

    start = [param.raw_value for _, _, param in knobs]
    print(f"\nMatching {len(knobs)} knobs with up to ~{args.evals} tries. Ctrl+C stops early.")
    _, score = search(lambda: board(di, SR), set_knobs, len(knobs), target, args.evals, start)

    from pedalboard.io import AudioFile
    matched = board(di, SR)
    matched = matched / (np.max(np.abs(matched)) + 1e-9) * 0.9
    with AudioFile(str(out / "matched.wav"), "w", SR, matched.shape[0] if matched.ndim > 1 else 1) as f:
        f.write(matched)

    settings = []
    for i, (path, plugin) in enumerate(chain, 1):
        settings.append({"plugin": plugin.name, "file": path,
                         "settings": {n: p.string_value for n, p in plugin.parameters.items()}})
        try:
            (out / f"{i:02d}_{plugin.name}.vstpreset").write_bytes(plugin.preset_data)
        except Exception as e:  # some plugins don't export presets; the JSON still has every value
            print(f"  Couldn't save a preset for {plugin.name}: {e}")
    (out / "settings.json").write_text(json.dumps(settings, indent=2))

    print(f"\nDone. Match distance: {score:.2f} (lower is closer; under ~2 is close)")
    print(f"  {out / 'matched.wav'}   your DI through the matched chain")
    print(f"  {out / 'settings.json'}  every knob value, to dial into Gig Performer")
    print(f"  {out}/*.vstpreset      load these in each plugin's preset menu")


if __name__ == "__main__":
    main()
