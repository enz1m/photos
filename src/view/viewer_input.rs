use crate::app::{Message, Photos};
use iced::advanced::{Widget, layout, mouse, overlay, renderer, widget as widget_advanced};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

pub(super) fn surface<'a>(app: &'a Photos, child: Element<'a, Message>) -> Element<'a, Message> {
    let Some(owner) = app.viewer_owner() else {
        return child;
    };
    let image = if app.viewer.as_ref().is_some_and(|v| v.drawing_crop) {
        child
    } else {
        enzui_iced_widgets::context_target::enz_context_target(child, move |at| {
            Message::OpenContext(crate::app::context::Target::Viewer(owner.clone()), at)
        })
        .id(iced::widget::Id::new("photos-image-actions"))
        .label("Photo actions")
        .into_element()
    };
    Element::new(ViewerInput { app, child: image })
}
struct ViewerInput<'a> {
    app: &'a Photos,
    child: Element<'a, Message>,
}
impl Widget<Message, iced::Theme, iced::Renderer> for ViewerInput<'_> {
    fn tag(&self) -> widget_advanced::tree::Tag {
        self.child.as_widget().tag()
    }

    fn state(&self) -> widget_advanced::tree::State {
        self.child.as_widget().state()
    }

    fn diff(&mut self, tree: &mut widget_advanced::Tree) {
        self.child.as_widget_mut().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.child.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget_advanced::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) {
        self.child.as_widget_mut().layout(tree, renderer, limits);
    }

    fn draw(
        &self,
        tree: &widget_advanced::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: layout::Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.child
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &widget_advanced::Tree,
        layout: layout::Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.child
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn operate(
        &mut self,
        tree: &mut widget_advanced::Tree,
        layout: layout::Layout,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
        operation: &mut dyn widget_advanced::Operation,
    ) {
        self.child
            .as_widget_mut()
            .operate(tree, layout, viewport, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget_advanced::Tree,
        layout: layout::Layout,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.child
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation, window)
    }

    fn frame_interest(&self, tree: &widget_advanced::Tree) -> bool {
        self.child.as_widget().frame_interest(tree)
    }

    fn update_frame(
        &mut self,
        tree: &mut widget_advanced::Tree,
        event: &Event,
        layout: layout::Layout,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.child
            .as_widget_mut()
            .update_frame(tree, event, layout, cursor, renderer, shell, viewport);
    }
    fn update(
        &mut self,
        tree: &mut widget_advanced::Tree,
        event: &Event,
        layout: layout::Layout,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let mut focused = Focused(false);
        self.child
            .as_widget_mut()
            .operate(tree, layout, viewport, renderer, &mut focused);
        let browsing = self
            .app
            .viewer
            .as_ref()
            .is_none_or(|v| !v.editing && v.video_editor.is_none());
        if browsing
            && self.app.dialog.is_none()
            && !self.app.context.is_open()
            && !self.app.chooser.is_pending()
            && self.app.chooser_request.is_none()
            && focused.0
            && let Event::Keyboard(event @ iced::keyboard::Event::KeyPressed { .. }) = event
        {
            let bindings: Vec<_> = crate::app::shortcuts::catalog(self.app)
                .into_iter()
                .filter(|b| {
                    matches!(
                        b.id(),
                        "photos-previous"
                            | "photos-next"
                            | "photos-copy-image"
                            | "photos-save-copy"
                    )
                })
                .collect();
            if let Some(command) =
                enzui_iced_core::action::dispatch_shortcut_command(&bindings, event, &())
            {
                shell.publish(command);
                shell.capture_event();
                return;
            }
        }
        self.child
            .as_widget_mut()
            .update(tree, event, layout, cursor, renderer, shell, viewport);
    }
}

struct Focused(bool);
impl widget_advanced::Operation for Focused {
    fn focusable(
        &mut self,
        _id: Option<&iced::widget::Id>,
        _bounds: Rectangle,
        state: &mut dyn widget_advanced::operation::Focusable,
    ) {
        self.0 |= state.is_focused();
    }
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget_advanced::Operation)) {
        operate(self);
    }
}
