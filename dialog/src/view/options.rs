//! The extra questions under the message: a switch for each checkbox and a
//! group of radio rows for each list.

use crownui::{
    kit::{Theme, ToggleOptions, radio_group, toggle, use_theme},
    prelude::{AnyView, Cx, Runtime, Signal, Styled, View, text, vstack},
};

use crate::options::{AlertOption, OptionKind};

use super::themed;

const OPTION_GAP: f32 = 8.0;
const LABEL_SIZE: f32 = 13.0;

#[derive(Clone)]
enum Control {
    Checkbox(Signal<bool>),
    List {
        items: Vec<String>,
        selected: Signal<Option<usize>>,
    },
}

/// One extra question with the answer the user gives.
#[derive(Clone)]
pub struct OptionState {
    label: String,
    control: Control,
}

pub fn option_states(cx: &mut Cx, options: &[AlertOption]) -> Vec<OptionState> {
    options
        .iter()
        .map(|option| OptionState {
            label: option.label.clone(),
            control: match &option.kind {
                OptionKind::Checkbox { initially_on } => {
                    Control::Checkbox(cx.signal(*initially_on))
                }
                OptionKind::List { items, initial } => Control::List {
                    items: items.clone(),
                    selected: cx.signal(Some(*initial)),
                },
            },
        })
        .collect()
}

/// One value per question, in the form the portal reads.
pub fn option_values(states: &[OptionState], runtime: &mut Runtime) -> Vec<String> {
    states
        .iter()
        .map(|state| match &state.control {
            Control::Checkbox(on) => on.get(runtime).to_string(),
            Control::List { selected, .. } => selected.get(runtime).unwrap_or_default().to_string(),
        })
        .collect()
}

pub fn options_view(cx: &mut Cx, states: &[OptionState]) -> AnyView {
    let theme = use_theme(cx);
    let rows: Vec<AnyView> = states
        .iter()
        .map(|state| match &state.control {
            Control::Checkbox(on) => toggle(*on).caption(state.label.clone()).boxed(),
            Control::List { items, selected } => vstack((
                text(state.label.clone())
                    .text_size(LABEL_SIZE)
                    .text_color(themed(theme, |theme: &Theme| theme.text.body)),
                radio_group(items.iter().cloned(), *selected),
            ))
            .gap(OPTION_GAP)
            .boxed(),
        })
        .collect();
    vstack(rows).gap(OPTION_GAP).boxed()
}
