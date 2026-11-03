"""Self-check: can the search recover a hidden tone? Uses pedalboard's built-in effects, no VSTs needed.
Run: python test_gigeyes.py
"""
import numpy as np
from pedalboard import Distortion, Gain, LowpassFilter, HighpassFilter, Pedalboard

import gigeyes as g


def fake_di(seconds=3):
    t = np.arange(int(g.SR * seconds)) / g.SR
    notes = [82.4, 110.0, 146.8, 196.0]  # open low strings
    sig = sum(np.sin(2 * np.pi * f * t) * np.exp(-3 * (t % 0.75)) for f in notes)
    return (0.2 * sig).astype(np.float32)


def make(values):
    drive, cut, low = values
    return Pedalboard([Gain(drive * 30), Distortion(drive * 30), HighpassFilter(60 + low * 200),
                       LowpassFilter(1500 + cut * 8000)])


def test_recovers_hidden_tone():
    di = fake_di()
    hidden = [0.7, 0.3, 0.5]
    target = g.profile(g.to_mono(make(hidden)(di, g.SR)))

    state = {"board": make([0, 0, 0])}
    start_score = g.distance(g.profile(state["board"](di, g.SR)), target)
    _, score = g.search(lambda: state["board"](di, g.SR), lambda v: state.update(board=make(v)),
                        3, target, evals=300)
    print(f"start {start_score:.2f} -> matched {score:.2f}")
    assert score < 1.0 and score < start_score / 3


if __name__ == "__main__":
    test_recovers_hidden_tone()
    print("ok")
