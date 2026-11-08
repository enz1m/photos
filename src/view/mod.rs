use enzui_iced_core::localization::text;
mod controls;
mod crop_surface;
mod dialogs;
mod editor;
mod gallery;
pub(crate) mod gallery_scroll;
mod media_tile;
mod selection_surface;
mod settings;
mod shell;
mod transfers;
pub(crate) use shell::{SEARCH_TRIGGER_ID, wide_search};
mod video_editor;
mod viewer;
mod viewer_input;

use iced::Element;

use crate::app::{Message, Photos};
mod cloud;

pub(crate) fn view(app: &Photos) -> Element<'_, Message> {
    let shell = app
        .context
        .host(shell::view(app), Message::Context)
        .id_prefix("photos-menu")
        .into_element();
    let host = app.dialogs.view_with(
        shell,
        |dialog| dialogs::view(app, dialog),
        Message::Modal,
        |dialog, host| {
            let host = host.label(text("photos-photos-dialog"));
            // Settings is a sheet against the bottom edge in a small window, as in
            // every app; the centred host would pad it like a dialog.
            match dialog {
                crate::app::Dialog::Settings | crate::app::Dialog::Help
                    if enzui_iced_widgets::settings_dialog::enz_settings_presentation_for(
                        iced::Size::new(app.viewport_width.get(), app.window_height.get()),
                    ) == enzui_iced_widgets::settings_dialog::EnzSettingsPresentation::Sheet =>
                {
                    host.placement(enzui_iced_widgets::modal_host::SurfacePlacement::BottomEdge)
                }
                _ => host,
            }
        },
    );
    let generation = app.chooser_generation;
    let host = app.chooser.view(host, move |event| {
        Message::ChooserFallback(generation, event)
    });
    // Notifications sit in the bottom-right corner, above the gallery's zoom
    // control. The import's progress is a notice (Cancel, and a close that
    // only hides it); status toasts come from the framework host.
    let progress = app
        .import_progress
        .filter(|_| !app.import_progress_hidden)
        .map(|progress| {
            let status = if progress.total == 0 {
                "Finding photos and videos…".to_owned()
            } else {
                format!(
                    "Importing media · {} of {}",
                    progress.completed, progress.total
                )
            };
            enzui_iced_widgets::notice_stack::enz_notice_stack(vec![
                enzui_iced_widgets::notice_stack::EnzNotice::new(
                    "photos-import-progress",
                    status,
                    enzui_iced_widgets::toast::ToastTone::Info,
                )
                .action("Cancel import", Message::CancelImport)
                .on_dismiss(Message::HideImportProgress),
            ])
            .into_element()
        });
    let toasts = (!app.toasts.is_empty()).then(|| app.toasts.view().map(Message::Toast));
    let corner = match (progress, toasts) {
        (None, None) => None,
        (progress, toasts) => Some(
            iced::widget::column(progress.into_iter().chain(toasts))
                .spacing(app.theme.theme().spacing.xs())
                .align_x(iced::Alignment::End)
                .into(),
        ),
    };
    let placement = enzui_iced_widgets::toast_host::ToastOverlayPlacement::default();
    enzui_iced_widgets::toast_host::enz_toast_layer(
        host,
        corner,
        if app.viewer.is_some() {
            placement
        } else {
            placement.bottom_inset(GALLERY_ZOOM_CLEARANCE)
        },
    )
}

/// The height of the gallery's bottom zoom control, which notifications float above.
const GALLERY_ZOOM_CLEARANCE: f32 = 60.0;

pub(super) fn file_target<'a>(
    app: &'a Photos,
    content: Element<'a, Message>,
    album: Option<u64>,
) -> Element<'a, Message> {
    let folder = if album.is_none() {
        match &app.destination {
            crate::app::Destination::Source(path) => Some(path.clone()),
            _ => None,
        }
    } else {
        None
    };
    import_target(app, content, album, folder)
}

pub(super) fn folder_target<'a>(
    app: &'a Photos,
    content: Element<'a, Message>,
    path: std::path::PathBuf,
) -> Element<'a, Message> {
    import_target(app, content, None, Some(path))
}

fn import_target<'a>(
    app: &'a Photos,
    content: Element<'a, Message>,
    album: Option<u64>,
    folder: Option<std::path::PathBuf>,
) -> Element<'a, Message> {
    let enabled = !(app.busy
        || app.close_requested
        || app.persistence_blocked
        || app.catalog_authority.is_none()
        || app.dialog.is_some()
        || app.cloud.open
        || app.viewer.as_ref().is_some_and(|viewer| viewer.editing));
    let key = ("photos-import", album, folder.clone());
    let file_folder = folder.clone();
    let content =
        enzui_iced_widgets::file_drop::enz_file_drop_target_with_receipt(content, move |drop| {
            Message::FilesDropped(album, file_folder.clone(), drop)
        })
        .enabled(enabled)
        .copy_only(true)
        .key(key.clone());
    enzui_iced_widgets::file_drop::enz_data_drop_target(
        content,
        crate::services::import::IMAGE_MIME_TYPES
            .iter()
            .copied()
            .map(enzui_iced_widgets::file_drag::MimeType::new),
        move |drop| Message::ImageDropped(album, folder.clone(), drop),
    )
    .enabled(enabled)
    .key((key, "encoded-image"))
    .into()
}
