//! The caution sign an alert leads with: a rounded amber triangle carrying a
//! white exclamation mark.

use xilem::core::{MessageContext, MessageResult, Mut, View, ViewMarker};
use xilem::masonry::accesskit::{Node, Role};
use xilem::masonry::core::{
    AccessCtx, BoxConstraints, ChildrenIds, LayoutCtx, NoAction, PaintCtx, PropertiesMut,
    PropertiesRef, RegisterCtx, Widget,
};
use xilem::masonry::kurbo::{Affine, BezPath, Circle, Point, RoundedRect, Size, Stroke};
use xilem::masonry::peniko::{Color, Fill, Gradient};
use xilem::masonry::vello::Scene;
use xilem::{Pod, ViewCtx};

/// Amber in the shades of a road sign, deliberately not the theme's: a
/// warning reads as one whatever accent the user picked.
const AMBER_TOP: Color = Color::from_rgb8(0xFF, 0xD9, 0x3D);
const AMBER_BOTTOM: Color = Color::from_rgb8(0xF2, 0xA9, 0x00);
const RIM: Color = Color::from_rgba8(0xB7, 0x7A, 0x00, 0x66);
const MARK: Color = Color::WHITE;

/// Fractions of the glyph's size.
const CORNER: f64 = 0.1;
const BAR_WIDTH: f64 = 0.09;
const BAR_TOP: f64 = 0.34;
const BAR_BOTTOM: f64 = 0.64;
const DOT_CENTER: f64 = 0.755;
const DOT_RADIUS: f64 = 0.056;

pub struct CautionGlyph {
    size: f64,
}

impl Widget for CautionGlyph {
    type Action = NoAction;

    fn register_children(&mut self, _ctx: &mut RegisterCtx<'_>) {}

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        bc: &BoxConstraints,
    ) -> Size {
        bc.constrain(Size::new(self.size, self.size))
    }

    fn paint(&mut self, _ctx: &mut PaintCtx<'_>, _props: &PropertiesRef<'_>, scene: &mut Scene) {
        let size = self.size;
        let triangle = rounded_triangle(size);
        let amber = Gradient::new_linear(Point::ORIGIN, Point::new(0.0, size))
            .with_stops([(0.0_f32, AMBER_TOP), (1.0_f32, AMBER_BOTTOM)]);
        scene.fill(Fill::NonZero, Affine::IDENTITY, &amber, None, &triangle);
        scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, RIM, None, &triangle);

        let half_bar = size * BAR_WIDTH / 2.0;
        let bar = RoundedRect::new(
            size / 2.0 - half_bar,
            size * BAR_TOP,
            size / 2.0 + half_bar,
            size * BAR_BOTTOM,
            half_bar,
        );
        let dot = Circle::new((size / 2.0, size * DOT_CENTER), size * DOT_RADIUS);
        scene.fill(Fill::NonZero, Affine::IDENTITY, MARK, None, &bar);
        scene.fill(Fill::NonZero, Affine::IDENTITY, MARK, None, &dot);
    }

    fn accessibility_role(&self) -> Role {
        Role::Image
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        node: &mut Node,
    ) {
        node.set_label("Warning");
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::new()
    }
}

/// An equilateral-ish triangle whose corners are rounded with quadratic arcs.
fn rounded_triangle(size: f64) -> BezPath {
    let corners = [
        Point::new(size * 0.5, size * 0.06),
        Point::new(size * 0.97, size * 0.9),
        Point::new(size * 0.03, size * 0.9),
    ];
    let toward = |from: Point, to: Point| from + (to - from).normalize() * (size * CORNER);
    let mut path = BezPath::new();
    for (index, &corner) in corners.iter().enumerate() {
        let previous = corners[(index + corners.len() - 1) % corners.len()];
        let next = corners[(index + 1) % corners.len()];
        let entry = toward(corner, previous);
        if index == 0 {
            path.move_to(entry);
        } else {
            path.line_to(entry);
        }
        path.quad_to(corner, toward(corner, next));
    }
    path.close_path();
    path
}

pub fn caution(size: f64) -> CautionView {
    CautionView { size }
}

#[must_use = "View values do nothing unless provided to Xilem."]
pub struct CautionView {
    size: f64,
}

impl ViewMarker for CautionView {}
impl<State: 'static, Action: 'static> View<State, Action, ViewCtx> for CautionView {
    type Element = Pod<CautionGlyph>;
    type ViewState = ();

    fn build(&self, ctx: &mut ViewCtx, _: &mut State) -> (Self::Element, Self::ViewState) {
        (ctx.create_pod(CautionGlyph { size: self.size }), ())
    }

    fn rebuild(
        &self,
        _: &Self,
        (): &mut (),
        _: &mut ViewCtx,
        _: Mut<'_, Self::Element>,
        _: &mut State,
    ) {
    }

    fn teardown(&self, (): &mut (), ctx: &mut ViewCtx, element: Mut<'_, Self::Element>) {
        ctx.teardown_leaf(element);
    }

    fn message(
        &self,
        (): &mut (),
        _: &mut MessageContext,
        _: Mut<'_, Self::Element>,
        _: &mut State,
    ) -> MessageResult<Action> {
        MessageResult::Stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_triangle_stays_inside_its_box() {
        let size = 64.0;
        let bounds = xilem::masonry::kurbo::Shape::bounding_box(&rounded_triangle(size));
        assert!(bounds.x0 >= 0.0 && bounds.y0 >= 0.0, "{bounds:?}");
        assert!(bounds.x1 <= size && bounds.y1 <= size, "{bounds:?}");
    }
}
