//! Describes a guitar tone from audio: EQ shape, brightness and how compressed or driven it sounds.

use realfft::{RealFftPlanner, RealToComplex};

const FFT: usize = 4096;
const BANDS: usize = 24;
const LOW_HZ: f32 = 60.0;
const HIGH_HZ: f32 = 12_000.0;

#[allow(dead_code)] // the band levels and slope only feed the knobs and the tests now
pub struct Tone {
    /// Band centre frequencies in Hz.
    pub centers: Vec<f32>,
    /// Band levels in dB, relative to the average of all bands.
    pub levels_db: Vec<f32>,
    /// Spectral slope: how fast the level falls per octave. More negative = darker.
    pub tilt_db_per_octave: f32,
    /// Peak-to-average ratio. Low = squashed (high gain or heavy compression), high = dynamic.
    pub crest_db: f32,
    pub seconds: f32,
    /// One sentence in player's words, e.g. "High-gain, with scooped mids, a bright top and a big low end."
    pub verdict: String,
    /// Gain, Bass, Middle, Treble, Presence: where an amp's knobs would sit, 0 to 10.
    pub knobs: Vec<Knob>,
    /// Beats per minute, when the recording has a clear beat.
    pub tempo_bpm: Option<f32>,
    /// How long the sound takes to die away by 60 dB at the fastest, in seconds: about the
    /// reverb's decay time. None when too little of the recording dies away to tell.
    pub reverb_s: Option<f32>,
    /// A delay heard in the recording.
    pub echo: Option<Echo>,
}

impl Tone {
    /// "120 BPM · an echo 375 ms after each note · about 2.1 s of reverb": the time-based
    /// things heard, in one line; empty when nothing was.
    pub fn heard(&self) -> String {
        let mut parts = vec![];
        if let Some(bpm) = self.tempo_bpm {
            parts.push(format!("{bpm:.0} BPM"));
        }
        if let Some(e) = self.echo {
            parts.push(format!("an echo {:.0} ms after each note", e.ms));
        }
        match self.reverb_s {
            Some(rt) if rt < 0.4 => parts.push("dry, the notes stop dead".into()),
            Some(rt) => parts.push(format!("about {rt:.1} s of reverb")),
            None => {}
        }
        parts.join(" · ")
    }
}

/// A delay: copies of the sound at a fixed time after it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Echo {
    pub ms: f32,
    /// The first repeat's level against the dry sound, 0 to 1.
    pub level: f32,
    /// Each repeat's level against the one before, 0 to 1; None when the recording is too
    /// short to hear a second repeat.
    pub feedback: Option<f32>,
}

pub struct Knob {
    pub label: &'static str,
    /// 0 to 10, 5 is noon.
    pub value: f32,
    /// One or two words, e.g. "Scooped".
    pub verdict: &'static str,
    /// The measurement behind it, shown on hover.
    pub detail: String,
}

/// Samples per loudness block for the peak-to-average measure.
const BLOCK: usize = 2048;
/// Samples per step of the attack envelope the tempo comes from: about 12 ms.
const HOP: usize = 512;
/// The tempo is read from the first four minutes or so: enough, and quick.
const TEMPO_STEPS: usize = 20_000;
/// Samples per frame for finding echoes: about 2.7 s, so a frame holds a sound and its repeat.
const ECHO_FFT: usize = 1 << 17;
/// Echoes are looked for in the first six minutes or so.
const ECHO_FRAMES: usize = 250;

/// Builds a Tone from audio fed in pieces, so a long concert never sits in memory whole.
pub struct Analyzer {
    fft: std::sync::Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    output: Vec<realfft::num_complex::Complex<f32>>,
    /// Summed power spectrum of all full FFT frames so far.
    power: Vec<f32>,
    frames: usize,
    /// Samples not yet used up by FFT frames (frames overlap by half).
    pending: Vec<f32>,
    /// Loudness (RMS) of each finished block, plus the block being filled.
    loudness: Vec<f32>,
    block_sum: f32,
    block_len: usize,
    peak: f32,
    total: usize,
    /// Energy of the sample-to-sample change per HOP: high at each pick attack and drum hit.
    attacks: Vec<f32>,
    attack_sum: f32,
    attack_len: usize,
    last: f32,
    /// Loudness (energy) per HOP, for how fast the sound dies away.
    envelope: Vec<f32>,
    envelope_sum: f32,
    echoes: Echoes,
}

impl Analyzer {
    pub fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT);
        let (input, output) = (fft.make_input_vec(), fft.make_output_vec());
        Self {
            window: (0..FFT)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT as f32).cos())
                .collect(),
            power: vec![0.0; output.len()],
            fft,
            input,
            output,
            frames: 0,
            pending: Vec::with_capacity(FFT * 2),
            loudness: vec![],
            block_sum: 0.0,
            block_len: 0,
            peak: 0.0,
            total: 0,
            attacks: vec![],
            attack_sum: 0.0,
            attack_len: 0,
            last: 0.0,
            envelope: vec![],
            envelope_sum: 0.0,
            echoes: Echoes::new(),
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        for &s in samples {
            self.peak = self.peak.max(s.abs());
            self.block_sum += s * s;
            self.block_len += 1;
            if self.block_len == BLOCK {
                self.end_block();
            }
            if self.attacks.len() < TEMPO_STEPS {
                let change = s - self.last;
                self.attack_sum += change * change;
                self.envelope_sum += s * s;
                self.attack_len += 1;
                if self.attack_len == HOP {
                    self.attacks.push(self.attack_sum);
                    self.envelope.push(self.envelope_sum);
                    (self.attack_sum, self.envelope_sum, self.attack_len) = (0.0, 0.0, 0);
                }
            }
            self.last = s;
        }
        self.total += samples.len();
        self.echoes.push(samples);
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= FFT {
            self.add_frame();
            self.pending.drain(..FFT / 2);
        }
    }

    pub fn finish(mut self, rate: u32) -> Tone {
        if self.frames == 0 {
            self.add_frame(); // shorter than one frame: use what there is, zero-padded
        }
        if self.block_len > 0 {
            self.end_block();
        }
        let frames = self.frames as f32;
        self.power.iter_mut().for_each(|p| *p /= frames);
        let crest_db = crest(&self.loudness, self.peak);
        let mut tone = describe(self.power, crest_db, self.total as f32 / rate as f32, rate);
        tone.tempo_bpm = tempo(&self.attacks, rate);
        tone.reverb_s = decay(&self.envelope, rate);
        tone.echo = self.echoes.finish(rate);
        tone
    }

    /// Hann-windowed FFT of the first FFT samples of `pending`, added to the running spectrum.
    fn add_frame(&mut self) {
        for (i, (x, w)) in self.input.iter_mut().zip(&self.window).enumerate() {
            *x = self.pending.get(i).copied().unwrap_or(0.0) * w;
        }
        self.fft
            .process(&mut self.input, &mut self.output)
            .expect("buffer sizes come from the planner");
        self.power
            .iter_mut()
            .zip(&self.output)
            .for_each(|(p, c)| *p += c.norm_sqr());
        self.frames += 1;
    }

    fn end_block(&mut self) {
        self.loudness
            .push((self.block_sum / self.block_len as f32).sqrt());
        self.block_sum = 0.0;
        self.block_len = 0;
    }
}

fn describe(power: Vec<f32>, crest_db: f32, seconds: f32, rate: u32) -> Tone {
    let rate_f = rate as f32;
    let bin_hz = rate_f / FFT as f32;

    // Band energy from the cumulative spectrum, so bands narrower than one FFT bin still get a value.
    let cumulative: Vec<f32> = power
        .iter()
        .scan(0.0, |acc, p| {
            *acc += p;
            Some(*acc)
        })
        .collect();
    let energy_below = |hz: f32| {
        let pos = (hz / bin_hz).clamp(0.0, (cumulative.len() - 1) as f32);
        let (i, frac) = (pos.floor() as usize, pos.fract());
        let next = cumulative[(i + 1).min(cumulative.len() - 1)];
        cumulative[i] + (next - cumulative[i]) * frac
    };
    let high = HIGH_HZ.min(rate_f * 0.45);
    let edges: Vec<f32> = (0..=BANDS)
        .map(|i| LOW_HZ * (high / LOW_HZ).powf(i as f32 / BANDS as f32))
        .collect();
    let mut levels_db: Vec<f32> = edges
        .windows(2)
        .map(|w| 10.0 * ((energy_below(w[1]) - energy_below(w[0])) / (w[1] - w[0]) + 1e-12).log10())
        .collect();
    let mean = levels_db.iter().sum::<f32>() / BANDS as f32;
    levels_db.iter_mut().for_each(|l| *l -= mean);
    let centers: Vec<f32> = edges.windows(2).map(|w| (w[0] * w[1]).sqrt()).collect();

    let tilt_db_per_octave = slope(
        &centers.iter().map(|c| c.log2()).collect::<Vec<_>>(),
        &levels_db,
    );
    let region = |lo: f32, hi: f32| {
        let v: Vec<f32> = centers
            .iter()
            .zip(&levels_db)
            .filter(|(c, _)| **c >= lo && **c < hi)
            .map(|(_, l)| *l)
            .collect();
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f32>() / v.len() as f32
        }
    };
    let (lows, low_mids, mids, highs, air) = (
        region(80.0, 250.0),
        region(250.0, 800.0),
        region(800.0, 2000.0),
        region(2000.0, 5000.0),
        region(5000.0, 12_000.0),
    );

    // shortcut: thresholds and knob mappings are rough guesses for guitar, tune them on real tracks.
    let knob_at = |value: f32| value.clamp(0.0, 10.0);
    let gain = Knob {
        label: "Gain",
        value: knob_at((16.0 - crest_db) / 12.0 * 10.0),
        verdict: match crest_db {
            c if c < 6.0 => "High gain",
            c if c < 10.0 => "Crunch",
            _ => "Clean",
        },
        detail: format!(
            "Peaks sit {crest_db:.1} dB over the average level. Lower means more squashed."
        ),
    };
    let low_end = lows - low_mids;
    let bass = Knob {
        label: "Bass",
        value: knob_at(5.0 + low_end / 2.0),
        verdict: if low_end > 2.0 {
            "Big"
        } else if low_end < -6.0 {
            "Tight"
        } else {
            "Moderate"
        },
        detail: format!("80–250 Hz sits {} 250–800 Hz.", relative(low_end)),
    };
    let scoop = mids - (low_mids + highs) / 2.0;
    let middle = Knob {
        label: "Middle",
        value: knob_at(5.0 + scoop / 1.5),
        verdict: if scoop < -4.0 {
            "Scooped"
        } else if scoop > 3.0 {
            "Pushed"
        } else {
            "Even"
        },
        detail: format!("800 Hz–2 kHz sits {} its neighbours.", relative(scoop)),
    };
    let treble = Knob {
        label: "Treble",
        value: knob_at(5.0 + (tilt_db_per_octave + 5.0)),
        verdict: match tilt_db_per_octave {
            t if t > -3.0 => "Bright",
            t if t < -7.0 => "Dark",
            _ => "Balanced",
        },
        detail: format!(
            "The level falls {:.1} dB per octave from lows to highs.",
            -tilt_db_per_octave
        ),
    };
    let sheen = air - highs;
    let presence = Knob {
        label: "Presence",
        value: knob_at(5.0 + (sheen + 6.0) / 2.0),
        verdict: if sheen > -3.0 {
            "Airy"
        } else if sheen < -10.0 {
            "Rolled off"
        } else {
            "Smooth"
        },
        detail: format!("5–12 kHz sits {} 2–5 kHz.", relative(sheen)),
    };

    let verdict = format!(
        "{}, with {} mids, a {} top and a {} low end.",
        match gain.verdict {
            "High gain" => "High-gain",
            "Crunch" => "Crunchy",
            _ => "Clean",
        },
        middle.verdict.to_lowercase(),
        treble.verdict.to_lowercase(),
        bass.verdict.to_lowercase(),
    );

    Tone {
        centers,
        levels_db,
        tilt_db_per_octave,
        crest_db,
        seconds,
        verdict,
        knobs: vec![gain, bass, middle, treble, presence],
        tempo_bpm: None,
        reverb_s: None,
        echo: None,
    }
}

/// The reverb's decay time from how fast the sound dies away: a note can stop at once, but its
/// reverb can't fade faster than the reverb's own decay. So the fastest fades, not the typical
/// ones, give it. Each fade is timed from 15 to 35 dB down, past the note itself and into its
/// tail. In seconds to fall 60 dB.
fn decay(envelope: &[f32], rate: u32) -> Option<f32> {
    let step = HOP as f32 / rate as f32;
    let db: Vec<f32> = envelope
        .iter()
        .map(|e| 10.0 * (e / HOP as f32 + 1e-12).log10())
        .collect();
    // Five steps (about 50 ms) averaged, so a reverb tail's grain doesn't end a fade.
    let db: Vec<f32> = db.windows(5).map(|w| w.iter().sum::<f32>() / 5.0).collect();
    let loudest = db.iter().copied().fold(f32::MIN, f32::max);
    let mut rates = vec![];
    let mut i = 1;
    while i + 1 < db.len() {
        let top = db[i];
        // A fade starts at a peak in the louder part of the recording.
        if top < loudest - 25.0 || top < db[i - 1] || top < db[i + 1] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < db.len() && db[j + 1] <= db[j] + 0.5 && db[j] > top - 40.0 {
            j += 1;
        }
        let first = (i..=j).find(|&k| db[k] <= top - 15.0);
        let last = (i..=j).find(|&k| db[k] <= top - 35.0);
        if let (Some(a), Some(b)) = (first, last) {
            rates.push((db[a] - db[b]) / ((b - a).max(1) as f32 * step));
        }
        i = j.max(i + 1);
    }
    if rates.len() < 3 {
        return None;
    }
    rates.sort_by(f32::total_cmp);
    // The 80th percentile: the fastest fades, past a stray few.
    let fastest = rates[(rates.len() * 4 / 5).min(rates.len() - 1)];
    Some(60.0 / fastest)
}

/// Finds a delay from the cepstrum: an echo is an exact copy of the sound, so the log spectrum
/// ripples at a rate set by the delay time, and that ripple turns into a sharp peak at the
/// delay time. Played notes that repeat are never exact copies, so a beat doesn't make one.
struct Echoes {
    fft: std::sync::Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    output: Vec<realfft::num_complex::Complex<f32>>,
    pending: Vec<f32>,
    /// Summed log magnitude spectrum of the frames so far.
    log: Vec<f32>,
    frames: usize,
}

impl Echoes {
    fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(ECHO_FFT);
        let (input, output) = (fft.make_input_vec(), fft.make_output_vec());
        Self {
            window: (0..ECHO_FFT)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / ECHO_FFT as f32).cos())
                .collect(),
            log: vec![0.0; output.len()],
            fft,
            input,
            output,
            pending: vec![],
            frames: 0,
        }
    }

    fn push(&mut self, samples: &[f32]) {
        if self.frames >= ECHO_FRAMES {
            return;
        }
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= ECHO_FFT && self.frames < ECHO_FRAMES {
            for ((x, w), s) in self.input.iter_mut().zip(&self.window).zip(&self.pending) {
                *x = s * w;
            }
            self.pending.drain(..ECHO_FFT / 2);
            self.fft
                .process(&mut self.input, &mut self.output)
                .expect("buffer sizes come from the planner");
            let power: Vec<f32> = self.output.iter().map(|c| c.norm_sqr()).collect();
            let top = power.iter().copied().fold(0.0, f32::max);
            if top <= 0.0 {
                continue; // silence
            }
            // Floored 100 dB down, so bins with nothing in them (above an MP3's cut-off) stay flat.
            for (l, p) in self.log.iter_mut().zip(&power) {
                *l += 0.5 * (p + top * 1e-10).ln();
            }
            self.frames += 1;
        }
    }

    fn finish(self, rate: u32) -> Option<Echo> {
        if self.frames == 0 {
            return None;
        }
        let inverse = RealFftPlanner::<f32>::new().plan_fft_inverse(ECHO_FFT);
        let mut spectrum: Vec<realfft::num_complex::Complex<f32>> = self
            .log
            .iter()
            .map(|l| realfft::num_complex::Complex::new(l / self.frames as f32, 0.0))
            .collect();
        let mut cepstrum = inverse.make_output_vec();
        inverse.process(&mut spectrum, &mut cepstrum).ok()?;
        cepstrum.iter_mut().for_each(|c| *c /= ECHO_FFT as f32);
        // Delays from a slapback (80 ms) to a long one (1.2 s), within the frame.
        let short = (0.08 * rate as f32) as usize;
        let long = ((1.2 * rate as f32) as usize).min(ECHO_FFT / 2 - 2);
        let range = &cepstrum[short..long];
        let mut sorted: Vec<f32> = range.to_vec();
        sorted.sort_by(f32::total_cmp);
        let median = sorted[sorted.len() / 2];
        let mut spread: Vec<f32> = sorted.iter().map(|c| (c - median).abs()).collect();
        spread.sort_by(f32::total_cmp);
        let noise = 1.4826 * spread[spread.len() / 2];
        let (at, &level) = range.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
        // Far above the ripple of the music itself, and loud enough to hear.
        if level < median + 10.0 * noise || level < 0.05 {
            return None;
        }
        let lag = short + at;
        // A real cepstrum splits each peak between +lag and −lag, so it shows half; and the
        // window lets only part of a sound and its repeat into the same frame.
        let overlap = |d: usize| {
            let both: f32 = (0..ECHO_FFT - d)
                .map(|i| self.window[i] * self.window[i + d])
                .sum();
            both / self.window.iter().map(|w| w * w).sum::<f32>()
        };
        let first = 2.0 * level / overlap(lag);
        // The second repeat sits at twice the time. With repeats a, a·g, a·g²…, the cepstrum
        // there is a(2g − a)/2, which gives g.
        let feedback = (2 * lag + 2 < ECHO_FFT / 2).then(|| {
            let second = cepstrum[2 * lag - 2..=2 * lag + 2]
                .iter()
                .copied()
                .max_by(|a, b| a.abs().total_cmp(&b.abs()))
                .unwrap_or(0.0);
            let second = 2.0 * second / overlap(2 * lag);
            (second / first + first / 2.0).clamp(0.0, 0.95)
        });
        Some(Echo {
            ms: lag as f32 / rate as f32 * 1000.0,
            level: first.min(1.0),
            feedback,
        })
    }
}

/// The beat, from how often the attacks repeat: the strongest repeat between 70 and 180 BPM,
/// leaning towards 120 so half or double the tempo loses a near tie. None without a clear beat.
fn tempo(attacks: &[f32], rate: u32) -> Option<f32> {
    let step = HOP as f32 / rate as f32;
    if attacks.len() as f32 * step < 10.0 {
        return None;
    }
    // Onsets: how much louder each step is than the one before.
    // Squashed like loudness, but relative to the average, so hiss between notes stays near zero.
    let typical = attacks.iter().sum::<f32>() / attacks.len() as f32 + 1e-12;
    let level: Vec<f32> = attacks
        .iter()
        .map(|e| (1.0 + 10.0 * e / typical).ln())
        .collect();
    let rises: Vec<f32> = level.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect();
    // Blurred over ±2 steps, so a beat that falls between steps still lines up with itself.
    let mut onset: Vec<f32> = rises
        .windows(5)
        .map(|w| (w[0] + 2.0 * w[1] + 3.0 * w[2] + 2.0 * w[3] + w[4]) / 9.0)
        .collect();
    let mean = onset.iter().sum::<f32>() / onset.len() as f32;
    onset.iter_mut().for_each(|o| *o -= mean);
    let lag_of = |bpm: f32| 60.0 / (bpm * step);
    let (short, long) = (lag_of(180.0).floor() as usize, lag_of(70.0).ceil() as usize);
    // Up to twice the longest lag: a beat also repeats every two beats.
    let at = |lag: usize| {
        if lag >= onset.len() {
            return 0.0;
        }
        let n = onset.len() - lag;
        (0..n).map(|i| onset[i] * onset[i + lag]).sum::<f32>() / n as f32
    };
    let whole = at(0);
    let repeat: Vec<f32> = (0..=2 * long + 2).map(at).collect();
    let lean = |lag: usize| {
        let octaves = (60.0 / (lag as f32 * step) / 120.0).log2();
        (-0.5 * (octaves / 1.4).powi(2)).exp()
    };
    // A beat at this lag repeats at twice it too; a half-time lag lacks the beats in between.
    let score = |lag: usize| (repeat[lag] + 0.5 * repeat[2 * lag]) * lean(lag);
    let best = (short.max(1)..=long)
        .filter(|&l| repeat[l] >= repeat[l - 1] && repeat[l] >= repeat[l + 1])
        .max_by(|&a, &b| score(a).total_cmp(&score(b)))?;
    // Every other beat repeats as well as every beat: take the faster when it's nearly as strong.
    let half = best as f32 / 2.0;
    let best = ((half - 1.0).floor() as usize..=(half + 1.0).ceil() as usize)
        .filter(|&l| l >= short.max(1))
        .max_by(|&a, &b| repeat[a].total_cmp(&repeat[b]))
        .filter(|&h| repeat[h] >= 0.6 * repeat[best])
        .unwrap_or(best);
    // Noise repeats at no lag: the peak must hold a good share of the onsets' whole energy.
    if whole <= 0.0 || repeat[best] < 0.15 * whole {
        return None;
    }
    // Between steps: the top of a parabola through the peak and its neighbours.
    let (a, b, c) = (repeat[best - 1], repeat[best], repeat[best + 1]);
    let shift = 0.5 * (a - c) / (a - 2.0 * b + c).min(-1e-12);
    let bpm = 60.0 / (best as f32 + shift) / step;
    (70.0..=180.0).contains(&bpm).then_some(bpm)
}

/// "5 dB below" / "3 dB above" / "level with", for knob details.
fn relative(db: f32) -> String {
    match db.round() {
        d if d < 0.0 => format!("{:.0} dB below", -d),
        d if d > 0.0 => format!("{d:.0} dB above"),
        _ => "level with".into(),
    }
}

/// Least-squares slope of y over x.
fn slope(x: &[f32], y: &[f32]) -> f32 {
    let n = x.len() as f32;
    let (mx, my) = (x.iter().sum::<f32>() / n, y.iter().sum::<f32>() / n);
    let cov: f32 = x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)).sum();
    let var: f32 = x.iter().map(|a| (a - mx).powi(2)).sum();
    if var == 0.0 { 0.0 } else { cov / var }
}

/// Peak-to-RMS in dB, ignoring near-silent blocks so gaps between notes don't inflate it.
fn crest(loudness: &[f32], peak: f32) -> f32 {
    let loudest = loudness.iter().cloned().fold(0.0, f32::max);
    let active: Vec<f32> = loudness
        .iter()
        .cloned()
        .filter(|r| *r > loudest * 0.05)
        .collect();
    if active.is_empty() {
        return 0.0;
    }
    let level = (active.iter().map(|r| r * r).sum::<f32>() / active.len() as f32).sqrt();
    20.0 * (peak / level).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(samples: &[f32], rate: u32) -> Tone {
        let mut a = Analyzer::new();
        a.push(samples);
        a.finish(rate)
    }

    #[test]
    fn feeding_in_pieces_gives_the_same_tone_as_all_at_once() {
        let x = plucks(10.0);
        let whole = analyze(&x, RATE);
        let mut a = Analyzer::new();
        x.chunks(1000).for_each(|c| a.push(c)); // odd size, so blocks and frames straddle pieces
        let pieces = a.finish(RATE);
        assert_eq!(whole.levels_db, pieces.levels_db);
        assert_eq!(
            (whole.crest_db, whole.seconds),
            (pieces.crest_db, pieces.seconds)
        );
    }

    const RATE: u32 = 48_000;

    fn noise(n: usize) -> Vec<f32> {
        let mut seed = 1u32;
        (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect()
    }

    fn lowpass(x: &[f32], hz: f32) -> Vec<f32> {
        let a = (-std::f32::consts::TAU * hz / RATE as f32).exp();
        let mut y = 0.0;
        x.iter()
            .map(|s| {
                y = (1.0 - a) * s + a * y;
                y
            })
            .collect()
    }

    /// Plucked low E: a decaying sine every half second.
    fn plucks(drive: f32) -> Vec<f32> {
        (0..RATE as usize * 3)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let s = (std::f32::consts::TAU * 82.4 * t).sin() * (-12.0 * (t % 0.5)).exp();
                (drive * s).tanh()
            })
            .collect()
    }

    #[test]
    fn darker_audio_has_a_steeper_tilt_and_says_so() {
        let white = analyze(&noise(RATE as usize * 2), RATE);
        // Two filters in a row: -12 dB per octave above 300 Hz, like a dark speaker cab.
        let dark = analyze(
            &lowpass(&lowpass(&noise(RATE as usize * 2), 300.0), 300.0),
            RATE,
        );
        assert!(
            white.tilt_db_per_octave > -1.0,
            "white noise is flat: {}",
            white.tilt_db_per_octave
        );
        assert!(dark.tilt_db_per_octave < white.tilt_db_per_octave - 3.0);
        assert_eq!(dark.knobs[3].verdict, "Dark");
        assert!(dark.knobs[3].value < white.knobs[3].value);
    }

    #[test]
    fn saturated_playing_has_a_lower_crest_than_clean() {
        let clean = analyze(&plucks(1.0), RATE);
        let driven = analyze(&plucks(40.0), RATE);
        assert!(clean.crest_db > 10.0, "clean crest {}", clean.crest_db);
        assert!(driven.crest_db < 6.0, "driven crest {}", driven.crest_db);
        assert_eq!(driven.knobs[0].verdict, "High gain");
        assert!(
            driven.verdict.starts_with("High-gain, with"),
            "{}",
            driven.verdict
        );
        assert!(driven.knobs.iter().all(|k| (0.0..=10.0).contains(&k.value)));
    }

    /// Muted strums on the beat over a little noise.
    fn strums(bpm: f32, seconds: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let beat = (60.0 / bpm * RATE as f32) as usize;
        let hiss = noise(n);
        (0..n)
            .map(|i| {
                let since = i % beat;
                let strum = (-(since as f32) / (0.03 * RATE as f32)).exp() * hiss[(i * 7) % n];
                0.8 * strum + 0.02 * hiss[i]
            })
            .collect()
    }

    #[test]
    fn finds_the_tempo_of_a_steady_beat() {
        for bpm in [96.0, 120.0, 150.0] {
            let found = analyze(&strums(bpm, 30.0), RATE)
                .tempo_bpm
                .expect("a clear beat");
            assert!((found - bpm).abs() < 2.0, "{bpm} BPM read as {found}");
        }
    }

    #[test]
    fn no_tempo_without_a_beat() {
        assert_eq!(analyze(&noise(RATE as usize * 20), RATE).tempo_bpm, None);
        assert_eq!(analyze(&strums(120.0, 5.0), RATE).tempo_bpm, None);
    }

    /// Notes plucked from fresh noise each time, so no two are exact copies, cut off short, in
    /// phrases of eight with a rest after each.
    fn phrases(seconds: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let hiss = noise(n + RATE as usize);
        let mut out = vec![0.0; n];
        let pitches = [82.4, 110.0, 146.8, 196.0, 246.9, 329.6, 220.0, 164.8];
        let (note, bar) = (RATE as usize / 4, RATE as usize * 7 / 2);
        for start in (0..n).step_by(note) {
            let k = start / note;
            if start % bar >= 8 * note {
                continue; // the rest
            }
            let period = (RATE as f32 / pitches[k % 8]) as usize;
            let mut string: Vec<f32> = hiss[(k * 977) % n..][..period].to_vec();
            for i in 0..note.min(n - start) {
                let s = string[i % period];
                string[i % period] = 0.996 * 0.5 * (s + string[(i + 1) % period]);
                // Cut off over 5 ms, 200 ms in.
                let gate =
                    ((0.205 * RATE as f32 - i as f32) / (0.005 * RATE as f32)).clamp(0.0, 1.0);
                out[start + i] += s * gate;
            }
        }
        out
    }

    /// Repeats `ms` later at `level`, each next one `feedback` as loud.
    fn delayed(x: &[f32], ms: f32, level: f32, feedback: f32) -> Vec<f32> {
        let d = (ms / 1000.0 * RATE as f32) as usize;
        let mut wet = vec![0.0; x.len()];
        for i in d..x.len() {
            wet[i] = level * x[i - d] + feedback * wet[i - d];
        }
        x.iter().zip(&wet).map(|(a, b)| a + b).collect()
    }

    /// A small Schroeder reverb: four combs set to fall 60 dB in `rt` seconds, then two
    /// all-passes to blur them.
    fn reverb(x: &[f32], rt: f32) -> Vec<f32> {
        let mut wet = vec![0.0; x.len()];
        for ms in [29.7, 37.1, 41.1, 43.7] {
            let d = (ms / 1000.0 * RATE as f32) as usize;
            let g = 10f32.powf(-3.0 * ms / 1000.0 / rt);
            let mut y = vec![0.0; x.len()];
            for i in 0..x.len() {
                y[i] = x[i] + if i >= d { g * y[i - d] } else { 0.0 };
                wet[i] += y[i] / 4.0;
            }
        }
        for ms in [5.0, 1.7] {
            let d = (ms / 1000.0 * RATE as f32) as usize;
            let mut y = vec![0.0; x.len()];
            for i in 0..x.len() {
                let back = if i >= d { y[i - d] } else { 0.0 };
                let fwd = if i >= d { wet[i - d] } else { 0.0 };
                y[i] = -0.7 * wet[i] + fwd + 0.7 * back;
            }
            wet = y;
        }
        x.iter().zip(&wet).map(|(a, b)| a + 0.3 * b).collect()
    }

    #[test]
    fn hears_a_delay_its_time_and_repeats() {
        let echo = analyze(&delayed(&phrases(30.0), 375.0, 0.4, 0.35), RATE)
            .echo
            .expect("a delay");
        assert!((echo.ms - 375.0).abs() < 2.0, "{echo:?}");
        assert!((echo.level - 0.4).abs() < 0.1, "{echo:?}");
        let feedback = echo.feedback.expect("room for a second repeat");
        assert!((feedback - 0.35).abs() < 0.12, "{echo:?}");
    }

    #[test]
    fn no_delay_in_dry_playing_or_a_beat() {
        assert_eq!(analyze(&phrases(30.0), RATE).echo, None);
        assert_eq!(analyze(&strums(120.0, 30.0), RATE).echo, None);
        assert_eq!(analyze(&noise(RATE as usize * 10), RATE).echo, None);
    }

    #[test]
    fn hears_how_long_the_reverb_is() {
        let dry = analyze(&phrases(30.0), RATE)
            .reverb_s
            .expect("fades to time");
        assert!(dry < 0.4, "dry playing read as {dry} s");
        for rt in [1.0, 2.5] {
            let wet = analyze(&reverb(&phrases(30.0), rt), RATE)
                .reverb_s
                .expect("fades to time");
            assert!(
                (wet / rt - 1.0).abs() < 0.35,
                "{rt} s reverb read as {wet} s"
            );
        }
    }

    #[test]
    fn short_and_silent_input_does_not_panic() {
        let t = analyze(&[0.0; 10], RATE);
        assert_eq!(t.levels_db.len(), BANDS);
    }
}
