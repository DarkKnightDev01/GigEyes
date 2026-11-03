"""Self-checks, no Gig Performer needed. Run: python test_gigeyes.py

1. The search recovers a hidden tone (pedalboard's built-in effects stand in for your plugins).
2. The OSC side works against a fake Gig Performer that answers the same messages.
"""
import threading
import time
from pathlib import Path

import numpy as np
from scipy.io import wavfile

import gigeyes as g


def fake_di(seconds=3):
    t = np.arange(int(g.SR * seconds)) / g.SR
    notes = [82.4, 110.0, 146.8, 196.0]  # open low strings
    sig = sum(np.sin(2 * np.pi * f * t) * np.exp(-3 * (t % 0.75)) for f in notes)
    return (0.2 * sig).astype(np.float32)


def make(values):
    from pedalboard import Distortion, Gain, HighpassFilter, LowpassFilter, Pedalboard
    drive, cut, low = values
    return Pedalboard([Gain(drive * 30), Distortion(drive * 30), HighpassFilter(60 + low * 200),
                       LowpassFilter(1500 + cut * 8000)])


def test_recovers_hidden_tone():
    di = fake_di()
    target = g.profile(make([0.7, 0.3, 0.5])(di, g.SR))
    state = {"board": make([0, 0, 0])}
    start_score = g.distance(g.profile(state["board"](di, g.SR)), target)
    _, score = g.search(lambda: g.profile(state["board"](di, g.SR)), lambda v: state.update(board=make(v)),
                        3, target, evals=300)
    print(f"search: start {start_score:.2f} -> matched {score:.2f}")
    assert score < 1.0 and score < start_score / 3


def test_osc_round_trip(tmp):
    """Fake GP: lists widgets, stores SetValue, answers GetValue, writes a take on /RecordInputs."""
    from pythonosc.dispatcher import Dispatcher
    from pythonosc.osc_server import ThreadingOSCUDPServer
    from pythonosc.udp_client import SimpleUDPClient

    reply = SimpleUDPClient("127.0.0.1", 54399)
    values = {"AmpGain": 0.25, "Treble": 0.5}

    def handler(addr, *args):
        if addr == "/RackSpace/ListOSCWidgets":
            for n in values:
                reply.send_message("/RackSpace/OSCWidget", n)
        elif addr.endswith("/SetValue"):
            values[addr.split("/")[1]] = args[0]
        elif addr.endswith("/GetValue"):
            reply.send_message(addr.replace("GetValue", "Value"), values[addr.split("/")[1]])
        elif addr == "/RecordInputs" and args[0] == 0:
            take = tmp / "Untitled" / f"take{time.time()}"
            take.mkdir(parents=True)
            wavfile.write(take / "O_Out 1-Out 2.wav", 44100,
                          (np.stack([fake_di(1)] * 2, axis=1) * 32767).astype(np.int16))

    d = Dispatcher()
    d.set_default_handler(handler)
    fake = ThreadingOSCUDPServer(("127.0.0.1", 54398), d)
    threading.Thread(target=fake.serve_forever, daemon=True).start()

    gp = g.GigPerformer(54398, 54399)
    assert gp.widgets() == ["AmpGain", "Treble"]
    assert abs(gp.get("AmpGain") - 0.25) < 1e-6
    gp.set("Treble", 0.8)
    time.sleep(0.2)
    assert abs(values["Treble"] - 0.8) < 1e-6
    audio, sr = gp.record(0.2, 0, tmp)
    assert sr == 44100 and audio.ndim == 1 and np.max(np.abs(audio)) > 0.1
    assert not list(tmp.rglob("*.wav")), "take should be cleaned up"
    fake.shutdown()
    gp.server.shutdown()
    print("osc: widgets listed, values set/read, take recorded and cleaned up")


if __name__ == "__main__":
    import tempfile
    test_recovers_hidden_tone()
    test_osc_round_trip(Path(tempfile.mkdtemp()))
    print("ok")
