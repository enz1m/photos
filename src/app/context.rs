use enzui_iced_core::context_menu::{InvocationKind, Point};
use enzui_iced_core::localization::text;
use enzui_iced_widgets::context_menu_host::{
    ContextMenu, ContextMenuEvent, ContextMenuItem, FocusRestoreTarget,
};
use iced::Task;
use photos_library::catalog::PhotoId;

use super::{Destination, Dialog, Message, Photos};

#[derive(Debug, Clone)]
pub(crate) enum Target {
    Photo(PhotoId),
    Viewer(super::viewer_actions::Owner),
    Sidebar(Destination),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PhotoAction {
    Edit,
    Info,
    Export,
    CleanCopy,
}

impl PhotoAction {
    pub fn message(self) -> Message {
        match self {
            Self::Edit => Message::BeginEdit,
            Self::Info => Message::ToggleInfo,
            Self::Export => Message::Dialog(Dialog::Export),
            Self::CleanCopy => Message::Dialog(Dialog::CleanMetadata),
        }
    }
}

pub(crate) type Context = ContextMenu<Target, Message>;

impl Photos {
    pub(crate) fn open_context(&mut self, target: Target, at: iced::Point) {
        if let Target::Viewer(owner) = &target {
            if self.viewer_owner().as_ref() != Some(owner) {
                return;
            }
            let items = self.viewer_menu(owner);
            let id = if matches!(owner, super::viewer_actions::Owner::Remote(..)) && self.cloud.open
            {
                "photos-service-viewer"
            } else {
                "photos-viewer"
            };
            self.context.open_restoring(
                target,
                InvocationKind::Pointer(Point { x: at.x, y: at.y }),
                items,
                FocusRestoreTarget::widget(iced::widget::Id::new(id)),
            );
            return;
        }
        let invoker = match &target {
            Target::Viewer(_) => unreachable!(),
            Target::Photo(id) => format!("photos-tile-{}", id.0),
            Target::Sidebar(destination) => match destination {
                Destination::Library => "photos-nav-library".into(),
                Destination::Favorites => "photos-nav-favorites".into(),
                Destination::Imports => "photos-nav-imports".into(),
                Destination::Albums => "photos-nav-albums".into(),
                Destination::Album(id) => format!("photos-nav-album-{id}"),
                Destination::Source(path) if path == &self.paths.photos => "photos-nav-home".into(),
                Destination::Source(path) => {
                    format!("photos-nav-source-{}", PhotoId::for_path(path).0)
                }
            },
        };
        let invoker = iced::widget::Id::from(invoker);
        let mut actions = Vec::new();
        match target.clone() {
            Target::Viewer(_) => unreachable!(),
            Target::Photo(id) => {
                self.selection.enter_selection_mode();
                if !self.selection.contains(&id) {
                    self.selection
                        .replace([id.clone()], Some(id.clone()), Some(id.clone()));
                }
                let multiple = self
                    .selection
                    .selected_in_order(self.library.click_model.order())
                    .len()
                    > 1;
                if !multiple {
                    actions.push(("photos-open-photo", Message::Open(id.clone())));
                    actions.push((
                        "photos-edit-photo",
                        Message::OpenPhotoAction(id.clone(), PhotoAction::Edit),
                    ));
                    actions.push((
                        "photos-photo-information",
                        Message::OpenPhotoAction(id.clone(), PhotoAction::Info),
                    ));
                    actions.push((
                        "photos-export-photo",
                        Message::OpenPhotoAction(id.clone(), PhotoAction::Export),
                    ));
                    actions.push((
                        "photos-clean-copy",
                        Message::OpenPhotoAction(id.clone(), PhotoAction::CleanCopy),
                    ));
                }
                actions.push((
                    "Upload to service…",
                    Message::Cloud(super::cloud::Event::Backup(None)),
                ));
                actions.push(if multiple {
                    (
                        "photos-favorite-selected-photos",
                        Message::FavoriteSelection,
                    )
                } else {
                    (
                        if self.catalog.favorites.contains(&id) {
                            "photos-remove-favorite"
                        } else {
                            "photos-favorite"
                        },
                        Message::Favorite(id),
                    )
                });
                actions.push(("photos-add-to-album", Message::Dialog(Dialog::AddToAlbum)));
                if matches!(self.destination, Destination::Album(_)) {
                    actions.push(("photos-remove-from-album", Message::RemoveFromAlbum));
                }
                actions.push(("photos-select-all", Message::SelectAll));
                actions.push(("photos-clear-selection", Message::SelectionMode(false)));
            }
            Target::Sidebar(destination) => {
                actions.push(("photos-open", Message::Navigate(destination.clone())));
                match destination {
                    Destination::Album(id) => {
                        actions.push((
                            "Upload to service…",
                            Message::Cloud(super::cloud::Event::Backup(Some(id))),
                        ));
                        actions.push((
                            "photos-rename-album",
                            Message::Dialog(Dialog::RenameAlbum(id)),
                        ));
                        actions.push((
                            "photos-remove-album-d9b8ab",
                            Message::Dialog(Dialog::DeleteAlbum(id)),
                        ));
                    }
                    Destination::Albums => {
                        actions.push(("photos-new-album", Message::Dialog(Dialog::NewAlbum)))
                    }
                    Destination::Source(path) => {
                        actions.push(("photos-rescan", Message::Refresh));
                        if self.catalog.sources.contains(&path) {
                            actions.push(("photos-remove-folder", Message::RemoveSource(path)));
                        }
                        actions.push(("photos-add-folder", Message::Dialog(Dialog::Sources)));
                    }
                    _ => actions.push(("photos-rescan", Message::Refresh)),
                }
            }
        }
        let mut section = None;
        let mut items = Vec::new();
        for (label, message) in actions {
            let group = match label {
                "photos-open-photo"
                | "photos-open"
                | "photos-edit-photo"
                | "photos-photo-information" => 0,
                "photos-export-photo" | "photos-clean-copy" | "Upload to service…" => 1,
                "photos-favorite"
                | "photos-remove-favorite"
                | "photos-favorite-selected-photos"
                | "photos-add-to-album"
                | "photos-remove-from-album" => 2,
                "photos-select-all" | "photos-clear-selection" => 3,
                _ => 4,
            };
            if section.is_some_and(|previous| previous != group) {
                items.push(ContextMenuItem::separator());
            }
            section = Some(group);
            let message = match message {
                Message::Dialog(dialog) => Message::DialogFrom(dialog, invoker.clone()),
                message => message,
            };
            items.push(
                ContextMenuItem::command(
                    message,
                    if label == "Upload to service…" {
                        label.into()
                    } else {
                        text(label)
                    },
                )
                .icon(match label {
                    "Upload to service…" => "cloud-upload",
                    "photos-open-photo" | "photos-open" => "image",
                    "photos-edit-photo" | "photos-rename-album" => "pencil",
                    "photos-photo-information" => "info",
                    "photos-export-photo" => "share",
                    "photos-clean-copy" => "shield-check",
                    "photos-favorite"
                    | "photos-remove-favorite"
                    | "photos-favorite-selected-photos" => "heart",
                    "photos-add-to-album" | "photos-new-album" => "album",
                    "photos-remove-from-album"
                    | "photos-remove-album-d9b8ab"
                    | "photos-remove-folder" => "minus",
                    "photos-select-all" => "check-check",
                    "photos-clear-selection" => "square",
                    "photos-add-folder" => "folder-plus",
                    "photos-rescan" => "refresh-cw",
                    _ => "circle",
                }),
            );
        }
        self.context.open_restoring(
            target,
            InvocationKind::Pointer(Point { x: at.x, y: at.y }),
            items,
            FocusRestoreTarget::widget(invoker),
        );
    }

    pub(crate) fn context_event(&mut self, event: ContextMenuEvent) -> Task<Message> {
        let (chosen, focus) = self.context.update(&event);
        let command = chosen.map_or_else(Task::none, |(_, message)| self.update(message));
        Task::batch([focus, command])
    }
}
