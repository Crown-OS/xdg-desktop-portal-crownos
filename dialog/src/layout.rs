//! The alert's metrics, and the window size they add up to.
//!
//! A Wayland toplevel cannot grow to fit its content after it maps without the
//! compositor moving it, so the height is worked out up front by laying the
//! text out with the same engine and style masonry will.

use crownuikit::widgets::ButtonSize;
use xilem::FontWeight;
use xilem::masonry::parley::{
    FontContext, GenericFamily, Layout, LayoutContext, LineHeight, StyleProperty,
};

use crate::alert::Alert;

/// Transparent room around the card for its shadow.
pub const MARGIN: f64 = 24.0;
pub const WIDTH: f64 = 300.0;
pub const RADIUS: f64 = 24.0;
pub const PADDING_X: f64 = 22.0;
pub const PADDING_TOP: f64 = 26.0;
pub const PADDING_BOTTOM: f64 = 20.0;
pub const ICON: f64 = 64.0;
pub const ICON_GAP: f64 = 22.0;
pub const TITLE_SIZE: f32 = 14.5;
pub const TITLE_GAP: f64 = 10.0;
pub const MESSAGE_SIZE: f32 = 14.0;
pub const ACTIONS_GAP: f64 = 22.0;
pub const BUTTON_GAP: f64 = 8.0;
pub const BUTTON: ButtonSize = ButtonSize::Medium;
/// The picker is wider: its rows carry a name and a line of detail.
pub const PICKER_WIDTH: f64 = 400.0;
pub const TABS: f64 = 28.0;
pub const TABS_GAP: f64 = 14.0;
/// `crownuikit`'s choice rows, and how many show before the list scrolls.
pub const ROW: f64 = 72.0;
pub const ROW_GAP: f64 = 8.0;
pub const VISIBLE_ROWS: usize = 3;

/// Masonry's own default: see `masonry::theme`.
const LINE_HEIGHT: f32 = 1.2;

pub fn card_width(alert: &Alert) -> f64 {
    if alert.confirm.is_some() {
        PICKER_WIDTH
    } else {
        WIDTH
    }
}

pub fn window_size(alert: &Alert) -> (f64, f64) {
    let width = card_width(alert);
    let measure = width - 2.0 * PADDING_X;
    let actions = match alert.confirm {
        Some(_) => {
            let tabs = if alert.sections.len() > 1 {
                TABS + TABS_GAP
            } else {
                0.0
            };
            tabs + list_height(alert) + ACTIONS_GAP + BUTTON.height()
        }
        None => {
            let buttons = alert.choices().count() + 1;
            buttons as f64 * BUTTON.height() + (buttons - 1) as f64 * BUTTON_GAP
        }
    };
    let height = PADDING_TOP
        + ICON
        + ICON_GAP
        + text_height(&alert.title, TITLE_SIZE, FontWeight::BOLD, measure)
        + TITLE_GAP
        + text_height(&alert.message, MESSAGE_SIZE, FontWeight::NORMAL, measure)
        + ACTIONS_GAP
        + actions
        + PADDING_BOTTOM;
    (width + 2.0 * MARGIN, height.ceil() + 2.0 * MARGIN)
}

/// Tall enough for the longest section, up to [`VISIBLE_ROWS`], so switching
/// tabs never resizes the window.
pub fn list_height(alert: &Alert) -> f64 {
    let rows = alert
        .sections
        .iter()
        .map(|section| section.choices.len())
        .max()
        .unwrap_or(1)
        .clamp(1, VISIBLE_ROWS);
    rows as f64 * ROW + (rows - 1) as f64 * ROW_GAP
}

fn text_height(text: &str, size: f32, weight: FontWeight, measure: f64) -> f64 {
    let mut fonts = FontContext::new();
    let mut layouts = LayoutContext::<()>::new();
    let mut builder = layouts.ranged_builder(&mut fonts, text, 1.0, true);
    builder.push_default(GenericFamily::SystemUi);
    builder.push_default(StyleProperty::FontSize(size));
    builder.push_default(StyleProperty::FontWeight(weight));
    builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
        LINE_HEIGHT,
    )));
    let mut layout: Layout<()> = builder.build(text);
    layout.break_all_lines(Some(measure as f32));
    f64::from(layout.height())
}
