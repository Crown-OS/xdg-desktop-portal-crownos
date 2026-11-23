//! The alert: a modal card over a dimmed output with a glyph, a bold title,
//! the message and the actions. A question stacks one tinted button per
//! choice over Cancel; a picker shows tabs over the sections, the open
//! section's rows, and Cancel beside a confirm button that waits for a pick.

use crownui::{
    kit::{
        ButtonOptions, ButtonVariant, Choice, DialogOptions, Theme, ToggleGroupOptions, ToggleItem,
        button, choice_list, dialog, icon_colored, icons, provide_configured_theme, toggle_group,
        use_theme,
    },
    prelude::{
        AnyView, Color, IntoProp, Listeners, Runtime, Semantic, Signal, Styled, View, bind,
        component, hstack, keyed, scroll_view, text, vstack,
    },
};

use std::rc::Rc;

use crate::alert::{Alert, AlertIcon, Section};
use crate::answer::Answer;

#[cfg(test)]
mod tests;

/// Where the user's answer goes: the binary prints it and exits, tests record it.
type Respond = Rc<dyn Fn(Answer)>;

const QUESTION_WIDTH: f32 = 320.0;
/// The picker is wider: its rows carry a name and a line of detail.
const PICKER_WIDTH: f32 = 400.0;
const ICON_SIZE: f32 = 64.0;
const ICON_STROKE: f32 = 1.5;
const TITLE_SIZE: f32 = 14.5;
const MESSAGE_SIZE: f32 = 14.0;
const CONTENT_GAP: f32 = 12.0;
const BUTTON_GAP: f32 = 8.0;
/// About three rows show before the list scrolls.
const LIST_MAX_HEIGHT: f32 = 236.0;

/// The whole overlay: closing the dialog any other way than a button declines.
pub fn root(alert: Alert, respond: impl Fn(Answer) + 'static) -> impl View {
    let respond: Respond = Rc::new(respond);
    component(move |cx| {
        provide_configured_theme(cx);
        let open = cx.signal(true);
        let decline = respond.clone();
        cx.effect(move |runtime| {
            if !open.get(runtime) {
                decline(Answer::Cancel);
            }
        });
        dialog(open, move || content(alert.clone(), respond.clone())).dismiss_on_scrim(false)
    })
}

fn content(alert: Alert, respond: Respond) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        let width = match alert.confirm {
            Some(_) => PICKER_WIDTH,
            None => QUESTION_WIDTH,
        };
        let actions = match alert.confirm.clone() {
            Some(confirm) => {
                let tab = cx.signal(0);
                let picked = cx.signal(None);
                cx.effect(move |runtime| {
                    tab.get(runtime);
                    picked.set(runtime, None);
                });
                picker(&alert, confirm, tab, picked, respond)
            }
            None => questions(&alert, respond),
        };
        vstack((
            glyph(alert.icon),
            text(alert.title.clone())
                .text_size(TITLE_SIZE)
                .font_semibold()
                .text_color(themed(theme, |theme| theme.text.primary)),
            text(alert.message.clone())
                .text_size(MESSAGE_SIZE)
                .text_color(themed(theme, |theme| theme.text.body)),
            actions,
        ))
        .gap(CONTENT_GAP)
        .w(width)
    })
}

fn questions(alert: &Alert, respond: Respond) -> AnyView {
    let choices: Vec<AnyView> = alert
        .choices()
        .map(|choice| {
            let id = choice.id.clone();
            let respond = respond.clone();
            button(choice.label.clone())
                .variant(ButtonVariant::Tinted)
                .stretch()
                .on_click(move |_, _| respond(Answer::Choice(id.clone())))
                .boxed()
        })
        .collect();
    vstack((choices, cancel_button(&alert.cancel, respond)))
        .gap(BUTTON_GAP)
        .boxed()
}

fn picker(
    alert: &Alert,
    confirm: String,
    tab: Signal<usize>,
    picked: Signal<Option<usize>>,
    respond: Respond,
) -> AnyView {
    let tabs = (alert.sections.len() > 1).then(|| {
        let items = alert
            .sections
            .iter()
            .map(|section| ToggleItem::text(section.title.clone()));
        toggle_group(items, tab).stretch()
    });
    let sections = alert.sections.clone();
    let rows = keyed(
        move |runtime| tab.get(runtime),
        move |_, open| match sections.get(open) {
            Some(section) => choice_list(section_choices(section), picked).boxed(),
            None => vstack(()).boxed(),
        },
    );
    let sections = alert.sections.clone();
    let confirm_respond = respond.clone();
    let share = button(confirm)
        .variant(ButtonVariant::Tinted)
        .stretch()
        .disabled(bind(move |runtime: &mut Runtime| {
            picked.get(runtime).is_none()
        }))
        .on_click(move |cx, _| {
            let id = picked.get(cx).and_then(|row| {
                let section = sections.get(tab.get(cx))?;
                Some(section.choices.get(row)?.id.clone())
            });
            if let Some(id) = id {
                confirm_respond(Answer::Choice(id));
            }
        });
    vstack((
        tabs,
        scroll_view(rows).max_h(LIST_MAX_HEIGHT),
        hstack((
            cancel_button(&alert.cancel, respond).flex_1(),
            share.flex_1(),
        ))
        .gap(BUTTON_GAP),
    ))
    .gap(CONTENT_GAP)
    .boxed()
}

fn section_choices(section: &Section) -> Vec<Choice> {
    let glyph = section_icon(&section.title);
    section
        .choices
        .iter()
        .map(|choice| Choice::new(choice.label.clone(), choice.detail.clone()).icon(glyph))
        .collect()
}

fn cancel_button(label: &str, respond: Respond) -> impl View + Styled + use<> {
    button(label.to_owned())
        .variant(ButtonVariant::Secondary)
        .stretch()
        .on_click(move |_, _| respond(Answer::Cancel))
}

fn section_icon(title: &str) -> &'static str {
    match title {
        "Windows" => icons::APP_WINDOW,
        "Workspaces" => icons::LAYOUT_DASHBOARD,
        _ => icons::MONITOR,
    }
}

fn glyph(kind: AlertIcon) -> impl View {
    let (body, color): (&'static str, fn(&Theme) -> Color) = match kind {
        AlertIcon::Caution => (icons::TRIANGLE_ALERT, |theme| theme.status.warning),
        AlertIcon::Screen => (icons::SCREEN_SHARE, |theme| theme.accent.end),
        AlertIcon::Access => (icons::SHIELD_QUESTION_MARK, |theme| theme.accent.end),
    };
    icon_colored(body, ICON_STROKE, color).size(ICON_SIZE)
}

fn themed(theme: Signal<Theme>, pick: fn(&Theme) -> Color) -> impl IntoProp<Color> {
    bind(move |runtime| theme.with(runtime, pick))
}
