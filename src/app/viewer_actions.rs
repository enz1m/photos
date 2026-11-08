use super::{Dialog, Message, Photos, PickPurpose};
use iced::Task;
use photos_library::catalog::PhotoId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Owner {
    Local(u64, PhotoId),
    Remote(u64, String),
}
impl Photos {
    pub(crate) fn viewer_owner(&self) -> Option<Owner> {
        if self.cloud.open || self.cloud.playback_service {
            self.cloud
                .preview
                .clone()
                .map(|id| Owner::Remote(self.cloud.generation, id))
        } else {
            self.viewer
                .as_ref()
                .map(|v| Owner::Local(self.load_generation, v.id.clone()))
        }
    }
    pub(crate) fn viewer_neighbours(&self) -> (bool, bool) {
        if self.cloud.open || self.cloud.playback_service {
            let Some((page, position)) = self.cloud.page.as_ref().and_then(|page| {
                page.assets
                    .iter()
                    .position(|(a, _)| Some(&a.id) == self.cloud.preview.as_ref())
                    .map(|p| (page, p))
            }) else {
                return (false, false);
            };
            (
                position > 0,
                position + 1 < page.assets.len() || page.next.is_some(),
            )
        } else {
            let position = self
                .viewer
                .as_ref()
                .and_then(|v| self.library.click_model.index_of(&v.id));
            (
                position.is_some_and(|p| p > 0),
                position.is_some_and(|p| p + 1 < self.library.visible.len()),
            )
        }
    }
    pub(crate) fn copy_image(&mut self) -> Task<Message> {
        if self.cloud.open {
            return self.cloud_event(super::cloud::Event::Copy);
        }
        let Some(viewer) = &self.viewer else {
            return Task::none();
        };
        if viewer.stream.is_some() {
            return Task::none();
        }
        let Some(source) = viewer.loaded.clone() else {
            return Task::none();
        };
        let generation = self.load_generation;
        let recipe = viewer.recipe.clone();
        let work = self.workers.submit(generation, move |context| {
            anyhow::ensure!(!context.is_cancelled(), "Copy cancelled");
            let pixels = recipe.apply(&source.pixels)?;
            anyhow::ensure!(!context.is_cancelled(), "Copy cancelled");
            Ok(iced::clipboard::Image {
                size: iced::Size::new(pixels.width(), pixels.height()),
                rgba: pixels.into_raw().into(),
            })
        });
        Task::perform(work, move |settled| {
            Message::CopyPrepared(
                generation,
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e: anyhow::Error| e.to_string())),
            )
        })
    }
    pub(crate) fn save_copy(&mut self) -> Task<Message> {
        if self.cloud.open || self.cloud.playback_service {
            let Some(asset) = self
                .cloud
                .page
                .as_ref()
                .and_then(|p| {
                    p.assets
                        .iter()
                        .find(|(a, _)| Some(&a.id) == self.cloud.preview.as_ref())
                })
                .map(|(a, _)| a.clone())
            else {
                return Task::none();
            };
            self.dialog_text = asset.name;
            return self.update(Message::Pick(PickPurpose::RemoteCopy));
        }
        if self
            .viewer
            .as_ref()
            .is_none_or(|v| v.loaded.is_none() || v.stream.is_some())
        {
            return Task::none();
        }
        let Some(photo) = self.selected_photo() else {
            return Task::none();
        };
        let stem = photo.path.file_stem().unwrap_or_default().to_string_lossy();
        self.dialog_text = format!("{stem}-copy.{}", self.export_options.format.extension());
        self.update(Message::Pick(PickPurpose::Export))
    }
}

impl Photos {
    pub(crate) fn viewer_menu(
        &self,
        owner: &Owner,
    ) -> Vec<enzui_iced_widgets::context_menu_host::ContextMenuItem<Message>> {
        use enzui_iced_widgets::context_menu_host::ContextMenuItem as Item;
        let remote = matches!(owner, Owner::Remote(..));
        let video = if remote {
            self.cloud
                .page
                .as_ref()
                .and_then(|p| {
                    p.assets
                        .iter()
                        .find(|(a, _)| Some(&a.id) == self.cloud.preview.as_ref())
                })
                .is_some_and(|(a, _)| a.video)
        } else {
            self.viewer.as_ref().is_some_and(|v| v.stream.is_some())
        };
        let ready = if remote {
            self.cloud.connected() && !self.cloud.busy
        } else {
            self.viewer.as_ref().is_some_and(|v| v.loaded.is_some())
        };
        let item = |label: &str, icon: &str, command: Message, enabled| {
            Item::command(
                Message::ViewerCommand(owner.clone(), Box::new(command)),
                label,
            )
            .icon(icon)
            .enabled(enabled)
        };
        let (previous, next) = self.viewer_neighbours();
        let editing = self
            .viewer
            .as_ref()
            .is_some_and(|v| v.editing || v.video_editor.is_some());
        let mut items = vec![
            item(
                "Previous photo",
                "chevron-left",
                Message::Next(-1),
                previous && !editing && ready,
            ),
            item(
                "Next photo",
                "chevron-right",
                Message::Next(1),
                next && !editing && ready,
            ),
            Item::separator(),
        ];
        if !video {
            items.push(item("Copy image", "copy", Message::CopyImage, ready));
        }
        if !video || remote {
            items.push(item(
                if remote {
                    "Save original copy…"
                } else {
                    "Save copy…"
                },
                "save",
                Message::SaveCopy,
                ready && !self.busy,
            ));
        }
        if remote {
            items.push(item(
                "Download to library",
                "download",
                Message::Cloud(super::cloud::Event::Import),
                ready,
            ));
        } else {
            if video {
                items.push(item("Save frame", "image-down", Message::SaveFrame, ready));
            } else {
                items.push(item(
                    "Export options…",
                    "share",
                    Message::Dialog(Dialog::Export),
                    ready,
                ));
            }
            items.push(Item::separator());
            items.push(item(
                if video { "Edit video" } else { "Edit photo" },
                "sliders-horizontal",
                Message::BeginEdit,
                ready && !editing,
            ));
            items.push(item("Photo information", "info", Message::ToggleInfo, true));
            if let Some(viewer) = &self.viewer {
                items.push(item(
                    if self.catalog.favorites.contains(&viewer.id) {
                        "Remove favorite"
                    } else {
                        "Favorite"
                    },
                    "heart",
                    Message::Favorite(viewer.id.clone()),
                    true,
                ));
            }
            items.push(item(
                "Upload to service…",
                "cloud-upload",
                Message::Cloud(super::cloud::Event::Backup(None)),
                ready,
            ));
            items.push(item(
                "Open containing folder",
                "folder-open",
                Message::OpenContainingFolder,
                true,
            ));
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::{StreamExt, executor::block_on};
    #[test]
    fn copy_renders_the_recipe_off_thread_and_rejects_a_retired_photo() {
        let (mut app, _sandbox) = super::super::tests::loaded();
        let first = app.library.photos[0].id.clone();
        super::super::tests::dispatch(&mut app, Message::Open(first));
        super::super::tests::dispatch(&mut app, Message::BeginEdit);
        super::super::tests::dispatch(&mut app, Message::Rotate);
        let dimensions = app
            .viewer
            .as_ref()
            .expect("viewer")
            .loaded
            .as_ref()
            .expect("pixels")
            .pixels
            .dimensions();
        let task = app.update(Message::CopyImage);
        let mut work = iced_runtime::task::into_stream(task).expect("copy worker");
        let Some(iced_runtime::Action::Output(Message::CopyPrepared(generation, Ok(image)))) =
            block_on(work.next())
        else {
            panic!("copy result")
        };
        assert_eq!(
            (image.size.width, image.size.height),
            (dimensions.1, dimensions.0)
        );
        let stale = image.clone();
        let task = app.update(Message::CopyPrepared(generation, Ok(image)));
        let mut clipboard = iced_runtime::task::into_stream(task).expect("clipboard task");
        let Some(iced_runtime::Action::Clipboard(iced_runtime::clipboard::Action::Write {
            content: iced::clipboard::Content::Image(image),
            channel,
        })) = block_on(clipboard.next())
        else {
            panic!("image clipboard offer")
        };
        assert_eq!(
            (image.size.width, image.size.height),
            (dimensions.1, dimensions.0)
        );
        assert_eq!(
            image.rgba.len(),
            (image.size.width * image.size.height * 4) as usize
        );
        let _ = channel.send(Ok(()));
        super::super::tests::dispatch(&mut app, Message::CancelEdit);
        let id = app.library.photos[1].id.clone();
        super::super::tests::dispatch(&mut app, Message::Open(id));
        assert!(
            iced_runtime::task::into_stream(
                app.update(Message::CopyPrepared(generation, Ok(stale)))
            )
            .is_none()
        );
    }
}
