//! The Teleprompter pane's script (MicroPrompt): Open script… reads a
//! `.txt` or `.md` file ([`crate::teleprompter::script`]) and the pane shows
//! it in large type, scrolling, in place of the live text until Close
//! script. With Labs › Interview prep on, a script with `## ` sections
//! opens as an interview ([`crate::teleprompter::interview`]): one section
//! at a time, its documents beside it, and the questions to ask ticked off.

use std::rc::Rc;

use rune::{El, TextStyle, col, row, text};
use rune_kit::{Markdown, MarkdownStyle};

use crate::desktop::file_dialog::{Dialog, Filter, Pending, Target};
use crate::desktop::i18n::{tr, tr_fill};
use crate::rune_ui::controls::{ButtonKind, METRICS, Paint, button};
use crate::rune_ui::labs::store::chip_shell;
use crate::rune_ui::platform::PlatformState;
use crate::rune_ui::{Ui, list_detail};
use crate::teleprompter::interview::{DOCUMENTS, Interview};
use crate::teleprompter::script::{Script, blocks};

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
    /// The script as an interview, opened while Labs › Interview prep is on.
    pub(in crate::rune_ui) interview: Option<Interview>,
    /// The interview's section in view.
    pub(in crate::rune_ui) section: usize,
    /// The native file dialog, while it is open, and the interview document
    /// it chooses; `None` chooses the script.
    chooser: Option<Rc<Pending>>,
    choosing: Option<usize>,
}

/// Open script…, or Close script while one shows.
pub(in crate::rune_ui) fn button_for(ui: &Ui, p: Paint) -> El {
    if ui.platform.teleprompter_script.script.is_some() {
        return button(
            p,
            "teleprompter-close-script",
            tr("Close script"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| {
                let pane = &mut ui.platform.teleprompter_script;
                pane.script = None;
                pane.interview = None;
                share(pane);
            },
        );
    }
    button(
        p,
        "teleprompter-open-script",
        tr("Open script…"),
        ButtonKind::Secondary,
        |ui: &mut Ui, _| choose(ui, None),
    )
}

/// Opens the dialog for the script, or for the interview's `document`.
fn choose(ui: &mut Ui, document: Option<usize>) {
    if ui.platform.teleprompter_script.chooser.is_some() {
        return;
    }
    let waker = ui.modes.waker.clone();
    let title = document.map_or("Open script", |index| DOCUMENTS[index].0);
    ui.platform.teleprompter_script.choosing = document;
    ui.platform.teleprompter_script.chooser = Some(Rc::new(crate::desktop::file_dialog::open(
        Dialog {
            title: tr(title).into(),
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
    let document = pane.choosing.take();
    let opened = choice.and_then(|path| {
        let script = path.as_deref().map(Script::open).transpose()?;
        Ok(script.zip(path))
    });
    match (opened, document) {
        (Ok(Some((script, path))), None) => {
            pane.interview = interview(&script, &path);
            pane.section = 0;
            pane.script = Some(script);
        }
        (Ok(Some((script, _))), Some(index)) => {
            if let Some(interview) = pane.interview.as_mut() {
                interview.documents[index] = Some(script);
            }
        }
        (Ok(None), _) => {}
        (Err(error), _) => {
            state.teleprompter.error = Some(tr_fill("Couldn't open it: {}", &error));
        }
    }
    share(&state.teleprompter_script);
    true
}

/// Hands the open script and its documents to the assistant, which reads
/// them with every action (AS-03).
fn share(pane: &ScriptPane) {
    let script = pane.script.iter().map(|script| ("Script", script));
    let documents = pane.interview.iter().flat_map(|interview| {
        DOCUMENTS
            .iter()
            .zip(&interview.documents)
            .filter_map(|((name, _), document)| Some((*name, document.as_ref()?)))
    });
    crate::teleprompter::assistant::set_documents(
        script
            .chain(documents)
            .map(|(name, document)| (name.to_owned(), document.text.clone()))
            .collect(),
    );
}

/// The script at `path` as an interview, with the documents beside it, when
/// it has sections and the Lab is on.
fn interview(script: &Script, path: &std::path::Path) -> Option<Interview> {
    let settings = crate::app_settings::AppSettings::read().unwrap_or_default();
    if !script.markdown || !crate::labs::Feature::Interview.on(&settings) {
        return None;
    }
    let mut interview = Interview::read(script)?;
    if let Some(folder) = path.parent() {
        interview.gather(folder);
    }
    Some(interview)
}

/// The reading column, under the file's name.
fn column(p: Paint, name: &str) -> El {
    col()
        .key("teleprompter-script-lines")
        .gap(BLOCK_GAP)
        .w_full()
        .max_w(MEASURE)
        .self_center()
        .px(METRICS.gutter)
        .pb(48.0)
        .child(
            text(name.to_owned())
                .key("script-name")
                .font(p.k.ty.ui)
                .color(p.faint()),
        )
}

/// The script, when one is open.
pub(in crate::rune_ui) fn view(ui: &Ui, p: Paint) -> Option<El> {
    let pane = &ui.platform.teleprompter_script;
    let script = pane.script.as_ref()?;
    if let Some(interview) = &pane.interview {
        return Some(interview_view(p, script, interview, pane.section));
    }
    let mut lines = column(p, &script.name);
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
                inset(lines),
            )),
    )
}

/// An interview: its sections to pick from, its documents, and the picked
/// section in the script's type. A task ticks off (the questions to ask).
fn interview_view(p: Paint, script: &Script, interview: &Interview, section: usize) -> El {
    let section = section.min(interview.sections.len().saturating_sub(1));
    // Chips that wrap, not a segmented control: section titles are long
    // and the pane can be narrow.
    let mut sections = row()
        .key("interview-sections")
        .role("radiogroup")
        .label(tr("Sections"))
        .gap(6.0)
        .flex_wrap();
    for (index, it) in interview.sections.iter().enumerate() {
        sections = sections.child(
            chip_shell(p, &format!("section-{index}"), &it.title, index == section)
                .checked(index == section)
                .focusable(true)
                .on_press(move |ui: &mut Ui| ui.platform.teleprompter_script.section = index),
        );
    }
    let mut documents = row().key("interview-documents").gap(8.0).flex_wrap();
    for (index, (name, _)) in DOCUMENTS.into_iter().enumerate() {
        let label = match &interview.documents[index] {
            Some(document) => format!("{}: {}", tr(name), document.name),
            None => format!("{}…", tr(name)),
        };
        documents = documents.child(button(
            p,
            &format!("interview-document-{index}"),
            &label,
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| choose(ui, Some(index)),
        ));
    }
    let style = style(p);
    let body = interview.sections.get(section).map_or("", |it| &it.body);
    let mut lines = column(p, &script.name);
    for (index, block) in blocks(body).into_iter().enumerate() {
        // The block is a slice of the body; a task's offset is within it.
        let start = block.as_ptr() as usize - body.as_ptr() as usize;
        lines = lines.child(
            Markdown::new(block)
                .on_task(move |ui: &mut Ui, _, offset, checked| {
                    if let Some(interview) = ui.platform.teleprompter_script.interview.as_mut() {
                        interview.tick(section, start + offset, checked);
                    }
                })
                .render(&style)
                .key(format!("interview-{section}-{index}"))
                .pt(if index > 0 && block.starts_with('#') {
                    SECTION_GAP
                } else {
                    0.0
                }),
        );
    }
    col()
        .key("teleprompter-interview")
        .grow(1.0)
        .shrink()
        .child(
            col()
                .key("interview-header")
                .gap(10.0)
                .px(METRICS.gutter)
                .pb(14.0)
                .child(sections)
                .child(documents),
        )
        .child(list_detail::card_scroll(p, "interview-text", inset(lines)))
}

/// The reading column inside its scroll, whose own right padding (the
/// bar's room) would replace the column's.
fn inset(column: El) -> El {
    col().key("script-inset").w_full().child(column)
}
