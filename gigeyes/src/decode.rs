use std::{fs::File, path::Path};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Decode an audio or video file (wav, mp3, flac, ogg, m4a/mp4, mkv) to mono samples.
/// shortcut: no Opus decoder, so most .webm/.opus files fail; add one if users need YouTube downloads.
/// Returns (samples, sample rate).
pub fn decode(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let file = File::open(path).map_err(|e| format!("Can't open the file: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|_| "GigEyes can't read this file type.".to_string())?;

    let track = format.default_track(TrackType::Audio).ok_or("This file has no audio.")?;
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("This file has no audio.")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|_| "GigEyes can't decode this audio format.".to_string())?;

    let (mut mono, mut rate, mut frame) = (Vec::new(), 0, Vec::<f32>::new());
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // A damaged tail shouldn't throw away everything decoded so far.
            Err(_) if !mono.is_empty() => break,
            Err(e) => return Err(format!("Couldn't read the audio: {e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                rate = audio.spec().rate();
                let channels = audio.spec().channels().count().max(1);
                frame.resize(audio.samples_interleaved(), 0.0);
                audio.copy_to_slice_interleaved(&mut frame);
                mono.extend(frame.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
            }
            Err(Error::DecodeError(_)) => continue, // skip a corrupt packet, keep going
            Err(e) => return Err(format!("Couldn't decode the audio: {e}")),
        }
    }
    if mono.is_empty() || rate == 0 {
        return Err("This file has no audio.".into());
    }
    Ok((mono, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit stereo WAV: left = +0.5, right = -0.25, so the mono mix is 0.125.
    fn write_wav(path: &Path, frames: u32, rate: u32) {
        let data_len = frames * 4;
        let mut bytes = Vec::new();
        bytes.extend(b"RIFF");
        bytes.extend((36 + data_len).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes()); // PCM
        bytes.extend(2u16.to_le_bytes()); // stereo
        bytes.extend(rate.to_le_bytes());
        bytes.extend((rate * 4).to_le_bytes());
        bytes.extend(4u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(data_len.to_le_bytes());
        for _ in 0..frames {
            bytes.extend(16384i16.to_le_bytes());
            bytes.extend((-8192i16).to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn decodes_wav_to_mono_with_its_rate() {
        let path = std::env::temp_dir().join("gigeyes_decode_test.wav");
        write_wav(&path, 1000, 44_100);
        let (mono, rate) = decode(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!((mono.len(), rate), (1000, 44_100));
        assert!((mono[500] - 0.125).abs() < 1e-3, "mono mix was {}", mono[500]);
    }

    #[test]
    fn non_audio_gives_a_plain_error() {
        let path = std::env::temp_dir().join("gigeyes_not_audio.txt");
        std::fs::write(&path, "hello").unwrap();
        let err = decode(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert_eq!(err, "GigEyes can't read this file type.");
    }
}

