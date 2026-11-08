use enzui_iced_core::localization::text;
use enzui_iced_widgets::button::{ButtonKind, ButtonSize, enz_button};
use enzui_iced_widgets::image_viewer::enz_image_viewer;
use enzui_iced_widgets::text::{TextSize, TextTone, enz_text};
use enzui_iced_widgets::title_bar::enz_title_bar;
use enzui_iced_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Length};

use crate::app::{Dialog, Message, Photos};

pub(crate) fn titlebar(app: &Photos) -> Element<'_, Message> {
    let Some(viewer) = app.viewer.as_ref() else {
        return Space::new().into();
    };
    let mut bar = enz_title_bar();
    if let Some(editor) = &viewer.video_editor {
        bar = bar.leading(super::controls::glass_chrome(
            row![
                enz_button(text("photos-cancel"), Message::VideoClose(true))
                    .id(iced::widget::Id::new("photos-video-cancel"))
                    .kind(ButtonKind::Quiet)
                    .size(enzui_iced_widgets::button::ButtonSize::Sm),
                enz_toolbar_icon("undo-2", Message::VideoUndo(true))
                    .id(iced::widget::Id::new("photos-video-undo"))
                    .aria_label(text("photos-undo-video-edit"))
                    .enabled(!editor.history.is_empty()),
                enz_toolbar_icon("redo-2", Message::VideoUndo(false))
                    .id(iced::widget::Id::new("photos-video-redo"))
                    .aria_label(text("photos-redo-video-edit"))
                    .enabled(!editor.redo.is_empty()),
            ]
            .align_y(Alignment::Center),
        ));
    } else if viewer.editing {
        let leading = row![
            enz_button(text("photos-cancel"), Message::CancelEdit)
                .id(iced::advanced::widget::Id::new("photos-edit-cancel"))
                .kind(ButtonKind::Quiet)
                .size(enzui_iced_widgets::button::ButtonSize::Sm),
            enz_toolbar_icon("undo-2", Message::Undo)
                .id(iced::advanced::widget::Id::new("photos-edit-undo"))
                .aria_label(text("photos-undo-edit"))
                .enabled(!viewer.history.is_empty()),
            enz_toolbar_icon("redo-2", Message::Redo)
                .id(iced::advanced::widget::Id::new("photos-edit-redo"))
                .aria_label(text("photos-redo-edit"))
                .enabled(!viewer.redo.is_empty()),
        ]
        .align_y(Alignment::Center);
        bar = bar.leading(super::controls::glass_chrome(leading));
    } else {
        let (previous, next) = app.viewer_neighbours();
        let navigation = enzui_iced_widgets::titlebar_icon_group::enz_titlebar_icon_group(vec![
            enz_toolbar_icon("arrow-left", Message::Back)
                .id(iced::advanced::widget::Id::new("photos-back"))
                .aria_label(text("photos-back-to-library"))
                .into_element(),
            enz_toolbar_icon("chevron-left", Message::Next(-1))
                .id(iced::advanced::widget::Id::new("photos-previous"))
                .aria_label(text("photos-previous-photo"))
                .enabled(previous)
                .into_element(),
            enz_toolbar_icon("chevron-right", Message::Next(1))
                .id(iced::advanced::widget::Id::new("photos-next"))
                .aria_label(text("photos-next-photo"))
                .enabled(next)
                .into_element(),
        ]);
        bar = bar.leading(super::controls::glass_chrome(navigation));
    }
    bar.id("photos-window").into_element()
}

pub(super) fn actions(
    app: &Photos,
) -> Vec<enzui_iced_widgets::action_bar::ActionItem<'_, Message>> {
    let Some(viewer) = app.viewer.as_ref() else {
        return Vec::new();
    };
    if viewer.video_editor.is_some() {
        return vec![super::controls::icon_action(
            "photos-viewer-actions",
            "photos-video-done",
            "check",
            "Done",
            Message::VideoClose(false),
            true,
        )];
    }
    if viewer.editing {
        return vec![
            super::controls::icon_action(
                "photos-viewer-actions",
                "photos-edit-compare",
                "eye",
                "Original",
                Message::Compare(!viewer.compare),
                true,
            ),
            super::controls::icon_action(
                "photos-viewer-actions",
                "photos-edit-done",
                "check",
                text("photos-done"),
                Message::SaveEdit,
                !viewer.drawing_crop,
            ),
        ];
    }
    let video = app
        .selected_photo()
        .is_some_and(|photo| photos_library::scan::is_video_path(&photo.path));
    [
        (
            "photos-viewer-backup",
            "cloud-upload",
            "Upload to service…".into(),
            Message::Cloud(crate::app::cloud::Event::Backup(None)),
            true,
        ),
        (
            "photos-favorite",
            if app.catalog.favorites.contains(&viewer.id) {
                "heart.fill"
            } else {
                "heart"
            },
            text("photos-favorite"),
            Message::Favorite(viewer.id.clone()),
            true,
        ),
        (
            "photos-info",
            "info",
            text("photos-photo-information"),
            Message::ToggleInfo,
            true,
        ),
        (
            "photos-add-to-album",
            "album",
            text("photos-add-to-album"),
            super::shell::action_dialog(
                app,
                "photos-viewer-actions",
                "photos-add-to-album",
                Dialog::AddToAlbum,
            ),
            true,
        ),
        (
            "photos-export",
            "download",
            if video {
                text("photos-export-video")
            } else {
                text("photos-export-photo")
            },
            super::shell::action_dialog(
                app,
                "photos-viewer-actions",
                "photos-export",
                Dialog::Export,
            ),
            viewer.loaded.is_some(),
        ),
        (
            "photos-edit",
            "sliders-horizontal",
            if video {
                text("photos-edit-video")
            } else {
                text("photos-edit-photo")
            },
            Message::BeginEdit,
            viewer.loaded.is_some(),
        ),
    ]
    .into_iter()
    .map(|(id, icon, label, message, enabled)| {
        super::controls::icon_action("photos-viewer-actions", id, icon, label, message, enabled)
    })
    .collect()
}

pub(crate) fn view<'a>(
    app: &'a Photos,
    geometry: &enzui_iced_widgets::app_shell::ShellGeometry,
) -> Element<'a, Message> {
    let width = geometry.content_width;
    let Some(viewer) = app.viewer.as_ref() else {
        return Space::new().into();
    };
    let streamed = viewer
        .stream
        .as_ref()
        .filter(|stream| stream.show_frame)
        .and_then(|stream| stream.presenter.handle());
    let picture = if !viewer.editing && streamed.is_some() {
        streamed
    } else if !viewer.editing && viewer.motion_preview.is_some() {
        viewer.motion_preview.as_ref()
    } else if viewer.compare {
        viewer.original_preview.as_ref().or(viewer.preview.as_ref())
    } else {
        viewer.preview.as_ref()
    };
    let image: Element<'_, Message> = if viewer.drawing_crop {
        crop_surface(app)
    } else if let Some(handle) = picture {
        let mut tokens = enzui_iced_widgets::image_viewer::ImageViewerTokens::from_palette(
            &app.theme.theme().palette,
        );
        tokens.radius = geometry
            .content_corner_radii
            .into_iter()
            .fold(0.0_f32, f32::max);
        enz_image_viewer(handle.clone(), viewer.transform)
            .tokens(tokens)
            .width(Length::Fill)
            .height(Length::Fill)
            .on_transform(Message::Transform)
            .alt_text(
                app.selected_photo()
                    .map_or("Photo", |photo| photo.name.as_str()),
            )
            .id(iced::advanced::widget::Id::new("photos-viewer"))
            .into()
    } else if let Some(error) = &viewer.load_error {
        container(
            column![
                enz_text(text("photos-this-media-could-not-be-opened")).size(TextSize::Title3),
                enz_text(error).tone(TextTone::Muted),
                enz_button(text("photos-try-again"), Message::Open(viewer.id.clone()))
                    .id(iced::advanced::widget::Id::new("photos-viewer-retry"))
                    .kind(ButtonKind::Quiet)
                    .size(ButtonSize::Sm),
                enz_button(text("photos-back-to-library"), Message::Back)
                    .id(iced::advanced::widget::Id::new("photos-viewer-error-back"))
                    .kind(ButtonKind::Quiet)
                    .size(ButtonSize::Sm)
            ]
            .spacing(16.0)
            .align_x(Alignment::Center),
        )
        .padding(24.0)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    } else {
        container(enz_text(text("photos-opening-c926c2")).tone(TextTone::Muted))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    };
    let image = super::viewer_input::surface(app, image);
    let controls: Element<'_, Message> = if viewer.editing || viewer.video_editor.is_some() {
        Space::new().into()
    } else if let Some(stream) = &viewer.stream {
        let snapshot = stream.status.snapshot(None);
        use enzui_iced_widgets::media_controls::{TimeDisplay, enz_media_controls};
        let generation = app.load_generation;
        let seekable = snapshot.capabilities.seekable;
        let ready = !app.busy
            && snapshot.error.is_none()
            && viewer
                .stream
                .as_ref()
                .is_none_or(|stream| !stream.status.loading);
        let transport: Element<'_, enzui_iced_widgets::media_controls::MediaControlsMessage> =
            enz_media_controls("photos-preview", snapshot)
                .time_display(TimeDisplay::Elapsed)
                .into();
        let extras = row![
            enz_toolbar_icon("step-forward", Message::VideoStep)
                .id(iced::widget::Id::new("photos-preview-step"))
                .aria_label(text("photos-next-frame"))
                .enabled(viewer.stream.is_some() && seekable),
            enz_button(text("photos-save-frame"), Message::SaveFrame)
                .id(iced::widget::Id::new("photos-preview-save-frame"))
                .icon("image-down")
                .enabled(ready)
                .kind(ButtonKind::Quiet)
                .size(ButtonSize::Sm),
        ]
        .spacing(8.0)
        .align_y(Alignment::Center);
        container(
            column![
                transport.map(move |intent| Message::PreviewControl(generation, intent)),
                extras,
            ]
            .spacing(4.0)
            .align_x(Alignment::Center),
        )
        .padding(12.0)
        .align_bottom(Length::Fill)
        .width(Length::Fill)
        .into()
    } else if let Some(motion) = viewer.motion() {
        let generation = app.load_generation;
        let controls = column![
            enzui_iced_widgets::slider::enz_slider(
                0.0..=motion.frames.len().saturating_sub(1) as f32,
                viewer.motion_position as f32,
                move |frame| Message::PreviewControl(
                    generation,
                    enzui_iced_widgets::media_controls::MediaControlsMessage::Seek(
                        std::time::Duration::from_secs_f32(
                            frame / crate::services::formats::MOTION_FPS
                        )
                    )
                ),
            )
            .id(iced::widget::Id::new("photos-motion-position"))
            .aria_label("Motion frame")
            .step(1.0)
            .value_format(move |frame| format!(
                "{:.1} / {:.1} s",
                frame / crate::services::formats::MOTION_FPS,
                motion.frames.len().saturating_sub(1) as f32 / crate::services::formats::MOTION_FPS
            )),
            row![
                enz_toolbar_icon(
                    if viewer.motion_playing {
                        "pause"
                    } else {
                        "play"
                    },
                    Message::PreviewControl(
                        generation,
                        enzui_iced_widgets::media_controls::MediaControlsMessage::PlayPause
                    )
                )
                .id(iced::widget::Id::new("photos-motion-play"))
                .aria_label(text("photos-play-or-pause-motion")),
                enz_button(text("photos-save-frame"), Message::SaveFrame)
                    .id(iced::widget::Id::new("photos-preview-save-frame"))
                    .icon("image-down")
                    .enabled(!app.busy)
                    .kind(ButtonKind::Quiet)
                    .size(ButtonSize::Sm),
            ]
            .spacing(8.0)
            .wrap(),
        ]
        .spacing(4.0)
        .align_x(Alignment::Center);
        container(controls)
            .padding(12.0)
            .align_bottom(Length::Fill)
            .width(Length::Fill)
            .into()
    } else {
        Space::new().into()
    };
    let picture = iced::widget::stack![image, controls]
        .height(Length::Fill)
        .width(Length::Fill);
    if viewer.video_editor.is_some() {
        column![picture, super::video_editor::view(app, width < 800.0)]
            .height(Length::Fill)
            .into()
    } else if viewer.editing {
        if width < 720.0 {
            column![picture, super::editor::view(app, true)].into()
        } else {
            row![
                picture,
                container(super::editor::view(app, false))
                    .padding(iced::Padding::ZERO.top(super::gallery_scroll::TOP_INSET))
            ]
            .into()
        }
    } else if viewer.info {
        if width < 720.0 {
            column![picture, info(app, true)].into()
        } else {
            row![
                picture,
                container(info(app, false))
                    .padding(iced::Padding::ZERO.top(super::gallery_scroll::TOP_INSET))
            ]
            .into()
        }
    } else {
        picture.into()
    }
}

fn crop_surface(app: &Photos) -> Element<'_, Message> {
    super::crop_surface::view(app)
}

fn info(app: &Photos, compact: bool) -> Element<'_, Message> {
    let mut content =
        column![enz_text(text("photos-information")).size(TextSize::Title3)].spacing(14.0);
    if let Some(photo) = app.selected_photo() {
        content = content
            .push(
                row![
                    enz_text(&photo.name).width(Length::Fill),
                    enz_toolbar_icon("folder-open", Message::OpenContainingFolder)
                        .id(iced::advanced::widget::Id::new("photos-open-folder"))
                        .aria_label(text("photos-open-containing-folder"))
                ]
                .align_y(Alignment::Center),
            )
            .push(
                enz_text({
                    let dimensions = app
                        .viewer
                        .as_ref()
                        .and_then(|v| v.loaded.as_ref())
                        .map(|loaded| {
                            loaded.video_info.as_ref().map_or_else(
                                || loaded.pixels.dimensions(),
                                |info| (info.width, info.height),
                            )
                        })
                        .unwrap_or((photo.width, photo.height));
                    if dimensions == (0, 0) {
                        "Dimensions available after opening".to_owned()
                    } else {
                        format!("{} × {} pixels", dimensions.0, dimensions.1)
                    }
                })
                .tone(TextTone::Muted),
            )
            .push(
                enz_text(format!(
                    "{:.1} MB · {}",
                    photo.bytes as f64 / 1_000_000.0,
                    photo.format.to_uppercase()
                ))
                .tone(TextTone::Muted),
            )
            .push(
                enz_text(photo.path.to_string_lossy())
                    .size(TextSize::Footnote)
                    .tone(TextTone::Muted),
            );
    }
    if let Some(photo) = app
        .selected_photo()
        .filter(|photo| crate::services::formats::rendered_copy(&photo.path))
    {
        content = content.push(enz_text(if photos_library::scan::is_camera_raw(&photo.path) {
            "RAW developed with camera white balance. Edits export a new rendered copy."
        } else { "Merged Photoshop image. Layers stay in the original; edits export a flattened copy." }).size(TextSize::Footnote).tone(TextTone::Muted));
    }
    if let Some(loaded) = app.viewer.as_ref().and_then(|v| v.loaded.as_ref()) {
        if let Some(video) = &loaded.video_info {
            content = content.push(
                enz_text(format!("Duration · {}", media_time(video.duration)))
                    .size(TextSize::Footnote)
                    .tone(TextTone::Muted),
            );
        }
        if let Some(warning) = &loaded.motion_warning {
            content = content.push(
                enz_text(warning)
                    .size(TextSize::Footnote)
                    .tone(TextTone::Muted),
            );
        }
        if loaded.video_info.is_none() {
            content = content.push(
                enz_text(if loaded.metadata.has_location {
                    "Location metadata is present"
                } else if !loaded.metadata.has_exif {
                    "No readable EXIF metadata. A clean copy removes embedded data."
                } else {
                    "No GPS coordinates found"
                })
                .tone(TextTone::Muted),
            );
        }
        for (name, value) in &loaded.metadata.fields {
            content = content.push(
                column![
                    enz_text(name)
                        .size(TextSize::Footnote)
                        .tone(TextTone::Muted),
                    enz_text(value).size(TextSize::Footnote)
                ]
                .spacing(4.0),
            );
        }
    }
    if matches!(app.destination, crate::app::Destination::Album(_)) {
        content = content.push(
            enz_button(text("photos-remove-from-album"), Message::RemoveFromAlbum)
                .id(iced::advanced::widget::Id::new("photos-remove-from-album"))
                .kind(ButtonKind::Quiet)
                .size(ButtonSize::Sm),
        );
    }
    let panel = enzui_iced_widgets::scroll_view::enz_scroll_view(container(content).padding(20.0))
        .into_element();
    container(column![
        panel,
        container(
            enz_button(
                if app
                    .selected_photo()
                    .is_some_and(|p| photos_library::scan::is_video_path(&p.path))
                {
                    "Export video copy"
                } else {
                    "Create clean copy"
                },
                Message::Dialog(Dialog::CleanMetadata)
            )
            .id(iced::advanced::widget::Id::new("photos-clean-copy"))
            .icon("shield-check")
            .kind(ButtonKind::Quiet)
            .size(ButtonSize::Sm)
        )
        .padding([12.0, 20.0])
        .align_right(Length::Fill)
    ])
    .width(if compact {
        Length::Fill
    } else {
        Length::Fixed(292.0)
    })
    .height(if compact {
        Length::Fixed(240.0)
    } else {
        Length::Fill
    })
    .into()
}

fn media_time(seconds: f32) -> String {
    let seconds = seconds.max(0.0) as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}
