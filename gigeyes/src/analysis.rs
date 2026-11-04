//! Describes a guitar tone from audio: EQ shape, brightness and how compressed or driven it sounds.

use realfft::RealFftPlanner;

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
    pub summary: Vec<String>,
}

pub fn analyze(samples: &[f32], rate: u32) -> Tone {
    let rate_f = rate as f32;
    let power = average_spectrum(samples);
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
    let edges: Vec<f32> =
        (0..=BANDS).map(|i| LOW_HZ * (high / LOW_HZ).powf(i as f32 / BANDS as f32)).collect();
    let mut levels_db: Vec<f32> = edges
        .windows(2)
        .map(|w| 10.0 * ((energy_below(w[1]) - energy_below(w[0])) / (w[1] - w[0]) + 1e-12).log10())
        .collect();
    let mean = levels_db.iter().sum::<f32>() / BANDS as f32;
    levels_db.iter_mut().for_each(|l| *l -= mean);
    let centers: Vec<f32> = edges.windows(2).map(|w| (w[0] * w[1]).sqrt()).collect();

    let tilt_db_per_octave = slope(&centers.iter().map(|c| c.log2()).collect::<Vec<_>>(), &levels_db);
    let crest_db = crest(samples);
    let region = |lo: f32, hi: f32| {
        let v: Vec<f32> =
            centers.iter().zip(&levels_db).filter(|(c, _)| **c >= lo && **c < hi).map(|(_, l)| *l).collect();
        if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 }
    };
    let (lows, low_mids, mids, highs) =
        (region(80.0, 250.0), region(250.0, 800.0), region(800.0, 2000.0), region(2000.0, 5000.0));

    let mut summary = vec![];
    // shortcut: thresholds are rough guesses for guitar, tune them against real reference tracks.
    summary.push(match crest_db {
        c if c < 6.0 => "Squashed and saturated: high gain or heavy compression.".to_string(),
        c if c < 10.0 => "Some drive or compression: crunchy, not fully clean.".to_string(),
        _ => "Dynamic: clean or lightly driven.".to_string(),
    });
    summary.push(match tilt_db_per_octave {
        t if t > -3.0 => "Bright overall.".to_string(),
        t if t < -7.0 => "Dark overall.".to_string(),
        _ => "Balanced brightness.".to_string(),
    });
    let scoop = mids - (low_mids + highs) / 2.0;
    summary.push(if scoop < -4.0 {
        format!("Scooped mids: 800 Hz–2 kHz sits {:.0} dB below its neighbours.", -scoop)
    } else if scoop > 3.0 {
        format!("Mid-forward: 800 Hz–2 kHz sits {scoop:.0} dB above its neighbours.")
    } else {
        "Mids are neither scooped nor pushed.".to_string()
    });
    summary.push(if lows - low_mids > 2.0 {
        "Big low end.".to_string()
    } else if lows - low_mids < -6.0 {
        "Tight low end: little below 250 Hz.".to_string()
    } else {
        "Moderate low end.".to_string()
    });

    Tone {
        centers,
        levels_db,
        tilt_db_per_octave,
        crest_db,
        seconds: samples.len() as f32 / rate_f,
        summary,
    }
}

/// Average power spectrum over Hann-windowed frames with 50% overlap.
fn average_spectrum(samples: &[f32]) -> Vec<f32> {
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT);
    let window: Vec<f32> =
        (0..FFT).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT as f32).cos()).collect();
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut power = vec![0.0f32; output.len()];
    let mut frames = 0;
    let mut start = 0;
    loop {
        let chunk = &samples[start.min(samples.len())..(start + FFT).min(samples.len())];
        input.iter_mut().zip(&window).enumerate().for_each(|(i, (x, w))| {
            *x = chunk.get(i).copied().unwrap_or(0.0) * w;
        });
        fft.process(&mut input, &mut output).expect("buffer sizes come from the planner");
        power.iter_mut().zip(&output).for_each(|(p, c)| *p += c.norm_sqr());
        frames += 1;
        start += FFT / 2;
        if start + FFT > samples.len() {
            break;
        }
    }
    power.iter_mut().for_each(|p| *p /= frames as f32);
    power
}

/// Least-squares slope of y over x.
fn slope(x: &[f32], y: &[f32]) -> f32 {
    let n = x.len() as f32;
    let (mx, my) = (x.iter().sum::<f32>() / n, y.iter().sum::<f32>() / n);
    let cov: f32 = x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)).sum();
    let var: f32 = x.iter().map(|a| (a - mx).powi(2)).sum();
    if var == 0.0 { 0.0 } else { cov / var }
}

/// Peak-to-RMS in dB, ignoring near-silent stretches so gaps between notes don't inflate it.
fn crest(samples: &[f32]) -> f32 {
    let rms: Vec<f32> =
        samples.chunks(2048).map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt()).collect();
    let loudest = rms.iter().cloned().fold(0.0, f32::max);
    let active: Vec<f32> = rms.into_iter().filter(|r| *r > loudest * 0.05).collect();
    if active.is_empty() {
        return 0.0;
    }
    let level = (active.iter().map(|r| r * r).sum::<f32>() / active.len() as f32).sqrt();
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    20.0 * (peak / level).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        x.iter().map(|s| { y = (1.0 - a) * s + a * y; y }).collect()
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
        let dark = analyze(&lowpass(&lowpass(&noise(RATE as usize * 2), 300.0), 300.0), RATE);
        assert!(white.tilt_db_per_octave > -1.0, "white noise is flat: {}", white.tilt_db_per_octave);
        assert!(dark.tilt_db_per_octave < white.tilt_db_per_octave - 3.0);
        assert!(dark.summary.iter().any(|s| s.starts_with("Dark")));
    }

    #[test]
    fn saturated_playing_has_a_lower_crest_than_clean() {
        let clean = analyze(&plucks(1.0), RATE);
        let driven = analyze(&plucks(40.0), RATE);
        assert!(clean.crest_db > 10.0, "clean crest {}", clean.crest_db);
        assert!(driven.crest_db < 6.0, "driven crest {}", driven.crest_db);
        assert!(driven.summary[0].starts_with("Squashed"));
    }

    #[test]
    fn short_and_silent_input_does_not_panic() {
        let t = analyze(&[0.0; 10], RATE);
        assert_eq!(t.levels_db.len(), BANDS);
    }
}
