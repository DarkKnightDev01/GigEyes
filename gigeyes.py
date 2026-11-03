"""GigEyes: match a guitar tone from a recording by turning knobs in Gig Performer Pro.

Usage:
    python gigeyes.py REFERENCE [--separate] [--evals 150] [--seconds 5]

REFERENCE  song or concert video/audio (anything ffmpeg can read)

Gig Performer does all the audio: your DI loops in an Audio File Player through your plugins,
GigEyes moves OSC-enabled widgets, records the output, and keeps the closest-sounding settings.
See README.md for the one-time Gig Performer setup.
"""
import argparse
import json
import queue
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import numpy as np
from scipy.io import wavfile
from scipy.optimize import differential_evolution
from scipy.signal import stft

SR = 48000
BAND_EDGES = np.geomspace(60, 12000, 41)
RECORDINGS = Path.home() / "Documents" / "Gig Performer Recordings"


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


def read_wav(path):
    """Read a WAV (16/24/32-bit int or float) as mono float32. Returns (audio, sample_rate)."""
    sr, data = wavfile.read(path)
    if data.dtype.kind in "iu":
        data = data / float(np.iinfo(data.dtype).max)
    data = data.astype(np.float32)
    return (data.mean(axis=1) if data.ndim > 1 else data), sr


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

def profile(audio, sr=SR):
    """Level-independent tone fingerprint: spectral shape in 40 log bands + crest factor."""
    f, _, z = stft(audio, sr, nperseg=4096)
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


# ---------- search ----------

def search(measure, set_knobs, n_knobs, target, evals=150, start=None, seed=0):
    """Turn n_knobs (each 0..1) until measure() is closest to target. Ctrl+C keeps the best so far.
    Returns (best values, score)."""
    best = {"x": None, "score": float("inf")}

    def loss(v):
        set_knobs(v)
        score = distance(measure(), target)
        if score < best["score"]:
            best.update(x=np.array(v), score=score)
            print(f"  try {loss.n:4d}: distance {score:.2f}  (new best)")
        loss.n += 1
        return score
    loss.n = 1

    popsize = 5
    maxiter = max(1, evals // (popsize * n_knobs) - 1)
    try:
        differential_evolution(loss, [(0.0, 1.0)] * n_knobs, popsize=popsize, maxiter=maxiter,
                               polish=False, seed=seed, x0=start, tol=1e-6)
    except KeyboardInterrupt:
        print("\nStopped early, keeping the best so far.")
    set_knobs(best["x"])
    return best["x"], best["score"]


# ---------- Gig Performer over OSC ----------

class GigPerformer:
    """Talks to Gig Performer Pro over OSC on this machine."""

    def __init__(self, gp_port, listen_port):
        from pythonosc.dispatcher import Dispatcher
        from pythonosc.osc_server import ThreadingOSCUDPServer
        from pythonosc.udp_client import SimpleUDPClient
        self.client = SimpleUDPClient("127.0.0.1", gp_port)
        self.replies = queue.Queue()
        dispatcher = Dispatcher()
        dispatcher.set_default_handler(lambda addr, *args: self.replies.put((addr, args)))
        try:
            self.server = ThreadingOSCUDPServer(("127.0.0.1", listen_port), dispatcher)
        except OSError:
            sys.exit(f"Port {listen_port} is busy. Close whatever uses it or pass --listen-port.")
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def send(self, address, *args):
        self.client.send_message(address, list(args))

    def ask(self, address, *args, wait=1.0):
        """Send a request and collect every reply that arrives within `wait` seconds."""
        while not self.replies.empty():
            self.replies.get_nowait()
        self.send(address, *args)
        got, end = [], time.time() + wait
        while (left := end - time.time()) > 0:
            try:
                got.append(self.replies.get(timeout=left))
            except queue.Empty:
                break
        return got

    def widgets(self):
        # Reply format isn't documented, so take every string GP sends back.
        names = [a for _, args in self.ask("/RackSpace/ListOSCWidgets") for a in args if isinstance(a, str)]
        return list(dict.fromkeys(names))

    def get(self, widget):
        for _, args in self.ask(f"/{widget}/GetValue", wait=0.5):
            for a in args:
                if isinstance(a, float):
                    return a
        return None

    def set(self, widget, value):
        self.send(f"/{widget}/SetValue", float(value))

    def record(self, seconds, settle, recordings):
        """Record GP's output for `seconds` and return (audio, sr). Deletes the take afterwards."""
        time.sleep(settle)  # let knob changes and effect tails settle
        started = time.time()
        self.send("/RecordInputs", 1)
        time.sleep(seconds)
        self.send("/RecordInputs", 0)
        for _ in range(50):  # wait up to 5 s for GP to finish writing the file
            takes = [p for p in recordings.rglob("O_*.wav") if p.stat().st_mtime >= started - 1]
            if takes:
                break
            time.sleep(0.1)
        else:
            sys.exit(f"No output recording appeared in {recordings}.\n"
                     "Check the recorder has your outputs selected, or pass --recordings.")
        take = max(takes, key=lambda p: p.stat().st_mtime)
        time.sleep(0.2)
        audio, sr = read_wav(take)
        folder = take.parent
        for p in folder.iterdir():  # this take's folder was made by us seconds ago
            p.unlink()
        folder.rmdir()
        return audio, sr


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


def ask_knobs(gp):
    names = gp.widgets()
    if not names:
        sys.exit("Gig Performer didn't list any OSC widgets. Check:\n"
                 "  - Options > OSC Setup: OSC enabled, remote client 127.0.0.1 and port matching --listen-port\n"
                 "  - the knobs you want matched are widgets with 'Enable OSC' ticked and an OSC handle")
    print("\nOSC widgets in the current rackspace:")
    for i, n in enumerate(names, 1):
        print(f"  {i:3}. {n}")
    picks = ask_numbers("Which should GigEyes turn? (numbers, blank = all): ", len(names))
    return [names[i - 1] for i in picks] or names


# ---------- main ----------

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("reference")
    ap.add_argument("--separate", action="store_true", help="isolate the guitar from a full mix first (needs demucs)")
    ap.add_argument("--evals", type=int, default=150, help="how many settings to try (each takes ~seconds+settle)")
    ap.add_argument("--seconds", type=float, default=5, help="seconds recorded per try; at least one loop of your DI")
    ap.add_argument("--settle", type=float, default=0.5, help="seconds to wait after moving knobs")
    ap.add_argument("--gp-port", type=int, default=54344, help="Gig Performer's OSC listening port")
    ap.add_argument("--listen-port", type=int, default=54345, help="port GigEyes listens on (GP's remote client port)")
    ap.add_argument("--recordings", type=Path, default=RECORDINGS, help="Gig Performer's recordings folder")
    ap.add_argument("--out", default="gigeyes_out")
    args = ap.parse_args()

    ref_path = args.reference
    if args.separate:
        ref_path = isolate_guitar(ref_path, tempfile.mkdtemp(prefix="gigeyes_"))
    target = profile(load_audio(ref_path))

    gp = GigPerformer(args.gp_port, args.listen_port)
    knobs = ask_knobs(gp)
    start = [gp.get(k) for k in knobs]
    start = None if None in start else start

    def set_knobs(values):
        for k, v in zip(knobs, values):
            gp.set(k, v)

    def measure():
        audio, sr = gp.record(args.seconds, args.settle, args.recordings)
        if np.max(np.abs(audio)) < 1e-4:
            sys.exit("The recording is silent. Is the Audio File Player looping your DI into the chain?")
        return profile(audio, sr)

    minutes = args.evals * (args.seconds + args.settle + 0.5) / 60
    print(f"\nMatching {len(knobs)} knobs, up to {args.evals} tries (about {minutes:.0f} min). "
          "Keep the DI looping. Ctrl+C stops early.")
    best, score = search(measure, set_knobs, len(knobs), target, args.evals, start)
    if best is None:
        sys.exit("No tries finished.")

    out = Path(args.out)
    out.mkdir(exist_ok=True)
    audio, sr = gp.record(args.seconds, args.settle, args.recordings)
    wavfile.write(out / "matched.wav", sr, audio)
    (out / "settings.json").write_text(json.dumps(dict(zip(knobs, map(float, best))), indent=2))

    print(f"\nDone. Match distance: {score:.2f} (lower is closer; under ~2 is close)")
    print("The widgets in Gig Performer are left on the matched settings: save your gig to keep them.")
    print(f"  {out / 'matched.wav'}    a recording of the matched tone")
    print(f"  {out / 'settings.json'}  widget values (0-1), in case you want them back later")


if __name__ == "__main__":
    main()
