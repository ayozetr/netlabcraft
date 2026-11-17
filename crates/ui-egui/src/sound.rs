//! The short tones Preferences ▸ Play Sound and Play Telephony Sound make. Generated here as
//! plain sine waves in a mono 16-bit WAV file (nothing is recorded or copied from anywhere); the
//! shell plays the bytes with the system's player ([`crate::UiRequest::PlaySound`]).

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// A PDU reached its destination.
    Delivered,
    /// A PDU failed.
    Dropped,
    /// A telephone rings: an IP phone or the Softphone places a call.
    Ring,
    /// A modem dials.
    Dial,
}

impl Sound {
    /// Telephony sounds follow their own preference.
    pub fn is_telephony(self) -> bool {
        matches!(self, Sound::Ring | Sound::Dial)
    }

    /// The tone as (first frequency, second frequency, milliseconds); a frequency of 0 is silence
    /// (when both are 0) or absent (the second).
    fn segments(self) -> &'static [(f32, f32, u32)] {
        match self {
            Sound::Delivered => &[(660.0, 0.0, 90), (880.0, 0.0, 130)],
            Sound::Dropped => &[(330.0, 0.0, 140), (220.0, 0.0, 240)],
            Sound::Ring => &[(440.0, 480.0, 400), (0.0, 0.0, 200), (440.0, 480.0, 400)],
            Sound::Dial => {
                &[(770.0, 1336.0, 70), (0.0, 0.0, 40), (852.0, 1209.0, 70), (0.0, 0.0, 40), (697.0, 1477.0, 70), (0.0, 0.0, 40), (941.0, 1336.0, 70)]
            }
        }
    }
}

const RATE: u32 = 11_025;

fn samples(segments: &[(f32, f32, u32)]) -> Vec<i16> {
    let mut out = Vec::new();
    for &(f1, f2, ms) in segments {
        let n = RATE * ms / 1000;
        for i in 0..n {
            if f1 <= 0.0 {
                out.push(0);
                continue;
            }
            let t = i as f32 / RATE as f32;
            let tone = |f: f32| (std::f32::consts::TAU * f * t).sin();
            let mix = if f2 > 0.0 { (tone(f1) + tone(f2)) * 0.5 } else { tone(f1) };
            // A short fade in and out so the tone does not click.
            let edge = i.min(n.saturating_sub(1).saturating_sub(i)) as f32 / (RATE as f32 * 0.008);
            out.push((mix * edge.min(1.0) * 0.45 * f32::from(i16::MAX)) as i16);
        }
    }
    out
}

/// The sound as the bytes of a WAV file.
pub fn wav(sound: Sound) -> Vec<u8> {
    let pcm = samples(sound.segments());
    let data_len = u32::try_from(pcm.len() * 2).unwrap_or(0);
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_is_a_well_formed_wav_that_is_not_silent() {
        for s in [Sound::Delivered, Sound::Dropped, Sound::Ring, Sound::Dial] {
            let w = wav(s);
            assert_eq!(&w[..4], b"RIFF");
            assert_eq!(&w[8..16], b"WAVEfmt ");
            let data = u32::from_le_bytes([w[40], w[41], w[42], w[43]]) as usize;
            assert_eq!(w.len(), 44 + data, "{s:?}");
            assert_eq!(u32::from_le_bytes([w[4], w[5], w[6], w[7]]) as usize, w.len() - 8);
            assert!(w[44..].chunks(2).any(|c| c != [0, 0]), "{s:?} is silent");
            // Under three seconds.
            assert!(data < 3 * RATE as usize * 2, "{s:?}");
        }
        assert!(Sound::Ring.is_telephony() && Sound::Dial.is_telephony() && !Sound::Delivered.is_telephony());
    }
}
