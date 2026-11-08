use enzui_iced_core::localization::text;
use std::path::PathBuf;

use enzui_iced_widgets::button::{ButtonKind, ButtonSize, enz_button};
use enzui_iced_widgets::surface::enz_surface;
use enzui_iced_widgets::text::{TextSize, TextTone, enz_text};
use enzui_iced_widgets::text_field::enz_text_field;
use iced::widget::{Column, Space, column, row};
use iced::{Element, Length};

use crate::app::{Dialog, Message, Photos, PickPurpose};

pub(crate) fn view<'a>(app: &'a Photos, dialog: &Dialog) -> Element<'a, Message> {
    let (title, explanation) = match dialog {
        Dialog::VideoEdit => return Space::new().into(),
        Dialog::Help | Dialog::Settings => return super::settings::view(app),
        Dialog::NewAlbum => (
            text("photos-new-album"),
            text("photos-albums-organize-your-photos-without-moving-or"),
        ),
        Dialog::RenameAlbum(_) => (
            text("photos-rename-album"),
            text("photos-your-photos-and-album-membership-stay-unchang"),
        ),
        Dialog::AddToAlbum => (
            text("photos-add-to-album"),
            text("photos-choose-a-collection-for-the-selected-photos"),
        ),
        Dialog::Import => (
            text("photos-import"),
            text("photos-copy-photos-and-videos-using-your-destination"),
        ),
        Dialog::Sources => (
            text("photos-sources"),
            text("photos-photos-reads-these-folders-in-place-adding-a-"),
        ),
        Dialog::Export
            if app
                .selected_photo()
                .is_some_and(|photo| crate::services::formats::rendered_copy(&photo.path)) =>
        {
            (
                text("photos-export-a-copy"),
                text("photos-save-a-flattened-image-the-original-layers-or"),
            )
        }
        Dialog::Export => (
            text("photos-export-a-copy"),
            text("photos-save-your-edits-as-a-new-image"),
        ),
        Dialog::CleanMetadata => (
            text("photos-create-a-clean-copy"),
            text("photos-remove-exif-gps-camera-and-text-metadata-by-r"),
        ),
        Dialog::DeleteAlbum(_) => (
            text("photos-remove-album"),
            text("photos-only-the-album-is-removed-your-photos-stay-in"),
        ),
        Dialog::CloseEdits => (
            text("photos-save-edits-before-closing"),
            text("photos-the-original-is-unchanged-save-the-reversible"),
        ),
        Dialog::LeaveEdits => (
            text("photos-save-edits-before-leaving"),
            text("photos-save-the-reversible-edits-discard-this-editin"),
        ),
    };
    let mut content = column![
        enz_text(title).size(TextSize::Title3),
        enz_text(explanation)
            .size(TextSize::Footnote)
            .tone(TextTone::Muted)
    ]
    .spacing(12.0);
    match dialog {
        Dialog::NewAlbum | Dialog::RenameAlbum(_) => {
            let submit = if let Dialog::RenameAlbum(id) = dialog {
                Message::RenameAlbum(*id)
            } else {
                Message::CreateAlbum
            };
            content = content.push(
                enz_text_field(
                    "Album name",
                    "e.g. Summer in the mountains",
                    &app.dialog_text,
                    Message::DialogText,
                )
                .id(iced::advanced::widget::Id::new("photos-album-name"))
                .on_submit(submit),
            );
        }
        Dialog::AddToAlbum => {
            let mut choices = Column::new().spacing(8.0);
            for album in &app.catalog.albums {
                choices = choices.push(
                    enz_button(&album.name, Message::AddToAlbum(album.id))
                        .id(iced::advanced::widget::Id::from(format!(
                            "photos-choose-album-{}",
                            album.id
                        )))
                        .icon("album")
                        .kind(ButtonKind::Quiet)
                        .size(ButtonSize::Sm),
                );
            }
            if app.catalog.albums.is_empty() {
                choices = choices.push(
                    enz_text(text("photos-create-an-album-first-from-the-sidebar"))
                        .tone(TextTone::Muted),
                );
            }
            content = content.push(
                enzui_iced_widgets::scroll_view::enz_scroll_view(choices)
                    .height(Length::Fixed(220.0)),
            );
        }
        Dialog::Sources => {
            content =
                content.push(enz_text(app.paths.photos.to_string_lossy()).size(TextSize::Footnote));
            let mut sources = Column::new().spacing(8.0);
            for source in &app.catalog.sources {
                sources = sources.push(
                    row![
                        enz_text(source.to_string_lossy())
                            .size(TextSize::Footnote)
                            .width(Length::Fill),
                        enz_button(text("photos-remove"), Message::RemoveSource(source.clone()))
                            .id(iced::advanced::widget::Id::from(format!(
                                "photos-remove-source-{}",
                                photos_library::catalog::PhotoId::for_path(source).0
                            )))
                            .kind(ButtonKind::Quiet)
                            .size(ButtonSize::Sm)
                    ]
                    .spacing(8.0),
                );
            }
            if !app.catalog.sources.is_empty() {
                content = content.push(
                    enzui_iced_widgets::scroll_view::enz_scroll_view(sources)
                        .height(Length::Fixed(160.0)),
                );
            }
            content = content.push(path_field(app)).push(
                enz_button(
                    text("photos-choose-folder"),
                    Message::Pick(PickPurpose::Source),
                )
                .id(iced::advanced::widget::Id::new("photos-choose-source"))
                .icon("folder-open")
                .kind(ButtonKind::Quiet)
                .size(ButtonSize::Sm),
            );
        }
        Dialog::Import | Dialog::Export | Dialog::CleanMetadata => {
            let purpose = match dialog {
                Dialog::Import => PickPurpose::Import,
                Dialog::CleanMetadata => PickPurpose::Clean,
                _ => PickPurpose::Export,
            };
            if matches!(dialog, Dialog::Export) {
                let format = app.export_options.format;
                content = content.push(
                    enzui_iced_widgets::view_switcher::enz_view_switcher(
                        vec![
                            enzui_iced_widgets::view_switcher::SwitcherPage::new("png", "PNG"),
                            enzui_iced_widgets::view_switcher::SwitcherPage::new("jpeg", "JPEG"),
                            enzui_iced_widgets::view_switcher::SwitcherPage::new("webp", "WebP"),
                        ],
                        match format {
                            crate::services::ExportFormat::Png => 0,
                            crate::services::ExportFormat::Jpeg => 1,
                            crate::services::ExportFormat::Webp => 2,
                        },
                        |index| {
                            Message::ExportFormat(match index {
                                1 => crate::services::ExportFormat::Jpeg,
                                2 => crate::services::ExportFormat::Webp,
                                _ => crate::services::ExportFormat::Png,
                            })
                        },
                    )
                    .tokens(export_switcher_tokens())
                    .aria_label(text("photos-export-file-type")),
                );
                let resolution = app.viewer.as_ref().and_then(|viewer| {
                    viewer.loaded.as_ref().map(|source| {
                        crate::services::scaled_dimensions(
                            viewer.recipe.output_dimensions(source.pixels.dimensions()),
                            app.export_options.scale_percent,
                        )
                    })
                });
                let mut size_label =
                    row![enz_text(text("photos-image-size")).size(TextSize::Footnote)]
                        .spacing(8.0)
                        .align_y(iced::Alignment::Center);
                if let Some((width, height)) = resolution {
                    size_label = size_label.push(
                        enz_text(format!("{width} × {height} px"))
                            .size(TextSize::Footnote)
                            .weight(iced::font::Weight::Semibold),
                    );
                }
                content = content
                    .push(
                        size_label.push(Space::new().width(Length::Fill)).push(
                            enz_text(format!("{}%", app.export_options.scale_percent))
                                .size(TextSize::Footnote)
                                .tone(TextTone::Muted),
                        ),
                    )
                    .push(
                        enzui_iced_widgets::slider::enz_slider(
                            1.0..=100.0,
                            f32::from(app.export_options.scale_percent),
                            Message::ExportScale,
                        )
                        .aria_label(&text("photos-image-size"))
                        .show_value(false)
                        .value_format(|value| format!("{value:.0}%"))
                        .track_height(4.0)
                        .thumb_width(14.0)
                        .thumb_height(14.0),
                    );
                content = content.push(
                    enzui_iced_widgets::view_switcher::enz_view_switcher(
                        vec![
                            enzui_iced_widgets::view_switcher::SwitcherPage::new(
                                "clear",
                                "Clear metadata",
                            ),
                            enzui_iced_widgets::view_switcher::SwitcherPage::new(
                                "keep",
                                "Keep metadata",
                            ),
                        ],
                        usize::from(app.export_options.keep_metadata),
                        |index| Message::ExportMetadata(index == 1),
                    )
                    .tokens(export_switcher_tokens())
                    .aria_label(text("photos-export-metadata")),
                );
                if format == crate::services::ExportFormat::Jpeg {
                    content = content.push(
                        enzui_iced_widgets::slider::enz_slider(
                            1.0..=100.0,
                            f32::from(app.export_options.quality),
                            Message::ExportQuality,
                        )
                        .label(&text("photos-jpeg-quality"))
                        .show_value(true),
                    );
                } else {
                    content = content.push(
                        enz_text(text("photos-lossless-encoding"))
                            .size(TextSize::Footnote)
                            .tone(TextTone::Muted),
                    );
                }
            }
            content = content.push(path_field(app)).push(
                enz_button(text("photos-browse"), Message::Pick(purpose))
                    .id(iced::advanced::widget::Id::new("photos-dialog-browse"))
                    .icon("folder-open")
                    .kind(ButtonKind::Quiet)
                    .size(ButtonSize::Sm),
            );
        }
        Dialog::Help
        | Dialog::DeleteAlbum(_)
        | Dialog::Settings
        | Dialog::VideoEdit
        | Dialog::CloseEdits
        | Dialog::LeaveEdits => {}
    }

    if let Some((notice, true)) = &app.notice {
        content = content.push(
            enz_text(notice)
                .tone(TextTone::Muted)
                .size(TextSize::Footnote),
        );
    }
    let confirm = match dialog {
        Dialog::NewAlbum => Some(("Create album", Message::CreateAlbum)),
        Dialog::RenameAlbum(id) => Some(("Rename album", Message::RenameAlbum(*id))),
        Dialog::Import => Some((
            "Import photos",
            Message::FolderChosen(PickPurpose::Import, PathBuf::from(&app.dialog_text)),
        )),
        Dialog::Sources => Some((
            "Add folder",
            Message::FolderChosen(PickPurpose::Source, PathBuf::from(&app.dialog_text)),
        )),
        Dialog::Export => Some((
            "Export copy",
            Message::FolderChosen(PickPurpose::Export, PathBuf::from(&app.dialog_text)),
        )),
        Dialog::CleanMetadata => Some((
            "Save clean copy",
            Message::FolderChosen(PickPurpose::Clean, PathBuf::from(&app.dialog_text)),
        )),
        Dialog::DeleteAlbum(id) => Some(("Remove album", Message::DeleteAlbum(*id))),
        Dialog::Help | Dialog::AddToAlbum | Dialog::Settings | Dialog::VideoEdit => None,
        Dialog::CloseEdits => Some(("Save", Message::SaveAndClose)),
        Dialog::LeaveEdits => Some(("Save", Message::SaveAndContinue)),
    };
    let mut buttons = row![
        Space::new().width(Length::Fill),
        enz_button(text("photos-cancel"), Message::DismissDialog)
            .id(iced::advanced::widget::Id::new("photos-dialog-cancel"))
            .kind(ButtonKind::Quiet)
            .size(ButtonSize::Sm)
    ]
    .spacing(8.0)
    .mirrored(
        enzui_iced_core::placement::resolve_direction(None)
            == enzui_iced_core::placement::Direction::RightToLeft,
    );
    let discard = match dialog {
        Dialog::CloseEdits => Some(Message::DiscardAndClose),
        Dialog::LeaveEdits => Some(Message::DiscardAndContinue),
        _ => None,
    };
    if let Some(message) = discard {
        buttons = buttons.push(
            enz_button(text("photos-discard"), message)
                .id(iced::widget::Id::new("photos-dialog-discard"))
                .kind(ButtonKind::Destructive)
                .size(ButtonSize::Sm),
        );
    }
    if let Some((label, message)) = confirm {
        buttons = buttons.push(
            enz_button(
                if matches!(dialog, Dialog::CloseEdits | Dialog::LeaveEdits) {
                    text("photos-save")
                } else {
                    label.into()
                },
                message,
            )
            .kind(if matches!(dialog, Dialog::DeleteAlbum(_)) {
                ButtonKind::Destructive
            } else {
                ButtonKind::Accent
            })
            .id(iced::advanced::widget::Id::new("photos-dialog-confirm"))
            .size(ButtonSize::Sm)
            .enabled(
                !app.busy
                    && (matches!(
                        dialog,
                        Dialog::DeleteAlbum(_) | Dialog::CloseEdits | Dialog::LeaveEdits
                    ) || !app.dialog_text.trim().is_empty()),
            ),
        );
    }
    content = content.push(buttons);
    enzui_iced_widgets::clamp::enz_clamp(
        enz_surface(content)
            .padding(20.0)
            .radius(
                app.theme
                    .theme()
                    .radius
                    .step(app.theme.theme().shape.dialog),
            )
            .width(Length::Fill),
        enzui_iced_widgets::clamp::ClampSize::Wide,
    )
    .max_width(440.0)
    .into_element()
}

fn path_field(app: &Photos) -> Element<'_, Message> {
    enz_text_field(
        "Local path",
        "/home/you/Pictures",
        &app.dialog_text,
        Message::DialogText,
    )
    .id(iced::advanced::widget::Id::new("photos-dialog-path"))
    .into()
}

fn export_switcher_tokens() -> enzui_iced_widgets::view_switcher::SwitcherTokens {
    enzui_iced_widgets::view_switcher::SwitcherTokens {
        label_size: 12.0,
        tab_min_width: 64.0,
        auto_floor: 0.0,
        ..enzui_iced_widgets::view_switcher::SwitcherTokens::PILL_STRIP
            .for_size(enzui_iced_widgets::text_field::TextFieldSize::Sm)
    }
}
