//! The Teleprompter pane's script (MicroPrompt): Open script… reads a
//! `.txt` or `.md` file ([`crate::teleprompter::script`]) and the pane shows
//! it in large type, scrolling, in place of the live text until Close
//! script.

use std::sync::Arc;

use rune::{El, TextStyle, col, text};
use rune_kit::{Markdown, MarkdownStyle};

use crate::desktop::file_dialog::{Dialog, Filter, Pending, Target};
use crate::desktop::i18n::{tr, tr_fill};
use crate::rune_ui::controls::{ButtonKind, METRICS, Paint, button};
use crate::rune_ui::platform::PlatformState;
use crate::rune_ui::{Ui, list_detail};
use crate::teleprompter::script::Script;

/// Read from a distance: larger than the live captions.
const BODY: TextStyle = TextStyle::new(30.0, 450.0).lh(44.0);
const HEADINGS: [TextStyle; 3] = [
    TextStyle::new(44.0, 700.0).lh(56.0),
    TextStyle::new(38.0, 650.0).lh(50.0),
    TextStyle::new(33.0, 650.0).lh(46.0),
];

/// rune-kit's Markdown look, scaled up to the script's type.
fn style(p: Paint) -> MarkdownStyle {
    let style = MarkdownStyle::from_kit(p.k)
        .body_font(BODY)
        .code_block_font(TextStyle {
            size: 24.0,
            line_height: 36.0,
            ..p.k.ty.mono
        })
        .code_block_padding(20.0, 14.0)
        .list(36.0, 12.0);
    (1..=3).fold(style, |style, level| {
        style.heading_font(level, HEADINGS[usize::from(level) - 1])
    })
}

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
    if script.markdown {
        lines = lines.child(
            Markdown::new(script.text.clone())
                .style(style(p))
                .build(p.k)
                .key("script-markdown"),
        );
    } else {
        for (index, paragraph) in script.paragraphs().enumerate() {
            lines = lines.child(
                text(paragraph)
                    .key(format!("script-{index}"))
                    .font(BODY)
                    .color(p.text())
                    .wrap(),
            );
        }
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
