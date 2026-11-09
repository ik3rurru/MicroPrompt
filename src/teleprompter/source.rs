//! MicroPrompt: what a Teleprompter session captions
//! (`labs.teleprompter_source`): the listener's microphone, the system
//! audio (what plays through the outputs: calls, videos), heard through the
//! meetings' source (`crate::meeting::system_audio`: WASAPI loopback on
//! Windows, the default output's PipeWire monitor on Linux), or both mixed
//! into the one stream. Voices are not told apart.

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{Chunk, TAP_CHUNKS, lock};
use crate::transcription::stream::SAMPLE_RATE;

/// What a session hears; saved, so the next session hears the same.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Microphone,
    System,
    Both,
}

impl Source {
    pub const ALL: [Self; 3] = [Self::Microphone, Self::System, Self::Both];
}

/// 16 kHz samples of each side held back before mixing, so the system
/// audio's blocks (about 100 ms) line up with the microphone's 10 ms ones.
const MIX_HOLD: usize = SAMPLE_RATE / 5;

/// The running session's system audio and mixer.
struct Running {
    system: Option<crate::meeting::SystemAudio>,
    mixer: Option<JoinHandle<()>>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

/// Connects the saved source to the session's `pump`. Returns the sender the
/// listener's microphone feeds, when the source hears it.
pub(super) fn connect(pump: SyncSender<Chunk>) -> Result<Option<SyncSender<Chunk>>, String> {
    let source = crate::app_settings::AppSettings::read()
        .unwrap_or_default()
        .labs
        .teleprompter_source;
    tracing::info!(?source, "teleprompter source");
    let (microphone, system, mixer) = match source {
        Source::Microphone => (Some(pump), None, None),
        Source::System => {
            let system = crate::meeting::system_audio(move |samples, rate| {
                let _ = pump.try_send((samples, rate));
            });
            (None, Some(system), None)
        }
        Source::Both => {
            let (microphone, heard) = mpsc::sync_channel(TAP_CHUNKS);
            let (system, played) = mpsc::sync_channel(TAP_CHUNKS);
            let mixer = thread::Builder::new()
                .name("teleprompter-mixer".into())
                .spawn(move || mix_into(&pump, &heard, &played))
                .map_err(|error| format!("couldn't start: {error}"))?;
            let system = crate::meeting::system_audio(move |samples, rate| {
                let _ = system.try_send((samples, rate));
            });
            (Some(microphone), Some(system), Some(mixer))
        }
    };
    let system = match system.transpose() {
        Ok(system) => system,
        Err(error) => return Err(format!("couldn't hear the system audio: {error}")),
    };
    *lock(&RUNNING) = Some(Running { system, mixer });
    Ok(microphone)
}

/// Stops the system audio and the mixer; the microphone's sender is gone.
pub(super) fn disconnect() {
    let Some(mut running) = lock(&RUNNING).take() else {
        return;
    };
    drop(running.system.take());
    if let Some(mixer) = running.mixer.take() {
        let _ = mixer.join();
    }
}

/// Mixes the microphone's chunks with the system audio's into 16 kHz chunks
/// for the pump, until the microphone's sender closes.
fn mix_into(pump: &SyncSender<Chunk>, heard: &Receiver<Chunk>, played: &Receiver<Chunk>) {
    let mut microphone = Side::default();
    let mut system = Side::default();
    loop {
        let open = match heard.recv_timeout(Duration::from_millis(20)) {
            Ok(chunk) => {
                microphone.push(chunk);
                true
            }
            Err(RecvTimeoutError::Timeout) => true,
            Err(RecvTimeoutError::Disconnected) => false,
        };
        while let Ok(chunk) = played.try_recv() {
            system.push(chunk);
        }
        let hold = if open { MIX_HOLD } else { 0 };
        let mixed = mix(&mut microphone.pending, &mut system.pending, hold);
        if !mixed.is_empty() && pump.send((mixed, SAMPLE_RATE as u32)).is_err() {
            return;
        }
        if !open {
            return;
        }
    }
}

/// One side's audio, at 16 kHz, not yet mixed.
#[derive(Default)]
struct Side {
    resampler: Option<(u32, crate::dictation::StreamingResampler)>,
    pending: Vec<f32>,
}

impl Side {
    fn push(&mut self, (samples, rate): Chunk) {
        if self.resampler.as_ref().is_none_or(|(at, _)| *at != rate) {
            match crate::dictation::StreamingResampler::new(rate) {
                Ok(new) => self.resampler = Some((rate, new)),
                Err(error) => {
                    tracing::warn!(%error, rate, "teleprompter can't resample its audio");
                    return;
                }
            }
        }
        if let Some((_, resampler)) = self.resampler.as_mut() {
            self.pending.extend(resampler.push(&samples));
        }
    }
}

/// Adds both sides up to the longer one's length less `hold`, clamped,
/// leaving the rest for the next call. A side that delivered nothing (the
/// loopback while nothing plays) adds nothing.
fn mix(a: &mut Vec<f32>, b: &mut Vec<f32>, hold: usize) -> Vec<f32> {
    let length = a.len().max(b.len()).saturating_sub(hold);
    let mut mixed = vec![0.0f32; length];
    for side in [a, b] {
        let take = side.len().min(length);
        for (out, sample) in mixed.iter_mut().zip(side.drain(..take)) {
            *out += sample;
        }
    }
    for sample in &mut mixed {
        *sample = sample.clamp(-1.0, 1.0);
    }
    mixed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sides_add_up_and_a_silent_one_adds_nothing() {
        // Only the microphone: its audio, less the hold.
        let (mut microphone, mut system) = (vec![0.25; 10], Vec::new());
        assert_eq!(mix(&mut microphone, &mut system, 4), vec![0.25; 6]);
        assert_eq!(microphone.len(), 4);

        // Both: sample by sample, clamped.
        let (mut microphone, mut system) = (vec![0.5; 8], vec![0.75; 6]);
        assert_eq!(mix(&mut microphone, &mut system, 2), vec![1.0; 6]);
        assert_eq!((microphone.len(), system.len()), (2, 0));
    }

    #[test]
    fn the_source_is_saved_by_name() {
        let names = Source::ALL.map(|source| serde_json::to_value(source).unwrap());
        assert_eq!(
            names,
            ["microphone", "system", "both"].map(serde_json::Value::from)
        );
        assert_eq!(Source::default(), Source::Microphone);
    }
}
