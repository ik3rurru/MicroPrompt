//! MicroPrompt's Teleprompter pane (Labs › Teleprompter, Linux and
//! Windows): the whole session's text, newest at the bottom, the open
//! segment's provisional words faint, scrolling to reread and following
//! the newest line while scrolled to the bottom. One button starts and
//! stops the session (`crate::teleprompter`), which runs in this process.

mod assistant;
pub(in crate::rune_ui) mod script;

use rune::{El, Span, TextStyle, Transition, col, el, rich, row, text};

use crate::app_settings::CaptionSize;
use crate::desktop::i18n::tr;
use crate::rune_ui::controls::{ButtonKind, METRICS, Paint, button, card, pulsing_dot, row_switch};
use crate::rune_ui::lists::placeholder;
use crate::rune_ui::platform::PlatformState;
use crate::rune_ui::{Tx, Ui, list_detail};
use crate::teleprompter::View;
use crate::teleprompter::source::Speaker;

/// Subtitles read from across the room, not like a settings row, at the
/// size Aa picks (`labs.teleprompter_text_size`, IN-05).
fn caption(size: CaptionSize) -> TextStyle {
    match size {
        CaptionSize::Small => TextStyle::new(20.0, 450.0).lh(28.0),
        CaptionSize::Medium => TextStyle::new(26.0, 450.0).lh(36.0),
        CaptionSize::Large => TextStyle::new(34.0, 450.0).lh(46.0),
        CaptionSize::Huge => TextStyle::new(44.0, 450.0).lh(58.0),
    }
}

/// A folded answer keeps this many non-empty lines, at most this many bytes.
const FOLDED_LINES: usize = 3;
const FOLDED_BYTES: usize = 240;
/// The assistant's answers: read at a glance during a call, below captions.
const ANSWER: TextStyle = TextStyle::new(20.0, 450.0).lh(30.0);

/// The pane shows while its Lab is on, where the listener feeds it.
pub(in crate::rune_ui) fn shown(ui: &Ui) -> bool {
    crate::labs::Feature::Teleprompter.on_this_os()
        && crate::labs::Feature::Teleprompter.on(&ui.settings)
}

/// Copies the session in when it changed (the host's poll, every frame;
/// the session wakes the window on each change). True when it did.
pub(in crate::rune_ui) fn poll(state: &mut PlatformState) -> bool {
    let script = script::poll(state) | assistant::poll(state);
    let generation = crate::teleprompter::generation();
    if generation == state.teleprompter.generation {
        return script;
    }
    let view = crate::teleprompter::view();
    // A new session starts its answers over: their flips go with the old.
    if view.answers.len() < state.teleprompter.answers.len() {
        state.teleprompter_flipped.clear();
    }
    state.teleprompter = view;
    true
}

pub(in crate::rune_ui) fn pane(ui: &Ui, p: Paint) -> (Vec<El>, El) {
    let view = &ui.platform.teleprompter;
    let lab = crate::labs::Feature::Assistant;
    let assistant = lab.on_this_os() && lab.on(&ui.settings);
    let body = script::view(ui, p).unwrap_or_else(|| body(ui, p, view, assistant));
    let body = match assistant.then(|| ask_box(ui, p)) {
        None => body,
        Some(ask) => col()
            .key("teleprompter-thread")
            .grow(1.0)
            .shrink()
            .child(body)
            .child(ask),
    };
    let script = script::button_for(ui, p);
    let actions = [text_size(ui, p), script]
        .into_iter()
        .chain(actions(ui, p, view))
        .collect();
    let assistant = assistant::card_for(ui, p);
    // Windows registers its own shortcut; elsewhere a desktop keybinding runs
    // `hex teleprompter toggle`. Hidden while a session runs: only its text.
    if !cfg!(target_os = "windows") || view.running {
        let page = match assistant {
            None => body,
            Some(assistant) => col()
                .key("teleprompter-page")
                .grow(1.0)
                .shrink()
                .child(assistant)
                .child(body),
        };
        return (actions, page);
    }
    use crate::app_settings::TeleprompterKey;
    use crate::rune_ui::ShortcutTarget::Teleprompter;
    let mut rows = crate::rune_ui::general::shortcut_rows(
        ui,
        p,
        "teleprompter-shortcut",
        &crate::rune_ui::icons::KEYBOARD,
        tr("Start and stop the session"),
        Teleprompter(TeleprompterKey::Toggle),
        Some(tr("Works with the window closed")),
    );
    // With the assistant on, its two actions from any app.
    if TeleprompterKey::Suggest.on(&ui.settings) {
        for (key, row, title, purpose) in [
            (
                TeleprompterKey::Suggest,
                "assistant-suggest-shortcut",
                "Suggest",
                "What to say now, from the conversation",
            ),
            (
                TeleprompterKey::Answer,
                "assistant-answer-shortcut",
                "Answer",
                "The full answer to what was asked",
            ),
        ] {
            rows.extend(crate::rune_ui::general::shortcut_rows(
                ui,
                p,
                row,
                &crate::rune_ui::icons::KEYBOARD,
                tr(title),
                Teleprompter(key),
                Some(tr(purpose)),
            ));
        }
    }
    let shortcut = col()
        .key("teleprompter-settings")
        .px(METRICS.gutter - METRICS.row_px)
        .pb(METRICS.section_gap)
        .child(card(p, "shortcut", rows));
    (
        actions,
        col()
            .key("teleprompter-page")
            .grow(1.0)
            .shrink()
            .child(shortcut)
            .children(assistant)
            .child(body),
    )
}

/// Start, or a recording state with Stop.
fn control(p: Paint, view: &View) -> El {
    let toggle = |_: &mut Ui, _: &mut Tx| crate::teleprompter::toggle();
    if !view.running {
        return button(
            p,
            "teleprompter-start",
            tr("Start session"),
            ButtonKind::Primary,
            toggle,
        );
    }
    let label = if view.stopping {
        tr("Stopping…")
    } else {
        tr("Stop session")
    };
    // Starts over: ends this session (kept in History) and opens a new one.
    let restart = (!view.stopping).then(|| {
        button(
            p,
            "teleprompter-restart",
            tr("New session"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| {
                if !ui.platform.teleprompter.stopping {
                    crate::teleprompter::restart();
                }
            },
        )
    });
    row()
        .key("teleprompter-running")
        .gap(8.0)
        .center_y()
        .children(restart)
        .child(pulsing_dot(p.danger()))
        .child(button(
            p,
            "teleprompter-stop",
            label,
            ButtonKind::Secondary,
            move |ui, tx| {
                if !ui.platform.teleprompter.stopping {
                    toggle(ui, tx);
                }
            },
        ))
}

fn body(ui: &Ui, p: Paint, view: &View, assistant: bool) -> El {
    let caption = caption(ui.settings.labs.teleprompter_text_size);
    let error = view.error.as_ref().map(|error| {
        text(error.clone())
            .key("teleprompter-error")
            .font(p.k.ty.ui)
            .color(p.danger())
            .wrap()
            .px(METRICS.gutter)
            .pb(8.0)
    });
    let open = format!("{}{}", view.stable, view.tentative);
    if view.segments.is_empty() && open.trim().is_empty() && view.answers.is_empty() {
        let hint = if view.running {
            tr("Listening… your words show here")
        } else {
            tr("Start a session and speak; your words show here")
        };
        return col()
            .key("teleprompter")
            .grow(1.0)
            .shrink()
            .children(error)
            .child(placeholder(p, "teleprompter-empty", hint));
    }
    let mut lines = col()
        .key("teleprompter-lines")
        .gap(14.0)
        .pl(METRICS.gutter)
        .pr(METRICS.gutter - list_detail::SCROLL_BAR)
        .pb(16.0);
    // A message names its speaker when the turn changes; with the
    // assistant on, each has a switch that leaves it out of what it reads.
    let mut last = None;
    for message in crate::teleprompter::conversation::messages(view) {
        if let Some(index) = message.answer {
            lines = lines.child(answer(ui, p, view, index, &message, assistant));
            last = None;
            continue;
        }
        let first = message.first;
        let named = message.speaker.filter(|speaker| last != Some(*speaker));
        last = message.speaker.or(last);
        let guess = message.guess_from.min(message.text.len());
        let faint = Span::new(guess, message.text.len()).color(p.faint());
        let color = if message.used || !assistant {
            p.text()
        } else {
            p.faint()
        };
        let words = rich(message.text, vec![faint])
            .key(format!("message-{first}"))
            .font(caption)
            .color(color);
        // Who speaks reads at a glance: the user's messages carry the
        // accent bar, the others' a neutral one; the name, on a turn's
        // first message, takes the same colour.
        let tint = message.speaker.map(|speaker| match speaker {
            Speaker::Me => p.accent(),
            Speaker::Others => p.k.c.border_strong,
        });
        let name = named.map(|speaker| {
            let (name, color) = match speaker {
                Speaker::Me => (tr("Me"), p.accent()),
                Speaker::Others => (tr("Others"), p.muted()),
            };
            text(name).font(p.k.ty.ui_medium).color(color)
        });
        let used = message.used;
        let switch = assistant.then(|| {
            use_in_ai(p, used, move || {
                crate::teleprompter::use_in_ai(first, !used)
            })
        });
        let header = (name.is_some() || switch.is_some()).then(|| {
            row()
                .key("header")
                .gap(8.0)
                .center_y()
                .children(name)
                .child(row().key("space").grow(1.0))
                .children(switch)
        });
        let spaced = header.is_some();
        let message = col()
            .key("message")
            .grow(1.0)
            .shrink()
            .gap(2.0)
            .children(header)
            .child(words);
        let bar = tint.map(|tint| el().key("speaker-bar").w(3.0).radius(1.5).fill(tint));
        lines = lines.child(
            row()
                .key(format!("thread-{first}"))
                .gap(12.0)
                .pt(if spaced { 6.0 } else { 0.0 })
                .children(bar)
                .child(message)
                .transition(arrives()),
        );
    }
    col()
        .key("teleprompter")
        .grow(1.0)
        .shrink()
        .children(error)
        // The scroll's own right padding (its bar's room) goes outside the
        // lines', so they end on the gutter as the ask box does.
        .child(list_detail::card_scroll_sticky(
            p,
            "teleprompter-text",
            col().key("teleprompter-inset").child(lines),
            true,
        ))
}

/// The session's button, after what it captions while none runs
/// (`labs.teleprompter_source`, kept for the next session).
fn actions(ui: &Ui, p: Paint, view: &View) -> Vec<El> {
    use crate::teleprompter::source::Source;
    if view.running {
        return vec![control(p, view)];
    }
    let labels = [tr("Microphone"), tr("System audio"), tr("Both")];
    let selected = Source::ALL
        .iter()
        .position(|source| *source == ui.settings.labs.teleprompter_source)
        .unwrap_or(0);
    let source = crate::rune_ui::controls::segmented(
        p,
        "teleprompter-source",
        &labels,
        selected,
        |ui, index| {
            let source = Source::ALL[index];
            ui.update(|settings| settings.labs.teleprompter_source = source);
        },
    );
    vec![language(ui, p), source, control(p, view)]
}

/// The session's language, picked before starting
/// (`crate::teleprompter::language`).
fn language(ui: &Ui, p: Paint) -> El {
    use crate::teleprompter::language::{Language, choose, chosen};
    let labels = Language::ALL.map(Language::label);
    let chosen = chosen(&ui.settings.transcription.language);
    let selected = Language::ALL.iter().position(|it| *it == chosen);
    crate::rune_ui::controls::segmented(
        p,
        "teleprompter-language",
        &labels,
        selected.unwrap_or(0),
        |_, index| choose(Language::ALL[index]),
    )
}

/// The first lines of a long answer, for its folded view (IN-05): up to
/// [`FOLDED_LINES`] non-empty lines and [`FOLDED_BYTES`], cut at a word.
/// `None` when the answer is short enough to show whole.
fn excerpt(text: &str) -> Option<&str> {
    let text = text.trim_end();
    let mut end = 0;
    let mut lines = 0;
    for line in text.split_inclusive('\n') {
        if lines == FOLDED_LINES {
            break;
        }
        end += line.len();
        if !line.trim().is_empty() {
            lines += 1;
        }
    }
    if end > FOLDED_BYTES {
        let mut cut = FOLDED_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        end = text[..cut].rfind(char::is_whitespace).unwrap_or(cut);
    }
    let shown = text[..end].trim_end();
    (shown.len() < text.len()).then_some(shown)
}

/// An answer of the assistant's in the thread: its action's name, its
/// switch, and its Markdown, larger than the UI to read during a call. A
/// long finished answer folds to its first lines once a newer one comes,
/// so the newest is easy to find; Show more and Show less flip it.
fn answer(
    ui: &Ui,
    p: Paint,
    view: &View,
    index: usize,
    message: &crate::teleprompter::conversation::Message,
    assistant: bool,
) -> El {
    let Some(answer) = view.answers.get(index) else {
        return col().key(format!("answer-{index}"));
    };
    let used = message.used;
    let switch = assistant.then(|| {
        use_in_ai(p, used, move || {
            crate::teleprompter::assistant::use_answer_in_ai(index, !used)
        })
    });
    let name = if answer.done {
        tr(answer.action.label()).to_owned()
    } else {
        format!("{}  ·  {}", tr(answer.action.label()), tr("Answering…"))
    };
    let header = row()
        .key("header")
        .gap(8.0)
        .center_y()
        .child(text(name).font(p.k.ty.ui).color(p.accent()))
        .child(row().key("space").grow(1.0))
        .children(switch);
    let typed = answer.question.as_ref().map(|typed| {
        text(format!("“{}”", typed.trim()))
            .key("typed")
            .font(p.k.ty.ui)
            .color(p.muted())
            .wrap()
    });
    let newest = index + 1 == view.answers.len();
    let excerpt = (answer.done && answer.error.is_none())
        .then(|| excerpt(&message.text))
        .flatten();
    let flipped = ui.platform.teleprompter_flipped.contains(&index);
    let folded = excerpt.is_some() && (newest == flipped);
    let fold = excerpt.is_some().then(|| {
        let label = if folded {
            tr("Show more")
        } else {
            tr("Show less")
        };
        crate::rune_ui::lists::small_button(
            p,
            "answer-fold",
            label,
            move |ui: &mut Ui, _: &mut Tx| {
                let flipped = &mut ui.platform.teleprompter_flipped;
                if !flipped.remove(&index) {
                    flipped.insert(index);
                }
            },
        )
        .self_start()
    });
    let body = if answer.error.is_some() {
        text(message.text.clone())
            .font(ANSWER)
            .color(p.danger())
            .wrap()
    } else {
        let style = rune_kit::MarkdownStyle::from_kit(p.k).body_font(ANSWER);
        let style = if used { style } else { style.text(p.faint()) };
        let shown = excerpt.filter(|_| folded).unwrap_or(&message.text);
        rune_kit::Markdown::new(shown).render(&style)
    };
    crate::rune_ui::controls::raised(p, &format!("answer-{index}"))
        .gap(6.0)
        .px(18.0)
        .py(14.0)
        .child(header)
        .children(typed)
        .child(body.key("answer-text"))
        .children(fold)
        .transition(arrives())
}

/// How a new message or answer joins the thread: it rises a few points as
/// it fades in, drawn only (its place in the thread is there at once, so
/// nothing below jumps). Keyed by its first segment or its answer, only a
/// new one enters; Rune keeps the fade alone under Reduce Motion.
fn arrives() -> Transition {
    Transition::asymmetric(
        Transition::opacity().combined(Transition::offset(0.0, 8.0)),
        Transition::opacity(),
    )
}

/// The box under the thread for writing to the assistant (AS-04): Enter or
/// Ask sends it; it stays while another answer is still being written.
fn ask_box(ui: &Ui, p: Paint) -> El {
    fn send(ui: &mut Ui) {
        let typed = ui.platform.teleprompter_ask.text.trim().to_owned();
        if typed.is_empty() {
            return;
        }
        let ask = crate::teleprompter::prompts::Action::Ask;
        if crate::teleprompter::assistant::ask(ask, Some(typed)) {
            ui.platform.teleprompter_ask = rune::TextEdit::default();
        }
    }
    let input = rune_kit::TextInput::new(&ui.platform.teleprompter_ask)
        .placeholder(tr("Ask the AI: an example, shorter, why…"))
        .width(None)
        .on_edit(|ui: &mut Ui| &mut ui.platform.teleprompter_ask)
        .on_submit(|ui: &mut Ui, _: &mut Tx| send(ui))
        .build(p.k)
        .key("ask-input")
        .grow(1.0);
    row()
        .key("teleprompter-ask")
        .px(METRICS.gutter)
        .py(10.0)
        .gap(8.0)
        .center_y()
        .child(input)
        .child(button(
            p,
            "teleprompter-ask-send",
            tr("Ask"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| send(ui),
        ))
}

/// Aa: the captions' size, one step larger per press, back to the
/// smallest after the largest (`labs.teleprompter_text_size`, IN-05).
fn text_size(ui: &Ui, p: Paint) -> El {
    let size = ui.settings.labs.teleprompter_text_size;
    button(
        p,
        "teleprompter-text-size",
        "Aa",
        ButtonKind::Secondary,
        |ui: &mut Ui, _: &mut Tx| {
            let next = ui.settings.labs.teleprompter_text_size.next();
            ui.update(|settings| settings.labs.teleprompter_text_size = next);
        },
    )
    .label(tr("Text size"))
    .help(crate::desktop::i18n::tr_fill(
        "Text size: {}",
        tr(size.label()),
    ))
}

/// A message's switch: the assistant reads the message while on; `flip`
/// turns it over.
fn use_in_ai(p: Paint, used: bool, flip: impl Fn() + 'static) -> El {
    row()
        .key("use-in-ai")
        .role("switch")
        .label(tr("Use in AI"))
        .checked(used)
        .focusable(true)
        .child(row_switch(p, used))
        .on_press(move |_: &mut Ui| flip())
}

#[cfg(test)]
mod tests {
    use super::excerpt;

    #[test]
    fn a_long_answer_folds_to_its_first_lines() {
        assert_eq!(excerpt("Short."), None);
        assert_eq!(excerpt("One.\n\nTwo.\n- three"), None);
        let long = "**Say this:** hola.\n\n**Key points:**\n- uno\n- dos";
        assert_eq!(
            excerpt(long),
            Some("**Say this:** hola.\n\n**Key points:**\n- uno")
        );
        let words = "palabra ".repeat(60);
        let cut = excerpt(&words).unwrap();
        assert!(cut.len() <= super::FOLDED_BYTES && cut.ends_with("palabra"));
        let accents = "é".repeat(200);
        assert!(excerpt(&accents).is_some());
    }
}
