//! MicroPrompt's Teleprompter pane (Labs › Teleprompter, Linux and
//! Windows): the whole session's text, newest at the bottom, the open
//! segment's provisional words faint, scrolling to reread and following
//! the newest line while scrolled to the bottom. One button starts and
//! stops the session (`crate::teleprompter`), which runs in this process.

use rune::{El, Span, TextStyle, col, rich, row, text};

use crate::desktop::i18n::tr;
use crate::rune_ui::controls::{ButtonKind, METRICS, Paint, button, card, pulsing_dot};
use crate::rune_ui::lists::placeholder;
use crate::rune_ui::platform::PlatformState;
use crate::rune_ui::{Tx, Ui, list_detail};
use crate::teleprompter::View;
use crate::teleprompter::source::Speaker;

/// Subtitles read from across the room, not like a settings row.
const CAPTION: TextStyle = TextStyle::new(26.0, 450.0).lh(36.0);

/// The pane shows while its Lab is on, where the listener feeds it.
pub(in crate::rune_ui) fn shown(ui: &Ui) -> bool {
    crate::labs::Feature::Teleprompter.on_this_os()
        && crate::labs::Feature::Teleprompter.on(&ui.settings)
}

/// Copies the session in when it changed (the host's poll, every frame;
/// the session wakes the window on each change). True when it did.
pub(in crate::rune_ui) fn poll(state: &mut PlatformState) -> bool {
    let script =
        super::teleprompter_script::poll(state) | super::teleprompter_assistant::poll(state);
    let generation = crate::teleprompter::generation();
    if generation == state.teleprompter.generation {
        return script;
    }
    state.teleprompter = crate::teleprompter::view();
    true
}

pub(in crate::rune_ui) fn pane(ui: &Ui, p: Paint) -> (Vec<El>, El) {
    let view = &ui.platform.teleprompter;
    let body = super::teleprompter_script::view(ui, p).unwrap_or_else(|| body(p, view));
    let script = super::teleprompter_script::button_for(ui, p);
    let actions = std::iter::once(script)
        .chain(actions(ui, p, view))
        .collect();
    let assistant = super::teleprompter_assistant::card_for(ui, p);
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
    let rows = crate::rune_ui::general::shortcut_rows(
        ui,
        p,
        "teleprompter-shortcut",
        &crate::rune_ui::icons::KEYBOARD,
        tr("Start and stop the session"),
        crate::rune_ui::ShortcutTarget::Teleprompter,
        Some(tr("Works with the window closed")),
    );
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
    row()
        .key("teleprompter-running")
        .gap(8.0)
        .center_y()
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

fn body(p: Paint, view: &View) -> El {
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
    if view.segments.is_empty() && open.trim().is_empty() {
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
        .px(METRICS.gutter)
        .pb(16.0);
    // With both sources, a line names its speaker when the turn changes.
    let mut last = None;
    let mut named = |line: El, key: String, speaker: Option<Speaker>| {
        let Some(speaker) = speaker.filter(|speaker| last != Some(*speaker)) else {
            return line;
        };
        last = Some(speaker);
        let name = match speaker {
            Speaker::Me => tr("Me"),
            Speaker::Others => tr("Others"),
        };
        let name = text(name).font(p.k.ty.ui).color(p.faint());
        col().key(key).gap(2.0).pt(6.0).child(name).child(line)
    };
    for (index, segment) in view.segments.iter().enumerate() {
        let speaker = view.speakers.get(index).copied().flatten();
        let line = text(segment.trim().to_owned())
            .key(format!("segment-{index}"))
            .font(CAPTION)
            .color(p.text())
            .wrap();
        lines = lines.child(named(line, format!("speaker-{index}"), speaker));
    }
    if !open.trim().is_empty() {
        let stable = view.stable.trim_start();
        let words = format!("{stable}{}", view.tentative);
        let words = words.trim_end().to_owned();
        let faint = Span::new(stable.len().min(words.len()), words.len()).color(p.faint());
        let line = rich(words, vec![faint])
            .key("open-segment")
            .font(CAPTION)
            .color(p.text());
        lines = lines.child(named(line, "speaker-open".into(), view.speaker));
    }
    col()
        .key("teleprompter")
        .grow(1.0)
        .shrink()
        .children(error)
        .child(list_detail::card_scroll_sticky(
            p,
            "teleprompter-text",
            lines,
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
