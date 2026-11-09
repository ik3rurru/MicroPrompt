//! The Teleprompter pane's script (MicroPrompt): Open script… reads a
//! `.txt` or `.md` file ([`crate::teleprompter::script`]) and the pane shows
//! it in large type, scrolling, in place of the live text until Close
//! script.

use std::sync::Arc;

use rune::{El, TextStyle, col, text};

use crate::desktop::file_dialog::{Dialog, Filter, Pending, Target};
use crate::desktop::i18n::{tr, tr_fill};
use crate::rune_ui::controls::{ButtonKind, METRICS, Paint, button};
use crate::rune_ui::platform::PlatformState;
use crate::rune_ui::{Ui, list_detail};
use crate::teleprompter::script::{Block, Script};

/// Read from a distance: larger than the live captions.
const BODY: TextStyle = TextStyle::new(30.0, 450.0).lh(44.0);
const HEADING: TextStyle = TextStyle::new(36.0, 650.0).lh(48.0);

#[derive(Clone, Debug, Default)]
pub(in crate::rune_ui) struct ScriptPane {
    pub(in crate::rune_ui) script: Option<Script>,
    /// The native file dialog, while it is open.
    chooser: Option<Arc<Pending>>,
}

/// Open script…, or Close script while one shows.
pub(in crate::rune_ui) fn button_for(ui: &Ui, p: Paint) -> El {
    if ui.platform.teleprompter_script.script.is_some() {
        return button(
            p,
            "teleprompter-close-script",
            tr("Close script"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.platform.teleprompter_script.script = None,
        );
    }
    button(
        p,
        "teleprompter-open-script",
        tr("Open script…"),
        ButtonKind::Secondary,
        |ui: &mut Ui, _| choose(ui),
    )
}

fn choose(ui: &mut Ui) {
    if ui.platform.teleprompter_script.chooser.is_some() {
        return;
    }
    let waker = ui.modes.waker.clone();
    ui.platform.teleprompter_script.chooser = Some(Arc::new(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Open script").into(),
            target: Target::File(vec![Filter {
                label: tr("Scripts").into(),
                extensions: &["txt", "md", "markdown"],
            }]),
            directory: crate::app_paths::document_dir(),
        },
        move || {
            if let Some(waker) = &waker {
                waker();
            }
        },
    )));
}

/// The dialog's answer (the host's poll): a chosen file is read and
/// shown; why it couldn't be shows as the pane's error. True when the pane
/// changed.
pub(in crate::rune_ui) fn poll(state: &mut PlatformState) -> bool {
    let pane = &mut state.teleprompter_script;
    let Some(choice) = pane.chooser.as_deref().and_then(Pending::poll) else {
        return false;
    };
    pane.chooser = None;
    match choice.and_then(|path| path.map(|path| Script::open(&path)).transpose()) {
        Ok(Some(script)) => pane.script = Some(script),
        Ok(None) => {}
        Err(error) => state.teleprompter.error = Some(tr_fill("Couldn't open it: {}", &error)),
    }
    true
}

/// The script, when one is open.
pub(in crate::rune_ui) fn view(ui: &Ui, p: Paint) -> Option<El> {
    let script = ui.platform.teleprompter_script.script.as_ref()?;
    let mut lines = col()
        .key("teleprompter-script-lines")
        .gap(20.0)
        .px(METRICS.gutter)
        .pb(48.0)
        .child(
            text(script.name.clone())
                .key("script-name")
                .font(p.k.ty.ui)
                .color(p.faint()),
        );
    for (index, block) in script.blocks.iter().enumerate() {
        let (words, font, indent) = match block {
            Block::Heading(words) => (words.clone(), HEADING, 0.0),
            Block::Paragraph(words) => (words.clone(), BODY, 0.0),
            Block::Item { marker, text } => (format!("{marker} {text}"), BODY, 24.0),
        };
        lines = lines.child(
            text(words)
                .key(format!("script-{index}"))
                .font(font)
                .color(p.text())
                .wrap()
                .pl(indent),
        );
    }
    Some(
        col()
            .key("teleprompter-script")
            .grow(1.0)
            .shrink()
            .child(list_detail::card_scroll(
                p,
                "teleprompter-script-text",
                lines,
            )),
    )
}
