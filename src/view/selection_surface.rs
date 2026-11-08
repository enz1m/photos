use enzui_iced_core::drag::ItemId;
use enzui_iced_core::marquee::{MarqueeBand, MarqueeSession, SelectionMode};
use iced::advanced::layout::Measure;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

use crate::app::{Message, Photos};

#[derive(Default)]
struct State {
    session: Option<MarqueeSession>,
    base: Vec<ItemId>,
    modifiers: iced::keyboard::Modifiers,
    generation: u64,
    last_selected: Option<Vec<photos_library::catalog::PhotoId>>,
}

pub(crate) fn surface<'a>(app: &'a Photos, child: Element<'a, Message>) -> Element<'a, Message> {
    Element::new(SelectionSurface { app, child })
}

struct SelectionSurface<'a> {
    app: &'a Photos,
    child: Element<'a, Message>,
}

impl Widget<Message, iced::Theme, iced::Renderer> for SelectionSurface<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.child));
        let state = tree.state.downcast_mut::<State>();
        if state.generation != self.app.gallery_generation {
            state.session = None;
            state.generation = self.app.gallery_generation;
        }
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) {
        layout::frame(limits, self.size(), |limits| {
            self.child
                .as_widget_mut()
                .measure(&mut tree.children[0], renderer, limits)
        })
        .apply(tree);
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let child = layout.children(&tree.children).next().expect("grid child");
        self.child.as_widget_mut().operate(
            &mut tree.children[0],
            child,
            viewport,
            renderer,
            operation,
        );
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();
        if let Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) = event {
            state.modifiers = *modifiers;
        }
        let mut suppress = false;
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(at) = cursor.position_in(bounds)
                    && at.x < bounds.width - self.app.theme.theme().scrollbar.normalized().hit_width
                {
                    state.session = Some(MarqueeSession::begin(
                        (at.x, at.y + self.app.library.scroll_top),
                        SelectionMode::from_modifiers(
                            state.modifiers.shift(),
                            state.modifiers.control(),
                        ),
                        enzui_iced_core::desktop_settings::input().drag_threshold,
                    ));
                    state.last_selected = None;
                    state.base = self
                        .app
                        .selection
                        .selected_in_order(self.app.library.click_model.order())
                        .into_iter()
                        .map(|id| ItemId::new(id.0))
                        .collect();
                    state.generation = self.app.gallery_generation;
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Window(iced::window::Event::RedrawRequested(_)) => {
                if let (Some(session), Some(at)) = (&mut state.session, cursor.position())
                    && let Some(band) = session.pointer_moved((
                        at.x - bounds.x,
                        at.y - bounds.y + self.app.library.scroll_top,
                    ))
                {
                    let geometry = self.app.library.geometry.get();
                    let rows = self.app.library.tile_rows.borrow();
                    let order = self.app.library.click_model.order();
                    let covered: Vec<_> = rows
                        .iter()
                        .flat_map(|(y, start, count)| {
                            (*start..*start + *count).filter_map(move |index| {
                                order
                                    .get(index)
                                    .filter(|_| {
                                        band.intersects([
                                            (index - start) as f32
                                                * (geometry.tile_width + geometry.gap_x),
                                            *y,
                                            geometry.tile_width,
                                            geometry.tile_height,
                                        ])
                                    })
                                    .map(|id| ItemId::new(&id.0))
                            })
                        })
                        .collect();
                    let selected: Vec<_> = session
                        .mode()
                        .apply(&state.base, &covered)
                        .into_iter()
                        .map(|id| photos_library::catalog::PhotoId(id.0))
                        .collect();
                    if state.last_selected.as_ref() != Some(&selected) {
                        state.last_selected = Some(selected.clone());
                        shell.publish(Message::DragSelection(state.generation, selected));
                    }
                    if matches!(event, Event::Mouse(_)) {
                        shell.request_redraw();
                    }
                    suppress = matches!(event, Event::Mouse(_));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(session) = state.session.take() {
                    suppress = session.is_active();
                }
            }
            Event::Window(iced::window::Event::Unfocused)
            | Event::Mouse(mouse::Event::CursorLeft) => {
                state.session = None;
            }
            Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            }) => {
                state.session = None;
            }
            _ => {}
        }
        let child = layout.children(&tree.children).next().expect("grid child");
        if suppress {
            // A completed band must cancel the tile's retained press before release can open it.
            self.child.as_widget_mut().update(
                &mut tree.children[0],
                &Event::Mouse(mouse::Event::CursorLeft),
                child,
                mouse::Cursor::Unavailable,
                renderer,
                shell,
                viewport,
            );
            if matches!(event, Event::Mouse(mouse::Event::ButtonReleased(_))) {
                self.child.as_widget_mut().update(
                    &mut tree.children[0],
                    event,
                    child,
                    mouse::Cursor::Unavailable,
                    renderer,
                    shell,
                    viewport,
                );
            }
            shell.capture_event();
        } else {
            self.child.as_widget_mut().update(
                &mut tree.children[0],
                event,
                child,
                cursor,
                renderer,
                shell,
                viewport,
            );
        }
        // Tiles and controls own presses they capture; only empty gallery space
        // starts a marquee. Otherwise its first move would cancel native file drag.
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) && shell.is_event_captured()
        {
            tree.state.downcast_mut::<State>().session = None;
        }
        if state_is_dragging(tree)
            && let Some(pointer) = cursor.position()
        {
            let delta = enzui_iced_core::drag_autoscroll::AutoScroll::new(32.0, 480.0).delta(
                pointer,
                bounds,
                std::time::Duration::from_millis(16),
            );
            let geometry = self.app.library.geometry.get();
            let rows = self
                .app
                .library
                .visible
                .len()
                .div_ceil(geometry.columns.max(1));
            let bottom = (super::gallery_scroll::TOP_INSET
                + rows as f32 * (geometry.tile_height + geometry.gap_y)
                - bounds.height)
                .max(0.0);
            if (delta.y > 0.0 && self.app.library.scroll_top < bottom)
                || (delta.y < 0.0 && self.app.library.scroll_top > 0.0)
            {
                shell.request_redraw();
            }
        }
        tree.children[0].refresh_frame_interest(self.child.as_widget());
        tree.refresh_frame_child(0);
    }
    fn frame_interest(&self, tree: &Tree) -> bool {
        tree.state
            .downcast_ref::<State>()
            .session
            .as_ref()
            .is_some_and(MarqueeSession::is_active)
            || tree.children.first().is_some_and(Tree::has_frame_interest)
    }
    fn update_frame(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if tree
            .state
            .downcast_ref::<State>()
            .session
            .as_ref()
            .is_some_and(MarqueeSession::is_active)
        {
            self.update(tree, event, layout, cursor, renderer, shell, viewport);
            return;
        }
        let child = layout.children(&tree.children).next().expect("grid child");
        if tree.children[0].has_frame_interest() {
            self.child.as_widget_mut().update_frame(
                &mut tree.children[0],
                event,
                child,
                cursor,
                renderer,
                shell,
                viewport,
            );
        }
        tree.children[0].refresh_frame_interest(self.child.as_widget());
        tree.refresh_frame_child(0);
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.child.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout.children(&tree.children).next().expect("grid child"),
            cursor,
            viewport,
        );
        if let Some(session) = &tree.state.downcast_ref::<State>().session
            && session.is_active()
        {
            let MarqueeBand {
                x,
                y,
                width,
                height,
            } = session.band();
            let bounds = layout.bounds();
            let band = Rectangle {
                x: bounds.x + x,
                y: bounds.y + y - self.app.library.scroll_top,
                width,
                height,
            };
            let tokens = enzui_iced_widgets::marquee_area::MarqueeTokens::from_palette(
                &self.app.theme.theme().palette,
            );
            if let Some(clip) = viewport.intersection(&bounds) {
                use iced::advanced::Renderer;
                renderer.with_layer(clip, |renderer| {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: band,
                            border: iced::Border {
                                color: tokens.band_border,
                                width: tokens.band_border_width,
                                radius: tokens.band_radius.into(),
                            },
                            ..Default::default()
                        },
                        tokens.band_fill,
                    )
                });
            }
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.child.as_widget().mouse_interaction(
            &tree.children[0],
            layout.children(&tree.children).next().expect("grid child"),
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let child = layout.children(&tree.children).next().expect("grid child");
        self.child.as_widget_mut().overlay(
            &mut tree.children[0],
            child,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

fn state_is_dragging(tree: &Tree) -> bool {
    tree.state
        .downcast_ref::<State>()
        .session
        .as_ref()
        .is_some_and(MarqueeSession::is_active)
}
