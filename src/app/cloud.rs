use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use enzui_iced_core::blocking_task::BlockingTaskOwner;
use enzui_services::credential_store::{CredentialKey, CredentialOutcome, Secret, connect_for_app};
use iced::Task;
use iced::widget::image::Handle;
use photos_library::catalog::{Photo, PhotoId, ServiceAccount, ServiceProvider};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{Message, Photos};
use crate::services::remote::{self, ApiKey, Asset, PhotoService, RemoteAlbum, SearchKind};

#[derive(Debug, Clone)]
pub(crate) enum Event {
    Owned(u64, Box<Event>),
    Configure,
    Section(SettingsSection),
    Backup(Option<u64>),
    CloseUpload,
    BackupServer(u64),
    Thumbnails(u64, Vec<(String, Result<Handle, String>)>),
    ClearQuery,
    SearchClose,
    Activate(String, bool),
    ClearSelection,
    Step(i32),
    Transform(enzui_iced_core::zoom_pan::ViewTransform),
    Close,
    New,
    Open(u64),
    Name(String),
    Url(String),
    Key(ApiKey),
    Connect,
    EditKey(u64),
    CancelForm,
    ForgetAccount(u64),
    FocusSearch,
    Back,
    Refresh,
    Query(String),
    RetryMore,
    RetryPreviews,
    Window(usize, usize),
    SearchKind(SearchKind),
    MoreDone(u64, String, Result<remote::Page, String>),
    Select(String),
    SelectAll,
    Albums,
    Album(Option<RemoteAlbum>),
    ImportAlbum,
    ExportAlbum(u64),
    Copy,
    ClipboardDone(u64, Result<(), String>),
    SaveCopy(PathBuf),
    Sync(super::cloud_sync::Event),
    Preview(Option<String>),
    Import,
    Upload,
    Cancel,
    Done(u64, Result<Outcome, String>),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    #[default]
    Connection,
    Upload,
    Download,
}

#[derive(Debug, Clone)]
pub(crate) enum Outcome {
    Key(ServiceAccount, Option<ApiKey>),
    Connected(ServiceAccount, ApiKey, Arc<dyn PhotoService>, String),
    Remembered(bool),
    Preview(String, Handle),
    Albums(Vec<RemoteAlbum>, BTreeMap<String, Handle>),
    AlbumImported(String, Vec<PathBuf>, Option<String>),
    Video(Arc<tempfile::TempDir>, PathBuf),
    Page(RenderedPage),
    Imported(Vec<PathBuf>, Option<String>),
    Uploaded(usize, usize, Option<String>),
    Copied(iced::clipboard::Image),
    Saved(PathBuf),
    Forgotten(u64, bool),
}

#[derive(Debug, Clone)]
pub(crate) struct RenderedPage {
    pub assets: Vec<(Asset, Option<Handle>)>,
    pub next: Option<String>,
    pub smart_search: bool,
}

pub(crate) struct ConnectionForm {
    pub account: Option<u64>,
    pub name: String,
    pub url: String,
    pub key: ApiKey,
}

pub(crate) struct State {
    pub open: bool,
    pub form: Option<ConnectionForm>,
    pub section: SettingsSection,
    pub backup_album: Option<u64>,
    pub backup_setup: bool,
    pub thumbnails_loading: bool,
    pub failed_previews: BTreeSet<String>,
    pub preview_error: Option<String>,
    thumbnail_cache: BTreeMap<(u64, String), Handle>,
    thumbnail_order: VecDeque<(u64, String)>,
    thumbnail_attempted: BTreeSet<String>,
    pub search_open: bool,
    pub preview_image: Option<(String, Handle)>,
    pub transform: enzui_iced_core::zoom_pan::ViewTransform,
    pub playback_service: bool,
    pub albums: Option<Vec<RemoteAlbum>>,
    pub album_covers: BTreeMap<String, Handle>,
    pub album: Option<RemoteAlbum>,
    pub sync: super::cloud_sync::State,
    key_edit_target: Option<u64>,
    playback: Option<Arc<tempfile::TempDir>>,
    pub active: Option<u64>,
    pub name: String,
    pub url: String,
    pub key: ApiKey,
    pub busy: bool,
    pub status: String,
    pub page: Option<RenderedPage>,
    pub query: String,
    pub selection: BTreeSet<String>,
    pub preview: Option<String>,
    pub upload_paths: Vec<Photo>,
    pub window: std::ops::Range<usize>,
    pub scroll: enzui_iced_widgets::scroll_view::ScrollTranslation,
    pub loading_more: bool,
    pub pending_step: bool,
    pub load_error: Option<String>,
    pub search_kind: SearchKind,
    pub smart_search: bool,
    applied_query: String,
    applied_kind: SearchKind,
    seen_cursors: BTreeSet<String>,
    seen_assets: BTreeSet<String>,
    pub generation: u64,
    pub(super) clients: BTreeMap<u64, Arc<dyn PhotoService>>,
    owner: BlockingTaskOwner,
    cancel: Option<Arc<AtomicBool>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            open: false,
            form: None,
            section: SettingsSection::Connection,
            backup_album: None,
            backup_setup: false,
            thumbnails_loading: false,
            failed_previews: BTreeSet::new(),
            preview_error: None,
            thumbnail_cache: BTreeMap::new(),
            thumbnail_order: VecDeque::new(),
            thumbnail_attempted: BTreeSet::new(),
            search_open: false,
            preview_image: None,
            transform: enzui_iced_core::zoom_pan::ViewTransform::IDENTITY,
            playback_service: false,
            albums: None,
            album_covers: BTreeMap::new(),
            album: None,
            sync: super::cloud_sync::State::new(),
            key_edit_target: None,
            playback: None,
            active: None,
            name: "My Immich".into(),
            url: String::new(),
            key: ApiKey::new(String::new()),
            busy: false,
            status: String::new(),
            page: None,
            query: String::new(),
            selection: BTreeSet::new(),
            preview: None,
            upload_paths: Vec::new(),
            window: 0..48,
            scroll: Default::default(),
            loading_more: false,
            pending_step: false,
            load_error: None,
            search_kind: SearchKind::Filename,
            smart_search: false,
            applied_query: String::new(),
            applied_kind: SearchKind::Filename,
            seen_cursors: BTreeSet::new(),
            seen_assets: BTreeSet::new(),
            generation: 0,
            clients: BTreeMap::new(),
            owner: BlockingTaskOwner::new(
                "photos-services",
                NonZeroUsize::MIN,
                NonZeroUsize::new(4).expect("four requests"),
            )
            .unwrap_or_else(|error| {
                tracing::error!(%error,"cannot start service worker");
                std::process::exit(1)
            }),
            cancel: None,
        }
    }

    pub fn retire(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Release);
        }
        self.generation = self.generation.wrapping_add(1);
        self.busy = false;
        self.thumbnails_loading = false;
        if self.loading_more
            && let Some(cursor) = self.page.as_ref().and_then(|page| page.next.as_ref())
        {
            self.seen_cursors.remove(cursor);
        }
        self.loading_more = false;
        self.pending_step = false;
        self.thumbnail_attempted.clear();
        self.failed_previews.clear();
        self.preview_error = None;
    }

    pub fn settings_closed(&mut self) {
        self.sync.close_form();
        self.form = None;
        self.backup_setup = false;
    }

    pub fn connected(&self) -> bool {
        self.active.is_some_and(|id| self.clients.contains_key(&id))
    }

    pub fn cancellable(&self) -> bool {
        self.cancel.is_some()
    }
}

pub(super) fn credential(catalog: &Path, account: &ServiceAccount) -> CredentialKey {
    CredentialKey::new(
        "org.enzimos.Photos",
        format!(
            "photo-service-{}-{}-{}",
            PhotoId::for_path(catalog).0,
            account.id,
            account.url
        ),
        "api-key",
    )
}

fn scroll_top() -> Task<Message> {
    iced::widget::operation::scroll_to(
        iced::widget::Id::new("photos-service-scroll"),
        iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
        iced::advanced::widget::operation::Animation::Instant,
    )
}

fn watch(
    work: enzui_iced_core::blocking_task::BlockingTask<Result<Outcome, String>>,
    generation: u64,
) -> Task<Message> {
    Task::batch([watch_result(work, generation), scroll_top()])
}

fn watch_result(
    work: enzui_iced_core::blocking_task::BlockingTask<Result<Outcome, String>>,
    generation: u64,
) -> Task<Message> {
    Task::perform(work, move |settled| {
        Message::Cloud(Event::Done(
            generation,
            settled
                .into_result()
                .map_err(|e| e.to_string())
                .and_then(|r| r),
        ))
    })
}

fn remote_thumbnail(
    client: &dyn PhotoService,
    id: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> anyhow::Result<Handle> {
    let bytes = client.thumbnail(id, cancelled)?;
    let limits = enzui_iced_core::decode_limits::DecodeLimits {
        max_encoded_bytes: 8 * 1024 * 1024,
        max_width: 8192,
        max_height: 8192,
        max_pixels: 32_000_000,
        max_rgba_bytes: 128_000_000,
    };
    Ok(enzui_iced_resources::decode_static_resized(&bytes, &limits, 512, 512, cancelled)?.handle)
}

impl Photos {
    fn local_backup_selection(&self) -> Vec<Photo> {
        if let Some(photo) = self.selected_photo() {
            vec![photo.clone()]
        } else {
            self.library
                .photos
                .iter()
                .filter(|photo| self.selection.contains(&photo.id))
                .cloned()
                .collect()
        }
    }

    fn cloud_thumbnails(&mut self) -> Task<Message> {
        let Some(account) = self.cloud.active else {
            return Task::none();
        };
        let Some(client) = self.cloud.clients.get(&account).cloned() else {
            return Task::none();
        };
        if !self.cloud.open
            || self.cloud.preview.is_some()
            || self.cloud.thumbnails_loading
            || self.cloud.busy
        {
            return Task::none();
        }
        let mut pending = Vec::new();
        if let Some(page) = &mut self.cloud.page {
            for (asset, thumbnail) in page
                .assets
                .iter_mut()
                .skip(self.cloud.window.start)
                .take(self.cloud.window.len())
            {
                if thumbnail.is_none() {
                    *thumbnail = self
                        .cloud
                        .thumbnail_cache
                        .get(&(account, asset.id.clone()))
                        .cloned();
                }
                if thumbnail.is_none() && !self.cloud.thumbnail_attempted.contains(&asset.id) {
                    pending.push(asset.id.clone());
                }
            }
        }
        if let Some(albums) = &self.cloud.albums {
            for album in albums
                .iter()
                .skip(self.cloud.window.start)
                .take(self.cloud.window.len())
            {
                if let Some(id) = &album.cover {
                    if let Some(handle) = self.cloud.thumbnail_cache.get(&(account, id.clone())) {
                        self.cloud
                            .album_covers
                            .insert(album.id.clone(), handle.clone());
                    } else if !self.cloud.thumbnail_attempted.contains(id) && !pending.contains(id)
                    {
                        pending.push(id.clone());
                    }
                }
            }
        }
        let ids: Vec<_> = pending.into_iter().take(4).collect();
        if ids.is_empty() {
            return Task::none();
        }
        self.cloud.thumbnail_attempted.extend(ids.iter().cloned());
        self.cloud.thumbnails_loading = true;
        let generation = self.cloud.generation;
        let flag = self
            .cloud
            .cancel
            .get_or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone();
        let work = self.cloud.owner.submit(generation, move |context| {
            let cancelled = || context.is_cancelled() || flag.load(Ordering::Acquire);
            std::thread::scope(|scope| {
                let requests: Vec<_> = ids
                    .into_iter()
                    .map(|id| {
                        let client = &client;
                        let cancelled = &cancelled;
                        scope.spawn(move || {
                            let result = remote_thumbnail(client.as_ref(), &id, cancelled)
                                .map_err(|e| e.to_string());
                            (id, result)
                        })
                    })
                    .collect();
                requests
                    .into_iter()
                    .map(|request| {
                        request.join().unwrap_or_else(|_| {
                            (String::new(), Err("Thumbnail worker stopped".into()))
                        })
                    })
                    .collect::<Vec<_>>()
            })
        });
        Task::perform(work, move |settled| {
            Message::Cloud(Event::Thumbnails(
                generation,
                settled
                    .into_result()
                    .unwrap_or_else(|error| vec![(String::new(), Err(error.to_string()))]),
            ))
        })
    }

    pub(super) fn resume_cloud_library(&mut self) -> Task<Message> {
        Task::batch([self.cloud_thumbnails(), self.cloud_more()])
    }

    fn cloud_more(&mut self) -> Task<Message> {
        if !(self.cloud.open || self.cloud.playback_service)
            || self.cloud.busy
            || self.cloud.loading_more
            || self.cloud.load_error.is_some()
            || (self.cloud.preview.is_some() && !self.cloud.pending_step)
            || self.cloud.albums.is_some()
        {
            return Task::none();
        }
        let Some(page) = &self.cloud.page else {
            return Task::none();
        };
        if page.assets.len() > self.cloud.window.end.saturating_add(12) {
            return Task::none();
        }
        let Some(cursor) = page.next.clone() else {
            return Task::none();
        };
        if cursor.len() > 4096 || !self.cloud.seen_cursors.insert(cursor.clone()) {
            self.cloud.load_error = Some(
                "The server returned an invalid continuation. Refresh the library to retry.".into(),
            );
            return Task::none();
        }
        let Some(client) = self
            .cloud
            .active
            .and_then(|id| self.cloud.clients.get(&id))
            .cloned()
        else {
            return Task::none();
        };
        self.cloud.loading_more = true;
        let query = self.cloud.applied_query.clone();
        let kind = self.cloud.applied_kind;
        let album = self.cloud.album.as_ref().map(|a| a.id.clone());
        let flag = self
            .cloud
            .cancel
            .get_or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone();
        let generation = self.cloud.generation;
        let requested = cursor.clone();
        let work = self.cloud.owner.submit(generation, move |context| {
            client
                .search(Some(&requested), &query, album.as_deref(), kind, &|| {
                    context.is_cancelled() || flag.load(Ordering::Acquire)
                })
                .map_err(|e| e.to_string())
        });
        Task::perform(work, move |settled| {
            Message::Cloud(Event::MoreDone(
                generation,
                cursor,
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            ))
        })
    }

    fn cloud_page(&mut self) -> Task<Message> {
        let Some(client) = self
            .cloud
            .active
            .and_then(|id| self.cloud.clients.get(&id))
            .cloned()
        else {
            return Task::none();
        };
        self.cloud.retire();
        self.cloud.busy = true;
        self.cloud.page = None;
        self.cloud.albums = None;
        self.cloud.selection.clear();
        self.cloud.preview = None;
        self.cloud.preview_image = None;
        self.cloud.status = "Loading photos…".into();
        let generation = self.cloud.generation;
        self.cloud.window = 0..48;
        self.cloud.scroll.set(iced::Vector::ZERO);
        self.cloud.load_error = None;
        self.cloud.seen_cursors.clear();
        self.cloud.seen_assets.clear();
        self.cloud.applied_query.clone_from(&self.cloud.query);
        self.cloud.applied_kind = self.cloud.search_kind;
        let query = self.cloud.applied_query.clone();
        let kind = self.cloud.applied_kind;
        let album = self.cloud.album.as_ref().map(|a| a.id.clone());
        let cancelled_flag = Arc::new(AtomicBool::new(false));
        self.cloud.cancel = Some(Arc::clone(&cancelled_flag));
        let work = self.cloud.owner.submit(generation, move |context| {
            let cancelled = || context.is_cancelled() || cancelled_flag.load(Ordering::Acquire);
            let smart = client.smart_search_available(&cancelled).map_err(|e| e.to_string())?;
            if kind == SearchKind::Smart && !query.trim().is_empty() && !smart {
                return Err("Immich Smart Search is disabled. Switch to filename search or enable Smart Search on your server.".into());
            }
            let page = client.search(None, &query, album.as_deref(), kind, &cancelled).map_err(|e| e.to_string())?;
            Ok(Outcome::Page(RenderedPage {
                assets: page.assets.into_iter().map(|asset| (asset, None)).collect(),
                next: page.next,
                smart_search: smart,
            }))
        });

        watch(work, generation)
    }

    pub(crate) fn cloud_event(&mut self, event: Event) -> Task<Message> {
        let event = match event {
            Event::Owned(generation, event) if generation == self.cloud.generation => *event,
            Event::Owned(..) => return Task::none(),
            event => event,
        };
        if let Event::Sync(event) = event {
            return self.cloud_sync_event(event);
        }
        if self.cloud.busy
            && !matches!(
                event,
                Event::Done(..)
                    | Event::Thumbnails(..)
                    | Event::MoreDone(..)
                    | Event::Window(..)
                    | Event::Cancel
            )
            && !(self.cloud.preview.is_some()
                && matches!(event, Event::Back | Event::Step(_) | Event::Transform(_)))
        {
            return Task::none();
        }
        match event {
            Event::Owned(..) | Event::Sync(_) => return Task::none(),
            Event::Thumbnails(generation, results) => {
                if generation != self.cloud.generation {
                    return Task::none();
                }
                self.cloud.thumbnails_loading = false;
                let Some(account) = self.cloud.active else {
                    return Task::none();
                };
                for (id, result) in results {
                    match result {
                        Ok(handle) => {
                            let key = (account, id);
                            self.cloud.thumbnail_order.retain(|entry| entry != &key);
                            self.cloud.thumbnail_order.push_back(key.clone());
                            self.cloud.thumbnail_cache.insert(key, handle);
                            while self.cloud.thumbnail_order.len() > 96 {
                                if let Some(oldest) = self.cloud.thumbnail_order.pop_front() {
                                    self.cloud.thumbnail_cache.remove(&oldest);
                                }
                            }
                        }
                        Err(error) => {
                            self.cloud.failed_previews.insert(id);
                            self.cloud.preview_error.get_or_insert(error);
                        }
                    }
                }
                return self.cloud_thumbnails();
            }
            Event::Window(start, end) => {
                let old = self.cloud.window.clone();
                self.cloud.window = start..end.max(start);
                if let Some(page) = &mut self.cloud.page {
                    for (_, image) in page
                        .assets
                        .iter_mut()
                        .skip(old.start)
                        .take(old.len())
                        .enumerate()
                        .filter_map(|(offset, item)| {
                            (!self.cloud.window.contains(&(old.start + offset))).then_some(item)
                        })
                    {
                        *image = None;
                    }
                    self.cloud.thumbnail_attempted.retain(|id| {
                        page.assets
                            .iter()
                            .skip(start)
                            .take(end.saturating_sub(start))
                            .any(|(asset, _)| &asset.id == id)
                    });
                }
                if let Some(albums) = &self.cloud.albums {
                    for album in albums
                        .iter()
                        .skip(old.start)
                        .take(old.len())
                        .enumerate()
                        .filter_map(|(offset, album)| {
                            (!self.cloud.window.contains(&(old.start + offset))).then_some(album)
                        })
                    {
                        self.cloud.album_covers.remove(&album.id);
                    }
                }
                return Task::batch([self.cloud_thumbnails(), self.cloud_more()]);
            }
            Event::RetryPreviews => {
                let failed = std::mem::take(&mut self.cloud.failed_previews);
                self.cloud
                    .thumbnail_attempted
                    .retain(|id| !failed.contains(id));
                self.cloud.preview_error = None;
                return self.cloud_thumbnails();
            }
            Event::RetryMore => {
                self.cloud.load_error = None;
                return if self
                    .cloud
                    .page
                    .as_ref()
                    .is_some_and(|page| page.next.is_none())
                {
                    self.cloud_page()
                } else {
                    self.cloud_more()
                };
            }
            Event::SearchKind(kind) => {
                if kind == SearchKind::Smart && !self.cloud.smart_search {
                    return Task::none();
                }
                self.cloud.search_kind = kind;
                return self.cloud_page();
            }
            Event::MoreDone(generation, cursor, result) => {
                if generation != self.cloud.generation
                    || !self.cloud.loading_more
                    || self.cloud.page.as_ref().and_then(|page| page.next.as_ref()) != Some(&cursor)
                {
                    return Task::none();
                }
                self.cloud.loading_more = false;
                match result {
                    Ok(batch) => {
                        let Some(page) = &mut self.cloud.page else {
                            return Task::none();
                        };
                        let before = page.assets.len();
                        page.assets.extend(
                            batch
                                .assets
                                .into_iter()
                                .filter(|asset| self.cloud.seen_assets.insert(asset.id.clone()))
                                .map(|asset| (asset, None)),
                        );
                        if batch.next.as_ref().is_some_and(|next| {
                            next == &cursor || self.cloud.seen_cursors.contains(next)
                        }) {
                            self.cloud.load_error = Some("The server repeated its continuation. Refresh the library to retry.".into());
                            page.next = None;
                        } else {
                            page.next = batch.next;
                            if page.assets.len() == before && page.next.is_some() {
                                self.cloud.load_error = Some("The server returned no new media. Retry loading or refresh the library.".into());
                            }
                        }
                    }
                    Err(error) => {
                        self.cloud.seen_cursors.remove(&cursor);
                        self.cloud.load_error = Some(error);
                    }
                }
                if self.cloud.pending_step {
                    self.cloud.pending_step = false;
                    if let Some(error) = self.cloud.load_error.clone() {
                        self.notice(error, true);
                        return Task::none();
                    }
                    return self.cloud_event(Event::Step(1));
                }
                return Task::batch([self.cloud_thumbnails(), self.cloud_more()]);
            }
            Event::Section(section) => {
                self.cloud.sync.close_form();
                self.cloud.backup_setup = false;
                self.settings_menu = None;
                self.cloud.section = section;
            }
            Event::CloseUpload => self.cloud.backup_setup = false,
            Event::Backup(album) => {
                self.settings_menu = None;
                self.cloud.album = None;
                self.cloud.backup_album = album;
                self.cloud.backup_setup = true;
                self.cloud.upload_paths = self.local_backup_selection();
                self.cloud.section = SettingsSection::Upload;
                self.cloud.status.clear();
                self.settings_page = "services".into();
                self.settings_page_open = true;
                self.context.close();
                return if matches!(self.dialog, Some(super::Dialog::Settings)) {
                    Task::none()
                } else {
                    self.update(Message::Dialog(super::Dialog::Settings))
                };
            }
            Event::Albums => {
                let Some(client) = self
                    .cloud
                    .active
                    .and_then(|id| self.cloud.clients.get(&id))
                    .cloned()
                else {
                    return Task::none();
                };
                self.cloud.retire();
                self.cloud.busy = true;
                self.cloud.window = 0..48;
                self.cloud.scroll.set(iced::Vector::ZERO);
                self.cloud.album = None;
                self.cloud.page = None;
                self.cloud.status = "Loading albums…".into();
                let generation = self.cloud.generation;
                let flag = Arc::new(AtomicBool::new(false));
                self.cloud.cancel = Some(flag.clone());
                let work = self.cloud.owner.submit(generation, move |context| {
                    let cancelled = || context.is_cancelled() || flag.load(Ordering::Acquire);
                    let albums = client.albums(&cancelled).map_err(|e| e.to_string())?;
                    Ok(Outcome::Albums(albums, BTreeMap::new()))
                });
                return watch(work, generation);
            }
            Event::Album(album) => {
                self.cloud.album = album;
                self.cloud.albums = None;
                self.cloud.query.clear();
                return self.cloud_page();
            }
            Event::ExportAlbum(id) => {
                if self.catalog_authority.is_none() || self.persistence_blocked {
                    return Task::none();
                }
                let Some(album) = self.catalog.albums.iter().find(|a| a.id == id) else {
                    return Task::none();
                };
                let paths: Vec<_> = self
                    .library
                    .photos
                    .iter()
                    .filter(|p| album.photos.contains(&p.id))
                    .cloned()
                    .collect();
                if paths.len() != album.photos.len() || paths.is_empty() {
                    self.cloud.status =
                        "Some album originals are unavailable; rescan before exporting.".into();
                    return Task::none();
                }
                let Some(client) = self
                    .cloud
                    .active
                    .and_then(|id| self.cloud.clients.get(&id))
                    .cloned()
                else {
                    return Task::none();
                };
                let name = album.name.clone();
                self.cloud.retire();
                self.cloud.busy = true;
                self.cloud.status = "Exporting a new remote album…".into();
                let generation = self.cloud.generation;
                let flag = Arc::new(AtomicBool::new(false));
                self.cloud.cancel = Some(flag.clone());
                let work = self.cloud.owner.submit(generation, move |context| {
                    let cancelled = || context.is_cancelled() || flag.load(Ordering::Acquire);
                    let album = client
                        .create_album(&name, &cancelled)
                        .map_err(|e| e.to_string())?;
                    let (mut created, mut duplicates, mut error) = (0, 0, None);
                    for path in paths {
                        match client.upload_into(&path, Some(&album.id), &cancelled) {
                            Ok(true) => created += 1,
                            Ok(false) => duplicates += 1,
                            Err(e) => {
                                error = Some(format!(
                                    "Album {} retains completed media. {e}",
                                    album.name
                                ));
                                break;
                            }
                        }
                    }
                    Ok(Outcome::Uploaded(created, duplicates, error))
                });
                return watch(work, generation);
            }
            Event::ImportAlbum => {
                let Some(album) = self.cloud.album.clone() else {
                    return Task::none();
                };
                let Some(client) = self
                    .cloud
                    .active
                    .and_then(|id| self.cloud.clients.get(&id))
                    .cloned()
                else {
                    return Task::none();
                };
                if self.catalog_authority.is_none() || self.persistence_blocked {
                    return Task::none();
                }
                let destination = self.import_destination().to_owned();
                let layout = self.catalog.settings.import_layout;
                self.cloud.retire();
                self.cloud.busy = true;
                self.cloud.status = "Importing album originals…".into();
                let generation = self.cloud.generation;
                let flag = Arc::new(AtomicBool::new(false));
                self.cloud.cancel = Some(flag.clone());
                let work = self.cloud.owner.submit(generation, move |context| {
                    let cancelled = || context.is_cancelled() || flag.load(Ordering::Acquire);
                    let (mut paths, mut cursor, mut seen, mut error) =
                        (Vec::new(), None, BTreeSet::new(), None);
                    loop {
                        let result = client
                            .list_album(&album.id, cursor.as_deref(), "", &cancelled)
                            .and_then(|page| {
                                let report = remote::import_batch(
                                    client.as_ref(),
                                    &page.assets,
                                    &destination,
                                    layout,
                                    &album.name,
                                    &cancelled,
                                )?;
                                paths.extend(report.paths);
                                anyhow::ensure!(
                                    report.errors.is_empty() && !report.cancelled,
                                    "{}",
                                    report
                                        .errors
                                        .first()
                                        .map(String::as_str)
                                        .unwrap_or("Transfer cancelled")
                                );
                                anyhow::ensure!(
                                    paths.len() <= 100_000,
                                    "Album media limit reached"
                                );
                                Ok(page.next)
                            });
                        match result {
                            Ok(Some(next)) if seen.insert(next.clone()) => cursor = Some(next),
                            Ok(None) => break,
                            Ok(Some(_)) => {
                                error = Some("Server repeated an album page".into());
                                break;
                            }
                            Err(e) => {
                                error = Some(e.to_string());
                                break;
                            }
                        }
                    }
                    Ok(Outcome::AlbumImported(album.name, paths, error))
                });
                return watch(work, generation);
            }
            Event::ClearQuery => {
                self.cloud.query.clear();
                return self.cloud_event(Event::Refresh);
            }
            Event::Configure => {
                self.cloud.section = SettingsSection::Connection;
                if !self.cloud.open {
                    self.cloud.upload_paths = if let Some(photo) = self.selected_photo() {
                        vec![photo.clone()]
                    } else {
                        self.library
                            .photos
                            .iter()
                            .filter(|p| self.selection.contains(&p.id))
                            .cloned()
                            .collect()
                    };
                }
                self.settings_page = "services".into();
                self.settings_page_open = true;
                return self.update(Message::Dialog(super::Dialog::Settings));
            }
            Event::SearchClose => self.cloud.search_open = false,
            Event::Transform(transform) => self.cloud.transform = transform,
            Event::ClearSelection => self.cloud.selection.clear(),
            Event::Activate(id, toggle) => {
                return self.cloud_event(if toggle || !self.cloud.selection.is_empty() {
                    Event::Select(id)
                } else {
                    Event::Preview(Some(id))
                });
            }
            Event::Step(step) => {
                let Some(page) = &self.cloud.page else {
                    return Task::none();
                };
                let Some(position) = self
                    .cloud
                    .preview
                    .as_ref()
                    .and_then(|id| page.assets.iter().position(|(a, _)| &a.id == id))
                else {
                    return Task::none();
                };
                let next = position.saturating_add_signed(step as isize);
                if next == position {
                    return Task::none();
                }
                if let Some((asset, _)) = page.assets.get(next) {
                    let id = asset.id.clone();
                    self.cloud.retire();
                    if self.cloud.playback_service {
                        self.cloud.playback_service = false;
                        self.retire_viewer();
                        self.cloud.open = true;
                    }
                    return self.cloud_event(Event::Preview(Some(id)));
                }
                if step > 0 && page.next.is_some() {
                    if self.cloud.busy {
                        self.cloud.retire();
                    }
                    self.cloud.pending_step = true;
                    self.cloud.window = position..position + 1;
                    self.cloud.load_error = None;
                    return self.cloud_more();
                }
            }
            Event::Close => {
                self.cloud.retire();
                self.cloud.search_open = false;
                self.cloud.open = false;
            }
            Event::New => {
                self.cloud.retire();
                self.cloud.form = Some(ConnectionForm {
                    account: None,
                    name: "My Immich".into(),
                    url: String::new(),
                    key: ApiKey::new(String::new()),
                });
                self.cloud.section = SettingsSection::Connection;
                self.cloud.status.clear();
                self.nav.dismiss_overlay();
                return self.cloud_event(Event::Configure);
            }
            Event::EditKey(id) => {
                let Some(account) = self
                    .catalog
                    .services
                    .iter()
                    .find(|account| account.id == id)
                else {
                    return Task::none();
                };
                let form = ConnectionForm {
                    account: Some(id),
                    name: account.name.clone(),
                    url: account.url.clone(),
                    key: ApiKey::new(String::new()),
                };
                self.cloud.retire();
                self.cloud.form = Some(form);
                self.cloud.status.clear();
            }
            Event::CancelForm => {
                self.cloud.retire();
                self.cloud.form = None;
                self.cloud.status.clear();
                return Task::batch([self.cloud_thumbnails(), self.cloud_more()]);
            }
            Event::Name(value) => {
                if let Some(form) = &mut self.cloud.form {
                    form.name = value.chars().take(100).collect();
                }
            }
            Event::Url(value) => {
                if let Some(form) = &mut self.cloud.form {
                    form.url = value.chars().take(2048).collect();
                }
            }
            Event::Key(value) => {
                if let Some(form) = &mut self.cloud.form {
                    form.key = value;
                } else {
                    self.cloud.key = value;
                }
            }
            Event::Query(value) => self.cloud.query = value.chars().take(200).collect(),
            Event::Open(id) | Event::BackupServer(id) => {
                self.settings_menu = None;
                let backup = matches!(event, Event::BackupServer(_));
                self.cloud.backup_setup = backup;
                self.search_open = false;
                self.cloud.form = None;
                let Some(account) = self.catalog.services.iter().find(|a| a.id == id).cloned()
                else {
                    return Task::none();
                };
                if !backup && !self.cloud.open {
                    self.cloud.upload_paths = if let Some(photo) = self.selected_photo() {
                        vec![photo.clone()]
                    } else {
                        self.library
                            .photos
                            .iter()
                            .filter(|photo| self.selection.contains(&photo.id))
                            .cloned()
                            .collect()
                    };
                }
                self.cloud.open = !backup;
                self.cloud.retire();
                self.cloud.active = Some(id);
                self.cloud.album_covers.clear();
                self.cloud.album = None;
                self.cloud.albums = None;
                self.cloud.sync.close_form();
                self.cloud.name.clone_from(&account.name);
                self.cloud.url.clone_from(&account.url);
                self.cloud.key = ApiKey::new(String::new());
                self.cloud.query.clear();
                self.cloud.page = None;
                self.context.close();
                self.nav.dismiss_overlay();
                if self.cloud.clients.contains_key(&id) {
                    return if backup {
                        Task::none()
                    } else {
                        self.cloud_page()
                    };
                }
                self.cloud.busy = true;
                self.cloud.status = "Reading the saved API key…".into();
                let generation = self.cloud.generation;
                let credential = credential(&self.paths.catalog, &account);
                return Task::perform(
                    async move {
                        let store = connect_for_app("org.enzimos.Photos").await;
                        let key = match store.get(credential).await {
                            CredentialOutcome::Retrieved(secret) => Some(ApiKey::new(
                                String::from_utf8_lossy(secret.as_bytes()).into_owned(),
                            )),
                            _ => None,
                        };
                        Outcome::Key(account, key)
                    },
                    move |outcome| Message::Cloud(Event::Done(generation, Ok(outcome))),
                );
            }
            Event::Connect => {
                if self.catalog_authority.is_none() || self.persistence_blocked {
                    self.cloud.status =
                        "Wait for the local catalog to load before adding a service.".into();
                    return Task::none();
                }
                let url = match remote::normalized_url(
                    self.cloud
                        .form
                        .as_ref()
                        .map_or(self.cloud.url.as_str(), |form| form.url.as_str()),
                ) {
                    Ok(url) => url,
                    Err(error) => {
                        self.cloud.status = error.to_string();
                        return Task::none();
                    }
                };
                let name = self
                    .cloud
                    .form
                    .as_ref()
                    .map_or(self.cloud.name.as_str(), |form| form.name.as_str())
                    .trim()
                    .to_owned();
                if name.is_empty() || name.chars().any(char::is_control) {
                    self.cloud.status = "Enter a server name.".into();
                    return Task::none();
                }
                let next = self.catalog.next_service.max(
                    self.catalog
                        .services
                        .iter()
                        .map(|account| account.id.saturating_add(1))
                        .max()
                        .unwrap_or(1),
                );
                let id = self
                    .cloud
                    .form
                    .as_ref()
                    .map_or(self.cloud.active, |form| form.account)
                    .unwrap_or(next);
                if id == u64::MAX {
                    self.cloud.status = "Service identities exhausted.".into();
                    return Task::none();
                }
                let account = ServiceAccount {
                    id,
                    name,
                    provider: ServiceProvider::Immich,
                    url,
                };
                let key = self
                    .cloud
                    .form
                    .as_ref()
                    .map_or_else(|| self.cloud.key.clone(), |form| form.key.clone());
                self.cloud.key_edit_target = self.cloud.form.as_ref().and_then(|form| form.account);
                self.cloud.retire();
                self.cloud.busy = true;
                self.cloud.status = "Connecting to Immich…".into();
                let generation = self.cloud.generation;
                let cancelled_flag = Arc::new(AtomicBool::new(false));
                self.cloud.cancel = Some(Arc::clone(&cancelled_flag));
                let work = self.cloud.owner.submit(generation, move |context| {
                    let client =
                        remote::connect(&account, key.clone()).map_err(|e| e.to_string())?;
                    let user = client
                        .check(&|| context.is_cancelled() || cancelled_flag.load(Ordering::Acquire))
                        .map_err(|e| e.to_string())?;
                    Ok(Outcome::Connected(account, key, client, user))
                });

                return watch(work, generation);
            }
            Event::FocusSearch => {
                if !self.cloud.connected() {
                    return Task::none();
                }
                self.cloud.preview = None;
                self.cloud.search_open = true;
                return Task::batch([
                    scroll_top(),
                    iced::advanced::widget::operate(
                        iced::advanced::widget::operation::focusable::focus(iced::widget::Id::new(
                            "photos-service-search",
                        )),
                    ),
                ]);
            }
            Event::Back => {
                if self.cloud.search_open {
                    self.cloud.search_open = false;
                    return Task::none();
                }
                if self.cloud.preview.take().is_some() {
                    self.cloud.preview_image = None;
                    self.cloud.retire();
                    return Task::batch([scroll_top(), self.cloud_thumbnails()]);
                }
                if self.cloud.album.is_some() {
                    return self.cloud_event(Event::Albums);
                }
                if self.cloud.albums.is_some() {
                    return self.cloud_event(Event::Album(None));
                }
                if !self.cloud.selection.is_empty() {
                    self.cloud.selection.clear();
                    return Task::none();
                }
                return self.cloud_event(Event::Close);
            }
            Event::Refresh => return self.cloud_page(),
            Event::Select(id) => {
                if self
                    .cloud
                    .page
                    .as_ref()
                    .is_some_and(|p| p.assets.iter().any(|(a, _)| a.id == id))
                    && !self.cloud.selection.remove(&id)
                {
                    self.cloud.selection.insert(id);
                }
            }
            Event::SelectAll => {
                if let Some(page) = &self.cloud.page {
                    self.cloud.selection = page
                        .assets
                        .iter()
                        .map(|(asset, _)| asset.id.clone())
                        .collect();
                }
            }
            Event::ClipboardDone(generation, result) => {
                if generation == self.cloud.generation
                    && let Err(error) = result
                {
                    self.notice(format!("Could not copy image: {error}"), true);
                }
            }
            Event::Copy | Event::SaveCopy(_) => {
                if self.cloud.busy {
                    return Task::none();
                }
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
                let Some(client) = self
                    .cloud
                    .active
                    .and_then(|id| self.cloud.clients.get(&id))
                    .cloned()
                else {
                    return Task::none();
                };
                self.cloud.retire();
                self.cloud.busy = true;
                let generation = self.cloud.generation;
                let flag = Arc::new(AtomicBool::new(false));
                self.cloud.cancel = Some(flag.clone());
                let work = self.cloud.owner.submit(generation, move |context| {
                    let cancelled = || context.is_cancelled() || flag.load(Ordering::Acquire);
                    (|| -> anyhow::Result<Outcome> {
                        match event {
                            Event::Copy => {
                                anyhow::ensure!(
                                    !asset.video,
                                    "Copy a still photo or save the video original"
                                );
                                let bytes = client.original(&asset.id, &cancelled)?;
                                let decoded = enzui_iced_resources::decode_static(
                                    &bytes,
                                    &enzui_iced_core::decode_limits::DecodeLimits::default(),
                                    cancelled,
                                )?;
                                let iced::widget::image::Handle::Rgba {
                                    width,
                                    height,
                                    pixels,
                                    ..
                                } = decoded.handle
                                else {
                                    anyhow::bail!("The decoded image has no clipboard pixels")
                                };
                                Ok(Outcome::Copied(iced::clipboard::Image {
                                    rgba: pixels,
                                    size: iced::Size::new(width, height),
                                }))
                            }
                            Event::SaveCopy(path) => {
                                remote::save_original_as(client.as_ref(), &asset, &path, &cancelled)
                                    .map(Outcome::Saved)
                            }
                            _ => unreachable!(),
                        }
                    })()
                    .map_err(|e| e.to_string())
                });
                return watch_result(work, generation);
            }
            Event::Preview(id) => {
                if let Some(asset) = id
                    .as_ref()
                    .and_then(|id| {
                        self.cloud
                            .page
                            .as_ref()?
                            .assets
                            .iter()
                            .find(|(a, _)| &a.id == id)
                    })
                    .map(|(a, _)| a.clone())
                    .filter(|a| a.video)
                {
                    let Some(client) = self
                        .cloud
                        .active
                        .and_then(|id| self.cloud.clients.get(&id))
                        .cloned()
                    else {
                        return Task::none();
                    };
                    self.cloud.retire();
                    self.cloud.busy = true;
                    self.cloud.preview = Some(asset.id.clone());
                    self.cloud.status = "Preparing video playback…".into();
                    let generation = self.cloud.generation;
                    let flag = Arc::new(AtomicBool::new(false));
                    self.cloud.cancel = Some(flag.clone());
                    let work = self.cloud.owner.submit(generation, move |context| {
                        let directory = Arc::new(tempfile::tempdir().map_err(|e| e.to_string())?);
                        let path = remote::import_original(
                            client.as_ref(),
                            &asset,
                            directory.path(),
                            false,
                            &|| context.is_cancelled() || flag.load(Ordering::Acquire),
                        )
                        .map_err(|e| e.to_string())?;
                        Ok(Outcome::Video(directory, path))
                    });
                    return watch(work, generation);
                }
                let Some(asset) = id
                    .as_ref()
                    .and_then(|id| {
                        self.cloud
                            .page
                            .as_ref()?
                            .assets
                            .iter()
                            .find(|(a, _)| &a.id == id)
                    })
                    .map(|(a, _)| a.clone())
                else {
                    self.cloud.preview = None;
                    return scroll_top();
                };
                let Some(client) = self
                    .cloud
                    .active
                    .and_then(|id| self.cloud.clients.get(&id))
                    .cloned()
                else {
                    return Task::none();
                };
                self.cloud.retire();
                self.cloud.preview = Some(asset.id.clone());
                self.cloud.preview_image = None;
                self.cloud.transform = enzui_iced_core::zoom_pan::ViewTransform::IDENTITY;
                self.cloud.search_open = false;
                self.cloud.busy = true;
                self.cloud.status = "Loading photo…".into();
                let generation = self.cloud.generation;
                let flag = Arc::new(AtomicBool::new(false));
                self.cloud.cancel = Some(flag.clone());
                let work = self.cloud.owner.submit(generation, move |context| {
                    let cancelled = || context.is_cancelled() || flag.load(Ordering::Acquire);
                    let decode = |bytes: Vec<u8>| {
                        let limits = enzui_iced_core::decode_limits::DecodeLimits {
                            max_encoded_bytes: 128 * 1024 * 1024,
                            max_width: 32768,
                            max_height: 32768,
                            max_pixels: 80_000_000,
                            max_rgba_bytes: 320_000_000,
                        };
                        enzui_iced_resources::decode_static_resized(
                            &bytes, &limits, 4096, 4096, cancelled,
                        )
                        .map(|image| image.handle)
                    };
                    let image = client
                        .original(&asset.id, &cancelled)
                        .and_then(|bytes| decode(bytes).map_err(anyhow::Error::from))
                        .or_else(|_| {
                            client
                                .preview(&asset.id, &cancelled)
                                .and_then(|bytes| decode(bytes).map_err(anyhow::Error::from))
                        })
                        .map_err(|e| e.to_string())?;
                    Ok(Outcome::Preview(asset.id, image))
                });
                return watch(work, generation);
            }
            Event::Import | Event::Upload => {
                if self.catalog_authority.is_none() || self.persistence_blocked {
                    self.cloud.status =
                        "The local catalog is unavailable; transfers are disabled.".into();
                    return Task::none();
                }
                let Some(client) = self
                    .cloud
                    .active
                    .and_then(|id| self.cloud.clients.get(&id))
                    .cloned()
                else {
                    return Task::none();
                };
                let assets: Vec<_> = self
                    .cloud
                    .page
                    .as_ref()
                    .into_iter()
                    .flat_map(|p| p.assets.iter())
                    .filter(|(a, _)| {
                        self.cloud
                            .preview
                            .as_ref()
                            .map_or_else(|| self.cloud.selection.contains(&a.id), |id| id == &a.id)
                    })
                    .map(|(a, _)| a.clone())
                    .collect();
                let paths = self.cloud.upload_paths.clone();
                let upload = matches!(event, Event::Upload);
                let album = self.cloud.album.as_ref().map(|a| a.id.clone());
                if (upload && paths.is_empty()) || (!upload && assets.is_empty()) {
                    return Task::none();
                }
                let destination = self.import_destination().to_owned();
                let layout = self.catalog.settings.import_layout;
                let source_name = self.cloud.name.clone();
                self.cloud.retire();
                self.cloud.busy = true;
                self.cloud.status = if upload {
                    "Uploading selected originals…"
                } else {
                    "Importing selected originals…"
                }
                .into();
                let generation = self.cloud.generation;
                let cancelled_flag = Arc::new(AtomicBool::new(false));
                self.cloud.cancel = Some(Arc::clone(&cancelled_flag));
                let work = self.cloud.owner.submit(generation, move |context| {
                    let cancelled =
                        || context.is_cancelled() || cancelled_flag.load(Ordering::Acquire);
                    let mut error = None;
                    if upload {
                        let (mut created, mut duplicates) = (0, 0);
                        for path in paths {
                            match client.upload_into(&path, album.as_deref(), &cancelled) {
                                Ok(true) => created += 1,
                                Ok(false) => duplicates += 1,
                                Err(e) => {
                                    error = Some(e.to_string());
                                    break;
                                }
                            }
                        }
                        Ok(Outcome::Uploaded(created, duplicates, error))
                    } else {
                        let report = remote::import_batch(
                            client.as_ref(),
                            &assets,
                            &destination,
                            layout,
                            &source_name,
                            &cancelled,
                        )
                        .map_err(|error| error.to_string())?;
                        Ok(Outcome::Imported(
                            report.paths,
                            if report.cancelled {
                                Some("Transfer cancelled".into())
                            } else {
                                report.errors.first().cloned()
                            },
                        ))
                    }
                });

                return watch(work, generation);
            }
            Event::Cancel => {
                // Already published copies remain; completion still reconciles them into Imports.
                if let Some(cancel) = self.cloud.cancel.take() {
                    cancel.store(true, Ordering::Release);
                    self.cloud.status = "Cancelling… Completed copies will be kept.".into();
                }
            }
            Event::ForgetAccount(id) => {
                if self.catalog_authority.is_none() || self.persistence_blocked {
                    return Task::none();
                }
                self.cloud.retire();
                self.cloud.busy = true;
                self.cloud.status = "Removing the saved connection…".into();
                let generation = self.cloud.generation;
                let Some(account) = self
                    .catalog
                    .services
                    .iter()
                    .find(|account| account.id == id)
                else {
                    return Task::none();
                };
                let credential = credential(&self.paths.catalog, account);
                return Task::perform(
                    async move {
                        let store = connect_for_app("org.enzimos.Photos").await;
                        let deleted =
                            matches!(store.delete(credential).await, CredentialOutcome::Deleted);
                        Outcome::Forgotten(id, deleted)
                    },
                    move |outcome| Message::Cloud(Event::Done(generation, Ok(outcome))),
                );
            }
            Event::Done(generation, result) => {
                if generation != self.cloud.generation {
                    return Task::none();
                }
                self.cloud.busy = false;
                match result {
                    Err(error) => {
                        self.cloud.status = error.clone();
                        if !(self.cloud.open
                            && self.cloud.page.is_none()
                            && self.cloud.albums.is_none()
                            && self.cloud.preview.is_none())
                        {
                            self.notice(error, true);
                        }
                    }
                    Ok(Outcome::Copied(image)) => {
                        if self
                            .cloud
                            .cancel
                            .as_ref()
                            .is_some_and(|flag| flag.load(Ordering::Acquire))
                        {
                            return Task::none();
                        }
                        return iced::clipboard::write(image).map(move |r| {
                            Message::Cloud(Event::ClipboardDone(
                                generation,
                                r.map_err(crate::services::clipboard_error),
                            ))
                        });
                    }
                    Ok(Outcome::Saved(path)) => {
                        self.notice(format!("Saved a new copy: {}", path.display()), false);
                    }
                    Ok(Outcome::Key(account, key)) => {
                        self.cloud.name = account.name.clone();
                        self.cloud.url = account.url.clone();
                        if let Some(key) = key {
                            self.cloud.key = key;
                            return self.cloud_event(Event::Connect);
                        }
                        self.cloud.form = Some(ConnectionForm {
                            account: Some(account.id),
                            name: account.name,
                            url: account.url,
                            key: ApiKey::new(String::new()),
                        });
                        self.cloud.section = SettingsSection::Connection;
                        self.settings_page = "services".into();
                        self.settings_page_open = true;
                        self.cloud.status =
                            "Unlock your keyring or enter this server's API key.".into();
                        return self.update(Message::Dialog(super::Dialog::Settings));
                    }
                    Ok(Outcome::Connected(account, key, client, user)) => {
                        self.cloud.form = None;
                        let edited_only = self.cloud.key_edit_target == Some(account.id);
                        if !edited_only || self.cloud.active == Some(account.id) {
                            self.cloud.name.clone_from(&account.name);
                            self.cloud.url.clone_from(&account.url);
                        }
                        self.catalog.version = self.catalog.version.max(2);
                        let id = account.id;
                        if edited_only
                            && self.cloud.sync.running.is_some_and(|(rule, _)| {
                                self.catalog
                                    .transfer_rules
                                    .get(&rule)
                                    .is_some_and(|rule| rule.account == id)
                            })
                        {
                            self.cloud.sync.cancel();
                        }
                        let credential = credential(&self.paths.catalog, &account);
                        if let Some(saved) = self.catalog.services.iter_mut().find(|a| a.id == id) {
                            *saved = account;
                        } else {
                            self.catalog.services.push(account);
                            self.catalog.next_service = id + 1;
                        }
                        self.cloud
                            .thumbnail_cache
                            .retain(|(account, _), _| *account != id);
                        self.cloud
                            .thumbnail_order
                            .retain(|(account, _)| *account != id);
                        if !edited_only || self.cloud.active == Some(id) {
                            self.cloud.active = Some(id);
                            self.cloud.album = None;
                            self.cloud.albums = None;
                            self.cloud.page = None;
                            self.cloud.preview = None;
                            self.cloud.preview_image = None;
                            self.cloud.album_covers.clear();
                            self.cloud.sync.close_form();
                            self.cloud.query.clear();
                        }
                        self.cloud.clients.insert(id, client);
                        self.cloud.key = ApiKey::new(String::new());
                        self.cloud.busy = true;
                        self.cloud.status = format!(
                            "Connected as {user}. Saving the API key in the system keyring…"
                        );
                        let store = Task::perform(
                            async move {
                                let store = connect_for_app("org.enzimos.Photos").await;
                                Outcome::Remembered(matches!(
                                    store
                                        .set(credential, Secret::from_string(key.text().to_owned()))
                                        .await,
                                    CredentialOutcome::Stored
                                ))
                            },
                            move |outcome| Message::Cloud(Event::Done(generation, Ok(outcome))),
                        );
                        return Task::batch([self.changed(), store]);
                    }
                    Ok(Outcome::Remembered(saved)) => {
                        if !saved {
                            self.notice("The keyring is unavailable. The API key is kept for this session only; reconnect with a key next time.",false);
                        }
                        if let Some(id) = self.cloud.key_edit_target.take() {
                            self.cloud.status.clear();
                            return if self.cloud.open && self.cloud.active == Some(id) {
                                self.cloud_page()
                            } else {
                                Task::none()
                            };
                        }
                        if self.cloud.backup_setup {
                            self.cloud.status = "Ready to back up to this server.".into();
                            return Task::none();
                        }
                        self.cloud.open = true;
                        return Task::batch([self.cloud_page(), self.dismiss_dialog()]);
                    }
                    Ok(Outcome::Albums(albums, covers)) => {
                        self.cloud.album_covers = covers;
                        self.cloud.status.clear();
                        self.cloud.albums = Some(albums);
                        return self.cloud_thumbnails();
                    }
                    Ok(Outcome::Video(directory, path)) => {
                        self.cloud.preview_image = None;
                        self.cloud.playback_service = true;
                        self.retire_viewer();
                        self.cloud.playback = Some(directory);
                        self.cloud.open = false;
                        return self.open_external(path);
                    }
                    Ok(Outcome::AlbumImported(name, paths, error)) => {
                        if !paths.is_empty() {
                            match self.catalog.create_album(&name) {
                                Ok(id) => {
                                    if let Some(album) =
                                        self.catalog.albums.iter_mut().find(|a| a.id == id)
                                    {
                                        album
                                            .photos
                                            .extend(paths.iter().map(|p| PhotoId::for_path(p)));
                                    }
                                }
                                Err(e) => self.notice(e, true),
                            }
                        }
                        return self.cloud_event(Event::Done(
                            generation,
                            Ok(Outcome::Imported(paths, error)),
                        ));
                    }
                    Ok(Outcome::Preview(id, image)) => {
                        if self.cloud.preview.as_deref() == Some(&id) {
                            self.cloud.preview_image = Some((id, image));
                            self.cloud.status.clear();
                        }
                    }
                    Ok(Outcome::Page(mut page)) => {
                        self.cloud.status.clear();
                        self.cloud.smart_search = page.smart_search;
                        self.cloud.seen_assets.clear();
                        page.assets
                            .retain(|(asset, _)| self.cloud.seen_assets.insert(asset.id.clone()));
                        self.cloud.page = Some(page);
                        return Task::batch([self.cloud_thumbnails(), self.cloud_more()]);
                    }
                    Ok(Outcome::Imported(paths, error)) => {
                        self.cloud.status = format!(
                            "Imported {} original{}. {}",
                            paths.len(),
                            if paths.len() == 1 { "" } else { "s" },
                            error.unwrap_or_default()
                        );
                        self.notice(self.cloud.status.clone(), false);
                        for path in &paths {
                            self.catalog.imported.insert(PhotoId::for_path(path));
                        }
                        let destination = self.import_destination().to_owned();
                        if !paths.is_empty()
                            && destination != self.paths.photos
                            && !self.catalog.sources.contains(&destination)
                        {
                            self.catalog.sources.push(destination);
                        }
                        return Task::batch([self.changed(), self.scan(), self.continue_close()]);
                    }
                    Ok(Outcome::Uploaded(created, duplicates, error)) => {
                        self.cloud.status = format!(
                            "Uploaded {created}; {duplicates} already on the server. {}",
                            error.unwrap_or_default()
                        );
                        self.notice(self.cloud.status.clone(), false);
                    }
                    Ok(Outcome::Forgotten(id, deleted)) => {
                        if !deleted {
                            self.cloud.status="The keyring could not remove the credential. Unlock it and retry; the saved connection was retained.".into();
                            return Task::none();
                        }
                        self.catalog.services.retain(|a| a.id != id);
                        self.catalog.service_sync.remove(&id);
                        let removed: Vec<_> = self
                            .catalog
                            .transfer_rules
                            .iter()
                            .filter(|(_, rule)| rule.account == id)
                            .map(|(id, _)| *id)
                            .collect();
                        self.catalog
                            .transfer_rules
                            .retain(|_, rule| rule.account != id);
                        if self
                            .cloud
                            .sync
                            .running
                            .as_ref()
                            .is_some_and(|(active, _)| removed.contains(active))
                        {
                            self.cloud.sync.cancel();
                        }
                        self.cloud.clients.remove(&id);
                        self.cloud
                            .thumbnail_cache
                            .retain(|(account, _), _| *account != id);
                        self.cloud
                            .thumbnail_order
                            .retain(|(account, _)| *account != id);
                        self.cloud.retire();
                        if self.cloud.active == Some(id) {
                            self.cloud.active = None;
                            self.cloud.open = false;
                            self.cloud.page = None;
                            self.cloud.key = ApiKey::new(String::new());
                            self.cloud.name = "My Immich".into();
                            self.cloud.url.clear();
                        }
                        if self
                            .cloud
                            .form
                            .as_ref()
                            .is_some_and(|form| form.account == Some(id))
                        {
                            self.cloud.form = None;
                        }
                        self.cloud.status.clear();
                        return self.changed();
                    }
                }
            }
        }
        self.continue_close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{dispatch, loaded};

    #[test]
    fn service_requests_isolate_credentials_reject_stale_pages_and_reconcile_completed_copies() {
        let (mut app, sandbox) = loaded();
        let account = ServiceAccount {
            id: 1,
            name: "test".into(),
            provider: ServiceProvider::Immich,
            url: "http://127.0.0.1:22830".into(),
        };
        assert_ne!(
            credential(&app.paths.catalog, &account),
            credential(&sandbox.path().join("other-catalog.json"), &account)
        );
        let other = ServiceAccount {
            url: "https://other.example.test".into(),
            ..account.clone()
        };
        assert_ne!(
            credential(&app.paths.catalog, &account),
            credential(&app.paths.catalog, &other)
        );
        app.catalog.services.push(account);
        app.catalog.version = 2;
        app.catalog.next_service = 2;
        let wire = serde_json::to_string(&app.catalog).expect("catalog");
        assert!(!wire.contains("api_key"));
        assert_eq!(
            serde_json::from_str::<photos_library::catalog::Catalog>(&wire)
                .expect("roundtrip")
                .services,
            app.catalog.services
        );
        let photo = app.library.photos[0].clone();
        app.selection.enter_selection_mode();
        app.selection.click(
            &app.library.click_model,
            &photo.id,
            enzui_iced_core::list_selection::ClickModifiers::PLAIN,
        );
        dispatch(&mut app, Message::Cloud(Event::Configure));
        assert!(matches!(app.dialog, Some(super::super::Dialog::Settings)));
        assert_eq!(app.settings_page, "services");
        assert_eq!(app.cloud.upload_paths.len(), 1);
        app.cloud.retire();
        let generation = app.cloud.generation;
        dispatch(
            &mut app,
            Message::Cloud(Event::Done(
                generation.wrapping_sub(1),
                Ok(Outcome::Page(RenderedPage {
                    assets: Vec::new(),
                    next: Some("stale".into()),
                    smart_search: false,
                })),
            )),
        );
        assert!(app.cloud.page.is_none());
        dispatch(
            &mut app,
            Message::Cloud(Event::Owned(
                generation.wrapping_sub(1),
                Box::new(Event::Key(ApiKey::new("retired-form-key".into()))),
            )),
        );
        assert!(app.cloud.key.text().is_empty());

        app.cloud.busy = true;
        let cancelled = Arc::new(AtomicBool::new(false));
        app.cloud.cancel = Some(Arc::clone(&cancelled));
        dispatch(&mut app, Message::CloseRequested);
        assert!(app.close_requested);
        assert_eq!(app.cloud.generation, generation);
        assert!(app.cloud.busy);
        assert!(cancelled.load(Ordering::Acquire));
        dispatch(
            &mut app,
            Message::Cloud(Event::Done(
                generation,
                Ok(Outcome::Imported(
                    vec![photo.path.clone()],
                    Some("Transfer cancelled".into()),
                )),
            )),
        );
        assert!(app.catalog.imported.contains(&photo.id));
        assert!(!app.cloud.busy);
        assert!(app.cloud.status.contains("Imported 1 original"));
        app.close_requested = false;
        let plan = photos_library::catalog::ServiceSync {
            folder: sandbox.path().join("sync"),
            direction: photos_library::catalog::SyncDirection::Download,
            album: None,
        };
        let stale = sandbox.path().join("stale.png");
        app.cloud.sync.running = Some((1, 9));
        dispatch(
            &mut app,
            Message::Cloud(Event::Sync(super::super::cloud_sync::Event::Done(
                8,
                1,
                photos_library::catalog::TransferRule {
                    account: 1,
                    enabled: true,
                    source: photos_library::catalog::TransferSource::Folder,
                    remote_label: "Entire server library".into(),
                    plan,
                    legacy_receipts: true,
                },
                remote::sync::Report {
                    paths: vec![stale.clone()],
                    ..Default::default()
                },
            ))),
        );
        assert_eq!(app.cloud.sync.running, Some((1, 9)));
        assert!(!app.catalog.imported.contains(&PhotoId::for_path(&stale)));
    }

    #[derive(Debug, Default)]
    struct CountingService {
        thumbnails: std::sync::atomic::AtomicUsize,
        uploads: std::sync::Mutex<Vec<(PathBuf, Option<String>)>>,
    }

    impl PhotoService for CountingService {
        fn check(&self, _: &(dyn Fn() -> bool + Sync)) -> anyhow::Result<String> {
            Ok("fixture".into())
        }
        fn list(
            &self,
            cursor: Option<&str>,
            _: &str,
            _: &(dyn Fn() -> bool + Sync),
        ) -> anyhow::Result<remote::Page> {
            Ok(remote::Page {
                assets: (if cursor.is_some() { 4..8 } else { 0..4 })
                    .map(|id| Asset {
                        id: id.to_string(),
                        name: format!("photo-{id}.png"),
                        video: false,
                        checksum: "immutable".into(),
                        taken: String::new(),
                        width: 8,
                        height: 6,
                    })
                    .collect(),
                next: None,
            })
        }
        fn thumbnail(&self, _: &str, _: &(dyn Fn() -> bool + Sync)) -> anyhow::Result<Vec<u8>> {
            self.thumbnails.fetch_add(1, Ordering::SeqCst);
            self.original("", &|| false)
        }
        fn original(&self, _: &str, _: &(dyn Fn() -> bool + Sync)) -> anyhow::Result<Vec<u8>> {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(8, 6, image::Rgba([20, 80, 140, 255]))
                .write_to(&mut bytes, image::ImageFormat::Png)?;
            Ok(bytes.into_inner())
        }
        fn create_album(
            &self,
            _: &str,
            _: &(dyn Fn() -> bool + Sync),
        ) -> anyhow::Result<RemoteAlbum> {
            Ok(RemoteAlbum {
                id: "new-album".into(),
                name: "Holiday".into(),
                count: 0,
                cover: None,
            })
        }
        fn upload_into(
            &self,
            photo: &Photo,
            album: Option<&str>,
            _: &(dyn Fn() -> bool + Sync),
        ) -> anyhow::Result<bool> {
            self.uploads
                .lock()
                .expect("uploads")
                .push((photo.path.clone(), album.map(str::to_owned)));
            Ok(true)
        }
        fn upload(
            &self,
            photo: &Photo,
            cancelled: &(dyn Fn() -> bool + Sync),
        ) -> anyhow::Result<bool> {
            self.upload_into(photo, None, cancelled)
        }
    }

    #[test]
    fn service_metadata_precedes_thumbnails_and_cache_is_account_scoped_and_stale_safe() {
        use iced::futures::{StreamExt, executor::block_on};
        let (mut app, _sandbox) = super::super::tests::loaded();
        let client = Arc::new(CountingService::default());
        app.cloud.clients.insert(1, client.clone());
        app.cloud.active = Some(1);
        app.cloud.open = true;
        let task = app.cloud_page();
        let mut stream = iced_runtime::task::into_stream(task).expect("page work");
        let page = loop {
            match block_on(stream.next()).expect("metadata response") {
                iced_runtime::Action::Output(message) => break message,
                _ => continue,
            }
        };
        assert_eq!(
            client.thumbnails.load(Ordering::SeqCst),
            0,
            "metadata must not wait for thumbnail downloads"
        );
        assert!(
            matches!(&page, Message::Cloud(Event::Done(_, Ok(Outcome::Page(page)))) if page.assets.len() == 4 && page.assets.iter().all(|(_, image)| image.is_none()))
        );
        let thumbnail_task = app.update(page);
        assert!(
            !app.cloud.busy,
            "gallery remains interactive during thumbnail work"
        );
        assert_eq!(
            app.cloud
                .page
                .as_ref()
                .expect("visible metadata")
                .assets
                .len(),
            4
        );
        super::super::tests::settle(&mut app, thumbnail_task);
        assert_eq!(client.thumbnails.load(Ordering::SeqCst), 4);
        let first = app.cloud.page.as_ref().expect("page").assets[0]
            .1
            .clone()
            .expect("thumbnail");
        super::super::tests::dispatch(&mut app, Message::Cloud(Event::Refresh));
        assert_eq!(
            client.thumbnails.load(Ordering::SeqCst),
            4,
            "returning to cached media must not refetch"
        );
        super::super::tests::dispatch(&mut app, Message::Density(1));
        assert_eq!(
            app.cloud.page.as_ref().expect("page").assets[0].1,
            Some(first.clone())
        );
        let stale = app.cloud.generation;
        app.cloud.retire();
        super::super::tests::dispatch(
            &mut app,
            Message::Cloud(Event::Thumbnails(
                stale,
                vec![("0".into(), Ok(Handle::from_rgba(1, 1, vec![0; 4])))],
            )),
        );
        assert_eq!(
            app.cloud.page.as_ref().expect("page").assets[0].1,
            Some(first)
        );
        app.cloud.active = Some(2);
        app.cloud.clients.insert(2, client.clone());
        super::super::tests::dispatch(&mut app, Message::Cloud(Event::Refresh));
        assert_eq!(
            client.thumbnails.load(Ordering::SeqCst),
            8,
            "different accounts must not share cached media"
        );
    }

    #[test]
    fn service_page_errors_are_inline_and_viewer_navigation_copy_and_save_keep_their_scope() {
        use super::super::tests::dispatch;
        use iced::futures::{StreamExt, executor::block_on};
        let (mut app, sandbox) = super::super::tests::loaded();
        app.cloud.open = true;
        let generation = app.cloud.generation;
        dispatch(
            &mut app,
            Message::Cloud(Event::Done(generation, Err("Server unavailable".into()))),
        );
        assert_eq!(app.cloud.status, "Server unavailable");
        assert!(app.notice.is_none());
        assert!(app.toasts.visible_ids().is_empty());
        app.cloud.open = false;
        dispatch(
            &mut app,
            Message::Cloud(Event::Done(
                generation,
                Err("Cannot save connection".into()),
            )),
        );
        assert!(
            app.notice
                .as_ref()
                .is_some_and(|(text, error)| *error && text == "Cannot save connection")
        );
        app.cloud.open = true;
        app.cloud.active = Some(1);
        app.cloud
            .clients
            .insert(1, Arc::new(CountingService::default()));
        app.catalog.services.push(ServiceAccount {
            id: 1,
            name: "Remote".into(),
            url: "https://remote.example.test".into(),
            provider: ServiceProvider::Immich,
        });
        app.catalog.next_service = 2;
        app.catalog.version = 2;
        dispatch(&mut app, Message::Cloud(Event::Refresh));
        dispatch(&mut app, Message::Cloud(Event::Preview(Some("0".into()))));
        let mut ui = super::super::render_tests::simulator(&app, 960.0, 720.0);
        ui.click(iced::widget::Id::new("photos-service-viewer"))
            .expect("focus remote photo");
        assert_eq!(
            ui.tap_key(iced::keyboard::key::Named::ArrowRight),
            iced::event::Status::Captured
        );
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(messages.iter().any(|m| matches!(m, Message::Next(1))));
        for message in messages {
            dispatch(&mut app, message);
        }
        assert_eq!(app.cloud.preview.as_deref(), Some("1"));
        dispatch(&mut app, Message::Cloud(Event::Preview(Some("3".into()))));
        app.cloud.page.as_mut().expect("page").next = Some("later".into());
        dispatch(&mut app, Message::Next(1));
        assert_eq!(app.cloud.preview.as_deref(), Some("4"));
        assert_eq!(
            app.cloud
                .page
                .as_ref()
                .expect("continued page")
                .assets
                .len(),
            8
        );
        let task = app.update(Message::SaveCopy);
        drop(task);
        let request = app.chooser_request.as_ref().expect("save picker").0.clone();
        assert_eq!(request.purpose, super::super::PickPurpose::RemoteCopy);
        dispatch(&mut app, Message::Next(1));
        let stale = sandbox.path().join("stale-picker.png");
        dispatch(
            &mut app,
            Message::ChooserDone(
                request,
                enzui_iced_widgets::file_chooser::PortalOutcome::Completed(
                    enzui_iced_widgets::file_chooser::FileResult {
                        uris: vec![enzui_iced_widgets::file_chooser::FileUri::new(format!(
                            "file://{}",
                            stale.display()
                        ))],
                    },
                ),
            ),
        );
        assert!(!stale.exists());
        dispatch(&mut app, Message::DismissDialog);
        let task = app.update(Message::CopyImage);
        let mut stream = iced_runtime::task::into_stream(task).expect("remote copy");
        let output = block_on(stream.next());
        let Some(iced_runtime::Action::Output(
            result @ Message::Cloud(Event::Done(_, Ok(Outcome::Copied(_)))),
        )) = output
        else {
            panic!("remote decoded image: {output:?}")
        };
        if let Message::Cloud(Event::Done(_, Ok(Outcome::Copied(image)))) = &result {
            assert_eq!((image.size.width, image.size.height), (8, 6));
        }
        dispatch(&mut app, Message::Next(1));
        assert!(iced_runtime::task::into_stream(app.update(result)).is_none());
        let target = sandbox.path().join("saved.png");
        std::fs::write(&target, b"keep existing").expect("existing copy");
        dispatch(&mut app, Message::Cloud(Event::SaveCopy(target.clone())));
        assert_eq!(
            std::fs::read(target).expect("existing retained"),
            b"keep existing"
        );
        let copy = sandbox.path().join("saved (1).png");
        assert_eq!(image::open(copy).expect("new original copy").width(), 8);
    }

    #[test]
    fn automatic_rules_are_opt_in_independent_and_follow_album_membership_without_overwriting() {
        use super::super::cloud_sync::Event as Transfer;
        use super::super::tests::dispatch;
        use photos_library::catalog::SyncDirection;
        let (mut app, _sandbox) = super::super::tests::loaded();
        let clients: Vec<_> = (1..=3)
            .map(|id| {
                let client = Arc::new(CountingService::default());
                app.cloud.clients.insert(id, client.clone());
                app.catalog.services.push(ServiceAccount {
                    id,
                    name: format!("Server {id}"),
                    url: format!("https://server-{id}.example.test"),
                    provider: ServiceProvider::Immich,
                });
                client
            })
            .collect();
        app.catalog.next_service = 4;
        let album = app.catalog.create_album("Camera").expect("album");
        let first = app.library.photos[0].clone();
        let second = app.library.photos[1].clone();
        let originals: Vec<_> = app
            .library
            .photos
            .iter()
            .map(|photo| {
                (
                    photo.path.clone(),
                    std::fs::read(&photo.path).expect("original"),
                )
            })
            .collect();
        app.catalog.albums[0].photos.insert(first.id.clone());
        for (account, source) in [(1, Some(album)), (1, None), (2, Some(album)), (3, None)] {
            app.cloud.active = Some(account);
            dispatch(
                &mut app,
                Message::Cloud(Event::Sync(Transfer::New(SyncDirection::Upload))),
            );
            dispatch(
                &mut app,
                Message::Cloud(Event::Sync(Transfer::Local(source))),
            );
            dispatch(&mut app, Message::Cloud(Event::Sync(Transfer::Add)));
        }
        assert_eq!(app.catalog.transfer_rules.len(), 4);
        assert!(
            app.catalog
                .transfer_rules
                .values()
                .all(|rule| !rule.enabled)
        );
        dispatch(&mut app, Message::Cloud(Event::Sync(Transfer::Tick)));
        assert!(
            clients
                .iter()
                .all(|c| c.uploads.lock().expect("uploads").is_empty())
        );
        let saved = crate::services::persistence::load(&app.paths.catalog).expect("saved rules");
        assert_eq!(saved.transfer_rules, app.catalog.transfer_rules);
        assert_eq!(saved.version, 4);
        dispatch(
            &mut app,
            Message::Cloud(Event::Sync(Transfer::Toggle(1, true))),
        );
        dispatch(
            &mut app,
            Message::Cloud(Event::Sync(Transfer::Toggle(3, true))),
        );
        assert_eq!(clients[0].uploads.lock().expect("uploads").len(), 1);
        assert_eq!(clients[1].uploads.lock().expect("uploads").len(), 1);
        let new = app.paths.photos.join("new arrival.png");
        image::RgbaImage::from_pixel(7, 5, image::Rgba([12, 34, 56, 255]))
            .save(&new)
            .expect("new image");
        app.catalog.albums[0].photos.insert(PhotoId::for_path(&new));
        dispatch(&mut app, Message::Cloud(Event::Sync(Transfer::Tick)));
        assert_eq!(clients[0].uploads.lock().expect("uploads").len(), 2);
        assert_eq!(clients[1].uploads.lock().expect("uploads").len(), 2);
        assert!(clients[2].uploads.lock().expect("uploads").is_empty());
        dispatch(
            &mut app,
            Message::Cloud(Event::Sync(Transfer::Toggle(1, false))),
        );
        app.catalog.albums[0].photos.insert(second.id.clone());
        dispatch(&mut app, Message::Cloud(Event::Sync(Transfer::Tick)));
        assert_eq!(clients[0].uploads.lock().expect("uploads").len(), 2);
        assert_eq!(clients[1].uploads.lock().expect("uploads").len(), 3);
        let hidden = app.paths.photos.join("private.png");
        image::RgbaImage::from_pixel(7, 5, image::Rgba([90, 80, 70, 255]))
            .save(&hidden)
            .expect("hidden photo");
        app.catalog.hidden.insert(PhotoId::for_path(&hidden));
        dispatch(
            &mut app,
            Message::Cloud(Event::Sync(Transfer::Toggle(4, true))),
        );
        assert_eq!(clients[2].uploads.lock().expect("uploads").len(), 3);
        app.catalog.albums.clear();
        dispatch(&mut app, Message::Cloud(Event::Sync(Transfer::Tick)));
        assert!(app.cloud.sync.status[&3].contains("source album is unavailable"));
        assert_eq!(clients[1].uploads.lock().expect("uploads").len(), 3);
        dispatch(
            &mut app,
            Message::Cloud(Event::Sync(Transfer::Toggle(4, false))),
        );
        app.cloud.active = Some(1);
        dispatch(
            &mut app,
            Message::Cloud(Event::Sync(Transfer::New(SyncDirection::Download))),
        );
        dispatch(&mut app, Message::Cloud(Event::Sync(Transfer::Add)));
        let before = app.catalog.imported.len();
        dispatch(&mut app, Message::Cloud(Event::Sync(Transfer::Tick)));
        assert_eq!(app.catalog.imported.len(), before);
        dispatch(
            &mut app,
            Message::Cloud(Event::Sync(Transfer::Toggle(5, true))),
        );
        assert_eq!(app.catalog.imported.len(), before + 4);
        let downloaded = app.catalog.imported.clone();
        dispatch(&mut app, Message::Cloud(Event::Sync(Transfer::Tick)));
        assert_eq!(app.catalog.imported, downloaded);
        for (path, bytes) in originals {
            assert_eq!(std::fs::read(path).expect("original retained"), bytes);
        }
        app.dialog = Some(super::super::Dialog::Settings);
        app.settings_page = "services".into();
        app.cloud.section = SettingsSection::Upload;
        for width in [375.0, 1280.0] {
            let mut ui = super::super::render_tests::simulator(&app, width, 800.0);
            ui.draw(&app.theme());
            let mut collector = enzui_iced_core::collect::SemanticsCollector::new();
            ui.operate(&mut collector);
            assert!(
                collector.duplicate_ids().is_empty(),
                "{:?}",
                collector.duplicate_ids()
            );
        }
        let rule = app.catalog.transfer_rules[&5].clone();
        app.cloud.sync.running = Some((5, 99));
        dispatch(
            &mut app,
            Message::Cloud(Event::Sync(Transfer::Ready(
                98,
                5,
                rule,
                Ok(clients[0].clone()),
            ))),
        );
        assert_eq!(app.cloud.sync.running, Some((5, 99)));
        dispatch(&mut app, Message::Cloud(Event::Sync(Transfer::Remove(5))));
        assert!(!app.catalog.transfer_rules.contains_key(&5));
        assert_eq!(app.catalog.imported, downloaded);
    }

    #[test]
    fn service_backup_keeps_selected_originals_when_choosing_destination_and_preserves_album_scope()
    {
        let (mut app, _sandbox) = super::super::tests::loaded();
        let client = Arc::new(CountingService::default());
        app.cloud.clients.insert(2, client.clone());
        app.catalog.services.push(ServiceAccount {
            id: 2,
            name: "Destination".into(),
            url: "https://example.test".into(),
            provider: ServiceProvider::Immich,
        });
        app.catalog.services.push(ServiceAccount {
            id: 1,
            name: "Other server".into(),
            url: "https://other.example.test".into(),
            provider: ServiceProvider::Immich,
        });
        let photo = app.library.photos[0].clone();
        let original = std::fs::read(&photo.path).expect("original");
        app.selection.replace(
            [photo.id.clone()],
            Some(photo.id.clone()),
            Some(photo.id.clone()),
        );
        app.cloud.active = Some(2);
        app.cloud.album = Some(RemoteAlbum {
            id: "previous-remote-album".into(),
            name: "Previously browsed album".into(),
            count: 1,
            cover: None,
        });
        super::super::tests::dispatch(&mut app, Message::Cloud(Event::Backup(None)));
        assert_eq!(app.cloud.section, SettingsSection::Upload);
        assert!(
            app.cloud.album.is_none(),
            "local backup must not inherit a previously browsed remote album"
        );
        app.selection.clear();
        app.settings_menu = Some("service-server".into());
        super::super::tests::dispatch(&mut app, Message::Cloud(Event::BackupServer(2)));
        assert!(app.settings_menu.is_none());
        assert!(!app.cloud.open);
        assert!(matches!(app.dialog, Some(super::super::Dialog::Settings)));
        assert_eq!(app.cloud.upload_paths.len(), 1);
        assert!(
            client.uploads.lock().expect("uploads").is_empty(),
            "choosing a server must not upload"
        );
        super::super::tests::dispatch(&mut app, Message::Cloud(Event::Upload));
        assert_eq!(
            *client.uploads.lock().expect("uploads"),
            vec![(photo.path.clone(), None)]
        );
        let album = app.catalog.create_album("Holiday").expect("album");
        app.catalog
            .albums
            .iter_mut()
            .find(|item| item.id == album)
            .expect("album")
            .photos
            .insert(photo.id);
        app.catalog
            .create_album("Another album")
            .expect("second album");
        app.settings_menu = Some("backup-source".into());
        super::super::tests::dispatch(&mut app, Message::Cloud(Event::Backup(Some(album))));
        assert!(app.settings_menu.is_none());
        for section in [
            SettingsSection::Connection,
            SettingsSection::Download,
            SettingsSection::Upload,
        ] {
            super::super::tests::dispatch(&mut app, Message::Cloud(Event::Section(section)));
            let mut ui = super::super::render_tests::simulator(&app, 375.0, 800.0);
            ui.draw(&app.theme());
            let mut collector = enzui_iced_core::collect::SemanticsCollector::new();
            ui.operate(&mut collector);
            assert!(
                collector.duplicate_ids().is_empty(),
                "{section:?}: {:?}",
                collector.duplicate_ids()
            );
            if section == SettingsSection::Upload {
                drop(ui);
                super::super::tests::dispatch(&mut app, Message::Cloud(Event::Backup(Some(album))));
                let mut ui = super::super::render_tests::simulator(&app, 375.0, 800.0);
                ui.click(iced::widget::Id::from(format!(
                    "photos-service-export-album-{album}"
                )))
                .expect("back up selected album");
                for message in ui.into_messages() {
                    super::super::tests::dispatch(&mut app, message);
                }
            }
        }
        assert_eq!(
            client.uploads.lock().expect("uploads")[1],
            (photo.path.clone(), Some("new-album".into()))
        );
        let active_url = app.cloud.url.clone();
        super::super::tests::dispatch(&mut app, Message::Cloud(Event::EditKey(1)));
        super::super::tests::dispatch(
            &mut app,
            Message::Cloud(Event::Key(ApiKey::new("discarded-form-key".into()))),
        );
        assert_eq!(app.cloud.active, Some(2));
        assert_eq!(app.cloud.url, active_url);
        app.cloud.key_edit_target = Some(1);
        let account = app
            .catalog
            .services
            .iter()
            .find(|account| account.id == 1)
            .expect("edited account")
            .clone();
        app.cloud.thumbnail_cache.insert(
            (1, "old-key-photo".into()),
            Handle::from_rgba(1, 1, vec![255; 4]),
        );
        let generation = app.cloud.generation;
        let pending_store = app.cloud_event(Event::Done(
            generation,
            Ok(Outcome::Connected(
                account,
                ApiKey::new("new-test-key".into()),
                client.clone(),
                "fixture".into(),
            )),
        ));
        drop(pending_store);
        super::super::tests::dispatch(
            &mut app,
            Message::Cloud(Event::Done(generation, Ok(Outcome::Remembered(false)))),
        );
        assert_eq!(app.cloud.active, Some(2));
        assert_eq!(app.cloud.url, active_url);
        assert!(matches!(app.dialog, Some(super::super::Dialog::Settings)));
        assert!(
            !app.cloud
                .thumbnail_cache
                .contains_key(&(1, "old-key-photo".into()))
        );
        super::super::tests::dispatch(&mut app, Message::DismissDialog);
        assert!(app.cloud.form.is_none());
        assert_eq!(app.cloud.active, Some(2));
        assert_eq!(app.catalog.services.len(), 2);
        assert_eq!(std::fs::read(&photo.path).expect("original"), original);
    }

    #[derive(Debug)]
    struct LazyService {
        requests: std::sync::Mutex<Vec<(Option<String>, String, SearchKind)>>,
        fail_once: AtomicBool,
        fail_preview: AtomicBool,
        repeat: bool,
    }
    impl PhotoService for LazyService {
        fn check(&self, _: &(dyn Fn() -> bool + Sync)) -> anyhow::Result<String> {
            Ok("fixture".into())
        }
        fn smart_search_available(&self, _: &(dyn Fn() -> bool + Sync)) -> anyhow::Result<bool> {
            Ok(true)
        }
        fn list(
            &self,
            cursor: Option<&str>,
            query: &str,
            cancelled: &(dyn Fn() -> bool + Sync),
        ) -> anyhow::Result<remote::Page> {
            self.search(cursor, query, None, SearchKind::Filename, cancelled)
        }
        fn search(
            &self,
            cursor: Option<&str>,
            query: &str,
            _: Option<&str>,
            kind: SearchKind,
            _: &(dyn Fn() -> bool + Sync),
        ) -> anyhow::Result<remote::Page> {
            self.requests.lock().expect("requests").push((
                cursor.map(str::to_owned),
                query.into(),
                kind,
            ));
            if cursor == Some("2") && self.fail_once.swap(false, Ordering::SeqCst) {
                anyhow::bail!("connection interrupted")
            }
            let page = cursor.unwrap_or("0").parse::<usize>()?;
            Ok(remote::Page {
                assets: (page * 24..(page + 1) * 24)
                    .map(|id| Asset {
                        id: id.to_string(),
                        name: format!("photo-{id}.png"),
                        taken: String::new(),
                        video: false,
                        checksum: "immutable".into(),
                        width: 8,
                        height: 6,
                    })
                    .collect(),
                next: if self.repeat && page > 0 {
                    cursor.map(str::to_owned)
                } else {
                    (page < 3).then(|| (page + 1).to_string())
                },
            })
        }
        fn thumbnail(&self, id: &str, _: &(dyn Fn() -> bool + Sync)) -> anyhow::Result<Vec<u8>> {
            if id == "0" && self.fail_preview.swap(false, Ordering::SeqCst) {
                anyhow::bail!("preview not ready")
            }
            CountingService::default().original("", &|| false)
        }
        fn original(&self, _: &str, _: &(dyn Fn() -> bool + Sync)) -> anyhow::Result<Vec<u8>> {
            anyhow::bail!("gallery must not request originals")
        }
        fn upload(&self, _: &Photo, _: &(dyn Fn() -> bool + Sync)) -> anyhow::Result<bool> {
            anyhow::bail!("unused")
        }
    }

    #[test]
    fn service_lazy_loading_retries_without_mixing_draft_searches_and_rejects_stale_or_repeated_continuations()
     {
        use super::super::tests::dispatch;
        let (mut app, _sandbox) = super::super::tests::loaded();
        let client = Arc::new(LazyService {
            requests: Default::default(),
            fail_once: AtomicBool::new(true),
            fail_preview: AtomicBool::new(true),
            repeat: false,
        });
        app.cloud.clients.insert(1, client.clone());
        app.cloud.active = Some(1);
        app.cloud.open = true;
        dispatch(&mut app, Message::Cloud(Event::Refresh));
        assert_eq!(app.cloud.page.as_ref().expect("page").assets.len(), 48);
        assert!(
            app.cloud
                .load_error
                .as_deref()
                .is_some_and(|error| error.contains("interrupted"))
        );
        assert!(app.cloud.failed_previews.contains("0"));
        dispatch(&mut app, Message::Cloud(Event::RetryPreviews));
        assert!(app.cloud.failed_previews.is_empty());
        assert!(app.cloud.preview_error.is_none());
        assert!(app.cloud.page.as_ref().expect("page").assets[0].1.is_some());
        app.cloud.selection.insert("2".into());
        dispatch(&mut app, Message::Cloud(Event::Query("draft query".into())));
        dispatch(&mut app, Message::Cloud(Event::RetryMore));
        assert_eq!(app.cloud.page.as_ref().expect("page").assets.len(), 72);
        app.cloud.scroll.set(iced::Vector::new(0.0, 800.0));
        let pending = app.cloud_event(Event::Window(60, 96));
        assert!(app.cloud.loading_more);
        let interrupted = app
            .cloud
            .page
            .as_ref()
            .expect("page")
            .next
            .clone()
            .expect("continuation");
        app.cloud.retire();
        assert!(!app.cloud.seen_cursors.contains(&interrupted));
        drop(pending);
        dispatch(&mut app, Message::Cloud(Event::Window(60, 96)));
        assert_eq!(app.cloud.page.as_ref().expect("page").assets.len(), 96);
        assert_eq!(app.cloud.scroll.get().y, 800.0);
        assert!(app.cloud.selection.contains("2"));
        assert!(
            app.cloud
                .page
                .as_ref()
                .expect("page")
                .assets
                .iter()
                .filter(|(_, handle)| handle.is_some())
                .count()
                <= 36
        );
        assert!(
            client
                .requests
                .lock()
                .expect("requests")
                .iter()
                .all(|(_, query, _)| query.is_empty())
        );
        let retired = app.cloud.generation;
        dispatch(
            &mut app,
            Message::Cloud(Event::SearchKind(SearchKind::Smart)),
        );
        assert!(
            client
                .requests
                .lock()
                .expect("requests")
                .iter()
                .any(|(_, query, kind)| query == "draft query" && *kind == SearchKind::Smart)
        );
        let count = app.cloud.page.as_ref().expect("page").assets.len();
        dispatch(
            &mut app,
            Message::Cloud(Event::MoreDone(
                retired,
                "3".into(),
                Ok(remote::Page {
                    assets: Vec::new(),
                    next: Some("stale".into()),
                }),
            )),
        );
        assert_eq!(app.cloud.page.as_ref().expect("page").assets.len(), count);
        let broken = Arc::new(LazyService {
            requests: Default::default(),
            fail_once: AtomicBool::new(false),
            fail_preview: AtomicBool::new(false),
            repeat: true,
        });
        app.cloud.clients.insert(1, broken.clone());
        dispatch(&mut app, Message::Cloud(Event::Refresh));
        assert_eq!(broken.requests.lock().expect("requests").len(), 2);
        assert!(
            app.cloud
                .load_error
                .as_deref()
                .is_some_and(|error| error.contains("repeated"))
        );
        assert!(app.cloud.page.as_ref().expect("page").next.is_none());
    }
}
