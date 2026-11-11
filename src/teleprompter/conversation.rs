//! A session as a thread (MicroPrompt, AS-02): its segments joined into
//! messages, one per speaker until the turn changes or they pause, each
//! used by the assistant or left out ([`View::unused`]), and the text the
//! assistant reads ([`for_ai`]).

use super::View;
use super::source::Speaker;

/// A pause this long between two segments starts a new message, as
/// Open-Cluely joins a speaker's words until 2.4 s of quiet.
pub(crate) const PAUSE_MS: u64 = 2_500;

/// What the assistant reads by default: the newest messages up to about
/// 3,000 words.
pub(crate) const BUDGET_CHARS: usize = 16_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    /// Its first segment's index: the message keeps it as segments join,
    /// so a message left out stays left out.
    pub first: usize,
    pub speaker: Option<Speaker>,
    /// Its closed segments, then the words still being heard.
    pub text: String,
    /// Where in `text` the model's guess begins, shown faint.
    pub guess_from: usize,
    pub used: bool,
}

/// The session's messages, oldest first; the open segment ends the last one
/// when its speaker said it, else it starts its own.
pub(crate) fn messages(view: &View) -> Vec<Message> {
    let mut messages: Vec<Message> = Vec::new();
    let mut previous_end = None;
    for (index, segment) in view.segments.iter().enumerate() {
        let speaker = view.speakers.get(index).copied().flatten();
        let time = view.times.get(index).copied();
        let paused = match (previous_end, time) {
            (Some(end), Some((start, _))) => start.saturating_sub(end) >= PAUSE_MS,
            _ => false,
        };
        previous_end = time.map(|(_, end)| end);
        push(&mut messages, view, index, speaker, segment.trim(), paused);
    }
    let stable = view.stable.trim_start();
    if !format!("{stable}{}", view.tentative).trim().is_empty() {
        push(
            &mut messages,
            view,
            view.segments.len(),
            view.speaker,
            stable,
            false,
        );
        let last = messages.last_mut().expect("just pushed");
        last.guess_from = last.text.len();
        let guess = view.tentative.trim_end();
        let guess = if last.text.is_empty() {
            guess.trim_start()
        } else {
            guess
        };
        last.text.push_str(guess);
    }
    messages
}

/// `text` (segment `index`'s) onto the last message, or as a new one.
fn push(
    messages: &mut Vec<Message>,
    view: &View,
    index: usize,
    speaker: Option<Speaker>,
    text: &str,
    paused: bool,
) {
    match messages.last_mut() {
        Some(last) if last.speaker == speaker && !paused => {
            if !last.text.is_empty() && !text.is_empty() {
                last.text.push(' ');
            }
            last.text.push_str(text);
            last.guess_from = last.text.len();
        }
        _ => messages.push(Message {
            first: index,
            speaker,
            text: text.to_owned(),
            guess_from: text.len(),
            used: !view.unused.contains(&index),
        }),
    }
}

/// The used messages the assistant reads, one a line under its speaker's
/// name, oldest first; when they pass `budget` characters the oldest are
/// dropped.
pub(crate) fn for_ai(view: &View, budget: usize) -> String {
    let mut lines = Vec::new();
    let mut used = 0;
    for message in messages(view).iter().rev().filter(|it| it.used) {
        let line = super::labelled(message.speaker, message.text.trim());
        used += line.len() + 1;
        if used > budget {
            break;
        }
        lines.push(line);
    }
    lines.reverse();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use Speaker::{Me, Others};

    fn view(segments: &[(Option<Speaker>, u64, u64, &str)]) -> View {
        View {
            segments: segments.iter().map(|it| it.3.into()).collect(),
            speakers: segments.iter().map(|it| it.0).collect(),
            times: segments.iter().map(|it| (it.1, it.2)).collect(),
            ..View::default()
        }
    }

    fn texts(view: &View) -> Vec<(Option<Speaker>, String)> {
        messages(view)
            .into_iter()
            .map(|it| (it.speaker, it.text))
            .collect()
    }

    #[test]
    fn a_speakers_words_join_until_the_turn_changes_or_they_pause() {
        let view = view(&[
            (Some(Others), 0, 2_000, "Hola."),
            (Some(Others), 2_300, 4_000, "¿Me cuentas algo de ti?"),
            (Some(Me), 4_500, 6_000, "Claro."),
            (Some(Me), 6_200, 9_000, "Llevo siete años en backend."),
            (Some(Me), 12_000, 14_000, "Y antes fui profesor."),
        ]);
        assert_eq!(
            texts(&view),
            [
                (Some(Others), "Hola. ¿Me cuentas algo de ti?".into()),
                (Some(Me), "Claro. Llevo siete años en backend.".into()),
                (Some(Me), "Y antes fui profesor.".into()),
            ]
        );
        let firsts: Vec<_> = messages(&view).iter().map(|it| it.first).collect();
        assert_eq!(firsts, [0, 2, 4]);
    }

    #[test]
    fn the_open_words_end_their_speakers_message_the_guess_last() {
        let mut view = view(&[(Some(Others), 0, 2_000, "Vale.")]);
        view.speaker = Some(Others);
        view.stable = " ¿Y por qué".into();
        view.tentative = " quieres ".into();
        let last = messages(&view).pop().unwrap();
        assert_eq!(last.text, "Vale. ¿Y por qué quieres");
        assert_eq!(&last.text[last.guess_from..], " quieres");

        view.speaker = Some(Me);
        let all = messages(&view);
        assert_eq!(all.len(), 2);
        assert_eq!(
            (all[1].first, all[1].text.as_str()),
            (1, "¿Y por qué quieres")
        );
    }

    #[test]
    fn the_assistant_reads_the_used_messages_newest_kept_within_the_budget() {
        let mut view = view(&[
            (Some(Others), 0, 1_000, "Primera."),
            (Some(Me), 1_500, 2_000, "Ruido de fondo."),
            (Some(Others), 2_500, 3_000, "Segunda."),
        ]);
        view.unused.insert(1);
        let me = crate::desktop::i18n::tr("Me");
        let others = crate::desktop::i18n::tr("Others");
        assert_eq!(
            for_ai(&view, BUDGET_CHARS),
            format!("{others}: Primera.\n{others}: Segunda.")
        );
        assert!(!for_ai(&view, BUDGET_CHARS).contains(me));
        assert_eq!(for_ai(&view, 20), format!("{others}: Segunda."));
    }
}
