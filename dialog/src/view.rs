//! The alert: a rounded card with a glyph, a bold title, the message and a
//! stack of full-width buttons, every choice tinted with the user's accent and
//! Cancel last.

use crownuikit::config::{theme, themed};
use crownuikit::widgets::{
    ButtonVariant, Choice, ToggleItem, button, choice_list, icon, soft_shadow, toggle_group,
};
use xilem::masonry::properties::Padding;
use xilem::masonry::properties::types::AsUnit;
use xilem::style::Style;
use xilem::view::FlexExt;
use xilem::view::{CrossAxisAlignment, FlexSpacer, flex_col, flex_row, portal, prose, sized_box};
use xilem::winit::dpi::LogicalSize;
use xilem::{AnyWidgetView, Color, FontWeight, WidgetView, WindowId, WindowView};

use crate::alert::{Alert, AlertIcon};
use crate::answer::Answer;
use crate::caution::caution;
use crate::escape::dismiss_on_escape;
use crate::layout;

pub struct Dialog {
    pub alert: Alert,
    pub window: WindowId,
    /// The picker's open section, and the row picked in it.
    pub tab: usize,
    pub picked: Option<usize>,
}

impl Dialog {
    fn answer(&mut self, answer: Answer) {
        answer.deliver();
    }

    fn confirm(&mut self) {
        let picked = self
            .picked
            .and_then(|row| self.alert.sections.get(self.tab)?.choices.get(row))
            .map(|choice| choice.id.clone());
        if let Some(id) = picked {
            self.answer(Answer::Choice(id));
        }
    }
}

impl xilem::AppState for Dialog {
    fn keep_running(&self) -> bool {
        false
    }
}

pub fn app_logic(dialog: &mut Dialog) -> impl Iterator<Item = WindowView<Dialog>> + use<> {
    let (width, height) = layout::window_size(&dialog.alert);
    let size = LogicalSize::new(width, height);
    let framed = sized_box(soft_shadow(card(dialog), layout::RADIUS))
        .expand()
        .padding(Padding::all(layout::MARGIN));
    let root = themed(dismiss_on_escape(framed, |dialog: &mut Dialog| {
        dialog.answer(Answer::Cancel);
    }));
    std::iter::once(
        xilem::window(dialog.window, dialog.alert.title.clone(), root)
            .with_base_color(Color::TRANSPARENT)
            .with_options(move |options| {
                options
                    .with_decorations(false)
                    .with_transparent(true)
                    .with_resizable(false)
                    .with_initial_inner_size(size)
                    .with_min_inner_size(size)
                    .with_max_inner_size(size)
                    .on_close(|dialog: &mut Dialog| dialog.answer(Answer::Cancel))
            }),
    )
}

fn card(dialog: &Dialog) -> impl WidgetView<Dialog> + use<> {
    let alert = &dialog.alert;
    let theme = theme();
    let actions = match alert.confirm.clone() {
        Some(confirm) => picker(dialog, confirm),
        None => buttons(alert),
    };

    let content = flex_col((
        glyph(alert.icon),
        FlexSpacer::Fixed(layout::ICON_GAP.px()),
        prose(alert.title.clone())
            .text_size(layout::TITLE_SIZE)
            .weight(FontWeight::BOLD)
            .text_color(theme.text.primary),
        FlexSpacer::Fixed(layout::TITLE_GAP.px()),
        prose(alert.message.clone())
            .text_size(layout::MESSAGE_SIZE)
            .text_color(theme.text.body),
        FlexSpacer::Flex(1.0),
        actions,
    ))
    .gap(0.0.px())
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .must_fill_major_axis(true)
    .padding(Padding {
        top: layout::PADDING_TOP,
        bottom: layout::PADDING_BOTTOM,
        left: layout::PADDING_X,
        right: layout::PADDING_X,
    });

    sized_box(content)
        .expand()
        .background_color(theme.surface.bg)
        .border(theme.surface.border, 1.0)
        .corner_radius(layout::RADIUS)
}

/// A question's answers: one tinted button per choice, then Cancel.
fn buttons(alert: &Alert) -> Box<AnyWidgetView<Dialog>> {
    let choices: Vec<_> = alert
        .choices()
        .map(|choice| {
            let id = choice.id.clone();
            button(choice.label.clone(), move |dialog: &mut Dialog| {
                dialog.answer(Answer::Choice(id.clone()));
            })
            .variant(ButtonVariant::Tinted)
            .stretch()
        })
        .collect();
    let cancel = button(alert.cancel.clone(), |dialog: &mut Dialog| {
        dialog.answer(Answer::Cancel)
    })
    .variant(ButtonVariant::Secondary)
    .stretch();
    Box::new(
        flex_col((choices, cancel))
            .gap(layout::BUTTON_GAP.px())
            .cross_axis_alignment(CrossAxisAlignment::Fill),
    )
}

/// A picker: tabs over the sections, the open section's rows, then Cancel
/// and a confirm button that waits for a pick.
fn picker(dialog: &Dialog, confirm: String) -> Box<AnyWidgetView<Dialog>> {
    let alert = &dialog.alert;
    let tabs = (alert.sections.len() > 1).then(|| {
        let items = alert
            .sections
            .iter()
            .map(|section| ToggleItem::text(section.title.clone()));
        flex_col((
            toggle_group(items, dialog.tab, |dialog: &mut Dialog, tab| {
                dialog.tab = tab;
                dialog.picked = None;
            })
            .stretch(),
            FlexSpacer::Fixed(layout::TABS_GAP.px()),
        ))
        .gap(0.0.px())
        .cross_axis_alignment(CrossAxisAlignment::Fill)
    });
    // One single-row list per choice: masonry's portal only scrolls a child
    // that outgrows it, and only a flex column of rows does.
    let rows: Vec<_> = alert
        .sections
        .get(dialog.tab)
        .into_iter()
        .flat_map(|section| {
            let glyph = section_icon(&section.title);
            section.choices.iter().map(move |choice| (choice, glyph))
        })
        .enumerate()
        .map(|(row, (choice, glyph))| {
            choice_list(
                [Choice::new(choice.label.clone(), choice.detail.clone()).icon(glyph)],
                (dialog.picked == Some(row)).then_some(0),
                move |dialog: &mut Dialog, _| dialog.picked = Some(row),
            )
        })
        .collect();
    let list = sized_box(portal(
        flex_col(rows)
            .gap(layout::ROW_GAP.px())
            .cross_axis_alignment(CrossAxisAlignment::Fill),
    ))
    .height(layout::list_height(alert).px());

    let cancel = button(alert.cancel.clone(), |dialog: &mut Dialog| {
        dialog.answer(Answer::Cancel)
    })
    .variant(ButtonVariant::Secondary)
    .stretch();
    let share = button(confirm, |dialog: &mut Dialog| dialog.confirm())
        .variant(ButtonVariant::Tinted)
        .disabled(dialog.picked.is_none())
        .stretch();
    Box::new(
        flex_col((
            tabs,
            list,
            FlexSpacer::Fixed(layout::ACTIONS_GAP.px()),
            flex_row((cancel.flex(1.0), share.flex(1.0))).gap(layout::BUTTON_GAP.px()),
        ))
        .gap(0.0.px())
        .cross_axis_alignment(CrossAxisAlignment::Fill),
    )
}

fn section_icon(title: &str) -> &'static str {
    match title {
        "Windows" => blinc_icons::icons::APP_WINDOW,
        "Workspaces" => blinc_icons::icons::LAYOUT_DASHBOARD,
        _ => blinc_icons::icons::MONITOR,
    }
}

fn glyph(kind: AlertIcon) -> Box<AnyWidgetView<Dialog>> {
    match kind {
        AlertIcon::Caution => Box::new(caution(layout::ICON)),
        AlertIcon::Screen => Box::new(
            icon(blinc_icons::icons::SCREEN_SHARE)
                .size(layout::ICON)
                .color(theme().accent.end),
        ),
        AlertIcon::Access => Box::new(
            icon(blinc_icons::icons::SHIELD_QUESTION_MARK)
                .size(layout::ICON)
                .color(theme().accent.end),
        ),
    }
}
