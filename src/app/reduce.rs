use std::path::PathBuf;
use std::sync::Arc;

use enzui_iced_core::modal::DismissCause;
use enzui_iced_widgets::file_chooser::{ChooserOptions, PortalOperation, PortalOutcome};
use iced::Task;
use photos_library::edit::{Crop, EditRecipe};

use super::{Destination, Dialog, Message, Photos, PickPurpose};

impl Photos {
    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        if let Some(dialog) = &self.dialog {
            self.dialogs.set_policy(
                dialog,
                if self.busy {
                    enzui_iced_widgets::modal_host::alert_dismiss_policy()
                } else {
                    enzui_iced_widgets::modal_host::modal_dismiss_policy()
                },
            );
        }
        if self.catalog_authority.is_none()
            && matches!(
                message,
                Message::ToggleInfo
                    | Message::Shortcut(_)
                    | Message::GallerySections(_)
                    | Message::OldestFirst(_)
                    | Message::ContainThumbnails(_)
                    | Message::ThumbnailCacheLimit(_)
                    | Message::ShowNames(_)
                    | Message::ImportLayout(_)
                    | Message::ImportDestination(_)
                    | Message::FilesDropped(..)
                    | Message::ImageDropped(..)
                    | Message::Favorite(_)
                    | Message::FavoriteSelection
                    | Message::CreateAlbum
                    | Message::RenameAlbum(_)
                    | Message::AddToAlbum(_)
                    | Message::RemoveFromAlbum
                    | Message::DeleteAlbum(_)
                    | Message::RemoveSource(_)
                    | Message::SaveEdit
                    | Message::SaveAndClose
                    | Message::SaveAndContinue
                    | Message::FolderChosen(
                        PickPurpose::Import | PickPurpose::Source | PickPurpose::ImportDestination,
                        _
                    )
                    | Message::Pick(PickPurpose::Import | PickPurpose::Source)
            )
        {
            self.notice(if self.persistence_blocked { "The catalog is unavailable. Library changes are disabled to protect your saved choices." } else { "The catalog is still loading. Try again in a moment." }, true);
            return Task::none();
        }
        if matches!(
            message,
            Message::OpenExternal(_)
                | Message::Navigate(_)
                | Message::Open(_)
                | Message::OpenPhotoAction(_, _)
                | Message::Back
                | Message::Dialog(Dialog::Import)
                | Message::DialogFrom(Dialog::Import, _)
                | Message::Cloud(super::cloud::Event::Open(_) | super::cloud::Event::Connect)
        ) && self
            .viewer
            .as_ref()
            .is_some_and(|viewer| viewer.editing && viewer.recipe != viewer.before_edit)
        {
            self.pending_navigation = Some(Box::new(message));
            return self.open_dialog(Dialog::LeaveEdits);
        }
        if self.cloud.busy
            && matches!(
                message,
                Message::Navigate(_) | Message::Open(_) | Message::OpenExternal(_)
            )
        {
            self.notice(
                "Wait for the service request, or cancel it before leaving.",
                false,
            );
            return Task::none();
        }
        match message {
            Message::FilesDropped(album, folder, drop) => {
                return self.files_dropped(album, folder, drop);
            }
            Message::ImageDropped(album, folder, drop) => {
                return self.image_dropped(album, folder, drop);
            }
            Message::DroppedImportFinished(generation, album, drop, report) => {
                return self.dropped_import_finished(generation, album, drop, report);
            }
            Message::Cloud(event) => return self.cloud_event(event),
            Message::InstanceWake => {
                let (intents, raise) = self.instance.drain(None);
                let mut tasks = vec![raise];
                for intent in intents {
                    if let enzui_iced_core::activation::ApplicationIntent::Open(uris) = intent {
                        if uris.len() != 1 {
                            self.notice("Open one local media file at a time.", true);
                            continue;
                        }
                        match enzui_iced_core::portal::uri::file_uri_to_path(&uris[0]) {
                            Ok(path) => tasks.push(self.update(Message::OpenExternal(path))),
                            Err(error) => self.notice(error.to_string(), true),
                        }
                    }
                }
                return Task::batch(tasks);
            }
            Message::OpenExternal(path) => {
                self.cloud.playback_service = false;
                return self.open_external(path);
            }
            Message::OpenContext(target, at) => self.open_context(target, at),
            Message::Context(event) => return self.context_event(event),
            Message::Theme(event) => return self.theme.update(event).map(Message::Theme),
            Message::Event(event, status, window) => {
                match &event {
                    iced::Event::Window(iced::window::Event::Opened { size, .. }) => {
                        self.window = Some(window);
                        self.viewport_width.set(size.width);
                        self.window_height.set(size.height);
                    }
                    iced::Event::Window(iced::window::Event::Resized(size))
                        if self.window == Some(window) =>
                    {
                        self.viewport_width.set(size.width);
                        self.window_height.set(size.height);
                    }
                    iced::Event::Window(iced::window::Event::Closed) => {
                        let _ = self.chooser.parent_closed(window);
                        if self.window == Some(window) {
                            self.window = None;
                            self.cancel_chooser();
                        }
                    }
                    _ => {}
                }
                return self.event(event, status);
            }
            Message::Window(enzui_iced_widgets::title_bar_gesture::WindowCommand::Close)
            | Message::CloseRequested => return self.request_close(),
            Message::Window(command) => {
                return iced::window::latest().then(move |window| {
                    window.map_or_else(Task::none, |window| command.into_task(window))
                });
            }
            Message::SaveAndClose => {
                let save = self.update(Message::SaveEdit);
                let dismiss = self.dismiss_dialog();
                let close = self.request_close();
                return Task::batch([save, dismiss, close]);
            }
            Message::DiscardAndClose => {
                let cancel = self.update(Message::CancelEdit);
                let dismiss = self.dismiss_dialog();
                let close = self.request_close();
                return Task::batch([cancel, dismiss, close]);
            }
            Message::SaveAndContinue | Message::DiscardAndContinue => {
                let next = self.pending_navigation.take();
                let decision = if matches!(message, Message::SaveAndContinue) {
                    Message::SaveEdit
                } else {
                    Message::CancelEdit
                };
                let edit = self.update(decision);
                let dismiss = self.dismiss_dialog();
                let navigation = next.map_or_else(Task::none, |message| self.update(*message));
                return Task::batch([edit, dismiss, navigation]);
            }
            Message::VideoWake(generation) => return self.video_wake(generation),
            Message::VideoReady(generation, ready) => return self.video_ready(generation, ready),
            Message::VideoScrub(seconds) => {
                if let Some(editor) = self.viewer.as_mut().and_then(|v| v.video_editor.as_mut()) {
                    editor.previewing = false;
                }
                if let Some(stream) = self.viewer.as_ref().and_then(|v| v.stream.as_ref()) {
                    stream.player.pause();
                }
                return self.update(Message::VideoSeek(seconds));
            }
            Message::PreviewControl(generation, intent) => {
                return self.preview_control(generation, intent);
            }
            Message::VideoSeek(seconds) => {
                if let Some(stream) = self.viewer.as_mut().and_then(|v| v.stream.as_mut()) {
                    stream.show_frame = true;
                    stream.player.seek(
                        std::time::Duration::from_secs_f32(seconds.max(0.0)),
                        enzui_services::media::SeekMode::Accurate,
                    );
                }
            }
            Message::VideoMute => {
                if let Some(stream) = self.viewer.as_ref().and_then(|v| v.stream.as_ref()) {
                    stream.player.set_mute(!stream.status.muted);
                }
            }
            Message::VideoStep => {
                if let Some(editor) = self.viewer.as_mut().and_then(|v| v.video_editor.as_mut()) {
                    editor.previewing = false;
                }
                if let Some(stream) = self.viewer.as_mut().and_then(|v| v.stream.as_mut()) {
                    stream.show_frame = true;
                    stream.player.pause();
                    stream.player.step_frame(true);
                }
            }
            Message::VideoTrimStart(value) => {
                return self.change_video(
                    |edit| edit.start = value.max(0.0).min((edit.end - 0.05).max(0.0)),
                    Some(value),
                );
            }
            Message::VideoTrimEnd(value) => {
                let duration = self.video_duration();
                return self.change_video(
                    |edit| edit.end = value.min(duration).max(edit.start + 0.05),
                    Some(value),
                );
            }
            Message::VideoRotate => {
                return self.change_video(|edit| edit.rotation = (edit.rotation + 1) % 4, None);
            }
            Message::VideoFlip => return self.change_video(|edit| edit.flip = !edit.flip, None),
            Message::VideoExportMute(value) => {
                return self.change_video(|edit| edit.mute = value, None);
            }
            Message::VideoSpeed(value) => {
                return self.change_video(|edit| edit.speed = value.clamp(0.5, 2.0), None);
            }
            Message::VideoMark(start) => {
                let position = self
                    .viewer
                    .as_ref()
                    .and_then(|v| v.stream.as_ref())
                    .map_or(0.0, |s| s.status.position.as_secs_f32());
                return self.update(if start {
                    Message::VideoTrimStart(position)
                } else {
                    Message::VideoTrimEnd(position)
                });
            }
            Message::VideoPreviousFrame => {
                if let Some(editor) = self.viewer.as_mut().and_then(|v| v.video_editor.as_mut()) {
                    editor.previewing = false;
                }
                if let Some(stream) = self.viewer.as_ref().and_then(|v| v.stream.as_ref()) {
                    stream.player.pause();
                    stream.player.step_frame(false);
                }
            }
            Message::VideoGesture(start) => self.video_gesture(start),
            Message::VideoUndo(undo) => return self.undo_video(undo),
            Message::VideoReset => {
                let duration = self.video_duration();
                return self.change_video(
                    |edit| {
                        *edit = crate::services::video::VideoEdit {
                            end: duration,
                            ..Default::default()
                        }
                    },
                    Some(0.0),
                );
            }
            Message::VideoClose(cancel) => return self.close_video_editor(cancel),
            Message::VideoLoop => {
                if let Some(editor) = self.viewer.as_mut().and_then(|v| v.video_editor.as_mut()) {
                    editor.loop_range = !editor.loop_range;
                }
            }
            Message::VideoFilmstrip(generation, revision, result) => {
                if generation == self.load_generation
                    && let Some(viewer) = self.viewer.as_mut()
                    && let Some(editor) = viewer.video_editor.as_mut()
                    && revision == editor.session
                {
                    match result {
                        Ok(handles) => editor.thumbnails = handles,
                        Err(error) => tracing::warn!(%error, "video timeline unavailable"),
                    }
                    editor.thumbnail_cancel = None;
                }
            }
            Message::VideoTransformed(generation, revision, result) => {
                return self.video_transformed(generation, revision, result);
            }
            Message::VideoEditedReady(generation, revision, ready) => {
                return self.video_edited_ready(generation, revision, ready);
            }
            Message::OpeningReady(generation, result) => {
                if generation == self.load_generation
                    && let Some(viewer) =
                        self.viewer.as_mut().filter(|v| v.preview_recipe.is_none())
                    && let Ok(allocation) = result
                {
                    viewer.preview = Some(allocation.handle().clone());
                    viewer.preview_allocation = Some(allocation);
                }
            }
            Message::AnimationLoaded(generation, result) => {
                if generation == self.load_generation
                    && let Some(viewer) = self.viewer.as_mut()
                {
                    viewer.animation_cancel = None;
                    match result {
                        Ok(motion) => viewer.animation = motion,
                        Err(error) => tracing::warn!(%error, "animation unavailable"),
                    }
                }
            }
            Message::Playback
                if self
                    .viewer
                    .as_ref()
                    .is_some_and(|v| v.video_editor.is_some()) =>
            {
                return self.play_video_range();
            }
            Message::Playback => {
                if let Some(stream) = self
                    .viewer
                    .as_mut()
                    .filter(|v| !v.editing)
                    .and_then(|v| v.stream.as_mut())
                {
                    stream.show_frame = true;
                    if stream.status.is_playing() {
                        stream.player.pause();
                    } else {
                        stream.player.play();
                    }
                    return Task::none();
                }
                if let Some(viewer) = self
                    .viewer
                    .as_mut()
                    .filter(|v| !v.editing && v.motion().is_some())
                {
                    viewer.motion_playing = !viewer.motion_playing;
                    if viewer.motion_playing {
                        let index = if viewer
                            .motion()
                            .is_some_and(|m| viewer.motion_position + 1 >= m.frames.len())
                        {
                            0
                        } else {
                            viewer.motion_position
                        };
                        return self.motion_frame(index);
                    }
                }
            }
            Message::MotionTick => {
                if let Some(viewer) = self
                    .viewer
                    .as_ref()
                    .filter(|v| v.motion_playing && !v.motion_busy && !v.editing)
                    && let Some(motion) = viewer.motion()
                {
                    if viewer.motion_position + 1 < motion.frames.len() {
                        return self.motion_frame(viewer.motion_position + 1);
                    }
                    if let Some(viewer) = self.viewer.as_mut() {
                        viewer.motion_playing = false;
                    }
                }
            }
            Message::MotionSeek(position) => {
                if let Some(viewer) = self.viewer.as_mut() {
                    viewer.motion_playing = false;
                }
                return self.motion_frame(position.max(0.0).round() as usize);
            }
            Message::MotionReady(generation, request, index, result) => {
                if generation == self.load_generation
                    && let Some(viewer) =
                        self.viewer.as_mut().filter(|v| v.motion_request == request)
                {
                    viewer.motion_busy = false;
                    if !viewer.editing {
                        match result {
                            Ok(allocation) => {
                                viewer.motion_preview = Some(allocation.handle().clone());
                                viewer.motion_allocation = Some(allocation);
                                viewer.motion_position = index;
                            }
                            Err(error) => {
                                viewer.motion_playing = false;
                                self.notice(error, true);
                            }
                        }
                    }
                }
            }
            Message::SaveFrame => {
                if self.busy {
                    return Task::none();
                }
                if self.viewer.as_ref().is_some_and(|v| v.stream.is_some()) {
                    return self.save_video_frame();
                }
                if let Some(viewer) = self.viewer.as_mut().filter(|v| !v.editing)
                    && let Some(motion) = viewer.motion()
                {
                    let path = motion.path.clone();
                    let frame = viewer.motion_position;
                    viewer.motion_playing = false;
                    self.busy = true;
                    let generation = self.load_generation;
                    let work = self.workers.submit(generation, move |context| {
                        crate::services::formats::extract_frame(&path, frame, &|| {
                            context.is_cancelled()
                        })
                        .map_err(|e| e.to_string())
                    });
                    return Task::perform(work, move |settled| {
                        Message::FrameSaved(
                            generation,
                            settled
                                .into_result()
                                .map_err(|e| e.to_string())
                                .and_then(|r| r),
                        )
                    });
                }
            }
            Message::FrameSaved(generation, result) => {
                self.busy = false;
                if generation != self.load_generation {
                    return if result.is_ok() {
                        self.scan()
                    } else {
                        Task::none()
                    };
                }
                match result {
                    Ok(path) => {
                        self.notice(
                            format!(
                                "Frame saved: {}",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            ),
                            false,
                        );
                        return self.scan();
                    }
                    Err(error) => self.notice(error, true),
                }
            }
            Message::FileOpened(generation, result) => {
                if generation != self.load_generation || !self.opening_file {
                    return Task::none();
                }
                match result {
                    Ok((photo, loaded)) => {
                        self.launch.file = Some(photo.path.clone());
                        let id = photo.id.clone();
                        self.library.photos = vec![photo];
                        self.refresh_folder_counts();
                        self.refresh_projection();
                        if let Some((_, generation, opening)) = self.prepare_photo(id) {
                            return Task::batch([
                                opening,
                                self.update(Message::Loaded(generation, Ok(loaded))),
                            ]);
                        }
                    }
                    Err(error) => self.notice(format!("Cannot open media: {error}"), true),
                }
                return self.finish_file_open();
            }
            Message::CatalogLoaded(result) => {
                match result {
                    Ok(session) => {
                        self.catalog = session.catalog;
                        match super::shortcuts::restore(&self.catalog.settings.shortcut_overrides) {
                            Ok(shortcuts) => self.shortcuts = shortcuts,
                            Err(error) => {
                                self.notice(format!("Cannot load shortcuts: {error}"), true)
                            }
                        }
                        self.thumbnail_store
                            .set_limit(self.catalog.settings.thumbnail_cache_mb);
                        self.density = self.catalog.settings.thumbnail_size.min(7);
                        self.catalog_authority = Some(session.authority);
                    }
                    Err(error) => {
                        self.persistence_blocked = true;
                        self.notice(error, true);
                    }
                }
                if let Some(folder) = &self.launch.add_folder
                    && self.catalog.sources.contains(folder)
                {
                    self.destination = Destination::Source(folder.clone());
                }
                let mut preview = Task::none();
                if self.launch.file.is_some()
                    && let Some(viewer) = self.viewer.as_mut()
                {
                    viewer.info = self.catalog.viewer_info;
                    if !viewer.editing {
                        viewer.recipe = self
                            .catalog
                            .edits
                            .get(&viewer.id)
                            .cloned()
                            .unwrap_or_default();
                        viewer.before_edit = viewer.recipe.clone();
                        if viewer.loaded.is_some() {
                            preview = self.request_preview();
                        }
                    }
                }
                if self.opening_file {
                    return preview;
                }
                return Task::batch([preview, self.scan()]);
            }
            Message::Refresh => {
                self.library.failed.clear();
                return self.scan();
            }
            Message::Scanned(generation, report, canonical_home) => {
                if generation != self.scan_generation {
                    return Task::none();
                }
                self.scanning = false;
                if self.scan_requested {
                    return self.scan();
                }
                self.canonical_home = canonical_home;
                if !report.warnings.is_empty() {
                    self.notice(report.warnings.join(" · "), true);
                }
                if report.truncated {
                    self.notice(
                        "Scan limit reached. Add a smaller source folder to find more photos.",
                        true,
                    );
                }
                let direct = self.launch.file.as_ref().and_then(|path| {
                    self.library
                        .photos
                        .iter()
                        .find(|photo| &photo.path == path)
                        .cloned()
                });
                self.library.photos = report.photos.clone();
                if let Some(photo) = direct
                    && !self.library.photos.iter().any(|item| item.id == photo.id)
                {
                    self.library.photos.push(photo);
                }
                self.refresh_folder_counts();
                self.refresh_projection();
                return self.schedule_thumbnails();
            }
            Message::Navigate(destination) => {
                self.cloud.retire();
                self.cloud.open = false;
                self.cloud.playback_service = false;
                self.selection.leave_selection_mode();
                self.destination = destination;
                self.retire_viewer();
                self.period_filter = None;
                self.nav.dismiss_overlay();
                self.refresh_projection();
                return Task::batch([
                    self.finish_file_open(),
                    self.reset_gallery_scroll(),
                    self.schedule_thumbnails(),
                ]);
            }
            Message::Search(query) => {
                self.query = query;
                self.refresh_projection();
                return Task::batch([self.reset_gallery_scroll(), self.schedule_thumbnails()]);
            }
            Message::OpenSearch | Message::FocusSearch => {
                if self.viewer.is_some() || self.dialog.is_some() {
                    return Task::none();
                }
                if matches!(message, Message::OpenSearch) || !crate::view::wide_search(self) {
                    self.search_open = true;
                    self.overflow_open = false;
                    self.nav.dismiss_overlay();
                }
                return iced::advanced::widget::operate(
                    iced::advanced::widget::operation::focusable::focus(
                        iced::advanced::widget::Id::new("photos-search"),
                    ),
                );
            }
            Message::CloseSearch => {
                self.search_open = false;
                let target = if crate::view::wide_search(self) {
                    "photos-search"
                } else {
                    crate::view::SEARCH_TRIGGER_ID
                };
                return iced::advanced::widget::operate(
                    iced::advanced::widget::operation::focusable::focus(iced::widget::Id::new(
                        target,
                    )),
                );
            }
            Message::Sidebar(next) => self.nav = next,
            Message::Grouping(grouping) => {
                self.selection.leave_selection_mode();
                self.grouping = grouping;
                self.period_filter = None;
                self.refresh_projection();
                return Task::batch([
                    iced::advanced::widget::operate(
                        iced::advanced::widget::operation::focusable::focus(
                            iced::advanced::widget::Id::new("photos-tabs-Library grouping"),
                        ),
                    ),
                    self.reset_gallery_scroll(),
                    self.schedule_thumbnails(),
                ]);
            }
            Message::Period(start, end, label) => {
                self.selection.leave_selection_mode();
                self.period_filter = Some((start, end, label));
                self.refresh_projection();
                return Task::batch([self.reset_gallery_scroll(), self.schedule_thumbnails()]);
            }
            Message::ClearPeriod => {
                self.selection.leave_selection_mode();
                self.period_filter = None;
                self.refresh_projection();
                return Task::batch([self.reset_gallery_scroll(), self.schedule_thumbnails()]);
            }
            Message::Ignore => {}
            Message::Density(direction) => {
                let density = self.density.saturating_add_signed(direction).min(7);
                if density == self.density {
                    return Task::none();
                }
                self.gallery_generation = self.gallery_generation.wrapping_add(1);
                self.density = density;
                self.catalog.settings.thumbnail_size = density;
                return Task::batch([self.changed(), self.schedule_thumbnails()]);
            }
            Message::Shortcut(event) => return self.edit_shortcut(event),
            Message::SettingsPage(page) => {
                self.cloud.sync.close_form();
                self.shortcut_capture = None;
                self.shortcut_feedback = None;
                self.settings_page = page;
                self.settings_page_open = true;
                self.settings_menu = None;
            }
            Message::SettingsBack => {
                self.settings_page_open = false;
                self.settings_menu = None;
            }
            Message::SettingsMenu(menu) => {
                self.settings_menu = if menu.is_some() && self.settings_menu == menu {
                    None
                } else {
                    menu
                }
            }
            Message::GallerySections(sections) => {
                self.catalog.settings.sections = sections;
                return self.gallery_settings_changed();
            }
            Message::OldestFirst(oldest) => {
                self.catalog.settings.oldest_first = oldest;
                return self.gallery_settings_changed();
            }
            Message::ContainThumbnails(contain) => {
                self.catalog.settings.contain_thumbnails = contain;
                return self.changed();
            }
            Message::ShowNames(show) => {
                self.catalog.settings.show_names = show;
                return self.changed();
            }
            Message::ImportLayout(layout) => {
                self.catalog.settings.import_layout = layout;
                self.settings_menu = None;
                return self.changed();
            }
            Message::ImportDestination(path) => {
                self.settings_menu = None;
                if path != self.paths.photos
                    && !self.catalog.sources.contains(&path)
                    && path != self.import_destination()
                {
                    self.notice("Choose an added source folder for imports.", true);
                    return Task::none();
                }
                return self.folder_chosen(PickPurpose::ImportDestination, path);
            }
            Message::GridScroll(viewport) => {
                self.library.scroll_top = viewport.absolute_offset().y;
                self.library.viewport_height = viewport.bounds().height;
                return self.schedule_thumbnails();
            }
            Message::ThumbnailCacheLimit(mb) => {
                self.catalog.settings.thumbnail_cache_mb = mb.min(2048);
                self.thumbnail_store.set_limit(mb);
                return Task::batch([self.changed(), self.refresh_thumbnail_cache()]);
            }
            Message::ClearThumbnailCache => {
                self.thumbnail_store.invalidate();
                self.library.cache.borrow_mut().clear();
                self.library.inflight.clear();
                self.library.failed.clear();
                return Task::batch([self.refresh_thumbnail_cache(), self.schedule_thumbnails()]);
            }
            Message::ThumbnailCacheUpdated(generation, result) => {
                if generation == self.thumbnail_store.generation() {
                    match result {
                        Ok(usage) => self.thumbnail_usage = usage,
                        Err(error) => tracing::debug!(%error, "thumbnail cache unavailable"),
                    }
                }
            }
            Message::Thumbnail(generation, key, result) => {
                if generation != self.thumbnail_store.generation() {
                    return Task::none();
                }
                self.library.inflight.remove(&key);
                match result {
                    Ok(handle) => self.library.cache.borrow_mut().insert(key, handle),
                    Err(_) => {
                        self.library.failed.insert(key);
                    }
                }
                return Task::batch([self.schedule_thumbnails(), self.refresh_thumbnail_cache()]);
            }
            Message::OpenPhotoAction(id, action) => {
                let load = self.open_photo(id);
                if self.viewer.is_some() {
                    self.after_load = Some((self.load_generation, action));
                }
                return load;
            }
            Message::Open(id) => {
                self.cloud.playback_service = false;
                return self.open_photo(id);
            }
            Message::DragSelection(generation, ids) => {
                if generation == self.gallery_generation {
                    self.selection.enter_selection_mode();
                    let anchor = ids.first().cloned();
                    let active = ids.last().cloned();
                    self.selection.replace(ids, anchor, active);
                }
            }
            Message::AutoScrolled(top) => {
                self.library.scroll_top = top;
                return self.schedule_thumbnails();
            }
            Message::TileClicked(id, mut modifiers) => {
                let was_empty = self.selection.is_empty();
                if !modifiers.range
                    && self.selection.mode()
                        == enzui_iced_core::list_selection::SelectionMode::Selection
                {
                    modifiers.toggle = true;
                }

                match self
                    .selection
                    .on_pointer_click(&self.library.click_model, &id, modifiers)
                {
                    enzui_iced_core::list_selection::ClickOutcome::Activated(id) => {
                        return self.update(Message::Open(id));
                    }
                    enzui_iced_core::list_selection::ClickOutcome::Selected { .. } => {
                        if self.selection.is_empty() {
                            self.selection.leave_selection_mode();
                        } else {
                            self.selection.enter_selection_mode();
                            if was_empty {
                                let selected = self
                                    .selection
                                    .selected_in_order(self.library.click_model.order());
                                self.selection.replace(selected, Some(id.clone()), Some(id));
                            }
                        }
                    }
                    enzui_iced_core::list_selection::ClickOutcome::Ignored => {}
                }
            }
            Message::SelectionMode(enabled) => {
                if enabled {
                    self.selection.enter_selection_mode();
                } else {
                    self.selection.leave_selection_mode();
                }
            }
            Message::SelectAll => {
                self.selection.enter_selection_mode();
                self.selection.replace(
                    self.library.click_model.order().iter().cloned(),
                    None,
                    None,
                );
            }
            Message::FavoriteSelection => {
                let selected = self
                    .selection
                    .selected_in_order(self.library.click_model.order());
                self.catalog.favorites.extend(selected);
                self.selection.leave_selection_mode();
                self.refresh_projection();
                return self.changed();
            }
            Message::Loaded(generation, result) => {
                if generation != self.load_generation {
                    return Task::none();
                }
                match result {
                    Ok(loaded) => {
                        if let Some(viewer) = self.viewer.as_mut() {
                            viewer.load_cancel = None;
                            let (width, height) =
                                viewer.recipe.output_dimensions(loaded.pixels.dimensions());
                            viewer.width_input = width.to_string();
                            viewer.height_input = height.to_string();
                            let original = iced::widget::image::allocate(
                                loaded.original_preview.clone(),
                            )
                            .map(move |result| {
                                Message::OriginalReady(
                                    generation,
                                    result.map_err(|error| error.to_string()),
                                )
                            });
                            viewer.loaded = Some(loaded);
                            let media = self.start_media();
                            let preview = self.request_preview();
                            let action = self
                                .after_load
                                .take()
                                .filter(|(owner, _)| *owner == generation)
                                .map_or_else(Task::none, |(_, action)| {
                                    self.update(action.message())
                                });
                            return Task::batch([preview, original, action, media]);
                        }
                    }
                    Err(error) => {
                        if let Some(viewer) = self.viewer.as_mut() {
                            viewer.load_cancel = None;
                            viewer.load_error = Some(error.clone());
                            viewer.preview = None;
                        }
                        self.after_load = None;
                        self.notice(format!("Cannot open photo: {error}"), true);
                    }
                }
            }
            Message::Back => {
                if self.cloud.playback_service {
                    self.cloud.playback_service = false;
                    self.cloud.preview = None;
                    self.retire_viewer();
                    self.cloud.open = true;
                    return self.finish_file_open();
                }
                if self.viewer.is_none() && self.period_filter.is_some() {
                    return self.update(Message::ClearPeriod);
                }
                self.retire_viewer();
                return Task::batch([self.finish_file_open(), self.schedule_thumbnails()]);
            }
            Message::ViewerCommand(owner, command) => {
                if self.viewer_owner().as_ref() == Some(&owner) {
                    return self.update(*command);
                }
            }
            Message::CopyImage => return self.copy_image(),
            Message::SaveCopy => return self.save_copy(),
            Message::CopyPrepared(generation, result) => {
                if generation == self.load_generation && self.viewer.is_some() && !self.cloud.open {
                    match result {
                        Ok(image) => {
                            return iced::clipboard::write(image).map(move |r| {
                                Message::CopyFinished(
                                    generation,
                                    r.map_err(crate::services::clipboard_error),
                                )
                            });
                        }
                        Err(error) => self.notice(format!("Could not copy image: {error}"), true),
                    }
                }
            }
            Message::CopyFinished(generation, result) => {
                if generation == self.load_generation
                    && let Err(error) = result
                {
                    self.notice(format!("Could not copy image: {error}"), true);
                }
            }
            Message::Next(direction) => {
                if self.cloud.open || self.cloud.playback_service {
                    return self.cloud_event(super::cloud::Event::Step(i32::from(direction)));
                }

                if let Some(viewer) = self.viewer.as_ref() {
                    if viewer.editing {
                        return Task::none();
                    }
                    if let Some(position) = self
                        .library
                        .visible
                        .iter()
                        .position(|index| self.library.photos[*index].id == viewer.id)
                    {
                        let next = position
                            .saturating_add_signed(isize::from(direction))
                            .min(self.library.visible.len().saturating_sub(1));
                        if next != position {
                            return self.open_photo(
                                self.library.photos[self.library.visible[next]].id.clone(),
                            );
                        }
                    }
                }
            }
            Message::Favorite(id) => {
                self.catalog.toggle_favorite(id);
                self.refresh_projection();
                return self.changed();
            }
            Message::ToggleInfo => {
                self.catalog.viewer_info = !self.catalog.viewer_info;
                if let Some(viewer) = self.viewer.as_mut() {
                    viewer.info = self.catalog.viewer_info;
                }
                return self.changed();
            }
            Message::OpenContainingFolder => {
                if let Some(parent) = self
                    .selected_photo()
                    .and_then(|photo| photo.path.parent())
                    .map(PathBuf::from)
                {
                    let generation = self.load_generation;
                    let work = self.workers.submit(generation, move |_| {
                        std::process::Command::new("xdg-open")
                            .arg(parent)
                            .status()
                            .map_err(|e| e.to_string())
                            .and_then(|status| {
                                if status.success() {
                                    Ok(())
                                } else {
                                    Err("Could not open the containing folder".to_owned())
                                }
                            })
                    });
                    return Task::perform(work, move |settled| {
                        Message::FolderOpened(
                            generation,
                            settled
                                .into_result()
                                .map_err(|e| e.to_string())
                                .and_then(|r| r),
                        )
                    });
                }
            }
            Message::FolderOpened(generation, result) => {
                if generation == self.load_generation
                    && let Err(error) = result
                {
                    self.notice(error, true);
                }
            }
            Message::ExportFormat(format) => {
                self.export_options.format = format;
                self.dialog_text = PathBuf::from(&self.dialog_text)
                    .with_extension(format.extension())
                    .to_string_lossy()
                    .into_owned();
            }
            Message::ExportScale(value) => {
                self.export_options.scale_percent = value.clamp(1.0, 100.0).round() as u8
            }
            Message::ExportMetadata(keep) => self.export_options.keep_metadata = keep,
            Message::Toast(event) => {
                self.toasts.update(event);
            }
            Message::ExportQuality(value) => {
                self.export_options.quality = value.clamp(1.0, 100.0) as u8
            }
            Message::BeginEdit
                if self
                    .selected_photo()
                    .is_some_and(|p| photos_library::scan::is_video_path(&p.path)) =>
            {
                return self.open_dialog(Dialog::VideoEdit);
            }
            Message::BeginEdit => {
                if let Some(viewer) = self
                    .viewer
                    .as_mut()
                    .filter(|v| v.loaded.is_some() && !v.editing)
                {
                    viewer.motion_playing = false;
                    if let Some(stream) = &mut viewer.stream {
                        stream.player.pause();
                        stream.show_frame = false;
                    }
                    viewer.editing = true;
                    viewer.before_edit = viewer.recipe.clone();
                    viewer.before_edit_preview =
                        if viewer.preview_recipe.as_ref() == Some(&viewer.recipe) {
                            viewer.preview.clone()
                        } else {
                            None
                        };
                    viewer.before_edit_allocation = if viewer.before_edit_preview.is_some() {
                        viewer.preview_allocation.clone()
                    } else {
                        None
                    };
                    viewer.history.clear();
                    viewer.redo.clear();
                }
            }
            Message::EditorTab(tab) => {
                if let Some(viewer) = self.viewer.as_mut() {
                    viewer.tab = tab;
                    viewer.drawing_crop = false;
                    viewer.crop_draft = None;
                }
            }
            Message::Adjust(adjustment, value) => {
                return self.edit_with(|recipe| adjustment.set(recipe, value));
            }
            Message::Rotate => return self.edit_with(EditRecipe::rotate_clockwise),
            Message::Flip => {
                return self.edit_with(|recipe| recipe.flip_horizontal = !recipe.flip_horizontal);
            }
            Message::CropPreset(preset) => {
                let dimensions = self
                    .viewer
                    .as_ref()
                    .and_then(|v| v.loaded.as_ref())
                    .map(|loaded| loaded.pixels.dimensions());
                if let Some((w, h)) = dimensions {
                    let aspect = match preset {
                        1 => Some(1.0),
                        2 => Some(4.0 / 3.0),
                        3 => Some(16.0 / 9.0),
                        4 => Some(3.0 / 4.0),
                        _ => None,
                    };
                    let rotated = self
                        .viewer
                        .as_ref()
                        .is_some_and(|viewer| viewer.recipe.quarter_turns % 2 == 1);
                    let crop = aspect.map(|aspect| {
                        let aspect = if rotated { 1.0 / aspect } else { aspect };
                        let source_aspect = w as f32 / h as f32;
                        let (width, height) = if source_aspect > aspect {
                            (aspect / source_aspect, 1.0)
                        } else {
                            (1.0, source_aspect / aspect)
                        };
                        Crop {
                            x: (1.0 - width) / 2.0,
                            y: (1.0 - height) / 2.0,
                            width,
                            height,
                        }
                    });
                    if let Some(viewer) = self.viewer.as_mut().filter(|v| v.editing) {
                        viewer.crop_draft = crop;
                        viewer.crop_aspect =
                            Some(crop.map_or(1.0, |crop| crop.width / crop.height));
                        viewer.drawing_crop = true;
                        viewer.compare = false;
                    }
                }
            }
            Message::DrawCrop => {
                if let Some(viewer) = self.viewer.as_mut().filter(|v| v.editing) {
                    viewer.crop_aspect = None;
                    viewer.crop_draft = viewer.recipe.crop.or(Some(Crop {
                        x: 0.1,
                        y: 0.1,
                        width: 0.8,
                        height: 0.8,
                    }));
                    viewer.drawing_crop = true;
                    viewer.compare = false;
                }
            }
            Message::CropRegion(crop) => {
                if crop.is_valid()
                    && let Some(viewer) =
                        self.viewer.as_mut().filter(|v| v.editing && v.drawing_crop)
                {
                    viewer.crop_draft = Some(crop);
                }
            }
            Message::ApplyCrop => {
                if let Some(viewer) = self.viewer.as_mut().filter(|v| v.editing && v.drawing_crop) {
                    let crop = viewer.crop_draft.take();
                    viewer.drawing_crop = false;
                    return self.edit_with(|recipe| recipe.crop = crop);
                }
            }
            Message::CancelCrop => {
                if let Some(viewer) = self.viewer.as_mut() {
                    viewer.drawing_crop = false;
                    viewer.crop_draft = None;
                }
            }
            Message::ResizeWidth(value) => {
                if let Some(viewer) = self.viewer.as_mut() {
                    if viewer.lock_aspect
                        && let Ok(width) = value.parse::<u32>()
                        && let Some(source) = &viewer.loaded
                    {
                        let (w, h) = viewer.recipe.output_dimensions(source.pixels.dimensions());
                        if w > 0 {
                            viewer.height_input = (u64::from(width) * u64::from(h) / u64::from(w))
                                .max(1)
                                .to_string();
                        }
                    }
                    viewer.width_input = value;
                }
            }
            Message::ResizeHeight(value) => {
                if let Some(viewer) = self.viewer.as_mut() {
                    if viewer.lock_aspect
                        && let Ok(height) = value.parse::<u32>()
                        && let Some(source) = &viewer.loaded
                    {
                        let (w, h) = viewer.recipe.output_dimensions(source.pixels.dimensions());
                        if h > 0 {
                            viewer.width_input = (u64::from(height) * u64::from(w) / u64::from(h))
                                .max(1)
                                .to_string();
                        }
                    }
                    viewer.height_input = value;
                }
            }
            Message::LockAspect(locked) => {
                if let Some(viewer) = self.viewer.as_mut() {
                    viewer.lock_aspect = locked;
                }
            }
            Message::ApplyResize => {
                if let Some(viewer) = self.viewer.as_ref() {
                    let parsed = viewer
                        .width_input
                        .parse::<u32>()
                        .ok()
                        .zip(viewer.height_input.parse::<u32>().ok());
                    if let Some(dimensions) = parsed {
                        return self.edit_with(|recipe| recipe.resize = Some(dimensions));
                    }
                    self.notice("Enter a width and height in pixels.", true);
                }
            }
            Message::Undo | Message::Redo
                if self
                    .viewer
                    .as_ref()
                    .is_some_and(|v| v.video_editor.is_some()) =>
            {
                return self.undo_video(matches!(message, Message::Undo));
            }
            Message::CancelEdit
                if self
                    .viewer
                    .as_ref()
                    .is_some_and(|v| v.video_editor.is_some()) =>
            {
                return self.close_video_editor(true);
            }
            Message::SaveEdit
                if self
                    .viewer
                    .as_ref()
                    .is_some_and(|v| v.video_editor.is_some()) =>
            {
                return self.close_video_editor(false);
            }
            Message::Undo | Message::Redo => {
                let undo = matches!(message, Message::Undo);
                if let Some(viewer) = self.viewer.as_mut().filter(|v| v.editing) {
                    viewer.drawing_crop = false;
                    viewer.crop_draft = None;
                    let previous = if undo {
                        viewer.history.pop()
                    } else {
                        viewer.redo.pop()
                    };
                    if let Some(previous) = previous {
                        let current = std::mem::replace(&mut viewer.recipe, previous);
                        if undo {
                            viewer.redo.push(current);
                        } else {
                            viewer.history.push(current);
                        }
                        return self.request_preview();
                    }
                }
            }
            Message::ResetEdit => return self.edit_with(|recipe| *recipe = EditRecipe::default()),
            Message::SaveEdit => {
                if self.viewer.as_ref().is_some_and(|v| v.drawing_crop) {
                    self.notice("Apply or cancel the crop first.", true);
                    return Task::none();
                }
                if let Some(viewer) = self.viewer.as_mut().filter(|v| v.editing) {
                    if viewer.recipe == EditRecipe::default() {
                        self.catalog.edits.remove(&viewer.id);
                    } else {
                        self.catalog
                            .edits
                            .insert(viewer.id.clone(), viewer.recipe.clone());
                    }
                    viewer.editing = false;
                    viewer.compare = false;
                    viewer.drawing_crop = false;
                    self.refresh_projection();
                    self.notice("Edits saved. Your original is unchanged.", false);
                    return self.changed();
                }
            }
            Message::CancelEdit => {
                if let Some(viewer) = self.viewer.as_mut().filter(|v| v.editing) {
                    viewer.recipe = viewer.before_edit.clone();
                    viewer.editing = false;
                    viewer.compare = false;
                    viewer.drawing_crop = false;
                    viewer.preview_generation = viewer.preview_generation.saturating_add(1);
                    viewer.preview_inflight = None;
                    viewer.preview_inflight_recipe = None;
                    viewer.preview_busy = false;
                    viewer.preview_dirty = false;
                    if let Some(cancel) = viewer.preview_cancel.take() {
                        cancel.cancel();
                    }
                    if let Some(preview) = viewer.before_edit_preview.take() {
                        viewer.preview_allocation = viewer.before_edit_allocation.take();
                        viewer.preview = Some(preview);
                        viewer.preview_recipe = Some(viewer.recipe.clone());
                        return Task::none();
                    }
                }
                return self.request_preview();
            }
            Message::Preview(load_generation, generation, result) => {
                if load_generation != self.load_generation
                    || self
                        .viewer
                        .as_ref()
                        .is_none_or(|viewer| viewer.preview_inflight != Some(generation))
                {
                    return Task::none();
                }
                match result {
                    Ok(handle) => {
                        return iced::widget::image::allocate(handle).map(move |result| {
                            Message::PreviewReady(
                                load_generation,
                                generation,
                                result.map_err(|error| error.to_string()),
                            )
                        });
                    }
                    Err(error) => {
                        return self.update(Message::PreviewReady(
                            load_generation,
                            generation,
                            Err(error),
                        ));
                    }
                }
            }
            Message::PreviewReady(load_generation, generation, result) => {
                if load_generation != self.load_generation {
                    return Task::none();
                }
                if let Some(viewer) = self.viewer.as_mut() {
                    if viewer.preview_inflight != Some(generation) {
                        return Task::none();
                    }
                    viewer.preview_inflight = None;
                    viewer.preview_cancel = None;
                    viewer.preview_busy = false;
                    match result {
                        Ok(allocation) => {
                            viewer.preview = Some(allocation.handle().clone());
                            viewer.preview_allocation = Some(allocation);
                            viewer.preview_recipe = viewer.preview_inflight_recipe.take();
                        }
                        Err(error) => {
                            self.notice(error, true);
                            return self.finish_file_open();
                        }
                    }
                    if viewer.preview_dirty {
                        return self.start_preview();
                    }
                }
                if self.opening_file
                    && self
                        .viewer
                        .as_ref()
                        .is_some_and(|viewer| viewer.preview_allocation.is_some())
                {
                    tracing::info!(
                        elapsed_ms = self.launch.started.elapsed().as_millis(),
                        "direct file preview ready"
                    );
                    return self.finish_file_open();
                }
            }
            Message::OriginalReady(generation, result) => {
                if generation != self.load_generation {
                    return Task::none();
                }
                match result {
                    Ok(allocation) => {
                        if let Some(viewer) = self.viewer.as_mut() {
                            viewer.original_preview = Some(allocation.handle().clone());
                            viewer.original_allocation = Some(allocation);
                        }
                    }
                    Err(error) => self.notice(error, true),
                }
            }
            Message::Compare(compare) => {
                if let Some(viewer) = self.viewer.as_mut() {
                    viewer.compare = compare;
                }
            }
            Message::Transform(transform) => {
                if let Some(viewer) = self.viewer.as_mut() {
                    viewer.transform = transform;
                }
            }
            Message::Dialog(dialog) => return self.open_dialog(dialog),
            Message::DialogFrom(dialog, invoker) => {
                return self.open_dialog_from(dialog, Some(invoker));
            }
            Message::DialogText(value) => self.dialog_text = value,
            Message::DismissDialog => {
                if !self.busy {
                    return self.dismiss_dialog();
                }
            }
            Message::Modal(event) => {
                let (closed, focus) = self.dialogs.update(event);
                if !closed.is_empty() {
                    self.shortcut_capture = None;
                    self.shortcut_feedback = None;
                    self.dialog = None;
                    self.pending_navigation = None;
                }
                return focus;
            }
            Message::CreateAlbum => match self.catalog.create_album(&self.dialog_text) {
                Ok(id) => {
                    let dismiss = self.dismiss_dialog();
                    let save = self.changed();
                    let navigate = if self.viewer.is_none() {
                        self.update(Message::Navigate(Destination::Album(id)))
                    } else {
                        Task::none()
                    };
                    return Task::batch([dismiss, save, navigate]);
                }
                Err(error) => self.notice(error, true),
            },
            Message::AddToAlbum(album_id) => {
                let ids = if let Some(viewer) = &self.viewer {
                    vec![viewer.id.clone()]
                } else {
                    self.selection
                        .selected_in_order(self.library.click_model.order())
                };
                if !ids.is_empty()
                    && let Some(album) = self.catalog.albums.iter_mut().find(|a| a.id == album_id)
                {
                    album.photos.extend(ids);
                    let name = album.name.clone();
                    self.notice(format!("Added to {name}"), false);
                    let dismiss = self.dismiss_dialog();
                    self.selection.leave_selection_mode();
                    self.refresh_projection();
                    return Task::batch([dismiss, self.changed(), self.schedule_thumbnails()]);
                }
            }
            Message::RemoveFromAlbum => {
                if let Destination::Album(id) = self.destination {
                    let ids = if let Some(viewer) = &self.viewer {
                        vec![viewer.id.clone()]
                    } else {
                        self.selection
                            .selected_in_order(self.library.click_model.order())
                    };
                    if let Some(album) = self.catalog.albums.iter_mut().find(|album| album.id == id)
                    {
                        for photo in ids {
                            album.photos.remove(&photo);
                        }
                    }
                    self.selection.leave_selection_mode();
                    self.retire_viewer();
                    self.refresh_projection();
                    return Task::batch([self.changed(), self.schedule_thumbnails()]);
                }
            }
            Message::DeleteAlbum(id) => {
                self.catalog.albums.retain(|a| a.id != id);
                self.destination = Destination::Albums;
                self.refresh_projection();
                let dismiss = self.dismiss_dialog();
                return Task::batch([dismiss, self.changed(), self.schedule_thumbnails()]);
            }
            Message::RenameAlbum(id) => match self.catalog.rename_album(id, &self.dialog_text) {
                Ok(()) => {
                    self.refresh_projection();
                    let dismiss = self.dismiss_dialog();
                    return Task::batch([dismiss, self.changed()]);
                }
                Err(error) => self.notice(error, true),
            },
            Message::Pick(purpose) => return self.pick(purpose),
            Message::ChooserWindow(request, window, parent) => {
                return self.chooser_window(request, window, parent);
            }
            Message::ChooserParent(request, window, handoff) => {
                self.chooser_parent(request, window, handoff)
            }
            Message::ChooserUnavailable(request) => self.chooser_unavailable(request),
            Message::ChooserWake => return self.chooser.pump(Message::ChooserDone),
            Message::ChooserFallback(generation, event) => {
                if generation == self.chooser_generation {
                    return self.chooser.handle(event, Message::ChooserDone);
                }
            }
            Message::ChooserDone(request, outcome) => {
                if request.generation != self.chooser_generation
                    || request.load_generation != self.load_generation
                    || request.service_generation != self.cloud.generation
                {
                    return Task::none();
                }
                let purpose = request.purpose;
                match outcome {
                PortalOutcome::Completed(files) => if let Some(path) = files.local_paths().into_iter().next() { if purpose == PickPurpose::VideoExport { self.dialog_text = path.to_string_lossy().into_owned(); return self.export_video(path); } else { return self.folder_chosen(purpose, path); } } else { self.notice("Choose a local folder or file.", true); },
                PortalOutcome::Cancelled(_) => {},
                _ => self.notice("The desktop file chooser is unavailable. Enter a local path in the dialog instead.", true),
            }
            }
            Message::FolderChosen(purpose, path) => return self.folder_chosen(purpose, path),
            Message::SourceValidated(result) => {
                self.busy = false;
                match result {
                    Ok(path) => {
                        if !self.catalog.sources.contains(&path)
                            && path != self.paths.photos
                            && Some(&path) != self.canonical_home.as_ref()
                        {
                            self.catalog.sources.push(path);
                        }
                        self.notice("Source folder added. Photos are read in place.", false);
                        let dismiss = self.dismiss_dialog();
                        return Task::batch([
                            dismiss,
                            self.changed(),
                            self.scan(),
                            self.continue_close(),
                        ]);
                    }
                    Err(error) => self.notice(error, true),
                }
                return self.continue_close();
            }
            Message::RemoveSource(path) => {
                self.catalog.sources.retain(|source| source != &path);
                if self.catalog.settings.import_destination.as_ref() == Some(&path) {
                    self.catalog.settings.import_destination = None;
                }
                if self.destination == Destination::Source(path) {
                    self.destination = Destination::Library;
                }
                self.notice("Source folder removed. Its files are unchanged.", false);
                return Task::batch([self.changed(), self.scan()]);
            }
            Message::ImportProgress(generation, progress) => {
                if generation == self.import_generation && self.import_cancel.is_some() {
                    self.import_progress = Some(progress);
                }
            }
            Message::HideImportProgress => self.import_progress_hidden = true,
            Message::CancelImport => {
                if let Some(cancel) = &self.import_cancel {
                    cancel.store(true, std::sync::atomic::Ordering::Release);
                }
            }
            Message::ImportFinished(generation, result) => {
                if generation != self.import_generation {
                    return Task::none();
                }
                self.import_cancel = None;
                self.import_progress = None;
                self.busy = false;
                match result {
                    Ok(report) => {
                        self.catalog
                            .imported
                            .extend(crate::services::imported_ids(&report.paths));
                        if !report.paths.is_empty()
                            && let Some(root) = &report.destination
                            && root != &self.paths.photos
                            && !self.catalog.sources.contains(root)
                        {
                            self.catalog.sources.push(root.clone());
                        }
                        let mut notice = format!(
                            "Imported {} items · {} skipped",
                            report.paths.len(),
                            report.skipped
                        );
                        if let Some(error) = report.errors.first() {
                            notice.push_str(&format!(" · {error}"));
                        }
                        if report.cancelled {
                            notice.push_str(" · Import cancelled");
                        }
                        self.notice(notice, !report.errors.is_empty() || report.cancelled);
                        self.destination = Destination::Imports;
                        return Task::batch([self.changed(), self.scan(), self.continue_close()]);
                    }
                    Err(error) => self.notice(error, true),
                }
                return self.continue_close();
            }
            Message::ExportFinished(result) => {
                self.busy = false;
                if let Some(editor) = self.viewer.as_mut().and_then(|v| v.video_editor.as_mut()) {
                    editor.exporting = false;
                }
                match result {
                    Ok(path) => self.notice(format!("Saved {}", path.display()), false),
                    Err(error) => self.notice(error, true),
                }
                return self.continue_close();
            }
            Message::PersistenceFinished(revision, result) => {
                self.settle_native_drops(revision, result.is_ok());
                self.save_inflight = false;
                match result {
                    Ok(()) => {
                        self.saved_revision = revision;
                        return Task::batch([self.persist(), self.continue_close()]);
                    }
                    Err(error) => {
                        self.close_requested = false;
                        self.notice(format!("Changes are not saved: {error}"), true);
                    }
                }
            }
            Message::ToggleOverflow => self.overflow_open = !self.overflow_open,
            Message::CloseOverflow => self.overflow_open = false,
        }
        Task::none()
    }

    fn event(&mut self, event: iced::Event, status: iced::event::Status) -> Task<Message> {
        if let iced::Event::Window(iced::window::Event::Resized(_)) = event {
            return self.schedule_thumbnails();
        }
        let iced::Event::Keyboard(event) = event else {
            return Task::none();
        };
        if status == iced::event::Status::Captured
            || self.dialog.is_some()
            || self.chooser.is_pending()
            || self.chooser_request.is_some()
            || self.context.is_open()
        {
            return Task::none();
        }
        let bindings = super::shortcuts::catalog(self);
        enzui_iced_core::action::dispatch_shortcut_command(&bindings, &event, &())
            .map_or_else(Task::none, |command| self.update(command))
    }

    fn open_dialog(&mut self, dialog: Dialog) -> Task<Message> {
        self.open_dialog_from(dialog, None)
    }

    fn open_dialog_from(
        &mut self,
        dialog: Dialog,
        invoker: Option<iced::advanced::widget::Id>,
    ) -> Task<Message> {
        let dialog = if matches!(dialog, Dialog::Help) {
            self.settings_page = "shortcuts".into();
            self.settings_page_open = true;
            Dialog::Settings
        } else {
            dialog
        };
        let dialog = if matches!(dialog, Dialog::Export | Dialog::CleanMetadata)
            && self
                .selected_photo()
                .is_some_and(|p| photos_library::scan::is_video_path(&p.path))
        {
            Dialog::VideoEdit
        } else {
            dialog
        };
        if matches!(dialog, Dialog::VideoEdit) {
            return self.open_video_editor();
        }
        if matches!(dialog, Dialog::Settings) {
            self.settings_menu = None;
        }
        self.dialog_text = match dialog {
            Dialog::RenameAlbum(id) => self
                .catalog
                .albums
                .iter()
                .find(|album| album.id == id)
                .map_or_else(String::new, |album| album.name.clone()),
            Dialog::VideoEdit => self.selected_photo().map_or_else(String::new, |p| {
                p.path
                    .with_file_name(format!(
                        "{}-edited.mp4",
                        p.path.file_stem().unwrap_or_default().to_string_lossy()
                    ))
                    .to_string_lossy()
                    .into_owned()
            }),
            Dialog::Export | Dialog::CleanMetadata => {
                self.selected_photo().map_or_else(String::new, |p| {
                    self.paths
                        .photos
                        .join(format!(
                            "{}-{}.png",
                            p.path.file_stem().unwrap_or_default().to_string_lossy(),
                            if matches!(dialog, Dialog::CleanMetadata) {
                                "clean"
                            } else {
                                "edited"
                            }
                        ))
                        .to_string_lossy()
                        .into_owned()
                })
            }
            _ => String::new(),
        };
        if matches!(dialog, Dialog::Export) {
            self.dialog_text = PathBuf::from(&self.dialog_text)
                .with_extension(self.export_options.format.extension())
                .to_string_lossy()
                .into_owned();
        }
        self.cancel_chooser();
        let settings = matches!(dialog, Dialog::Settings);
        self.overflow_open = false;
        let focus = if let Some(previous) = self.dialog.take() {
            self.dialogs.hand_off(&previous, dialog.clone())
        } else {
            self.dialogs.open(dialog.clone(), invoker)
        };
        self.dialog = Some(dialog);
        if settings {
            Task::batch([focus, self.refresh_thumbnail_cache()])
        } else {
            focus
        }
    }

    pub(super) fn dismiss_dialog(&mut self) -> Task<Message> {
        let settings = matches!(self.dialog, Some(Dialog::Settings));
        if settings {
            self.cloud.settings_closed();
            self.settings_menu = None;
        }
        self.shortcut_capture = None;
        self.shortcut_feedback = None;
        self.cancel_chooser();
        self.pending_navigation = None;
        let focus = self.dialog.take().map_or_else(Task::none, |dialog| {
            self.dialogs.dismiss(&dialog, DismissCause::CancelAction).1
        });
        if settings {
            Task::batch([focus, self.resume_cloud_library()])
        } else {
            focus
        }
    }

    fn request_close(&mut self) -> Task<Message> {
        if self.cloud.busy && !self.cloud.cancellable() {
            self.notice(
                "Wait for the system keyring request before closing Photos.",
                false,
            );
            return Task::none();
        }
        if self.cloud.busy {
            self.close_requested = true;
            return self.cloud_event(super::cloud::Event::Cancel);
        }
        self.cloud.sync.cancel();
        self.cloud.retire();
        self.cancel_chooser();
        if self
            .viewer
            .as_ref()
            .is_some_and(|viewer| viewer.editing && viewer.recipe != viewer.before_edit)
        {
            return self.open_dialog(Dialog::CloseEdits);
        }
        self.close_requested = true;
        self.continue_close()
    }

    pub(super) fn continue_close(&mut self) -> Task<Message> {
        if !self.close_requested {
            return Task::none();
        }
        if self.busy || self.save_inflight || self.cloud.busy || self.cloud.sync.running.is_some() {
            self.notice("Finishing photo work before closing…", false);
            return Task::none();
        }
        if self.revision != self.saved_revision {
            if self.persistence_blocked {
                self.close_requested = false;
                self.notice("The catalog is unavailable. Changes cannot be saved; keep Photos open to avoid losing them.", true);
                return Task::none();
            }
            return self.persist();
        }
        self.cancel_chooser();
        self.retire_viewer();
        iced::exit()
    }

    pub(super) fn cancel_chooser(&mut self) {
        self.chooser_generation = self.chooser_generation.saturating_add(1);
        self.chooser_request = None;
        if let Some(cancel) = self.chooser_cancel.take() {
            cancel.cancel(enzui_iced_widgets::file_chooser::CancelReason::Application);
        }
        let _ = self
            .chooser
            .cancel(enzui_iced_widgets::file_chooser::CancelReason::Application);
    }

    fn pick(&mut self, purpose: PickPurpose) -> Task<Message> {
        if self.chooser.is_pending() || self.chooser_request.is_some() {
            return Task::none();
        }
        let (operation, title) = match purpose {
            PickPurpose::Import => (
                PortalOperation::OpenDirectory,
                "Import photos and videos from a folder",
            ),
            PickPurpose::Source => (PortalOperation::OpenDirectory, "Add a photo source folder"),
            PickPurpose::ImportDestination => (
                PortalOperation::OpenDirectory,
                "Choose where imported photos are saved",
            ),
            PickPurpose::VideoExport => (PortalOperation::SaveFile, "Save a video copy"),
            PickPurpose::RemoteCopy => (PortalOperation::SaveFile, "Save an original copy"),
            PickPurpose::Export | PickPurpose::Clean => {
                (PortalOperation::SaveFile, "Save a photo copy")
            }
        };
        let mut options = ChooserOptions::new().title(title);
        if matches!(
            purpose,
            PickPurpose::Export
                | PickPurpose::Clean
                | PickPurpose::VideoExport
                | PickPurpose::RemoteCopy
        ) {
            options = options.current_name(
                PathBuf::from(&self.dialog_text)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("photo-copy.png"),
            );
        }
        self.chooser_generation = self.chooser_generation.saturating_add(1);
        let request = super::state::PickRequest {
            purpose,
            generation: self.chooser_generation,
            load_generation: self.load_generation,
            service_generation: self.cloud.generation,
        };
        self.prepare_chooser(request, operation, options)
    }

    fn folder_chosen(&mut self, purpose: PickPurpose, path: PathBuf) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        if !path.is_absolute() {
            self.notice("Enter an absolute local path.", true);
            return Task::none();
        }
        match purpose {
            PickPurpose::RemoteCopy => self.cloud_event(super::cloud::Event::SaveCopy(path)),
            PickPurpose::VideoExport => self.export_video(path),
            PickPurpose::ImportDestination => {
                self.catalog.settings.import_destination = Some(path);
                self.changed()
            }
            PickPurpose::Source => {
                self.busy = true;
                let work = self.workers.submit(0, move |_| {
                    crate::services::validate_source(&path).map_err(|e| e.to_string())
                });
                Task::perform(work, |settled| {
                    Message::SourceValidated(
                        settled
                            .into_result()
                            .map_err(|e| e.to_string())
                            .and_then(|r| r),
                    )
                })
            }
            PickPurpose::Import => {
                self.busy = true;
                self.import_generation = self.import_generation.saturating_add(1);
                let generation = self.import_generation;
                let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
                self.import_cancel = Some(Arc::clone(&cancel));
                self.import_progress_hidden = false;
                self.import_progress = Some(crate::services::import::ImportProgress {
                    completed: 0,
                    total: 0,
                });
                let (progress, receiver, _) =
                    enzui_iced_core::worker_bridge::channel(std::num::NonZeroUsize::MIN);
                let destination = self.import_destination().to_owned();
                let layout = self.catalog.settings.import_layout;
                let work = self.workers.submit(generation, move |context| {
                    crate::services::import::from_folder_organized(
                        &path,
                        &destination,
                        layout,
                        || {
                            context.is_cancelled()
                                || cancel.load(std::sync::atomic::Ordering::Acquire)
                        },
                        |update| {
                            let _ = progress.send_progress(update);
                        },
                    )
                    .map(Arc::new)
                    .map_err(|e| e.to_string())
                });
                Task::batch([
                    self.dismiss_dialog(),
                    Task::run(receiver, move |update| {
                        Message::ImportProgress(generation, update)
                    }),
                    Task::perform(work, move |settled| {
                        Message::ImportFinished(
                            generation,
                            settled
                                .into_result()
                                .map_err(|e| e.to_string())
                                .and_then(|r| r),
                        )
                    }),
                ])
            }
            PickPurpose::Export | PickPurpose::Clean => {
                let Some(viewer) = self.viewer.as_ref() else {
                    return Task::none();
                };
                let Some(source) = viewer.loaded.clone() else {
                    return Task::none();
                };
                let recipe = if purpose == PickPurpose::Clean {
                    EditRecipe::default()
                } else {
                    viewer.recipe.clone()
                };
                let mut options = self.export_options;
                if purpose == PickPurpose::Clean {
                    options = Default::default();
                }
                let path = if purpose == PickPurpose::Export {
                    path.with_extension(options.format.extension())
                } else {
                    path
                };
                self.busy = true;
                let work = self.workers.submit(0, move |context| {
                    crate::services::export_with_metadata(&source, &recipe, &path, options, || {
                        context.is_cancelled()
                    })
                    .map_err(|e| e.to_string())
                });
                Task::batch([
                    self.dismiss_dialog(),
                    Task::perform(work, |settled| {
                        Message::ExportFinished(
                            settled
                                .into_result()
                                .map_err(|e| e.to_string())
                                .and_then(|r| r),
                        )
                    }),
                ])
            }
        }
    }
    pub(crate) fn import_destination(&self) -> &std::path::Path {
        self.catalog
            .settings
            .import_destination
            .as_deref()
            .unwrap_or(&self.paths.photos)
    }

    fn gallery_settings_changed(&mut self) -> Task<Message> {
        self.settings_menu = None;
        self.refresh_projection();
        Task::batch([
            self.changed(),
            self.reset_gallery_scroll(),
            self.schedule_thumbnails(),
        ])
    }
}
