//! A session as a thread (MicroPrompt, AS-02): its segments joined into
//! messages, one per speaker until the turn changes or they pause, each
//! used by the assistant or left out ([`View::unused`]); the assistant's
//! answers between them where they were asked (AS-03); and the text the
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
    /// so a message left out stays left out. An answer's is the segment it
    /// follows.
    pub first: usize,
    /// The assistant's answer it shows ([`View::answers`]), if it is one.
    pub answer: Option<usize>,
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
        // An answer asked here ends the message before it.
        let asked = view.answers.iter().any(|answer| answer.after == index);
        push(
            &mut messages,
            view,
            index,
            speaker,
            segment.trim(),
            paused || asked,
        );
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
    with_answers(messages, view)
}

/// `speech` with each answer after the messages heard before it was asked.
fn with_answers(speech: Vec<Message>, view: &View) -> Vec<Message> {
    let mut all = Vec::with_capacity(speech.len() + view.answers.len());
    let mut answers = view.answers.iter().enumerate().peekable();
    for message in speech {
        while let Some((index, answer)) = answers.next_if(|(_, it)| it.after <= message.first) {
            all.push(answer_message(index, answer));
        }
        all.push(message);
    }
    all.extend(answers.map(|(index, answer)| answer_message(index, answer)));
    all
}

fn answer_message(index: usize, answer: &super::Answer) -> Message {
    let text = answer
        .error
        .clone()
        .unwrap_or_else(|| answer.text.trim().to_owned());
    Message {
        first: answer.after,
        answer: Some(index),
        speaker: None,
        guess_from: text.len(),
        text,
        used: answer.used,
    }
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
            answer: None,
            speaker,
            text: text.to_owned(),
            guess_from: text.len(),
            used: !view.unused.contains(&index),
        }),
    }
}

/// The used messages the assistant reads, one a line under its speaker's
/// name (`AI:` for its finished answers, with `answers`), oldest first;
/// when they pass `budget` characters the oldest are dropped.
pub(crate) fn for_ai(view: &View, budget: usize, answers: bool) -> String {
    let mut lines = Vec::new();
    let mut used = 0;
    let read = |message: &&Message| {
        message.used
            && message.answer.is_none_or(|index| {
                let answer = &view.answers[index];
                answers && answer.done && answer.error.is_none()
            })
    };
    for message in messages(view).iter().rev().filter(read) {
        let line = match message.answer {
            Some(index) => {
                let answer = format!("AI: {}", message.text.trim());
                // What the user typed comes before its answer (AS-04).
                match &view.answers[index].question {
                    Some(typed) => format!(
                        "{} (typed): {}\n{answer}",
                        crate::desktop::i18n::tr("Me"),
                        typed.trim()
                    ),
                    None => answer,
                }
            }
            None => super::labelled(message.speaker, message.text.trim()),
        };
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
            for_ai(&view, BUDGET_CHARS, false),
            format!("{others}: Primera.\n{others}: Segunda.")
        );
        assert!(!for_ai(&view, BUDGET_CHARS, false).contains(me));
        assert_eq!(for_ai(&view, 20, false), format!("{others}: Segunda."));
    }

    #[test]
    fn an_answer_follows_what_was_heard_when_it_was_asked() {
        let mut view = view(&[
            (Some(Others), 0, 2_000, "¿Qué es un índice?"),
            (Some(Others), 2_200, 3_000, "Y otra cosa."),
        ]);
        let answer = |after, text: &str, done| crate::teleprompter::Answer {
            action: crate::teleprompter::prompts::Action::Answer,
            after,
            text: text.into(),
            done,
            error: None,
            used: true,
            question: None,
        };
        view.answers = vec![answer(1, "Una estructura que acelera búsquedas.", true)];
        let all = messages(&view);
        let shown: Vec<_> = all.iter().map(|it| (it.answer, it.text.as_str())).collect();
        assert_eq!(
            shown,
            [
                (None, "¿Qué es un índice?"),
                (Some(0), "Una estructura que acelera búsquedas."),
                (None, "Y otra cosa."),
            ]
        );
        let others = crate::desktop::i18n::tr("Others");
        assert_eq!(
            for_ai(&view, BUDGET_CHARS, true),
            format!(
                "{others}: ¿Qué es un índice?
AI: Una estructura que acelera búsquedas.
{others}: Y otra cosa."
            )
        );
        assert!(!for_ai(&view, BUDGET_CHARS, false).contains("AI:"));
        // A typed question comes before its answer (AS-04).
        view.answers[0].question = Some("¿Qué es?".into());
        let me = crate::desktop::i18n::tr("Me");
        assert!(
            for_ai(&view, BUDGET_CHARS, true)
                .contains(&format!("{me} (typed): ¿Qué es?\nAI: Una estructura"))
        );
        view.answers.push(answer(2, "Escribiendo", false));
        assert!(!for_ai(&view, BUDGET_CHARS, true).contains("Escribiendo"));
        assert_eq!(messages(&view).last().unwrap().answer, Some(1));
    }
}
