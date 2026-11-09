//! The Teleprompter pane's script (MicroPrompt): Open script… reads a
//! `.txt` or `.md` file ([`crate::teleprompter::script`]) and the pane shows
//! it in large type, scrolling, in place of the live text until Close
//! script.

use std::rc::Rc;

use rune::{El, TextStyle, col, text};
use rune_kit::{Markdown, MarkdownStyle};

use crate::desktop::file_dialog::{Dialog, Filter, Pending, Target};
use crate::desktop::i18n::{tr, tr_fill};
use crate::rune_ui::controls::{ButtonKind, METRICS, Paint, button};
use crate::rune_ui::platform::PlatformState;
use crate::rune_ui::{Ui, list_detail};
use crate::teleprompter::script::Script;

/// Read from a distance: larger than the live captions.
const BODY: TextStyle = TextStyle::new(32.0, 450.0).lh(48.0);
const HEADINGS: [TextStyle; 3] = [
    TextStyle::new(48.0, 700.0).lh(60.0),
    TextStyle::new(40.0, 680.0).lh(52.0),
    TextStyle::new(35.0, 650.0).lh(46.0),
];
const TABLE: TextStyle = TextStyle::new(28.0, 450.0).lh(38.0);
/// The column's widest: about 60 characters a line, easy to follow back.
const MEASURE: f32 = 860.0;
/// Between a script's blocks (rune-kit's margins suit small text), and more
/// above a heading so each section reads as one.
const BLOCK_GAP: f32 = 22.0;
const SECTION_GAP: f32 = 30.0;

/// rune-kit's Markdown look, scaled up to the script's type.
fn style(p: Paint) -> MarkdownStyle {
    let c = &p.k.c;
    let style = MarkdownStyle::from_kit(p.k)
        .body_font(BODY)
        .strong_weight(720.0)
        .code_block_font(TextStyle {
            size: 26.0,
            line_height: 38.0,
            ..p.k.ty.mono
        })
        .code_block_padding(24.0, 18.0)
        .inline_code_pad(6.0)
        .list(44.0, 14.0)
        .quote_rule(c.border_base)
        .table_font(TABLE)
        .table_cell_padding(24.0, 14.0)
        .table_min_column(140.0)
        .table_rules(c.border_base)
        .table_header_weight(680.0)
        .table_margin(0.0, 0.0);
    (1..=3).fold(style, |style, level| {
        style.heading_font(level, HEADINGS[usize::from(level) - 1])
    })
}

#[derive(Clone, Debug, Default)]
pub(in crate::rune_ui) struct ScriptPane {
    pub(in crate::rune_ui) script: Option<Script>,
    /// The native file dialog, while it is open.
    chooser: Option<Rc<Pending>>,
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
    ui.platform.teleprompter_script.chooser = Some(Rc::new(crate::desktop::file_dialog::open(
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
        .gap(BLOCK_GAP)
        .w_full()
        .max_w(MEASURE)
        .self_center()
        .px(METRICS.gutter)
        .pb(48.0)
        .child(
            text(script.name.clone())
                .key("script-name")
                .font(p.k.ty.ui)
                .color(p.faint()),
        );
    if script.markdown {
        let style = style(p);
        for (index, block) in script.blocks().into_iter().enumerate() {
            lines = lines.child(
                Markdown::new(block)
                    .render(&style)
                    .key(format!("script-{index}"))
                    .pt(if index > 0 && block.starts_with('#') {
                        SECTION_GAP
                    } else {
                        0.0
                    }),
            );
        }
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
