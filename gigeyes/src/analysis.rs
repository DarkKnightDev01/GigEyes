//! Describes a guitar tone from audio: EQ shape, brightness and how compressed or driven it sounds.

use realfft::{RealFftPlanner, RealToComplex};

const FFT: usize = 4096;
const BANDS: usize = 24;
const LOW_HZ: f32 = 60.0;
const HIGH_HZ: f32 = 12_000.0;

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
    /// Gain, Brightness, Mids, Low end: in that order.
    pub summary: Vec<Reading>,
}

/// One plain-language reading, e.g. label "Mids", verdict "Scooped", detail "800 Hz–2 kHz sits 5 dB below…".
pub struct Reading {
    pub label: &'static str,
    pub verdict: &'static str,
    pub detail: String,
}

fn reading(label: &'static str, verdict: &'static str, detail: impl Into<String>) -> Reading {
    Reading {
        label,
        verdict,
        detail: detail.into(),
    }
}

/// Samples per loudness block for the peak-to-average measure.
const BLOCK: usize = 2048;

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
        }
        self.total += samples.len();
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
        describe(self.power, crest_db, self.total as f32 / rate as f32, rate)
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
    let (lows, low_mids, mids, highs) = (
        region(80.0, 250.0),
        region(250.0, 800.0),
        region(800.0, 2000.0),
        region(2000.0, 5000.0),
    );

    // shortcut: thresholds are rough guesses for guitar, tune them against real reference tracks.
    let gain = match crest_db {
        c if c < 6.0 => reading(
            "Gain",
            "High gain",
            "Squashed and saturated, or heavily compressed.",
        ),
        c if c < 10.0 => reading(
            "Gain",
            "Crunch",
            "Some drive or compression, not fully clean.",
        ),
        _ => reading("Gain", "Clean", "Dynamic: clean or lightly driven."),
    };
    let slope_text = format!("Falls {:.1} dB per octave.", -tilt_db_per_octave);
    let brightness = match tilt_db_per_octave {
        t if t > -3.0 => reading("Brightness", "Bright", slope_text),
        t if t < -7.0 => reading("Brightness", "Dark", slope_text),
        _ => reading("Brightness", "Balanced", slope_text),
    };
    let scoop = mids - (low_mids + highs) / 2.0;
    let mid_reading = if scoop < -4.0 {
        reading(
            "Mids",
            "Scooped",
            format!("800 Hz–2 kHz sits {:.0} dB below its neighbours.", -scoop),
        )
    } else if scoop > 3.0 {
        reading(
            "Mids",
            "Pushed",
            format!("800 Hz–2 kHz sits {scoop:.0} dB above its neighbours."),
        )
    } else {
        reading("Mids", "Even", "Neither scooped nor pushed.")
    };
    let low_end = if lows - low_mids > 2.0 {
        reading("Low end", "Big", "Lots of weight below 250 Hz.")
    } else if lows - low_mids < -6.0 {
        reading("Low end", "Tight", "Little below 250 Hz.")
    } else {
        reading("Low end", "Moderate", "Some weight below 250 Hz.")
    };
    let summary = vec![gain, brightness, mid_reading, low_end];

    Tone {
        centers,
        levels_db,
        tilt_db_per_octave,
        crest_db,
        seconds,
        summary,
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
        assert_eq!(dark.summary[1].verdict, "Dark");
    }

    #[test]
    fn saturated_playing_has_a_lower_crest_than_clean() {
        let clean = analyze(&plucks(1.0), RATE);
        let driven = analyze(&plucks(40.0), RATE);
        assert!(clean.crest_db > 10.0, "clean crest {}", clean.crest_db);
        assert!(driven.crest_db < 6.0, "driven crest {}", driven.crest_db);
        assert_eq!(driven.summary[0].verdict, "High gain");
    }

    #[test]
    fn short_and_silent_input_does_not_panic() {
        let t = analyze(&[0.0; 10], RATE);
        assert_eq!(t.levels_db.len(), BANDS);
    }
}
