use iced::widget::{Space, canvas, image, stack};
use iced::{Color, Element, Length, Point, Rectangle, Size, mouse};
use photos_library::edit::Crop;

use crate::app::{Message, Photos};

const FULL: Crop = Crop {
    x: 0.0,
    y: 0.0,
    width: 1.0,
    height: 1.0,
};

pub(crate) fn view(app: &Photos) -> Element<'_, Message> {
    let Some(viewer) = app.viewer.as_ref() else {
        return Space::new().into();
    };
    let Some(handle) = viewer.original_preview.as_ref() else {
        return Space::new().into();
    };
    let image::Handle::Rgba { width, height, .. } = handle else {
        return Space::new().into();
    };
    stack![
        image(handle.clone())
            .content_fit(iced::ContentFit::Contain)
            .width(Length::Fill)
            .height(Length::Fill),
        canvas(Overlay {
            intrinsic: Size::new(*width as f32, *height as f32),
            tokens: enzui_iced_widgets::image_cropper::ImageCropperTokens::from_palette(
                &app.theme.theme().palette
            ),
            crop: viewer.crop_draft.unwrap_or(FULL),
            aspect: viewer.crop_aspect,
            generation: app.load_generation,
        })
        .width(Length::Fill)
        .height(Length::Fill),
    ]
    .into()
}

struct Overlay {
    intrinsic: Size,
    tokens: enzui_iced_widgets::image_cropper::ImageCropperTokens,
    crop: Crop,
    aspect: Option<f32>,
    generation: u64,
}
#[derive(Default)]
pub(crate) struct State {
    drag: Option<Drag>,
    generation: u64,
}
struct Drag {
    origin: Point,
    crop: Crop,
    edges: u8,
}

impl Overlay {
    fn image_bounds(&self, bounds: Size) -> Rectangle {
        enzui_iced_core::image_space::ImageProjection::new(
            Rectangle::new(Point::ORIGIN, bounds),
            self.intrinsic,
            iced::ContentFit::Contain,
            enzui_iced_core::zoom_pan::ViewTransform::default(),
        )
        .map_or(Rectangle::default(), |projection| {
            projection.image_rect(self.intrinsic)
        })
    }

    fn crop_bounds(&self, image: Rectangle) -> Rectangle {
        Rectangle::new(
            Point::new(
                image.x + self.crop.x * image.width,
                image.y + self.crop.y * image.height,
            ),
            Size::new(
                self.crop.width * image.width,
                self.crop.height * image.height,
            ),
        )
    }
    fn edges(&self, image: Rectangle, point: Point) -> Option<u8> {
        let crop = self.crop_bounds(image);
        if !crop.expand(9.0).contains(point) {
            return None;
        }
        let mut edges = 0;
        if (point.x - crop.x).abs() <= 9.0 {
            edges |= 1;
        }
        if (point.x - (crop.x + crop.width)).abs() <= 9.0 {
            edges |= 2;
        }
        if (point.y - crop.y).abs() <= 9.0 {
            edges |= 4;
        }
        if (point.y - (crop.y + crop.height)).abs() <= 9.0 {
            edges |= 8;
        }
        Some(edges)
    }
}

fn move_crop(crop: Crop, dx: f32, dy: f32, edges: u8) -> Crop {
    if edges == 0 {
        return Crop {
            x: (crop.x + dx).clamp(0.0, 1.0 - crop.width),
            y: (crop.y + dy).clamp(0.0, 1.0 - crop.height),
            ..crop
        };
    }
    let mut left = crop.x;
    let mut right = crop.x + crop.width;
    let mut top = crop.y;
    let mut bottom = crop.y + crop.height;
    if edges & 1 != 0 {
        left = (left + dx).clamp(0.0, right - right.min(0.01));
    }
    if edges & 2 != 0 {
        right = (right + dx).clamp((left + 0.01).min(1.0), 1.0);
    }
    if edges & 4 != 0 {
        top = (top + dy).clamp(0.0, bottom - bottom.min(0.01));
    }
    if edges & 8 != 0 {
        bottom = (bottom + dy).clamp((top + 0.01).min(1.0), 1.0);
    }
    Crop {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

fn locked_resize(original: Crop, proposed: Crop, edges: u8, aspect: f32) -> Crop {
    let horizontal = edges & 3 != 0;
    let vertical = edges & 12 != 0;
    let anchor_x = if edges & 1 != 0 {
        original.x + original.width
    } else if edges & 2 != 0 {
        original.x
    } else {
        original.x + original.width / 2.0
    };
    let anchor_y = if edges & 4 != 0 {
        original.y + original.height
    } else if edges & 8 != 0 {
        original.y
    } else {
        original.y + original.height / 2.0
    };
    let max_width = if edges & 1 != 0 {
        anchor_x
    } else if edges & 2 != 0 {
        1.0 - anchor_x
    } else {
        2.0 * anchor_x.min(1.0 - anchor_x)
    };
    let max_height = if edges & 4 != 0 {
        anchor_y
    } else if edges & 8 != 0 {
        1.0 - anchor_y
    } else {
        2.0 * anchor_y.min(1.0 - anchor_y)
    };
    let use_width = horizontal
        && (!vertical
            || ((proposed.width - original.width) / original.width).abs()
                >= ((proposed.height - original.height) / original.height).abs());
    let width = if use_width {
        proposed.width
    } else {
        proposed.height * aspect
    };
    let width = width.min(max_width).min(max_height * aspect);
    let height = width / aspect;
    Crop {
        x: if edges & 1 != 0 {
            anchor_x - width
        } else if edges & 2 != 0 {
            anchor_x
        } else {
            anchor_x - width / 2.0
        }
        .max(0.0),
        y: if edges & 4 != 0 {
            anchor_y - height
        } else if edges & 8 != 0 {
            anchor_y
        } else {
            anchor_y - height / 2.0
        }
        .max(0.0),
        width,
        height,
    }
}

impl canvas::Program<Message> for Overlay {
    type State = State;
    fn frame_interest(&self, _: &State) -> bool {
        false
    }
    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if state.generation != self.generation {
            state.drag = None;
            state.generation = self.generation;
        }
        let image = self.image_bounds(bounds.size());
        if image.width <= 0.0 || image.height <= 0.0 {
            return None;
        }
        let point = cursor
            .position()
            .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y));
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let point = point.filter(|p| image.contains(*p))?;
                let edges = self.edges(image, point)?;
                state.drag = Some(Drag {
                    origin: point,
                    crop: self.crop,
                    edges,
                });
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let drag = state.drag.as_ref()?;
                let point = point?;
                let crop = move_crop(
                    drag.crop,
                    (point.x - drag.origin.x) / image.width,
                    (point.y - drag.origin.y) / image.height,
                    drag.edges,
                );
                let crop = self
                    .aspect
                    .filter(|_| drag.edges != 0)
                    .map_or(crop, |aspect| {
                        locked_resize(drag.crop, crop, drag.edges, aspect)
                    });
                Some(canvas::Action::publish(Message::CropRegion(crop)).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.drag.take().is_some() =>
            {
                Some(canvas::Action::capture())
            }
            iced::Event::Window(iced::window::Event::Unfocused) => {
                state.drag = None;
                None
            }
            _ => None,
        }
    }
    fn draw(
        &self,
        _: &State,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let image = self.image_bounds(bounds.size());
        let crop = self.crop_bounds(image);
        let shade = self.tokens.scrim;
        for r in [
            Rectangle::new(
                image.position(),
                Size::new(image.width, (crop.y - image.y).max(0.0)),
            ),
            Rectangle::new(
                Point::new(image.x, crop.y + crop.height),
                Size::new(
                    image.width,
                    (image.y + image.height - crop.y - crop.height).max(0.0),
                ),
            ),
            Rectangle::new(
                Point::new(image.x, crop.y),
                Size::new((crop.x - image.x).max(0.0), crop.height),
            ),
            Rectangle::new(
                Point::new(crop.x + crop.width, crop.y),
                Size::new(
                    (image.x + image.width - crop.x - crop.width).max(0.0),
                    crop.height,
                ),
            ),
        ] {
            frame.fill_rectangle(r.position(), r.size(), shade);
        }
        frame.stroke(
            &canvas::Path::rectangle(crop.position(), crop.size()),
            canvas::Stroke::default()
                .with_color(self.tokens.outline)
                .with_width(2.0),
        );
        for part in [1.0 / 3.0, 2.0 / 3.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(crop.x + crop.width * part, crop.y),
                    Point::new(crop.x + crop.width * part, crop.y + crop.height),
                ),
                canvas::Stroke::default().with_color(Color {
                    a: 0.4,
                    ..self.tokens.outline
                }),
            );
            frame.stroke(
                &canvas::Path::line(
                    Point::new(crop.x, crop.y + crop.height * part),
                    Point::new(crop.x + crop.width, crop.y + crop.height * part),
                ),
                canvas::Stroke::default().with_color(Color {
                    a: 0.4,
                    ..self.tokens.outline
                }),
            );
        }
        for x in [crop.x, crop.center_x(), crop.x + crop.width] {
            for y in [crop.y, crop.center_y(), crop.y + crop.height] {
                if x == crop.center_x() && y == crop.center_y() {
                    continue;
                }
                frame.fill_rectangle(
                    Point::new(x - 4.0, y - 4.0),
                    Size::new(8.0, 8.0),
                    self.tokens.handle,
                );
            }
        }
        vec![frame.into_geometry()]
    }
    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let image = self.image_bounds(bounds.size());
        match cursor
            .position_in(bounds)
            .and_then(|p| self.edges(image, p))
        {
            Some(0) => mouse::Interaction::Grab,
            Some(1 | 2) => mouse::Interaction::ResizingHorizontally,
            Some(4 | 8) => mouse::Interaction::ResizingVertically,
            Some(_) => mouse::Interaction::Crosshair,
            None => mouse::Interaction::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::canvas::Program;

    #[test]
    fn crop_overlay_drag_publishes_a_moved_draft_without_confirming() {
        let overlay = Overlay {
            intrinsic: Size::new(100.0, 100.0),
            tokens: enzui_iced_widgets::image_cropper::ImageCropperTokens::from_palette(
                &enzui_iced_theme::Palette::light(),
            ),
            crop: Crop {
                x: 0.1,
                y: 0.2,
                width: 0.5,
                height: 0.6,
            },
            aspect: None,
            generation: 1,
        };
        let mut state = State::default();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(200.0, 100.0));
        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        overlay
            .update(
                &mut state,
                &press,
                bounds,
                mouse::Cursor::Available(Point::new(80.0, 50.0)),
            )
            .expect("drag captured");
        let event = iced::Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(100.0, 60.0),
        });
        let action = overlay
            .update(
                &mut state,
                &event,
                bounds,
                mouse::Cursor::Available(Point::new(100.0, 60.0)),
            )
            .expect("draft change");
        let Some(Message::CropRegion(crop)) = action.into_inner().0 else {
            panic!("only a draft should be emitted")
        };
        assert!(crop.is_valid());
        assert!((crop.x - 0.3).abs() < 0.0001 && (crop.y - 0.3).abs() < 0.0001);
        assert_eq!((crop.width, crop.height), (0.5, 0.6));
        let release = iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let action = overlay
            .update(
                &mut state,
                &release,
                bounds,
                mouse::Cursor::Available(Point::new(100.0, 60.0)),
            )
            .expect("release captured");
        assert!(action.into_inner().0.is_none());
        assert!(state.drag.is_none());
    }

    #[test]
    fn crop_overlay_resize_and_move_stay_inside_the_original() {
        let crop = Crop {
            x: 0.2,
            y: 0.3,
            width: 0.4,
            height: 0.5,
        };
        let moved = move_crop(crop, 2.0, -2.0, 0);
        assert!(moved.is_valid());
        assert_eq!(
            (moved.x, moved.y, moved.width, moved.height),
            (0.6, 0.0, 0.4, 0.5)
        );
        let resized = move_crop(crop, -2.0, -2.0, 1 | 4);
        assert!(resized.is_valid());
        assert_eq!((resized.x, resized.y), (0.0, 0.0));
        assert!(resized.width > crop.width && resized.height > crop.height);
        for edges in [1, 2, 4, 8, 1 | 4, 2 | 8] {
            let proposed = move_crop(crop, 0.9, -0.9, edges);
            let locked = locked_resize(crop, proposed, edges, 0.8);
            assert!(locked.is_valid(), "edges={edges}, crop={locked:?}");
            assert!((locked.width / locked.height - 0.8).abs() < 0.0001);
        }
    }
}
