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
// of the others' last word loses its start, and words said over the others'
// voice are not heard (SU-08: two streams). Arrival timestamps per block and
// Hex's meeting echo model (`meeting/echo`) if it matters.
const OTHERS_TAIL: usize = MICROPHONE_DELAY + MIX_HOLD + SAMPLE_RATE / 4;
/// A frame's RMS above which the system audio plays (about -54 dBFS) and
/// the microphone hears a voice (about -48 dBFS).
const PLAYING: f32 = 0.002;
const SPEAKING: f32 = 0.004;
/// The system audio takes the turn only for a voice (SU-07): background
/// noise in a call, music, or a fan plays without taking the microphone's
/// words. The speech detector judges the last [`VOICE_PAST`] played and the
/// next [`VOICE_AHEAD`] held back, every [`VOICE_STEP`]; looking ahead finds
/// the others' first word in time. Without the detector, any sound counts.
const VOICE_PAST: usize = SAMPLE_RATE * 3 / 10;
const VOICE_AHEAD: usize = MIX_HOLD;
const VOICE_STEP: usize = SAMPLE_RATE / 10;
/// The user speaking over the others' turn (SU-08): the microphone's audio
/// is kept aside from just before its first sound of the user's own
/// ([`OVERLAP_LEAD`]) until [`OVERLAP_QUIET`] without it or the turn comes
/// back, and, with at least [`OVERLAP_MIN`] of it, transcribed on its own and
/// put in the thread. The user's own is sound the call's bleed doesn't
/// explain (`meeting::echo` over the last [`MINE_WINDOW`] ms, every
/// [`VOICE_STEP`]), so the speakers' echo opens nothing.
const OVERLAP_LEAD: usize = SAMPLE_RATE / 2;
const MINE_WINDOW: u64 = 600;
const OVERLAP_QUIET: usize = SAMPLE_RATE * 3 / 5;
const OVERLAP_MIN: usize = SAMPLE_RATE * 2 / 5;
const OVERLAP_MAX: usize = SAMPLE_RATE * 20;
/// How long `stop` waits for the last overlaps' text.
const RECOVER_WAIT: Duration = Duration::from_secs(10);

/// The running session's system audio and mixer.
struct Running {
    system: Option<crate::meeting::SystemAudio>,
    mixer: Option<JoinHandle<()>>,
    /// Transcribes what the user said over the others.
    recover: Option<JoinHandle<()>>,
}

/// What the user said over the others' turn, on the session's timeline.
pub(super) struct Overlap {
    start_ms: u64,
    end_ms: u64,
    audio: Vec<f32>,
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
    let (microphone, system, mixer, recover) = match source {
        Source::Microphone => (Some(pump), None, None, None),
        Source::System => {
            let system = crate::meeting::system_audio(move |samples, rate| {
                let _ = pump.try_send((samples, rate, None));
            });
            (None, Some(system), None, None)
        }
        Source::Both => {
            let (microphone, heard) = mpsc::sync_channel(TAP_CHUNKS);
            let (system, played) = mpsc::sync_channel(TAP_CHUNKS);
            let (found, overlaps) = mpsc::sync_channel(8);
            let recover = thread::Builder::new()
                .name("teleprompter-overlaps".into())
                .spawn(move || recover(&overlaps))
                .map_err(|error| format!("couldn't start: {error}"))?;
            let mixer = thread::Builder::new()
                .name("teleprompter-mixer".into())
                .spawn(move || take_turns_into(&pump, &heard, &played, &found))
                .map_err(|error| format!("couldn't start: {error}"))?;
            let system = crate::meeting::system_audio(move |samples, rate| {
                let _ = system.try_send((samples, rate, None));
            });
            (Some(microphone), Some(system), Some(mixer), Some(recover))
        }
    };
    let system = match system.transpose() {
        Ok(system) => system,
        Err(error) => return Err(format!("couldn't hear the system audio: {error}")),
    };
    *lock(&RUNNING) = Some(Running {
        system,
        mixer,
        recover,
    });
    Ok(microphone)
}

/// Stops the system audio and the mixer; the microphone's sender is gone.
/// Returns the thread still transcribing overlaps, for [`settle`] once the
/// stream has finished: its last one waits for the stream's last window.
pub(super) fn disconnect() -> Option<JoinHandle<()>> {
    let mut running = lock(&RUNNING).take()?;
    drop(running.system.take());
    if let Some(mixer) = running.mixer.take() {
        let _ = mixer.join();
    }
    running.recover.take()
}

/// Waits a bounded time for the last overlaps' text.
pub(super) fn settle(recover: Option<JoinHandle<()>>) {
    let Some(recover) = recover else {
        return;
    };
    let waited = std::time::Instant::now();
    while !recover.is_finished() && waited.elapsed() < RECOVER_WAIT {
        thread::sleep(Duration::from_millis(50));
    }
    if recover.is_finished() {
        let _ = recover.join();
    }
}

/// Transcribes each overlap that holds speech on the session's model, once
/// the live window it waits on has closed, and puts it in the thread.
fn recover(overlaps: &Receiver<Overlap>) {
    use crate::transcription::stream::Engine as _;
    let engine = match super::language::engine() {
        Ok((engine, _, _)) => engine,
        Err(_) => return,
    };
    for Overlap {
        start_ms,
        end_ms,
        audio,
    } in overlaps
    {
        if crate::transcription::speech_detector::contains_speech(&audio) == Some(false) {
            continue;
        }
        match engine.transcribe(audio, &|| false) {
            Ok(text) if !text.trim().is_empty() => super::recovered(start_ms, end_ms, text.trim()),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "teleprompter couldn't transcribe an overlap"),
        }
    }
}

/// Hands the pump 16 kHz chunks of whoever has the turn, the microphone's
/// audio or the system's, until the microphone's sender closes.
fn take_turns_into(
    pump: &SyncSender<Chunk>,
    heard: &Receiver<Chunk>,
    played: &Receiver<Chunk>,
    found: &SyncSender<Overlap>,
) {
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
            turns.close();
        }
        for overlap in turns.overlaps.drain(..) {
            // ponytail: a full queue drops the overlap rather than block the mixer.
            let _ = found.try_send(overlap);
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
    /// The system audio's last [`VOICE_PAST`] samples, for the detector.
    played: Vec<f32>,
    /// Whether the system audio holds a voice, as of `voice_at`.
    voice: Option<bool>,
    voice_at: Option<usize>,
    /// Both sides' frame energies on this timeline, to tell the user's
    /// voice from the call's bleed.
    echo: crate::meeting::echo::EchoEnvelope,
    /// The microphone's last [`OVERLAP_LEAD`] samples.
    heard: Vec<f32>,
    /// The user speaking over the others, while it lasts.
    open: Option<Open>,
    /// Whether the microphone's sound is the user's, as of `mine_at`.
    mine: bool,
    mine_at: Option<usize>,
    /// Overlaps closed since the caller last took them.
    overlaps: Vec<Overlap>,
}

/// An overlap being heard: where it starts, its audio, and how much of it
/// has sound and how much quiet has followed.
struct Open {
    start: usize,
    audio: Vec<f32>,
    loud: usize,
    quiet: usize,
}

impl Turns {
    /// Takes both sides' whole frames up to the longer one's length less
    /// `hold`, leaving the rest for the next call, and returns one side's
    /// audio per frame as runs of one speaker: the system audio while a
    /// voice plays in it and for [`OTHERS_TAIL`] after, else the microphone.
    /// A side that delivered nothing (the loopback while nothing plays) is
    /// silent.
    fn take(
        &mut self,
        microphone: &mut Vec<f32>,
        system: &mut Vec<f32>,
        hold: usize,
    ) -> Vec<(Vec<f32>, Option<Speaker>)> {
        let frames = microphone.len().max(system.len()).saturating_sub(hold) / FRAME;
        let mut runs: Vec<(Vec<f32>, Option<Speaker>)> = Vec::new();
        for _ in 0..frames {
            let mut played: Vec<f32> = system.drain(..FRAME.min(system.len())).collect();
            let mut heard: Vec<f32> = microphone.drain(..FRAME.min(microphone.len())).collect();
            played.resize(FRAME, 0.0);
            heard.resize(FRAME, 0.0);
            let at_ms = (self.position * 1000 / SAMPLE_RATE) as u64;
            let rate = SAMPLE_RATE as u32;
            use crate::meeting::MeetingSource;
            self.echo.push(MeetingSource::System, at_ms, &played, rate);
            self.echo
                .push(MeetingSource::Microphone, at_ms, &heard, rate);
            self.played.extend_from_slice(&played);
            let old = self.played.len().saturating_sub(VOICE_PAST);
            self.played.drain(..old);
            if level(&played) > PLAYING && self.voiced(system) {
                self.others_until = self.position + OTHERS_TAIL;
            }
            let others = self.position < self.others_until;
            self.overhear(others, &heard);
            let mut frame = if others {
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

    /// Follows the microphone's `heard` frame: while the `others` hold the
    /// turn, its sound opens or extends an overlap, which closes after
    /// [`OVERLAP_QUIET`], at [`OVERLAP_MAX`], or when the turn comes back.
    fn overhear(&mut self, others: bool, heard: &[f32]) {
        let loud = others && level(heard) > SPEAKING && self.mine();
        if others && (loud || self.open.is_some()) {
            let (lead, position) = (&self.heard, self.position);
            let open = self.open.get_or_insert_with(|| Open {
                start: position - lead.len(),
                audio: lead.clone(),
                loud: 0,
                quiet: 0,
            });
            open.audio.extend_from_slice(heard);
            if loud {
                open.loud += heard.len();
                open.quiet = 0;
            } else {
                open.quiet += heard.len();
            }
            if open.quiet >= OVERLAP_QUIET || open.audio.len() >= OVERLAP_MAX {
                self.close();
            }
        } else {
            self.close();
        }
        self.heard.extend_from_slice(heard);
        let old = self.heard.len().saturating_sub(OVERLAP_LEAD);
        self.heard.drain(..old);
    }

    /// Whether the microphone's sound up to this frame is the user's,
    /// asking the echo model at most once per [`VOICE_STEP`].
    fn mine(&mut self) -> bool {
        if self
            .mine_at
            .is_none_or(|at| self.position >= at + VOICE_STEP)
        {
            let end_ms = ((self.position + FRAME) * 1000 / SAMPLE_RATE) as u64;
            let start_ms = end_ms.saturating_sub(MINE_WINDOW);
            self.mine = self.echo.heard_locally(start_ms, end_ms);
            self.mine_at = Some(self.position);
        }
        self.mine
    }

    /// Ends the open overlap, keeping it when enough of it was the user's.
    fn close(&mut self) {
        let Some(open) = self.open.take() else {
            return;
        };
        let ms = |at: usize| (at * 1000 / SAMPLE_RATE) as u64;
        let (start_ms, end_ms) = (ms(open.start), ms(open.start + open.audio.len()));
        if open.loud >= OVERLAP_MIN {
            self.overlaps.push(Overlap {
                start_ms,
                end_ms,
                audio: open.audio,
            });
        }
    }

    /// Whether the system audio around the current frame holds a voice,
    /// asking the detector at most once per [`VOICE_STEP`]; `ahead` is what
    /// is held back. Without a detector, every sound counts as a voice.
    fn voiced(&mut self, ahead: &[f32]) -> bool {
        if self
            .voice_at
            .is_none_or(|at| self.position >= at + VOICE_STEP)
        {
            let mut window = self.played.clone();
            window.extend_from_slice(&ahead[..VOICE_AHEAD.min(ahead.len())]);
            self.voice = crate::transcription::speech_detector::contains_speech(&window);
            self.voice_at = Some(self.position);
        }
        self.voice.unwrap_or(true)
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
    fn noise_in_the_call_leaves_me_the_turn() {
        let second = SAMPLE_RATE;
        // The call plays noise, no voice, while I speak: my words, as mine.
        let _noise = crate::transcription::speech_detector::override_for_test(Some(false));
        let mut turns = Turns::default();
        let (mut microphone, mut system) = (vec![0.1; second], vec![0.2; second]);
        assert_eq!(
            turns.take(&mut microphone, &mut system, 0),
            [(vec![0.1; second], Some(Speaker::Me))]
        );
        // A voice in the call takes the turn, as before.
        let _voice = crate::transcription::speech_detector::override_for_test(Some(true));
        let mut turns = Turns::default();
        let (mut microphone, mut system) = (vec![0.1; second], vec![0.2; second]);
        assert_eq!(
            turns.take(&mut microphone, &mut system, 0),
            [(vec![0.2; second], Some(Speaker::Others))]
        );
    }

    #[test]
    fn what_i_say_over_the_others_is_kept_aside_and_their_echo_is_not() {
        use crate::meeting::echo::tests::{bleed, speech};
        let (second, frames) = (SAMPLE_RATE, |seconds: usize| seconds * SAMPLE_RATE);
        let theirs = speech(8.0, 11);
        // Headphones: the microphone hears only me, from 3 s to 5 s, over them.
        let mut mine = vec![0.0; theirs.len()];
        for (offset, sample) in speech(2.0, 29).iter().enumerate() {
            mine[frames(3) + offset] = sample * 0.9;
        }
        let mut turns = Turns::default();
        let (mut microphone, mut system) = (mine, theirs.clone());
        let runs = turns.take(&mut microphone, &mut system, 0);
        assert!(
            runs.iter()
                .all(|(_, speaker)| *speaker == Some(Speaker::Others))
        );
        turns.close();
        let [overlap] = turns.overlaps.as_slice() else {
            panic!("one overlap, not {}", turns.overlaps.len());
        };
        assert!(
            (2_700..=3_100).contains(&overlap.start_ms),
            "{}",
            overlap.start_ms
        );
        assert!(
            (5_000..=5_700).contains(&overlap.end_ms),
            "{}",
            overlap.end_ms
        );
        assert!(overlap.audio.len() >= 2 * second);
        // Speakers: the microphone hears them alone, as late as the mixer
        // delays it; nothing is kept.
        let echo = bleed(&theirs, MICROPHONE_DELAY * 1000 / SAMPLE_RATE + 60, 0.3);
        let mut turns = Turns::default();
        let (mut microphone, mut system) = (echo.clone(), theirs.clone());
        turns.take(&mut microphone, &mut system, 0);
        turns.close();
        assert!(turns.overlaps.is_empty());
        // Speakers, and me over them from 3 s: mine is kept.
        let mut heard = echo;
        for (offset, sample) in speech(2.0, 29).iter().enumerate() {
            heard[frames(3) + offset] += sample * 0.9;
        }
        let mut turns = Turns::default();
        let (mut microphone, mut system) = (heard, theirs);
        turns.take(&mut microphone, &mut system, 0);
        turns.close();
        let kept = turns.overlaps.iter().map(|it| (it.start_ms, it.end_ms));
        let kept = kept.collect::<Vec<_>>();
        assert!(
            matches!(kept.as_slice(), [(start, end)]
                if (2_300..=3_300).contains(start) && (5_000..=6_000).contains(end)),
            "{kept:?}"
        );
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
