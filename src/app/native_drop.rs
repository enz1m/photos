use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use enzui_iced_widgets::file_drop::{DataDrop, DragAction, FileDrop};
use iced::Task;

use super::{Message, Photos};
use crate::services::import::ImportReport;

#[derive(Debug, Clone)]
pub(crate) enum Receipt {
    File(FileDrop),
    Image(DataDrop),
}

impl Receipt {
    fn finish(self) {
        match self {
            Self::File(receipt) => receipt.finish(),
            Self::Image(receipt) => receipt.finish(),
        }
    }
}

pub(super) struct Pending {
    revision: u64,
    receipt: Receipt,
}

impl Photos {
    pub(super) fn files_dropped(
        &mut self,
        album: Option<u64>,
        folder: Option<std::path::PathBuf>,
        receipt: FileDrop,
    ) -> Task<Message> {
        if self.busy
            || self.close_requested
            || self.catalog_authority.is_none()
            || self.persistence_blocked
            || self.dialog.is_some()
            || self.cloud.open
            || self.viewer.as_ref().is_some_and(|viewer| viewer.editing)
            || receipt.paths().is_empty()
            || receipt.paths().len() > 128
            || receipt
                .action()
                .is_some_and(|action| action != DragAction::Copy)
            || album.is_some_and(|id| !self.catalog.albums.iter().any(|album| album.id == id))
            || folder.as_ref().is_some_and(|folder| {
                folder != &self.paths.photos && !self.catalog.sources.contains(folder)
            })
        {
            return Task::none();
        }
        let known: Vec<_> = receipt
            .paths()
            .iter()
            .filter_map(|path| {
                self.library
                    .photos
                    .iter()
                    .find(|photo| {
                        photo.path == *path
                            && folder
                                .as_ref()
                                .is_none_or(|folder| path.parent() == Some(folder.as_path()))
                    })
                    .map(|photo| photo.id.clone())
            })
            .collect();
        if known.len() == receipt.paths().len() {
            let Some(album_id) = album else {
                receipt.finish();
                return Task::none();
            };
            if let Some(album) = self
                .catalog
                .albums
                .iter_mut()
                .find(|album| album.id == album_id)
            {
                album.photos.extend(known);
            }
            self.refresh_projection();
            let task = self.changed();
            self.retain_native_drop(Receipt::File(receipt));
            return task;
        }
        self.busy = true;
        self.import_generation = self.import_generation.saturating_add(1);
        let generation = self.import_generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.import_cancel = Some(Arc::clone(&cancel));
        self.import_progress_hidden = false;
        self.import_progress = Some(crate::services::import::ImportProgress {
            completed: 0,
            total: 0,
        });
        let destination = folder
            .clone()
            .unwrap_or_else(|| self.import_destination().to_owned());
        let layout = self.catalog.settings.import_layout;
        let (known_paths, paths): (Vec<_>, Vec<_>) =
            receipt.paths().iter().cloned().partition(|path| {
                self.library.photos.iter().any(|photo| photo.path == *path)
                    && folder
                        .as_ref()
                        .is_none_or(|folder| path.parent() == Some(folder.as_path()))
            });
        let (progress, receiver, _) =
            enzui_iced_core::worker_bridge::channel(std::num::NonZeroUsize::MIN);
        let work = self.workers.submit(generation, move |context| {
            crate::services::import::from_paths_organized(
                &paths,
                &destination,
                layout,
                || context.is_cancelled() || cancel.load(Ordering::Acquire),
                |update| {
                    let _ = progress.send_progress(update);
                },
            )
            .map(|mut report| {
                report.paths.extend(known_paths);
                Arc::new(report)
            })
            .map_err(|error| error.to_string())
        });
        Task::batch([
            Task::run(receiver, move |update| {
                Message::ImportProgress(generation, update)
            }),
            Task::perform(work, move |settled| {
                Message::DroppedImportFinished(
                    generation,
                    album,
                    Receipt::File(receipt.clone()),
                    settled
                        .into_result()
                        .map_err(|error| error.to_string())
                        .and_then(|result| result),
                )
            }),
        ])
    }

    pub(super) fn image_dropped(
        &mut self,
        album: Option<u64>,
        folder: Option<std::path::PathBuf>,
        receipt: DataDrop,
    ) -> Task<Message> {
        if self.busy
            || self.close_requested
            || self.catalog_authority.is_none()
            || self.persistence_blocked
            || self.dialog.is_some()
            || self.cloud.open
            || self.viewer.as_ref().is_some_and(|viewer| viewer.editing)
            || receipt
                .action()
                .is_some_and(|action| action != DragAction::Copy)
            || receipt.data().bytes().is_empty()
            || receipt.data().bytes().len() > crate::services::import::ENCODED_IMAGE_LIMIT
            || album.is_some_and(|id| !self.catalog.albums.iter().any(|album| album.id == id))
            || folder.as_ref().is_some_and(|folder| {
                folder != &self.paths.photos && !self.catalog.sources.contains(folder)
            })
        {
            return Task::none();
        }
        self.busy = true;
        self.import_generation = self.import_generation.saturating_add(1);
        let generation = self.import_generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.import_cancel = Some(Arc::clone(&cancel));
        self.import_progress_hidden = false;
        self.import_progress = Some(crate::services::import::ImportProgress {
            completed: 0,
            total: 1,
        });
        let destination = folder.unwrap_or_else(|| self.import_destination().to_owned());
        let layout = self.catalog.settings.import_layout;
        let data = receipt.data().clone();
        let (progress, receiver, _) =
            enzui_iced_core::worker_bridge::channel(std::num::NonZeroUsize::MIN);
        let work = self.workers.submit(generation, move |context| {
            crate::services::import::from_encoded_image(
                data.bytes(),
                data.mime().as_str(),
                &destination,
                layout,
                || context.is_cancelled() || cancel.load(Ordering::Acquire),
                |update| {
                    let _ = progress.send_progress(update);
                },
            )
            .map(Arc::new)
            .map_err(|error| error.to_string())
        });
        Task::batch([
            Task::run(receiver, move |update| {
                Message::ImportProgress(generation, update)
            }),
            Task::perform(work, move |settled| {
                Message::DroppedImportFinished(
                    generation,
                    album,
                    Receipt::Image(receipt.clone()),
                    settled
                        .into_result()
                        .map_err(|error| error.to_string())
                        .and_then(|result| result),
                )
            }),
        ])
    }

    pub(super) fn dropped_import_finished(
        &mut self,
        generation: u64,
        album: Option<u64>,
        receipt: Receipt,
        report: Result<Arc<ImportReport>, String>,
    ) -> Task<Message> {
        if generation != self.import_generation {
            return Task::none();
        }
        let succeeded = report.as_ref().is_ok_and(|report| {
            !report.paths.is_empty()
                && report.skipped == 0
                && report.errors.is_empty()
                && !report.cancelled
        });
        let mut album_exists = album.is_none();
        if let Some(id) = album
            && let Some(album) = self.catalog.albums.iter_mut().find(|album| album.id == id)
        {
            album_exists = true;
            if let Ok(report) = &report {
                album
                    .photos
                    .extend(crate::services::imported_ids(&report.paths));
            }
        }
        let destination = self.destination.clone();
        let task = self.update(Message::ImportFinished(generation, report));
        self.destination = destination;
        self.refresh_projection();
        if succeeded && album_exists {
            self.retain_native_drop(receipt);
        }
        task
    }

    fn retain_native_drop(&mut self, receipt: Receipt) {
        self.native_drops.push(Pending {
            revision: self.revision,
            receipt,
        });
    }

    pub(super) fn settle_native_drops(&mut self, revision: u64, success: bool) {
        let mut waiting = Vec::new();
        for pending in self.native_drops.drain(..) {
            if pending.revision > revision {
                waiting.push(pending);
            } else if success {
                pending.receipt.finish();
            }
        }
        self.native_drops = waiting;
    }
}
