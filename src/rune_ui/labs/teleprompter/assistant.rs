//! The Teleprompter pane's assistant card (MicroPrompt, Labs › Assistant):
//! whether Ollama runs, the model that answers (`labs.assistant_model`), and
//! Suggest and Answer (AS-03), whose answers stream into the thread. The
//! model is [`crate::teleprompter::assistant`].

use rune::{El, col, icon, row, text};
use rune_kit::{Select, SelectAppearance};

use crate::desktop::i18n::tr;
use crate::rune_ui::controls::{
    ButtonKind, METRICS, Paint, button, card, described_row, segmented,
};
use crate::rune_ui::platform::PlatformState;
use crate::rune_ui::{Menu, Ui};
use crate::teleprompter::assistant::{self, Status};
use crate::teleprompter::prompts::Action;

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
    let picking = ui.open == Some(Menu::Pick(PICKER));
    let control = match model {
        Some(model) if view.models.len() > SEGMENTS => picker(p, &model.name, picking),
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
    if let Some(model) = model.filter(|_| picking && view.models.len() > SEGMENTS) {
        rows.push(choices(p, &view.models, &model.name));
    }
    if model.is_some() {
        rows.push(actions(ui, p));
    }
    Some(
        col()
            .key("teleprompter-assistant")
            .px(METRICS.gutter - METRICS.row_px)
            .pb(METRICS.section_gap)
            .child(card(p, "assistant", rows)),
    )
}

/// Suggest, Answer, Notes, and Insights; the one answering says so, and
/// none starts another until it is done.
fn actions(ui: &Ui, p: Paint) -> El {
    let answering = ui
        .platform
        .teleprompter
        .answers
        .iter()
        .find(|answer| !answer.done)
        .map(|answer| answer.action);
    let mut row = row()
        .key("assistant-actions")
        .px(METRICS.row_px)
        .py(12.0)
        .gap(8.0)
        .center_y()
        .flex_wrap();
    for action in Action::ALL {
        let label = if answering == Some(action) {
            tr("Answering…")
        } else {
            tr(action.label())
        };
        row = row.child(button(
            p,
            &format!("assistant-{}", action.label().to_lowercase()),
            label,
            ButtonKind::Secondary,
            move |_, _| {
                assistant::ask(action, None);
            },
        ));
    }
    row
}

/// Up to this many models show side by side; more open from a select.
const SEGMENTS: usize = 3;
/// The select's key, and its menu's while open.
const PICKER: &str = "assistant-model";

/// The model as a select field; it opens [`choices`] in the card. The
/// pane mounts no overlay layer, so the list opens in place.
fn picker(p: Paint, current: &str, open: bool) -> El {
    Select::new::<&str>(format!("{PICKER}-field"), &[])
        .appearance(SelectAppearance::Field)
        .width(PICKER_WIDTH)
        .current(Some(current))
        .open(open)
        .on_toggle(|ui: &mut Ui| {
            ui.open = if ui.open == Some(Menu::Pick(PICKER)) {
                None
            } else {
                Some(Menu::Pick(PICKER))
            };
        })
        .trigger(p.k)
        .label(tr("Local model"))
        .a11y_value(current)
}

const PICKER_WIDTH: f32 = 200.0;

/// The installed models, the chosen one checked; picking one closes them.
fn choices(p: Paint, models: &[assistant::Model], current: &str) -> El {
    let c = &p.k.c;
    let count = models.len();
    let mut list = col()
        .key("assistant-models")
        .role("listbox")
        .label(tr("Local model"))
        .px(METRICS.row_px - 6.0)
        .py(6.0)
        .gap(2.0);
    for (index, model) in models.iter().enumerate() {
        let selected = model.name == current;
        let name = model.name.clone();
        list = list.child(
            row()
                .key(format!("model{index}"))
                .role("option")
                .label(model.name.clone())
                .checked(selected)
                .position_in_set(index + 1, count)
                .focusable(true)
                .h(28.0)
                .px(6.0)
                .gap(8.0)
                .center_y()
                .radius(p.k.radius.md)
                .hover(c.hover)
                .child(
                    text(model.name.clone())
                        .key("label")
                        .font(p.k.ty.ui)
                        .color(p.text())
                        .grow(1.0)
                        .shrink()
                        .truncate()
                        .a11y_hidden(),
                )
                .child(
                    icon(p.k.glyph.check_small, 14.0)
                        .key("check")
                        .color(c.icon_base)
                        .opacity(if selected { 1.0 } else { 0.0 })
                        .a11y_hidden(),
                )
                .on_press(move |ui: &mut Ui| {
                    let name = name.clone();
                    ui.open = None;
                    ui.update(|settings| settings.labs.assistant_model = Some(name));
                }),
        );
    }
    list
}
