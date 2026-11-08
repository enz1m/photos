use enzui_iced_core::localization::text;
use enzui_iced_widgets::button::{ButtonKind, enz_button};
use enzui_iced_widgets::surface::{SurfaceTone, enz_surface};
use enzui_iced_widgets::text::{TextSize, TextTone, enz_text};
use enzui_iced_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{Space, canvas, column, container, row, stack};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Size, mouse};

use crate::app::{Message, Photos, PickPurpose};

pub(crate) fn time(seconds: f32) -> String {
    let milliseconds = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}.{:03}",
        milliseconds / 60_000,
        milliseconds / 1000 % 60,
        milliseconds % 1000
    )
}

pub(crate) fn view(app: &Photos, compact: bool) -> Element<'_, Message> {
    let Some(viewer) = &app.viewer else {
        return Space::new().into();
    };
    let Some(editor) = &viewer.video_editor else {
        return Space::new().into();
    };
    let edit = app.video_edit;
    let status = viewer.stream.as_ref().map(|s| &s.status);
    let position = status.map_or(0.0, |s| s.position.as_secs_f32());
    let playing = status.is_some_and(|s| s.is_playing());
    let transport = row![
        enz_toolbar_icon("step-back", Message::VideoPreviousFrame)
            .id(iced::widget::Id::new("photos-video-previous-frame"))
            .aria_label(text("photos-previous-frame"))
            .enabled(!app.busy && status.is_some_and(|s| s.seekable)),
        enz_toolbar_icon(if playing { "pause" } else { "play" }, Message::Playback)
            .id(iced::widget::Id::new("photos-video-play"))
            .aria_label(text("photos-preview-selected-range"))
            .enabled(status.is_some_and(|s| s.seekable)),
        enz_toolbar_icon("step-forward", Message::VideoStep)
            .id(iced::widget::Id::new("photos-video-next-frame"))
            .aria_label(text("photos-next-frame"))
            .enabled(!app.busy && status.is_some_and(|s| s.seekable)),
        enz_text(time(position))
            .size(TextSize::Footnote)
            .weight_rung(enzui_iced_theme::WeightRung::Semibold),
        enz_toolbar_icon("repeat", Message::VideoLoop)
            .id(iced::widget::Id::new("photos-video-loop"))
            .aria_label(text("photos-loop-selection"))
            .active(editor.loop_range),
    ]
    .spacing(4.0)
    .align_y(Alignment::Center);
    let output = row![
        enz_text(format!(
            "{} selected",
            time((edit.end - edit.start) / edit.speed)
        ))
        .size(TextSize::Footnote)
        .tone(TextTone::Muted),
        enz_button(text("photos-save-frame"), Message::SaveFrame)
            .id(iced::widget::Id::new("photos-video-save-frame"))
            .icon("image-down")
            .kind(ButtonKind::Quiet)
            .size(enzui_iced_widgets::button::ButtonSize::Sm)
            .enabled(!app.busy && status.is_some_and(|s| s.error.is_none() && !s.loading)),
        enz_button(
            if editor.exporting {
                "Exporting…"
            } else {
                "Export copy…"
            },
            Message::Pick(PickPurpose::VideoExport)
        )
        .id(iced::widget::Id::new("photos-video-export"))
        .icon("download")
        .size(enzui_iced_widgets::button::ButtonSize::Sm)
        .enabled(!app.busy),
    ]
    .spacing(10.0)
    .align_y(Alignment::Center);
    let head: Element<'_, Message> = if compact {
        column![transport.wrap(), output.wrap()].spacing(6.0).into()
    } else {
        row![transport, Space::new().width(Length::Fill), output.wrap()]
            .align_y(Alignment::Center)
            .into()
    };
    let trim = row![
        enz_button(
            format!("In  {}", time(edit.start)),
            Message::VideoMark(true)
        )
        .id(iced::widget::Id::new("photos-video-mark-in"))
        .icon("chevron-first")
        .kind(ButtonKind::Quiet)
        .size(enzui_iced_widgets::button::ButtonSize::Sm),
        enz_button(
            format!("Out  {}", time(edit.end)),
            Message::VideoMark(false)
        )
        .id(iced::widget::Id::new("photos-video-mark-out"))
        .icon("chevron-last")
        .kind(ButtonKind::Quiet)
        .size(enzui_iced_widgets::button::ButtonSize::Sm),
    ]
    .spacing(4.0)
    .align_y(Alignment::Center);
    let speeds = [0.5, 1.0, 1.5, 2.0];
    let speed_index = speeds
        .iter()
        .position(|speed| *speed == edit.speed)
        .unwrap_or(1);
    let speed = enzui_iced_widgets::combo_box::enz_combo_box(
        "",
        ["0.5×", "1×", "1.5×", "2×"].map(str::to_owned).to_vec(),
        None,
        |_| Message::VideoSpeed(1.0),
    )
    .selected_index(Some(speed_index))
    .on_select_index(move |index| Message::VideoSpeed(speeds.get(index).copied().unwrap_or(1.0)))
    .id(iced::widget::Id::new("photos-video-speed"))
    .id_prefix("photos-video-speed")
    .aria_label("Playback speed")
    .size(enzui_iced_widgets::text_field::TextFieldSize::Sm)
    .width(80.0)
    .enabled(!app.busy);
    let tools = row![
        enz_toolbar_icon("rotate-cw", Message::VideoRotate)
            .id(iced::widget::Id::new("photos-video-rotate"))
            .aria_label(text("photos-rotate-clockwise")),
        enz_toolbar_icon("flip-horizontal-2", Message::VideoFlip)
            .id(iced::widget::Id::new("photos-video-flip"))
            .aria_label(text("photos-flip-horizontally"))
            .active(edit.flip),
        enz_toolbar_icon(
            if edit.mute { "volume-x" } else { "volume-2" },
            Message::VideoExportMute(!edit.mute)
        )
        .id(iced::widget::Id::new("photos-video-audio"))
        .aria_label(text("photos-keep-audio-in-export"))
        .active(edit.mute)
        .enabled(status.is_some_and(|s| s.has_audio)),
        enz_text(text("photos-speed"))
            .size(TextSize::Footnote)
            .tone(TextTone::Muted),
        speed,
        enz_toolbar_icon("rotate-ccw", Message::VideoReset)
            .id(iced::widget::Id::new("photos-video-reset"))
            .aria_label(text("photos-reset-video-edits")),
    ]
    .spacing(8.0)
    .align_y(Alignment::Center);
    let footer: Element<'_, Message> = if compact {
        column![trim.wrap(), tools.wrap()].spacing(4.0).into()
    } else {
        row![trim, Space::new().width(Length::Fill), tools]
            .align_y(Alignment::Center)
            .into()
    };
    let palette = &app.theme.theme().palette;
    container(
        enz_surface(
            column![
                head,
                stack![
                    canvas(Filmstrip {
                        thumbnails: &editor.thumbnails
                    })
                    .width(Length::Fill)
                    .height(94.0),
                    canvas(Timeline {
                        duration: app.video_duration(),
                        position,
                        start: edit.start,
                        end: edit.end,
                        thumbnails: &editor.thumbnails,
                        generation: app.load_generation,
                        session: editor.session,
                        accent: palette.accent,
                        ink: palette.text,
                        disabled: app.busy || !status.is_some_and(|s| s.seekable)
                    })
                    .width(Length::Fill)
                    .height(94.0),
                ]
                .width(Length::Fill)
                .height(94.0),
                enzui_iced_widgets::slider::enz_slider(
                    0.0..=app.video_duration().max(0.001),
                    position,
                    Message::VideoScrub
                )
                .id(iced::widget::Id::new("photos-video-position"))
                .aria_label("Preview position")
                .value_format(time)
                .step(0.01)
                .enabled(!app.busy && status.is_some_and(|status| status.seekable)),
                footer,
            ]
            .spacing(8.0),
        )
        .tone(SurfaceTone::SurfaceAlt)
        .radius(app.theme.theme().radius.step(app.theme.theme().shape.card))
        .padding(12.0)
        .width(Length::Fill),
    )
    .padding(iced::Padding::ZERO.left(12.0).right(12.0).bottom(12.0))
    .into()
}

struct Timeline<'a> {
    duration: f32,
    position: f32,
    start: f32,
    end: f32,
    thumbnails: &'a [iced::widget::image::Handle],
    generation: u64,
    session: u64,
    accent: Color,
    ink: Color,
    disabled: bool,
}
#[derive(Default)]
pub(crate) struct TimelineState {
    drag: Option<Drag>,
    owner: (u64, u64),
}
#[derive(Clone, Copy)]
enum Drag {
    Start,
    End,
    Playhead,
}

impl Timeline<'_> {
    fn x(&self, seconds: f32, width: f32) -> f32 {
        10.0 + (width - 20.0).max(1.0) * (seconds / self.duration.max(0.001)).clamp(0.0, 1.0)
    }
    fn seconds(&self, x: f32, width: f32) -> f32 {
        ((x - 10.0) / (width - 20.0).max(1.0)).clamp(0.0, 1.0) * self.duration
    }
}

impl canvas::Program<Message> for Timeline<'_> {
    type State = TimelineState;
    fn update(
        &self,
        state: &mut TimelineState,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if state.owner != (self.generation, self.session) {
            state.drag = None;
            state.owner = (self.generation, self.session);
        }
        let point = cursor
            .position()
            .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y));
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if !self.disabled =>
            {
                let point = point.filter(|p| Rectangle::with_size(bounds.size()).contains(*p))?;
                let left = (point.x - self.x(self.start, bounds.width)).abs();
                let right = (point.x - self.x(self.end, bounds.width)).abs();
                let drag = if point.y <= 70.0 && left.min(right) <= 12.0 {
                    if left <= right {
                        Drag::Start
                    } else {
                        Drag::End
                    }
                } else {
                    Drag::Playhead
                };
                state.drag = Some(drag);
                Some(
                    canvas::Action::publish(match drag {
                        Drag::Playhead => Message::VideoScrub(self.seconds(point.x, bounds.width)),
                        _ => Message::VideoGesture(true),
                    })
                    .and_capture(),
                )
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let drag = state.drag?;
                let seconds = self.seconds(point?.x, bounds.width);
                Some(
                    canvas::Action::publish(match drag {
                        Drag::Start => Message::VideoTrimStart(seconds),
                        Drag::End => Message::VideoTrimEnd(seconds),
                        Drag::Playhead => Message::VideoScrub(seconds),
                    })
                    .and_capture(),
                )
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.drag.take().is_some() =>
            {
                Some(canvas::Action::publish(Message::VideoGesture(false)).and_capture())
            }
            iced::Event::Window(iced::window::Event::Unfocused) if state.drag.take().is_some() => {
                Some(canvas::Action::publish(Message::VideoGesture(false)))
            }
            _ => None,
        }
    }
    fn draw(
        &self,
        _: &TimelineState,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let width = (bounds.width - 20.0).max(1.0);
        let y = 12.0;
        let height = 48.0;
        frame.fill_rectangle(
            Point::new(10.0, y),
            Size::new(width, height),
            Color::from_rgba(0.0, 0.0, 0.0, 0.15),
        );
        if self.thumbnails.is_empty() {
            frame.fill_text(canvas::Text {
                content: "Preparing timeline…".into(),
                position: Point::new(bounds.width / 2.0, y + 14.0),
                color: Color { a: 0.6, ..self.ink },
                size: iced::Pixels(12.0),
                align_x: iced::alignment::Horizontal::Center.into(),
                ..Default::default()
            });
        }
        let left = self.x(self.start, bounds.width);
        let right = self.x(self.end, bounds.width);
        let shade = Color::from_rgba(0.0, 0.0, 0.0, 0.62);
        frame.fill_rectangle(
            Point::new(10.0, y),
            Size::new((left - 10.0).max(0.0), height),
            shade,
        );
        frame.fill_rectangle(
            Point::new(right, y),
            Size::new((10.0 + width - right).max(0.0), height),
            shade,
        );
        frame.stroke(
            &canvas::Path::rectangle(
                Point::new(left, y),
                Size::new((right - left).max(1.0), height),
            ),
            canvas::Stroke::default()
                .with_width(2.0)
                .with_color(self.accent),
        );
        for x in [left, right] {
            frame.fill_rectangle(
                Point::new(x - 5.0, y - 3.0),
                Size::new(10.0, height + 6.0),
                self.accent,
            );
            frame.fill_rectangle(
                Point::new(x - 1.0, y + 16.0),
                Size::new(2.0, 16.0),
                Color::WHITE,
            );
        }
        let playhead = self.x(self.position, bounds.width);
        frame.fill_rectangle(
            Point::new(playhead - 1.0, 4.0),
            Size::new(2.0, 62.0),
            self.ink,
        );
        let mut marker = canvas::path::Builder::new();
        marker.move_to(Point::new(playhead - 5.0, 2.0));
        marker.line_to(Point::new(playhead + 5.0, 2.0));
        marker.line_to(Point::new(playhead, 9.0));
        marker.close();
        frame.fill(&marker.build(), self.ink);
        for index in 0..=5 {
            let seconds = self.duration * index as f32 / 5.0;
            let x = self.x(seconds, bounds.width);
            frame.fill_rectangle(
                Point::new(x, 68.0),
                Size::new(1.0, 4.0),
                Color {
                    a: 0.35,
                    ..self.ink
                },
            );
            frame.fill_text(canvas::Text {
                content: time(seconds),
                position: Point::new(x, 76.0),
                color: Color { a: 0.7, ..self.ink },
                size: iced::Pixels(11.0),
                align_x: if index == 0 {
                    iced::alignment::Horizontal::Left.into()
                } else if index == 5 {
                    iced::alignment::Horizontal::Right.into()
                } else {
                    iced::alignment::Horizontal::Center.into()
                },
                ..Default::default()
            });
        }
        vec![frame.into_geometry()]
    }
    fn mouse_interaction(
        &self,
        state: &TimelineState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.disabled {
            mouse::Interaction::default()
        } else if state.drag.is_some() || cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

struct Filmstrip<'a> {
    thumbnails: &'a [iced::widget::image::Handle],
}
impl canvas::Program<Message> for Filmstrip<'_> {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let width = (bounds.width - 20.0).max(1.0);
        let y = 12.0;
        let height = 48.0;
        let count = self.thumbnails.len().max(1);
        let cell = width / count as f32;
        for (index, handle) in self.thumbnails.iter().enumerate() {
            let iced::widget::image::Handle::Rgba {
                width: w,
                height: h,
                ..
            } = handle
            else {
                continue;
            };
            let size = iced::ContentFit::Cover
                .fit(Size::new(*w as f32, *h as f32), Size::new(cell, height));
            frame.with_clip(
                Rectangle::new(
                    Point::new(10.0 + index as f32 * cell, y),
                    Size::new(cell, height),
                ),
                |frame| {
                    frame.draw_image(
                        Rectangle::new(
                            Point::new(
                                10.0 + index as f32 * cell + (cell - size.width) / 2.0,
                                y + (height - size.height) / 2.0,
                            ),
                            size,
                        ),
                        handle,
                    );
                },
            );
        }
        vec![frame.into_geometry()]
    }
}
