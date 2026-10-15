//! Escape dismisses the dialog, wherever focus is inside it.

use xilem::core::{MessageContext, MessageResult, Mut, View, ViewId, ViewMarker, ViewPathTracker};
use xilem::masonry::accesskit::{Node, Role};
use xilem::masonry::core::keyboard::{Key, KeyState, NamedKey};
use xilem::masonry::core::{
    AccessCtx, BoxConstraints, ChildrenIds, EventCtx, LayoutCtx, PaintCtx, PropertiesMut,
    PropertiesRef, RegisterCtx, TextEvent, Widget, WidgetMut, WidgetPod,
};
use xilem::masonry::kurbo::{Point, Size};
use xilem::masonry::vello::Scene;
use xilem::{Pod, ViewCtx, WidgetView};

const CONTENT_VIEW_ID: ViewId = ViewId::new(0);

#[derive(Debug, Clone, Copy)]
pub struct Dismissed;

/// The window's root, so xilem makes it the focus fallback: Escape reaches it
/// before anything is focused, and bubbles up to it from a focused button.
pub struct EscapeScope {
    child: WidgetPod<dyn Widget>,
}

impl EscapeScope {
    fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.child)
    }
}

impl Widget for EscapeScope {
    type Action = Dismissed;

    fn on_text_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &TextEvent,
    ) {
        if let TextEvent::Keyboard(key) = event
            && key.state == KeyState::Down
            && key.key == Key::Named(NamedKey::Escape)
        {
            ctx.submit_action::<Self::Action>(Dismissed);
            ctx.set_handled();
        }
    }

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.child);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        bc: &BoxConstraints,
    ) -> Size {
        let size = ctx.run_layout(&mut self.child, bc);
        ctx.place_child(&mut self.child, Point::ORIGIN);
        size
    }

    fn paint(&mut self, _ctx: &mut PaintCtx<'_>, _props: &PropertiesRef<'_>, _scene: &mut Scene) {}

    fn accessibility_role(&self) -> Role {
        Role::AlertDialog
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        _node: &mut Node,
    ) {
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.child.id()])
    }
}

pub fn dismiss_on_escape<V, F, State, Action>(content: V, on_dismiss: F) -> EscapeView<V, F>
where
    V: WidgetView<State, Action>,
    F: Fn(&mut State) -> Action + Send + Sync + 'static,
{
    EscapeView {
        content,
        on_dismiss,
    }
}

#[must_use = "View values do nothing unless provided to Xilem."]
pub struct EscapeView<V, F> {
    content: V,
    on_dismiss: F,
}

impl<V, F> ViewMarker for EscapeView<V, F> {}
impl<V, F, State, Action> View<State, Action, ViewCtx> for EscapeView<V, F>
where
    V: WidgetView<State, Action>,
    F: Fn(&mut State) -> Action + Send + Sync + 'static,
    State: 'static,
    Action: 'static,
{
    type Element = Pod<EscapeScope>;
    type ViewState = V::ViewState;

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State) -> (Self::Element, Self::ViewState) {
        let (child, child_state) =
            ctx.with_id(CONTENT_VIEW_ID, |ctx| self.content.build(ctx, app_state));
        let scope = EscapeScope {
            child: child.new_widget.erased().to_pod(),
        };
        (
            ctx.with_action_widget(|ctx| ctx.create_pod(scope)),
            child_state,
        )
    }

    fn rebuild(
        &self,
        prev: &Self,
        view_state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) {
        ctx.with_id(CONTENT_VIEW_ID, |ctx| {
            let mut child = EscapeScope::child_mut(&mut element);
            self.content
                .rebuild(&prev.content, view_state, ctx, child.downcast(), app_state);
        });
    }

    fn teardown(
        &self,
        view_state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
    ) {
        ctx.with_id(CONTENT_VIEW_ID, |ctx| {
            let mut child = EscapeScope::child_mut(&mut element);
            self.content.teardown(view_state, ctx, child.downcast());
        });
        ctx.teardown_leaf(element);
    }

    fn message(
        &self,
        view_state: &mut Self::ViewState,
        message: &mut MessageContext,
        mut element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) -> MessageResult<Action> {
        match message.take_first() {
            Some(CONTENT_VIEW_ID) => {
                let mut child = EscapeScope::child_mut(&mut element);
                self.content
                    .message(view_state, message, child.downcast(), app_state)
            }
            None => match message.take_message::<Dismissed>() {
                Some(_) => MessageResult::Action((self.on_dismiss)(app_state)),
                None => MessageResult::Stale,
            },
            Some(_) => MessageResult::Stale,
        }
    }
}
