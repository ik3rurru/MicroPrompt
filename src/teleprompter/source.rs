//! MicroPrompt: what a Teleprompter session captions
//! (`labs.teleprompter_source`): the listener's microphone, the system
//! audio (what plays through the outputs: calls, videos), heard through the
//! meetings' source (`crate::meeting::system_audio`: WASAPI loopback on
//! Windows, the default output's PipeWire monitor on Linux), or both, taking
//! turns in the one stream ([`Turns`]): the engine runs one live stream at a
//! time, so while the system audio plays the session hears it alone, and the
//! microphone otherwise. Each turn is a [`Speaker`], and what the speakers
//! play never reaches the stream a second time through the microphone.

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

/// Whose words a line holds when a session hears both sources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Speaker {
    /// The microphone.
    Me,
    /// The system audio.
    Others,
}

/// 16 kHz samples of each side held back before taking turns, so the system
/// audio's blocks (about 100 ms) line up with the microphone's 10 ms ones.
const MIX_HOLD: usize = SAMPLE_RATE / 5;
/// The turns are decided per frame of 20 ms.
const FRAME: usize = SAMPLE_RATE / 50;
/// The microphone runs this far behind the system audio, so a block of
/// system audio is seen before the microphone's echo of it.
const MICROPHONE_DELAY: usize = SAMPLE_RATE / 4;
/// How long the others keep the turn after the system audio's last sound:
/// the microphone's delay, the hold's slack, and the room's echo.
// ponytail: fixed times and levels; an answer begun within a quarter second
// of the others' last word loses its start, and words said over the others
// are not heard. Arrival timestamps per block and Hex's meeting echo model
// (`meeting/echo`) if it matters.
const OTHERS_TAIL: usize = MICROPHONE_DELAY + MIX_HOLD + SAMPLE_RATE / 4;
/// A frame's RMS above which the system audio plays (about -54 dBFS) and
/// the microphone hears a voice (about -48 dBFS).
const PLAYING: f32 = 0.002;
const SPEAKING: f32 = 0.004;

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
                let _ = pump.try_send((samples, rate, None));
            });
            (None, Some(system), None)
        }
        Source::Both => {
            let (microphone, heard) = mpsc::sync_channel(TAP_CHUNKS);
            let (system, played) = mpsc::sync_channel(TAP_CHUNKS);
            let mixer = thread::Builder::new()
                .name("teleprompter-mixer".into())
                .spawn(move || take_turns_into(&pump, &heard, &played))
                .map_err(|error| format!("couldn't start: {error}"))?;
            let system = crate::meeting::system_audio(move |samples, rate| {
                let _ = system.try_send((samples, rate, None));
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

/// Hands the pump 16 kHz chunks of whoever has the turn, the microphone's
/// audio or the system's, until the microphone's sender closes.
fn take_turns_into(pump: &SyncSender<Chunk>, heard: &Receiver<Chunk>, played: &Receiver<Chunk>) {
    let mut microphone = Side {
        pending: vec![0.0; MICROPHONE_DELAY],
        ..Side::default()
    };
    let mut system = Side::default();
    let mut turns = Turns::default();
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
        for (samples, speaker) in turns.take(&mut microphone.pending, &mut system.pending, hold) {
            if pump.send((samples, SAMPLE_RATE as u32, speaker)).is_err() {
                return;
            }
        }
        if !open {
            return;
        }
    }
}

/// One side's audio, at 16 kHz, waiting for its turn.
#[derive(Default)]
struct Side {
    resampler: Option<(u32, crate::dictation::StreamingResampler)>,
    pending: Vec<f32>,
}

impl Side {
    fn push(&mut self, (samples, rate, _): Chunk) {
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

/// Who has the turn, frame by frame.
#[derive(Default)]
struct Turns {
    /// Samples handed on so far.
    position: usize,
    /// The others keep the turn until here.
    others_until: usize,
    /// Who spoke last; a quiet frame changes nothing.
    speaker: Option<Speaker>,
}

impl Turns {
    /// Takes both sides' whole frames up to the longer one's length less
    /// `hold`, leaving the rest for the next call, and returns one side's
    /// audio per frame as runs of one speaker: the system audio while it
    /// plays and for [`OTHERS_TAIL`] after, else the microphone. A side that
    /// delivered nothing (the loopback while nothing plays) is silent.
    fn take(
        &mut self,
        microphone: &mut Vec<f32>,
        system: &mut Vec<f32>,
        hold: usize,
    ) -> Vec<(Vec<f32>, Option<Speaker>)> {
        let frames = microphone.len().max(system.len()).saturating_sub(hold) / FRAME;
        let mut runs: Vec<(Vec<f32>, Option<Speaker>)> = Vec::new();
        for _ in 0..frames {
            let played: Vec<f32> = system.drain(..FRAME.min(system.len())).collect();
            let heard: Vec<f32> = microphone.drain(..FRAME.min(microphone.len())).collect();
            if level(&played) > PLAYING {
                self.others_until = self.position + OTHERS_TAIL;
            }
            let mut frame = if self.position < self.others_until {
                self.speaker = Some(Speaker::Others);
                played
            } else {
                if level(&heard) > SPEAKING {
                    self.speaker = Some(Speaker::Me);
                }
                heard
            };
            frame.resize(FRAME, 0.0);
            self.position += FRAME;
            match runs.last_mut() {
                Some((run, speaker)) if *speaker == self.speaker => run.extend(frame),
                _ => runs.push((frame, self.speaker)),
            }
        }
        runs
    }
}

/// A frame's RMS; nothing is silence.
fn level(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    (frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sides_take_turns_and_the_echo_is_not_heard() {
        let second = SAMPLE_RATE;
        let mut turns = Turns::default();
        // I speak for a second: my turn, my audio.
        let (mut microphone, mut system) = (vec![0.1; second], Vec::new());
        assert_eq!(
            turns.take(&mut microphone, &mut system, 0),
            [(vec![0.1; second], Some(Speaker::Me))]
        );
        // The others speak and the microphone hears them, quieter and for
        // longer: their turn, the system audio alone, through the echo.
        let (mut microphone, mut system) =
            (vec![0.05; second + OTHERS_TAIL - FRAME], vec![0.2; second]);
        let mut theirs = vec![0.2; second];
        theirs.resize(second + OTHERS_TAIL - FRAME, 0.0);
        assert_eq!(
            turns.take(&mut microphone, &mut system, 0),
            [(theirs, Some(Speaker::Others))]
        );
        // Quiet keeps the last speaker; then I answer.
        let (mut microphone, mut system) = (vec![0.0; FRAME], Vec::new());
        assert_eq!(
            turns.take(&mut microphone, &mut system, 0),
            [(vec![0.0; FRAME], Some(Speaker::Others))]
        );
        let (mut microphone, mut system) = (vec![0.1; FRAME * 3], Vec::new());
        assert_eq!(
            turns.take(&mut microphone, &mut system, FRAME),
            [(vec![0.1; FRAME * 2], Some(Speaker::Me))]
        );
        assert_eq!(microphone.len(), FRAME);
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
