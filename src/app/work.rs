use std::sync::Arc;

use iced::Task;
use photos_library::catalog::PhotoId;
use photos_library::edit::EditRecipe;

use super::{Destination, Message, Photos, Viewer};

impl Photos {
    pub(crate) fn refresh_folder_counts(&mut self) {
        self.library.folder_counts.clear();
        for photo in &self.library.photos {
            for folder in photo.path.ancestors().skip(1) {
                *self
                    .library
                    .folder_counts
                    .entry(folder.to_owned())
                    .or_default() += 1;
            }
        }
    }

    pub(crate) fn folder_count(&self, path: &std::path::Path) -> usize {
        let path = if path == self.paths.photos {
            self.canonical_home.as_deref().unwrap_or(path)
        } else {
            path
        };
        self.library.folder_counts.get(path).copied().unwrap_or(0)
    }

    pub(crate) fn refresh_projection(&mut self) {
        self.gallery_generation = self.gallery_generation.wrapping_add(1);
        self.context.close();
        self.library.tile_rows.borrow_mut().clear();
        self.library.recipes.clone_from(&self.catalog.edits);
        let query = self.query.trim().to_lowercase();
        self.library.visible = self
            .library
            .photos
            .iter()
            .enumerate()
            .filter_map(|(index, photo)| {
                if self.catalog.hidden.contains(&photo.id) {
                    return None;
                }
                let matches = match &self.destination {
                    Destination::Library | Destination::Albums => true,
                    Destination::Favorites => self.catalog.favorites.contains(&photo.id),
                    Destination::Imports => self.catalog.imported.contains(&photo.id),
                    Destination::Album(id) => self
                        .catalog
                        .albums
                        .iter()
                        .find(|album| album.id == *id)
                        .is_some_and(|album| album.photos.contains(&photo.id)),
                    Destination::Source(path) => {
                        photo.path.starts_with(if path == &self.paths.photos {
                            self.canonical_home.as_ref().unwrap_or(path)
                        } else {
                            path
                        })
                    }
                };
                let text_matches = query.is_empty()
                    || photo.name.to_lowercase().contains(&query)
                    || photo.path.to_string_lossy().to_lowercase().contains(&query)
                    || photo
                        .camera
                        .as_ref()
                        .is_some_and(|camera| camera.to_lowercase().contains(&query));
                let date_matches = self.period_filter.as_ref().is_none_or(|(start, end, _)| {
                    photo.captured_at >= *start && photo.captured_at < *end
                });
                (matches && text_matches && date_matches).then_some(index)
            })
            .collect();
        self.library.visible.sort_by(|a, b| {
            let a = &self.library.photos[*a];
            let b = &self.library.photos[*b];
            let order = a.captured_at.cmp(&b.captured_at);
            if self.catalog.settings.oldest_first {
                order
            } else {
                order.reverse()
            }
        });
        self.library.groups = photos_library::grouping::groups(
            &self.library.photos,
            &self.library.visible,
            if self.grouping == super::Grouping::Years {
                photos_library::grouping::Period::Year
            } else {
                photos_library::grouping::Period::Month
            },
        );
        self.library.click_model = enzui_iced_core::list_selection::ClickModel::new(
            self.library
                .visible
                .iter()
                .map(|index| self.library.photos[*index].id.clone()),
        );
        let by_id: std::collections::BTreeMap<_, _> = self
            .library
            .photos
            .iter()
            .enumerate()
            .map(|(index, photo)| (&photo.id, index))
            .collect();
        self.library.albums = self
            .catalog
            .albums
            .iter()
            .filter(|album| {
                self.destination != Destination::Albums
                    || query.is_empty()
                    || album.name.to_lowercase().contains(&query)
            })
            .map(|album| super::state::AlbumTile {
                id: album.id,
                name: album.name.clone(),
                count: album.photos.len(),
                cover: album
                    .photos
                    .iter()
                    .filter_map(|id| by_id.get(id).copied())
                    .filter(|index| {
                        !self
                            .catalog
                            .hidden
                            .contains(&self.library.photos[*index].id)
                    })
                    .min(),
            })
            .collect();
    }

    pub(crate) fn scan(&mut self) -> Task<Message> {
        if self.scanning {
            self.scan_requested = true;
            return Task::none();
        }
        self.scanning = true;
        self.scan_requested = false;
        self.scan_generation = self.scan_generation.saturating_add(1);
        let generation = self.scan_generation;
        let home = self.paths.photos.clone();
        let catalog = self.catalog.clone();
        let task = self.workers.submit(generation, move |context| {
            let report = crate::services::discovery(&home, &catalog, || context.is_cancelled());
            (report, std::fs::canonicalize(&home).ok())
        });
        Task::perform(task, move |settled| {
            let (report, canonical_home) = settled.into_result().unwrap_or_else(|error| {
                (
                    photos_library::scan::ScanReport {
                        warnings: vec![error.to_string()],
                        ..Default::default()
                    },
                    None,
                )
            });
            Message::Scanned(generation, Arc::new(report), canonical_home)
        })
    }

    pub(crate) fn refresh_thumbnail_cache(&self) -> Task<Message> {
        let store = self.thumbnail_store.clone();
        let generation = store.generation();
        let task = self.thumbnails.submit(generation, move |_| {
            store.maintain().map_err(|e| e.to_string())
        });
        Task::perform(task, move |settled| {
            Message::ThumbnailCacheUpdated(
                generation,
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            )
        })
    }

    pub(crate) fn schedule_thumbnails(&mut self) -> Task<Message> {
        if self.viewer.is_some() {
            return Task::none();
        }
        let geometry = self.library.geometry.get();
        let columns = if geometry.columns == 0 {
            4
        } else {
            geometry.columns
        };
        let row_height = if geometry.is_resolved() {
            (geometry.tile_height + geometry.gap_y).max(1.0)
        } else {
            224.0
        };
        let first_row = ((self.library.scroll_top - crate::view::gallery_scroll::TOP_INSET)
            .max(0.0)
            / row_height)
            .floor()
            .max(0.0) as usize;
        let first = first_row.saturating_sub(1).saturating_mul(columns);
        let count = ((self.library.viewport_height / row_height).ceil() as usize + 3)
            .saturating_mul(columns);
        let mut tasks = Vec::new();
        let requested: Vec<usize> = if self.destination == Destination::Albums {
            self.library
                .albums
                .iter()
                .skip(first)
                .take(count)
                .filter_map(|album| album.cover)
                .collect()
        } else if self.grouping == super::Grouping::All || self.period_filter.is_some() {
            let rows = self.library.tile_rows.borrow();
            if rows.is_empty() {
                self.library
                    .visible
                    .iter()
                    .skip(first)
                    .take(count)
                    .copied()
                    .collect()
            } else {
                let top = (self.library.scroll_top - row_height).max(0.0);
                let bottom = self.library.scroll_top + self.library.viewport_height + row_height;
                rows.iter()
                    .filter(|(y, _, _)| *y + row_height >= top && *y <= bottom)
                    .flat_map(|(_, start, count)| {
                        self.library.visible[*start..*start + *count]
                            .iter()
                            .copied()
                    })
                    .collect()
            }
        } else {
            self.library
                .groups
                .iter()
                .skip(first)
                .take(count)
                .filter_map(|group| group.indices.first().copied())
                .collect()
        };
        let keys = requested.iter().map(|index| {
            let photo = &self.library.photos[*index];
            crate::services::thumbnail_key(
                photo,
                self.catalog.edits.get(&photo.id),
                crate::services::THUMBNAIL_LIMIT,
            )
        });
        if self.thumbnail_store.protect(keys) {
            tasks.push(self.refresh_thumbnail_cache());
        }
        for index in requested {
            if self.library.inflight.len() >= 8 {
                break;
            }
            let photo = &self.library.photos[index];
            let recipe = self.catalog.edits.get(&photo.id).cloned();
            let key = crate::services::thumbnail_key(
                photo,
                recipe.as_ref(),
                crate::services::THUMBNAIL_LIMIT,
            );
            if self.library.cache.borrow().contains(&key)
                || self.library.inflight.contains(&key)
                || self.library.failed.contains(&key)
            {
                continue;
            }
            self.library.inflight.insert(key.clone());
            let photo = photo.clone();
            let limit = crate::services::THUMBNAIL_LIMIT;
            let store = self.thumbnail_store.clone();
            let generation = store.generation();
            let work = self.thumbnails.submit(generation, move |context| {
                store
                    .thumbnail(&photo, recipe.as_ref(), limit, generation, || {
                        context.is_cancelled()
                    })
                    .map_err(|error| error.to_string())
            });
            tasks.push(Task::perform(work, move |settled| {
                Message::Thumbnail(
                    generation,
                    key.clone(),
                    settled
                        .into_result()
                        .map_err(|e| e.to_string())
                        .and_then(|r| r),
                )
            }));
        }
        Task::batch(tasks)
    }

    pub(crate) fn open_external(&mut self, path: std::path::PathBuf) -> Task<Message> {
        if self.busy || self.dialog.is_some() || self.opening_file {
            self.notice(
                "Finish the current operation before opening another file.",
                true,
            );
            return Task::none();
        }
        self.nav.preference = enzui_iced_widgets::nav_split::SidebarPreference::Hidden;
        self.retire_viewer();
        self.load_generation = self.load_generation.saturating_add(1);
        let generation = self.load_generation;
        self.opening_file = true;
        let work = self.interactive.submit(generation, move |context| {
            (|| -> anyhow::Result<_> {
                anyhow::ensure!(!context.is_cancelled(), "opening cancelled");
                let photo = photos_library::scan::inspect_file(&path)
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                let loaded = crate::services::load_photo(&photo.path, || context.is_cancelled())?;
                Ok((photo, std::sync::Arc::new(loaded)))
            })()
            .map_err(|error| error.to_string())
        });
        Task::perform(work, move |settled| {
            Message::FileOpened(
                generation,
                settled
                    .into_result()
                    .map_err(|error| error.to_string())
                    .and_then(|result| result),
            )
        })
    }

    pub(crate) fn open_photo(&mut self, id: PhotoId) -> Task<Message> {
        let Some((path, generation, opening)) = self.prepare_photo(id) else {
            return Task::none();
        };
        let work = self.interactive.submit(generation, move |context| {
            crate::services::load_photo(&path, || context.is_cancelled())
                .map(Arc::new)
                .map_err(|e| e.to_string())
        });
        if let Some(viewer) = self.viewer.as_mut() {
            viewer.load_cancel = Some(work.cancellation());
        }
        let loading = Task::perform(work, move |settled| {
            Message::Loaded(
                generation,
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            )
        });
        Task::batch([loading, opening])
    }

    pub(crate) fn prepare_photo(
        &mut self,
        id: PhotoId,
    ) -> Option<(std::path::PathBuf, u64, Task<Message>)> {
        let photo = self.library.photos.iter().find(|photo| photo.id == id)?;
        let path = photo.path.clone();
        let recipe = self.catalog.edits.get(&id).cloned().unwrap_or_default();
        let thumbnail_key = crate::services::thumbnail_key(
            photo,
            self.catalog.edits.get(&id),
            crate::services::THUMBNAIL_LIMIT,
        );
        let cached = self.library.cache.borrow_mut().get(&thumbnail_key);
        let opening = cached.map(iced::widget::image::allocate);
        let transition_preview = self
            .viewer
            .as_ref()
            .and_then(|viewer| viewer.preview.clone());
        let transition_allocation = self
            .viewer
            .as_ref()
            .and_then(|viewer| viewer.preview_allocation.clone());
        self.retire_viewer();
        self.load_generation = self.load_generation.saturating_add(1);
        let generation = self.load_generation;
        self.video_edit = Default::default();
        self.viewer = Some(Viewer {
            video_editor: None,
            video_revision: 0,
            stream: None,
            animation: None,
            animation_cancel: None,
            motion_position: 0,
            motion_playing: false,
            motion_request: 0,
            motion_busy: false,
            motion_preview: None,
            motion_allocation: None,
            id,
            loaded: None,
            load_cancel: None,
            preview_cancel: None,
            preview: transition_preview,
            preview_allocation: transition_allocation,
            original_allocation: None,
            before_edit_allocation: None,
            original_preview: None,
            recipe: recipe.clone(),
            before_edit: recipe,
            before_edit_preview: None,
            preview_inflight: None,
            preview_recipe: None,
            preview_inflight_recipe: None,
            editing: false,
            tab: super::EditorTab::Adjust,
            history: Vec::new(),
            redo: Vec::new(),
            transform: enzui_iced_core::zoom_pan::ViewTransform::IDENTITY,
            compare: false,
            drawing_crop: false,
            crop_draft: None,
            crop_aspect: None,
            info: self.catalog.viewer_info,
            preview_generation: 0,
            preview_busy: false,
            preview_dirty: false,
            width_input: String::new(),
            height_input: String::new(),
            lock_aspect: true,
            load_error: None,
        });
        let opening = opening.map_or_else(Task::none, |task| {
            task.map(move |result| {
                Message::OpeningReady(generation, result.map_err(|e| e.to_string()))
            })
        });
        Some((path, generation, opening))
    }

    pub(crate) fn motion_frame(&mut self, index: usize) -> Task<Message> {
        let Some(viewer) = self.viewer.as_mut().filter(|v| !v.editing) else {
            return Task::none();
        };
        let Some(motion) = viewer.motion() else {
            return Task::none();
        };
        let index = index.min(motion.frames.len().saturating_sub(1));
        let handle = motion.frames[index].clone();
        viewer.motion_request = viewer.motion_request.wrapping_add(1);
        viewer.motion_busy = true;
        let request = viewer.motion_request;
        let generation = self.load_generation;
        iced::widget::image::allocate(handle).map(move |result| {
            Message::MotionReady(
                generation,
                request,
                index,
                result.map_err(|error| error.to_string()),
            )
        })
    }

    pub(crate) fn request_preview(&mut self) -> Task<Message> {
        let Some(viewer) = self.viewer.as_mut() else {
            return Task::none();
        };
        if let Some(source) = &viewer.loaded {
            let (width, height) = viewer.recipe.output_dimensions(source.pixels.dimensions());
            viewer.width_input = width.to_string();
            viewer.height_input = height.to_string();
        }
        viewer.preview_dirty = true;
        viewer.preview_generation = viewer.preview_generation.saturating_add(1);
        self.start_preview()
    }

    pub(crate) fn start_preview(&mut self) -> Task<Message> {
        let Some(viewer) = self.viewer.as_mut() else {
            return Task::none();
        };
        if viewer.preview_busy {
            return Task::none();
        }
        let Some(source) = viewer.loaded.clone() else {
            return Task::none();
        };
        viewer.preview_busy = true;
        viewer.preview_dirty = false;
        let generation = viewer.preview_generation;
        viewer.preview_inflight = Some(generation);
        let load_generation = self.load_generation;
        let recipe = viewer.recipe.clone();
        viewer.preview_inflight_recipe = Some(recipe.clone());
        if recipe == EditRecipe::default() {
            return iced::widget::image::allocate(source.original_preview.clone()).map(
                move |result| {
                    Message::PreviewReady(
                        load_generation,
                        generation,
                        result.map_err(|error| error.to_string()),
                    )
                },
            );
        }
        let work = self.interactive.submit(generation, move |_| {
            crate::services::preview(&source.proxy, &recipe).map_err(|e| e.to_string())
        });
        viewer.preview_cancel = Some(work.cancellation());
        Task::perform(work, move |settled| {
            Message::Preview(
                load_generation,
                generation,
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            )
        })
    }

    pub(crate) fn edit_with(&mut self, change: impl FnOnce(&mut EditRecipe)) -> Task<Message> {
        let Some(viewer) = self.viewer.as_mut().filter(|v| v.editing) else {
            return Task::none();
        };
        let before = viewer.recipe.clone();
        change(&mut viewer.recipe);
        if let Err(error) = viewer.recipe.validate() {
            viewer.recipe = before;
            self.notice(error.to_string(), true);
            return Task::none();
        }
        if viewer.recipe != before {
            let geometry_changed = before.quarter_turns != viewer.recipe.quarter_turns
                || before.flip_horizontal != viewer.recipe.flip_horizontal
                || before.crop != viewer.recipe.crop
                || before.resize != viewer.recipe.resize;
            viewer.history.push(before);
            if viewer.history.len() > 100 {
                viewer.history.remove(0);
            }
            viewer.redo.clear();
            if geometry_changed {
                viewer.transform = enzui_iced_core::zoom_pan::ViewTransform::IDENTITY;
            }
        }
        self.request_preview()
    }

    pub(crate) fn changed(&mut self) -> Task<Message> {
        self.revision = self.revision.saturating_add(1);
        self.persist()
    }

    pub(crate) fn reset_gallery_scroll(&mut self) -> Task<Message> {
        self.library.scroll_top = 0.0;
        self.library.scroll.set(iced::Vector::ZERO);
        iced::widget::operation::scroll_to(
            iced::widget::Id::new("photos-grid-scroll"),
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
            iced::advanced::widget::operation::Animation::Instant,
        )
    }

    pub(crate) fn persist(&mut self) -> Task<Message> {
        if self.save_inflight || self.persistence_blocked || self.revision == self.saved_revision {
            return Task::none();
        }
        let revision = self.revision;
        let catalog = self.catalog.clone();
        let Some(authority) = self.catalog_authority.clone() else {
            return Task::none();
        };
        self.save_inflight = true;
        let work = self.workers.submit(revision, move |_| {
            crate::services::persistence::publish(&authority, &catalog).map_err(|e| e.to_string())
        });
        Task::perform(work, move |settled| {
            Message::PersistenceFinished(
                revision,
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            )
        })
    }

    pub(crate) fn notice(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        self.toasts.push(
            enzui_iced_widgets::toast_host::ToastSpec::new(&text)
                // An error stays until it is closed; a success expires.
                .tone(if error {
                    enzui_iced_widgets::toast::ToastTone::Error
                } else {
                    enzui_iced_widgets::toast::ToastTone::Success
                })
                .coalesce_key("photos-status"),
        );
        self.notice = Some((text, error));
    }

    pub(crate) fn finish_file_open(&mut self) -> Task<Message> {
        if !self.opening_file {
            return Task::none();
        }
        self.opening_file = false;
        if self.catalog_authority.is_some() || self.persistence_blocked {
            self.scan()
        } else {
            Task::none()
        }
    }

    pub(crate) fn retire_viewer(&mut self) {
        self.search_open = false;
        self.cancel_chooser();
        self.after_load = None;
        if let Some(viewer) = self.viewer.take() {
            if let Some(editor) = viewer.video_editor
                && let Some(cancel) = editor.thumbnail_cancel
            {
                cancel.cancel();
            }
            if let Some(cancel) = viewer.animation_cancel {
                cancel.cancel();
            }
            if let Some(cancel) = viewer.load_cancel {
                cancel.cancel();
            }
            if let Some(cancel) = viewer.preview_cancel {
                cancel.cancel();
            }
        }
        self.load_generation = self.load_generation.saturating_add(1);
    }
}
