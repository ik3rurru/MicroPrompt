//! The Teleprompter pane's assistant card (MicroPrompt, Labs › Assistant):
//! whether Ollama runs, the model that answers (`labs.assistant_model`), and
//! Try it, a one-sentence summary of the thread the assistant may read (an
//! introduction before anything is said), streaming in. The model is
//! [`crate::teleprompter::assistant`].

use rune::{El, col, row, text};

use crate::desktop::i18n::tr;
use crate::rune_ui::Ui;
use crate::rune_ui::controls::{
    ButtonKind, METRICS, Paint, button, card, described_row, segmented,
};
use crate::rune_ui::platform::PlatformState;
use crate::teleprompter::assistant::{self, Status};

/// Copies the assistant in when it changed. True when it did.
pub(in crate::rune_ui) fn poll(state: &mut PlatformState) -> bool {
    if assistant::generation() == state.teleprompter_assistant.generation {
        return false;
    }
    state.teleprompter_assistant = assistant::view();
    true
}

/// The card, while Labs › Assistant is on.
pub(in crate::rune_ui) fn card_for(ui: &Ui, p: Paint) -> Option<El> {
    let lab = crate::labs::Feature::Assistant;
    if !lab.on_this_os() || !lab.on(&ui.settings) {
        return None;
    }
    let view = &ui.platform.teleprompter_assistant;
    // Looks once the card first shows; Check again looks again.
    if view.status == Status::Unchecked {
        assistant::check();
    }
    let chosen = ui.settings.labs.assistant_model.as_deref();
    let model = view.model(chosen);
    let (status, color) = match view.status {
        Status::Unchecked | Status::Checking => (tr("Checking Ollama…"), p.muted()),
        Status::NotRunning => (tr("Ollama isn't running"), p.danger()),
        Status::Running if model.is_none() => (
            tr("Ollama has no local models: ollama pull gemma4:12b"),
            p.danger(),
        ),
        Status::Running => (tr("Ollama is running"), p.muted()),
    };
    let control = match model {
        // ponytail: a segmented row fits a few models; a select once
        // people keep many.
        Some(model) => {
            let names: Vec<String> = view.models.iter().map(|it| it.name.clone()).collect();
            let selected = names.iter().position(|it| *it == model.name).unwrap_or(0);
            let labels: Vec<&str> = view.models.iter().map(|it| it.name.as_str()).collect();
            segmented(p, "assistant-model", &labels, selected, move |ui, index| {
                let name = names[index].clone();
                ui.update(|settings| settings.labs.assistant_model = Some(name));
            })
        }
        None => button(
            p,
            "assistant-check",
            tr("Check again"),
            ButtonKind::Secondary,
            |_, _| assistant::check(),
        ),
    };
    let mut rows = vec![described_row(
        p,
        "assistant-model-row",
        tr("Local model"),
        Some((status.to_owned(), color)),
        control,
    )];
    if let Some(model) = model.cloned() {
        rows.push(answer(p, view, model));
    }
    Some(
        col()
            .key("teleprompter-assistant")
            .px(METRICS.gutter - METRICS.row_px)
            .pb(METRICS.section_gap)
            .child(card(p, "assistant", rows)),
    )
}

/// Try it, and the answer as it streams in.
fn answer(p: Paint, view: &assistant::View, model: assistant::Model) -> El {
    let label = if view.asking {
        tr("Answering…")
    } else {
        tr("Try it")
    };
    let ask = button(
        p,
        "assistant-try",
        label,
        ButtonKind::Secondary,
        move |ui, _| {
            if !ui.platform.teleprompter_assistant.asking {
                assistant::try_it(model.clone(), question(&ui.platform.teleprompter));
            }
        },
    );
    let said = view
        .error
        .clone()
        .map(|error| (error, p.danger()))
        .or_else(|| (!view.answer.is_empty()).then(|| (view.answer.trim().to_owned(), p.text())));
    let said = said.map(|(copy, color)| {
        text(copy)
            .key("assistant-answer")
            .font(p.k.ty.ui)
            .color(color)
            .wrap()
            .grow(1.0)
            .shrink()
    });
    row()
        .key("assistant-try-row")
        .px(METRICS.row_px)
        .py(12.0)
        .gap(12.0)
        .center_y()
        .child(ask)
        .children(said)
}

/// Try it's question: a one-sentence summary of what the thread lets the
/// assistant read, or, before anything is said, an introduction.
fn question(session: &crate::teleprompter::View) -> String {
    use crate::teleprompter::conversation::{BUDGET_CHARS, for_ai};
    let conversation = for_ai(session, BUDGET_CHARS);
    if conversation.is_empty() {
        return tr("Introduce yourself in one short sentence.").into();
    }
    format!(
        "{}

{conversation}",
        tr("Summarize this conversation in one sentence:")
    )
}
