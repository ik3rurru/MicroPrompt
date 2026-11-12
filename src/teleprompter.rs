//! MicroPrompt: live subtitles of the listener's own microphone, inside Hex
//! (Labs › Teleprompter, Linux and Windows). `hex teleprompter start`
//! copies the microphone the listener already reads (through
//! [`crate::capture_host::ListenerTimeline`], which keeps it open while a
//! session runs) into a `/streams` stream on the dictation engine's model,
//! with its captions lookahead; `status` reads its text and latency, `stop`
//! finishes it and keeps it in History, timed for its `.txt` and `.srt`
//! ([`export`]). The window's Teleprompter pane reads the same text
//! ([`view`]) and starts or stops it ([`toggle`]). Dictation still comes
//! first: a press closes the stream's window, which catches up afterwards.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::control::{ControlReply, TeleprompterAction};
use crate::history::{HistoryDraft, HistoryEntry, HistoryKind, HistorySegment};

#[cfg(target_os = "windows")]
pub(crate) mod assistant;
pub(crate) mod conversation;
pub(crate) mod hints;
pub(crate) mod hotkey;
pub(crate) mod interview;
pub(crate) mod language;
pub(crate) mod prompts;
pub(crate) mod script;
pub(crate) mod source;
use crate::transcription::stream::{End, Event, SAMPLE_RATE, SampleFormat, Stream, Streams};
use source::Speaker;

/// Microphone chunks (10 ms each) waiting for the pump: about 5 s. Past it a
/// chunk is dropped rather than block the listener's loop.
const TAP_CHUNKS: usize = 500;
/// How long `stop` waits for the last window's text.
const FINISH_WAIT: Duration = Duration::from_secs(15);

/// A chunk of audio, its sample rate, and whose turn it is when the session
/// hears both sources ([`source::Speaker`]).
type Chunk = (Vec<f32>, u32, Option<Speaker>);

static TAP: Mutex<Option<SyncSender<Chunk>>> = Mutex::new(None);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);
/// The latest session's text, kept after it stops until the next starts.
static LIVE: Mutex<Live> = Mutex::new(Live::new());
/// Asks the window for a frame when the session changes.
static WAKER: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// One microphone chunk from the listener's loop; never blocks it.
pub(crate) fn feed(samples: &[f32], sample_rate: u32) {
    let mut tap = lock(&TAP);
    let Some(sender) = tap.as_ref() else {
        return;
    };
    match sender.try_send((samples.to_vec(), sample_rate, None)) {
        Ok(()) => {}
        // ponytail: a stalled pump loses audio; a gap marker if it matters.
        Err(TrySendError::Full(_)) => tracing::warn!("teleprompter is behind; dropping audio"),
        Err(TrySendError::Disconnected(_)) => *tap = None,
    }
}

/// Whether a session wants the microphone open.
pub(crate) fn wants_audio() -> bool {
    lock(&TAP).is_some()
}

struct Session {
    stream: Arc<Stream>,
    started: Instant,
    /// The speech model's name, for History.
    model: String,
    pump: JoinHandle<()>,
    reader: JoinHandle<()>,
}

/// What the window shows of the latest session.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct View {
    pub(crate) running: bool,
    /// `stop` waits for the last words.
    pub(crate) stopping: bool,
    /// Closed segments, oldest first.
    pub(crate) segments: Vec<String>,
    /// Who said each segment, when the session hears both sources.
    pub(crate) speakers: Vec<Option<Speaker>>,
    /// Each segment's start and end in the session's audio, in ms.
    pub(crate) times: Vec<(u64, u64)>,
    /// The messages the assistant leaves out, by their first segment
    /// ([`conversation::Message::first`]).
    pub(crate) unused: std::collections::BTreeSet<usize>,
    /// The assistant's answers in the thread, oldest first.
    pub(crate) answers: Vec<Answer>,
    /// Who is saying the open segment.
    pub(crate) speaker: Option<Speaker>,
    /// The open segment: committed words, then the model's guess.
    pub(crate) stable: String,
    pub(crate) tentative: String,
    /// Why the last start or stop failed.
    pub(crate) error: Option<String>,
    /// Bumped by every change, so the window copies only a new view.
    pub(crate) generation: u64,
}

/// An answer of the assistant's in the thread (AS-03).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Answer {
    pub(crate) action: prompts::Action,
    /// The segments heard when it was asked, the open one included: the
    /// messages it follows.
    pub(crate) after: usize,
    /// So far, while `done` is false.
    pub(crate) text: String,
    pub(crate) done: bool,
    pub(crate) error: Option<String>,
    /// Whether the assistant reads it again (Answer reads earlier answers).
    pub(crate) used: bool,
    /// What the user typed, for [`prompts::Action::Ask`] (AS-04).
    pub(crate) question: Option<String>,
}

/// What a session has shown so far, as `status` reports it.
struct Live {
    view: View,
    /// Audio sent when the first words showed.
    first_words_ms: Option<u64>,
    /// When each partial with new text arrived.
    partials: Vec<Instant>,
    /// Audio sent past each segment's end when its final text arrived.
    behind_ms: Vec<u64>,
    /// Each change of speaker and the audio sent when it came.
    turns: Vec<(u64, Speaker)>,
    /// Labs › Interview prep: the interviewer's questions get hints (EN-03).
    hints: bool,
    /// The session hears only the system audio: every line is the other
    /// person's.
    others_only: bool,
    /// The first segment of the interviewer's turn that last got a hint:
    /// one per turn.
    hinted_turn: Option<usize>,
    /// The History entry the stopped session was kept as, where its notes
    /// go (AS-06).
    kept: Option<u64>,
    /// What the user said over the others, transcribed apart (SU-08):
    /// start, end, and text, for History.
    recovered: Vec<(u64, u64, String)>,
}

impl Live {
    const fn new() -> Self {
        Self {
            view: View {
                running: false,
                stopping: false,
                segments: Vec::new(),
                speakers: Vec::new(),
                times: Vec::new(),
                unused: std::collections::BTreeSet::new(),
                answers: Vec::new(),
                speaker: None,
                stable: String::new(),
                tentative: String::new(),
                error: None,
                generation: 0,
            },
            first_words_ms: None,
            partials: Vec::new(),
            behind_ms: Vec::new(),
            turns: Vec::new(),
            hints: false,
            others_only: false,
            hinted_turn: None,
            kept: None,
            recovered: Vec::new(),
        }
    }

    fn text(&self) -> String {
        let view = &self.view;
        let open = format!("{}{}", view.stable, view.tentative);
        view.segments
            .iter()
            .map(String::as_str)
            .chain(Some(open.trim()))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn latency(&self) -> String {
        let intervals = self
            .partials
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]).as_millis() as u64)
            .collect::<Vec<_>>();
        let mut parts = vec![format!("{} partials", self.partials.len())];
        if let Some(ms) = median(intervals) {
            parts.push(format!("every {:.2} s", ms as f64 / 1000.0));
        }
        if let Some(ms) = self.first_words_ms {
            parts.push(format!(
                "first words at {:.2} s of audio",
                ms as f64 / 1000.0
            ));
        }
        if let Some(ms) = median(self.behind_ms.clone()) {
            parts.push(format!(
                "final text {:.2} s after its audio ({} segments)",
                ms as f64 / 1000.0,
                self.behind_ms.len()
            ));
        }
        parts.join(", ")
    }
}

/// Who spoke the segment from `start_ms` to `end_ms`: a change of speaker
/// ends a segment, so its middle lies in one turn.
fn speaker_at(turns: &[(u64, Speaker)], start_ms: u64, end_ms: u64) -> Option<Speaker> {
    let middle = start_ms.midpoint(end_ms);
    let turn = turns.iter().rfind(|(at, _)| *at <= middle);
    turn.or(turns.first()).map(|(_, speaker)| *speaker)
}

/// "Me: …" or "Others: …" in the interface language, for History.
fn labelled(speaker: Option<Speaker>, text: &str) -> String {
    use crate::desktop::i18n::tr;
    match speaker {
        Some(Speaker::Me) => format!("{}: {text}", tr("Me")),
        Some(Speaker::Others) => format!("{}: {text}", tr("Others")),
        None => text.to_string(),
    }
}

fn median(mut values: Vec<u64>) -> Option<u64> {
    values.sort_unstable();
    values.get(values.len() / 2).copied()
}

/// The latest session's text for the window.
pub(crate) fn view() -> View {
    lock(&LIVE).view.clone()
}

/// The [`View::generation`] now, to tell whether to copy the view.
pub(crate) fn generation() -> u64 {
    lock(&LIVE).view.generation
}

/// The window's waker; each change of the view calls it.
pub(crate) fn set_waker(wake: Arc<dyn Fn() + Send + Sync>) {
    *lock(&WAKER) = Some(wake);
}

/// Wakes the window to copy a changed view in (also the assistant's).
fn wake() {
    if let Some(wake) = lock(&WAKER).clone() {
        wake();
    }
}

/// Lets the assistant use the message that starts at segment `first`, or
/// leaves it out (the thread's switch).
pub(crate) fn use_in_ai(first: usize, used: bool) {
    update(|live| {
        if used {
            live.view.unused.remove(&first);
        } else {
            live.view.unused.insert(first);
        }
    });
}

/// What the user said over the others from `start_ms` to `end_ms`,
/// transcribed once the overlap ended (SU-08).
pub(crate) fn recovered(start_ms: u64, end_ms: u64, text: &str) {
    update(|live| insert_recovered(live, start_ms, end_ms, text));
}

/// Puts a line of the user's in its place in the thread, after the last
/// line begun before it, unless its words are mostly those of the others'
/// lines around it: their voice through the speakers.
fn insert_recovered(live: &mut Live, start_ms: u64, end_ms: u64, text: &str) {
    let view = &mut live.view;
    let theirs = (0..view.segments.len())
        .filter(|&index| view.speakers[index] == Some(Speaker::Others))
        .filter(|&index| {
            let (start, end) = view.times[index];
            start <= end_ms + 1_000 && start_ms <= end + 1_000
        })
        .map(|index| view.segments[index].as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if repeats(text, &theirs) {
        tracing::info!(
            start_ms,
            end_ms,
            "teleprompter overlap repeats the others; left out"
        );
        return;
    }
    let at = view
        .times
        .iter()
        .rposition(|(start, _)| *start <= start_ms)
        .map_or(0, |index| index + 1);
    view.segments.insert(at, text.to_string());
    view.speakers.insert(at, Some(Speaker::Me));
    view.times.insert(at, (start_ms, end_ms));
    // Everything that counts lines moves past the new one.
    view.unused = view
        .unused
        .iter()
        .map(|&index| index + usize::from(index >= at))
        .collect();
    for answer in &mut view.answers {
        answer.after += usize::from(answer.after > at);
    }
    if let Some(turn) = live.hinted_turn.as_mut() {
        *turn += usize::from(*turn >= at);
    }
    live.recovered.push((start_ms, end_ms, text.to_string()));
}

/// Whether at least 60% of `mine`'s words are in `theirs`.
// ponytail: word sets, not order; a short "sí" of the user's right after
// theirs is left out too.
fn repeats(mine: &str, theirs: &str) -> bool {
    let words = |text: &str| {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
    };
    let theirs = words(theirs)
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let mine = words(mine);
    let repeated = mine.iter().filter(|word| theirs.contains(*word)).count();
    !mine.is_empty() && repeated * 5 >= mine.len() * 3
}

/// Changes the view through `change`, then wakes the window.
fn update(change: impl FnOnce(&mut Live)) {
    {
        let mut live = lock(&LIVE);
        change(&mut live);
        live.view.generation += 1;
    }
    wake();
}

/// The pane's New session: the running session ends (kept in History like
/// any stop) and a fresh one starts with the same source and language, its
/// thread and answers empty; on a thread of its own, as [`toggle`].
pub(crate) fn restart() {
    let spawned = thread::Builder::new()
        .name("teleprompter-restart".into())
        .spawn(|| {
            if lock(&SESSION).is_none() {
                return;
            }
            for action in [TeleprompterAction::Stop, TeleprompterAction::Start] {
                if let ControlReply::Error { message, .. } = command(action, true) {
                    update(|live| live.view.error = Some(message));
                    return;
                }
            }
        });
    if let Err(error) = spawned {
        update(|live| live.view.error = Some(error.to_string()));
    }
}

/// The pane's button: stops a running session, else starts one, on a
/// thread of its own (a stop waits for the last words). A failure shows in
/// [`View::error`].
pub(crate) fn toggle() {
    let spawned = thread::Builder::new()
        .name("teleprompter-toggle".into())
        .spawn(|| {
            let action = if lock(&SESSION).is_some() {
                TeleprompterAction::Stop
            } else {
                TeleprompterAction::Start
            };
            if let ControlReply::Error { message, .. } = command(action, true) {
                update(|live| live.view.error = Some(message));
            }
        });
    if let Err(error) = spawned {
        update(|live| live.view.error = Some(error.to_string()));
    }
}

/// `hex teleprompter …`; `enabled` is the lab's switch.
pub(crate) fn command(action: TeleprompterAction, enabled: bool) -> ControlReply {
    match action {
        TeleprompterAction::Start if !enabled => ControlReply::error(
            "feature-off",
            "Teleprompter is a Lab; turn it on with `hex labs on teleprompter`",
        ),
        TeleprompterAction::Start => start(),
        TeleprompterAction::Stop => stop(),
        TeleprompterAction::Status => status(),
        TeleprompterAction::Toggle if lock(&SESSION).is_some() => stop(),
        TeleprompterAction::Toggle => command(TeleprompterAction::Start, enabled),
    }
}

fn start() -> ControlReply {
    let mut session = lock(&SESSION);
    if session.is_some() {
        return ControlReply::unchanged("Teleprompter is already running");
    }
    let (engine, selection, language) = match language::engine() {
        Ok(engine) => engine,
        Err(reply) => return reply,
    };
    let stream = match Streams::default().create(
        engine,
        SampleFormat::F32le,
        selection.clone(),
        String::new(),
    ) {
        Ok(stream) => stream,
        Err(error) => return ControlReply::error("failed", format!("{error:?}")),
    };
    let (sender, receiver) = mpsc::sync_channel(TAP_CHUNKS);
    let sent = Arc::new(AtomicU64::new(0));
    let spawned = (|| {
        let pump = {
            let (stream, sent) = (stream.clone(), sent.clone());
            thread::Builder::new()
                .name("teleprompter-pump".into())
                .spawn(move || pump(&stream, &receiver, &sent))?
        };
        let reader = {
            let stream = stream.clone();
            thread::Builder::new()
                .name("teleprompter-reader".into())
                .spawn(move || read(&stream, &sent))?
        };
        std::io::Result::Ok((pump, reader))
    })();
    let (pump, reader) = match spawned {
        Ok(threads) => threads,
        Err(error) => {
            stream.cancel();
            return ControlReply::error("failed", format!("couldn't start: {error}"));
        }
    };
    let microphone = match source::connect(sender) {
        Ok(microphone) => microphone,
        Err(error) => {
            stream.cancel();
            return ControlReply::error("failed", error);
        }
    };
    update(|live| {
        let generation = live.view.generation;
        *live = Live::new();
        live.view.running = true;
        live.view.generation = generation;
    });
    *lock(&TAP) = microphone;
    *session = Some(Session {
        stream,
        started: Instant::now(),
        model: crate::transcription::models::definition(selection.model)
            .name
            .to_string(),
        pump,
        reader,
    });
    tracing::info!(model = ?selection.model, language = language.code(), "teleprompter started");
    let settings = crate::app_settings::AppSettings::read().unwrap_or_default();
    if crate::labs::Feature::Assistant.on(&settings) {
        assistant::warm();
    }
    update(|live| {
        live.hints = crate::labs::Feature::Interview.on(&settings);
        live.others_only = settings.labs.teleprompter_source == source::Source::System;
    });
    ControlReply::done(format!(
        "Teleprompter started on {} ({}); `hex teleprompter status` shows the text",
        crate::transcription::models::definition(selection.model).name,
        language.label()
    ))
}

fn stop() -> ControlReply {
    let Some(session) = lock(&SESSION).take() else {
        return ControlReply::unchanged("Teleprompter isn't running");
    };
    update(|live| live.view.stopping = true);
    // The pump sends what it already took, then ends with the tap.
    *lock(&TAP) = None;
    let recovering = source::disconnect();
    let _ = session.pump.join();
    let end = session.stream.finish(FINISH_WAIT, &|| false);
    if end.is_none() {
        session.stream.cancel();
    }
    source::settle(recovering);
    let _ = session.reader.join();
    update(|live| {
        live.view.running = false;
        live.view.stopping = false;
    });
    let mut live = lock(&LIVE);
    let elapsed = session.started.elapsed().as_secs();
    tracing::info!(seconds = elapsed, latency = %live.latency(), "teleprompter stopped");
    match end {
        Some(End::Final {
            text,
            segments,
            duration_ms,
        }) => {
            let (saved, kept) = keep(
                &text,
                &segments,
                &live.turns,
                &live.recovered,
                duration_ms,
                session.model,
            );
            live.kept = kept;
            let reply = format!(
                "Teleprompter stopped after {elapsed} s{saved}; {}\n{text}",
                live.latency()
            );
            drop(live);
            // Notes and Insights made during the session.
            assistant::keep_notes();
            ControlReply::done(reply)
        }
        Some(End::Failed { message, .. }) => {
            ControlReply::error("failed", format!("the last window failed: {message}"))
        }
        Some(End::Cancelled) | None => ControlReply::error(
            "timeout",
            format!("the last window took too long; so far:\n{}", live.text()),
        ),
    }
}

fn status() -> ControlReply {
    let session = lock(&SESSION);
    let Some(session) = session.as_ref() else {
        return ControlReply::unchanged("Teleprompter isn't running");
    };
    let live = lock(&LIVE);
    ControlReply::unchanged(format!(
        "Teleprompter running for {} s; {}\n{}",
        session.started.elapsed().as_secs(),
        live.latency(),
        live.text()
    ))
}

/// Resamples the tap's chunks to 16 kHz and pushes them, renewing the lease
/// while the microphone is quiet, until the tap closes or the stream ends.
/// A change of speaker ends the stream's window, so each segment is one's.
fn pump(stream: &Stream, chunks: &Receiver<Chunk>, sent: &AtomicU64) {
    let mut resampler: Option<(u32, crate::dictation::StreamingResampler)> = None;
    let mut turn = None;
    loop {
        let (samples, rate, speaker) = match chunks.recv_timeout(Duration::from_secs(1)) {
            Ok(chunk) => chunk,
            Err(RecvTimeoutError::Timeout) if stream.heartbeat() => continue,
            Err(_) => return,
        };
        if resampler.as_ref().is_none_or(|(at, _)| *at != rate) {
            match crate::dictation::StreamingResampler::new(rate) {
                Ok(new) => resampler = Some((rate, new)),
                Err(error) => {
                    tracing::warn!(%error, rate, "teleprompter can't resample the microphone");
                    return;
                }
            }
        }
        let Some((_, resampler)) = resampler.as_mut() else {
            return;
        };
        let audio = resampler.push(&samples);
        if let Some(speaker) = speaker.filter(|speaker| turn != Some(*speaker)) {
            if turn.is_some() {
                stream.cut();
            }
            turn = Some(speaker);
            let at_ms = sent.load(Ordering::Relaxed) * 1000 / SAMPLE_RATE as u64;
            lock(&LIVE).turns.push((at_ms, speaker));
        }
        if stream.push(&audio).is_err() {
            return;
        }
        sent.fetch_add(audio.len() as u64, Ordering::Relaxed);
    }
}

/// Follows the stream's events into `live` and the log until it ends.
fn read(stream: &Stream, sent: &AtomicU64) {
    let sent_ms = || sent.load(Ordering::Relaxed) * 1000 / SAMPLE_RATE as u64;
    stream.subscribe(
        Duration::from_secs(1),
        &|| false,
        &mut |event| {
            update(|live| match event {
                Event::Partial {
                    stable, tentative, ..
                } => {
                    let view = &live.view;
                    let new = format!("{stable}{tentative}").trim().to_string();
                    let old = format!("{}{}", view.stable, view.tentative);
                    if new != old.trim() && !new.is_empty() {
                        live.first_words_ms.get_or_insert_with(sent_ms);
                        live.partials.push(Instant::now());
                    }
                    if old.trim().is_empty() {
                        live.view.speaker = live.turns.last().map(|(_, speaker)| *speaker);
                    }
                    live.view.stable = stable.clone();
                    live.view.tentative = tentative.clone();
                }
                Event::Segment(segment) => {
                    let behind_ms = sent_ms().saturating_sub(segment.end_ms);
                    tracing::info!(
                        segment = segment.segment,
                        start_ms = segment.start_ms,
                        end_ms = segment.end_ms,
                        behind_ms,
                        text = %segment.text,
                        "teleprompter segment"
                    );
                    live.behind_ms.push(behind_ms);
                    live.view.segments.push(segment.text.clone());
                    let speaker = speaker_at(&live.turns, segment.start_ms, segment.end_ms);
                    live.view.speakers.push(speaker);
                    live.view.times.push((segment.start_ms, segment.end_ms));
                    live.view.stable.clear();
                    live.view.tentative.clear();
                }
                Event::Error { code, message } => {
                    tracing::warn!(code, message, "teleprompter stream failed");
                }
                Event::Final { .. } | Event::Cancelled => {}
            });
            if let Event::Segment(_) = event
                && let Some(question) = asked()
            {
                assistant::hint(&question);
            }
            true
        },
        &mut || true,
    );
}

/// Quiet between two of the other person's segments that starts a new turn,
/// as the EN-01 prototype told them apart.
const TURN_GAP_MS: u64 = 5_000;

/// The interviewer's turn, once a closed segment makes it a question it
/// hasn't had a hint for (EN-03): the other person's segments since the
/// user last spoke.
fn asked() -> Option<String> {
    asked_in(&mut lock(&LIVE))
}

fn asked_in(live: &mut Live) -> Option<String> {
    if !live.hints {
        return None;
    }
    let others = |index: usize| match live.view.speakers.get(index).copied().flatten() {
        Some(speaker) => speaker == Speaker::Others,
        None => live.others_only,
    };
    let last = live.view.segments.len().checked_sub(1)?;
    if !others(last) {
        return None;
    }
    // A long quiet in the other person's audio is the user answering, which
    // a session hearing only the system audio has no other way to tell.
    let joined = |index: usize| {
        index == last
            || match (live.view.times.get(index), live.view.times.get(index + 1)) {
                (Some((_, end)), Some((start, _))) => start.saturating_sub(*end) < TURN_GAP_MS,
                _ => true,
            }
    };
    let first = (0..=last)
        .rev()
        .take_while(|index| others(*index) && joined(*index))
        .last()?;
    if live.hinted_turn == Some(first) {
        return None;
    }
    let turn = live.view.segments[first..=last].join(" ");
    if !hints::is_question(&turn) {
        return None;
    }
    live.hinted_turn = Some(first);
    Some(turn.trim().to_owned())
}

/// Keeps a finished session in History, each line under its speaker's name
/// when it has `turns`; what the reply adds about it, and the entry.
fn keep(
    text: &str,
    segments: &[crate::transcription::stream::Segment],
    turns: &[(u64, Speaker)],
    recovered: &[(u64, u64, String)],
    duration_ms: u64,
    model: String,
) -> (&'static str, Option<u64>) {
    if text.trim().is_empty() {
        return ("", None);
    }
    let mut segments = segments
        .iter()
        .map(|segment| HistorySegment {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: labelled(
                speaker_at(turns, segment.start_ms, segment.end_ms),
                &segment.text,
            ),
        })
        .chain(
            recovered
                .iter()
                .map(|(start_ms, end_ms, text)| HistorySegment {
                    start_ms: *start_ms,
                    end_ms: *end_ms,
                    text: labelled(Some(Speaker::Me), text),
                }),
        )
        .collect::<Vec<_>>();
    segments.sort_by_key(|segment| segment.start_ms);
    let final_text = if turns.is_empty() {
        text.to_string()
    } else {
        let lines = segments.iter().map(|segment| segment.text.as_str());
        lines.collect::<Vec<_>>().join("\n")
    };
    let draft = HistoryDraft {
        kind: HistoryKind::Teleprompter,
        raw_text: text.to_string(),
        final_text,
        application: None,
        processing: None,
        audio_ms: duration_ms,
        inference_ms: 0,
        total_ms: 0,
        recording: None,
        model: Some(model),
    };
    match history().map(|history| history.record_with(draft, segments)) {
        Some(Ok(Some(id))) => (", kept in History", Some(id)),
        Some(Ok(None)) => (", not kept: History is off", None),
        Some(Err(error)) => {
            tracing::warn!(%error, "could not keep the teleprompter session");
            (", not kept in History", None)
        }
        None => (", not kept in History", None),
    }
}

/// Puts `notes` on the stopped session's History entry, if it was kept;
/// whether it was.
fn keep_notes_on_entry(notes: String) -> bool {
    let Some(id) = lock(&LIVE).kept else {
        return false;
    };
    match history().map(|history| history.set_notes(id, notes)) {
        Some(Ok(kept)) => kept,
        Some(Err(error)) => {
            tracing::warn!(%error, "could not keep the session's notes");
            false
        }
        None => false,
    }
}

/// The running app's History, or the saved one.
fn history() -> Option<crate::history::History> {
    #[cfg(target_os = "windows")]
    if let Some(history) = crate::windows::rune::runtime().and_then(|runtime| runtime.history()) {
        return Some(history);
    }
    let retention = crate::app_settings::AppSettings::read()
        .unwrap_or_default()
        .history_retention;
    crate::history::History::open_default(retention)
        .inspect_err(|error| tracing::warn!(%error, "could not open History"))
        .ok()
}

/// A session's subtitles: one cue per line, timed from its start.
pub(crate) fn srt(segments: &[HistorySegment]) -> String {
    use crate::control::transcript_cli::{cue_text, subtitle_time};
    let mut out = String::new();
    let lines = segments
        .iter()
        .map(|segment| (segment, cue_text(&segment.text)))
        .filter(|(_, text)| !text.is_empty());
    for (number, (segment, text)) in lines.enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{text}\n\n",
            number + 1,
            subtitle_time(segment.start_ms, ','),
            subtitle_time(segment.end_ms.max(segment.start_ms), ',')
        ));
    }
    out
}

/// What a kept session saves as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Export {
    Text,
    Subtitles,
    /// Its notes and insights, Markdown (AS-06).
    Notes,
}

/// Writes a kept session as `.txt`, `.srt`, or its notes as `.md` into
/// Documents/MicroPrompt, named for when it was kept, and shows it in the
/// file manager.
pub(crate) fn export(entry: &HistoryEntry, export: Export) -> Result<std::path::PathBuf, String> {
    let folder = crate::app_paths::document_dir()
        .ok_or("there is no Documents folder")?
        .join("MicroPrompt");
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let when = crate::meeting::query::iso_local(entry.timestamp_ms);
    let when = when
        .get(..16)
        .unwrap_or(&when)
        .replace('T', " ")
        .replace(':', "-");
    let (name, extension, body) = match export {
        Export::Subtitles => ("Teleprompter", "srt", srt(&entry.segments)),
        Export::Text => (
            "Teleprompter",
            "txt",
            format!("{}\n", entry.final_text.trim()),
        ),
        Export::Notes => (
            "Notes",
            "md",
            format!("{}\n", entry.notes.as_deref().unwrap_or_default().trim()),
        ),
    };
    let path = folder.join(format!("{name} {when}.{extension}"));
    std::fs::write(&path, body).map_err(|error| error.to_string())?;
    crate::meeting::reveal(&path);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_text_joins_segments_and_the_partial() {
        let mut live = Live::new();
        assert_eq!(live.text(), "");
        live.view.segments = vec!["Hola.".into(), "Esto es.".into()];
        live.view.stable = "una".into();
        live.view.tentative = " prueba".into();
        assert_eq!(live.text(), "Hola. Esto es. una prueba");
        assert_eq!(median(vec![300, 100, 200]), Some(200));
        assert_eq!(median(vec![]), None);
    }

    #[test]
    fn what_i_said_over_them_goes_in_its_place_unless_it_is_their_echo() {
        let mut live = Live::new();
        let view = &mut live.view;
        view.segments = vec![
            "Me: hola.".into(),
            "Ellos hablan del presupuesto.".into(),
            "Vale.".into(),
        ];
        view.speakers = vec![Some(Speaker::Me), Some(Speaker::Others), Some(Speaker::Me)];
        view.times = vec![(0, 1_000), (2_000, 8_000), (9_000, 10_000)];
        view.unused.insert(2);
        view.answers.push(Answer {
            action: prompts::Action::Suggest,
            after: 3,
            text: String::new(),
            done: true,
            error: None,
            used: true,
            question: None,
        });
        live.hinted_turn = Some(1);
        // Said at 4 s, over their line begun at 2 s: right after it.
        insert_recovered(&mut live, 4_000, 6_000, "Pero eso ya lo vimos.");
        assert_eq!(live.view.segments[2], "Pero eso ya lo vimos.");
        assert_eq!(live.view.speakers[2], Some(Speaker::Me));
        assert_eq!(live.view.times[2], (4_000, 6_000));
        assert!(live.view.unused.contains(&3) && !live.view.unused.contains(&2));
        assert_eq!(live.view.answers[0].after, 4);
        assert_eq!(live.hinted_turn, Some(1));
        assert_eq!(live.recovered.len(), 1);
        // Their own words, heard through the speakers: left out.
        insert_recovered(&mut live, 5_000, 7_000, "hablan del presupuesto");
        assert_eq!(live.view.segments.len(), 4);
        assert_eq!(live.recovered.len(), 1);
    }

    #[test]
    fn a_segment_is_its_turns_speaker() {
        assert_eq!(speaker_at(&[], 0, 1_000), None);
        let turns = [(2_000, Speaker::Me), (5_000, Speaker::Others)];
        // Silence before the first words belongs to who spoke them.
        assert_eq!(speaker_at(&turns, 0, 3_000), Some(Speaker::Me));
        assert_eq!(speaker_at(&turns, 2_000, 5_010), Some(Speaker::Me));
        assert_eq!(speaker_at(&turns, 5_010, 9_000), Some(Speaker::Others));
        assert_eq!(labelled(None, "Hola."), "Hola.");
    }

    #[test]
    fn subtitles_number_and_time_each_line() {
        let line = |start_ms, end_ms, text: &str| HistorySegment {
            start_ms,
            end_ms,
            text: text.into(),
        };
        let srt = srt(&[
            line(0, 3_580, "Hola, esto es una prueba."),
            line(3_580, 3_600, "  "),
            line(3_661_042, 3_665_000, "Dos --> tres"),
        ]);
        assert_eq!(
            srt,
            "1\n00:00:00,000 --> 00:00:03,580\nHola, esto es una prueba.\n\n\
             2\n01:01:01,042 --> 01:01:05,000\nDos -> tres\n\n"
        );
    }

    #[test]
    fn start_is_refused_while_the_lab_is_off() {
        assert!(matches!(
            command(TeleprompterAction::Start, false),
            ControlReply::Error { code, .. } if code == "feature-off"
        ));
        assert!(!wants_audio());
    }

    #[test]
    fn the_interviewers_turn_gets_one_hint_once_it_is_a_question() {
        use source::Speaker::{Me, Others};
        let mut live = Live::new();
        live.hints = true;
        let say = |live: &mut Live, speaker, start: u64, text: &str| {
            live.view.segments.push(text.into());
            live.view.speakers.push(Some(speaker));
            live.view.times.push((start, start + 1_000));
            asked_in(live)
        };
        assert_eq!(say(&mut live, Others, 0, "Hablemos de dinero."), None);
        assert_eq!(
            say(&mut live, Others, 1_500, "Cuánto te gustaría cobrar"),
            Some("Hablemos de dinero. Cuánto te gustaría cobrar".into())
        );
        // The same turn going on gets no second hint; the user's words none.
        assert_eq!(say(&mut live, Others, 3_000, "¿Más o menos?"), None);
        assert_eq!(say(&mut live, Me, 5_000, "¿Puedo preguntar algo?"), None);
        assert_eq!(
            say(&mut live, Others, 8_000, "¿Y en cinco años?"),
            Some("¿Y en cinco años?".into())
        );
        // Hearing only the system audio, a long quiet starts a new turn.
        let mut alone = Live::new();
        alone.hints = true;
        alone.others_only = true;
        for (start, text) in [(0, "¿Qué tal?"), (20_000, "Cuéntame un proyecto.")] {
            alone.view.segments.push(text.into());
            alone.view.speakers.push(None);
            alone.view.times.push((start, start + 1_000));
            assert_eq!(asked_in(&mut alone), Some(text.into()));
        }
    }
}
