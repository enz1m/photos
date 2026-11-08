use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use enzui_iced_core::blocking_task::{BlockingTaskCancellation, BlockingTaskOwner};
use enzui_iced_core::theme_file::integration::ThemeIntegration;
use enzui_iced_core::virtual_grid::{ResolvedTileGrid, TileProvider};
use enzui_iced_core::virtual_sequence::{ItemProvider, KeyedProvider, SequenceKey};
use enzui_iced_core::zoom_pan::ViewTransform;
use enzui_iced_resources::HandleCache;
use enzui_iced_widgets::file_chooser::ChooserSession;
use enzui_iced_widgets::modal_host::DialogStack;
use enzui_iced_widgets::nav_split::NavSplitState;
use enzui_iced_widgets::scroll_view::ScrollTranslation;
use iced::widget::image::Handle;
use photos_library::catalog::{Catalog, Photo, PhotoId};
use photos_library::edit::EditRecipe;

use crate::platform::Paths;
use crate::services::LoadedPhoto;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Destination {
    Library,
    Favorites,
    Albums,
    Album(u64),
    Imports,
    Source(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grouping {
    All,
    Months,
    Years,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorTab {
    Adjust,
    Crop,
    Resize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Adjustment {
    Exposure,
    Brightness,
    Contrast,
    Saturation,
    Temperature,
    Tint,
    Highlights,
    Shadows,
    Sharpness,
    Monochrome,
}

impl Adjustment {
    pub(crate) fn value(self, recipe: &EditRecipe) -> f32 {
        match self {
            Self::Exposure => recipe.exposure,
            Self::Brightness => recipe.brightness,
            Self::Contrast => recipe.contrast,
            Self::Saturation => recipe.saturation,
            Self::Temperature => recipe.temperature,
            Self::Tint => recipe.tint,
            Self::Highlights => recipe.highlights,
            Self::Shadows => recipe.shadows,
            Self::Sharpness => recipe.sharpness,
            Self::Monochrome => {
                if recipe.monochrome {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
    pub(crate) fn set(self, recipe: &mut EditRecipe, value: f32) {
        match self {
            Self::Exposure => recipe.exposure = value,
            Self::Brightness => recipe.brightness = value,
            Self::Contrast => recipe.contrast = value,
            Self::Saturation => recipe.saturation = value,
            Self::Temperature => recipe.temperature = value,
            Self::Tint => recipe.tint = value,
            Self::Highlights => recipe.highlights = value,
            Self::Shadows => recipe.shadows = value,
            Self::Sharpness => recipe.sharpness = value,
            Self::Monochrome => recipe.monochrome = value > 0.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Dialog {
    Help,
    NewAlbum,
    AddToAlbum,
    Import,
    Sources,
    Export,
    CleanMetadata,
    DeleteAlbum(u64),
    RenameAlbum(u64),
    CloseEdits,
    LeaveEdits,
    Settings,
    VideoEdit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PickPurpose {
    ImportDestination,
    Import,
    Source,
    Export,
    Clean,
    VideoExport,
    RemoteCopy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PickRequest {
    pub purpose: PickPurpose,
    pub generation: u64,
    pub load_generation: u64,
    pub service_generation: u64,
}

pub(crate) struct Streaming {
    pub show_frame: bool,
    pub player: enzui_services::media::VideoService,
    pub status: enzui_services::media::VideoStatus,
    pub presenter: enzui_iced_widgets::video::FramePresenter,
    pub uploading: bool,
    pub pending: Option<Arc<enzui_services::media::Rgba8Frame>>,
    pub last_frame: Option<Arc<enzui_services::media::Rgba8Frame>>,
}

pub(crate) struct VideoEditor {
    pub session: u64,
    pub before: crate::services::video::VideoEdit,
    pub history: Vec<crate::services::video::VideoEdit>,
    pub redo: Vec<crate::services::video::VideoEdit>,
    pub gesture: Option<crate::services::video::VideoEdit>,
    pub loop_range: bool,
    pub previewing: bool,
    pub exporting: bool,
    pub thumbnails: Vec<Handle>,
    pub thumbnail_cancel: Option<BlockingTaskCancellation<Result<Vec<Handle>, String>>>,
    pub playback_mute: bool,
}

pub(crate) struct Viewer {
    pub video_editor: Option<VideoEditor>,
    pub video_revision: u64,
    pub stream: Option<Streaming>,
    pub animation: Option<Arc<crate::services::formats::Motion>>,
    pub animation_cancel: Option<
        BlockingTaskCancellation<Result<Option<Arc<crate::services::formats::Motion>>, String>>,
    >,
    pub motion_position: usize,
    pub motion_playing: bool,
    pub motion_request: u64,
    pub motion_busy: bool,
    pub motion_preview: Option<Handle>,
    pub motion_allocation: Option<iced::widget::image::Allocation>,
    pub id: PhotoId,
    pub loaded: Option<Arc<LoadedPhoto>>,
    pub load_cancel: Option<BlockingTaskCancellation<Result<Arc<LoadedPhoto>, String>>>,
    pub preview_cancel: Option<BlockingTaskCancellation<Result<Handle, String>>>,
    pub preview: Option<Handle>,
    pub preview_allocation: Option<iced::widget::image::Allocation>,
    pub original_allocation: Option<iced::widget::image::Allocation>,
    pub before_edit_allocation: Option<iced::widget::image::Allocation>,
    pub original_preview: Option<Handle>,
    pub recipe: EditRecipe,
    pub before_edit: EditRecipe,
    pub before_edit_preview: Option<Handle>,
    pub preview_inflight: Option<u64>,
    pub preview_recipe: Option<EditRecipe>,
    pub preview_inflight_recipe: Option<EditRecipe>,
    pub editing: bool,
    pub tab: EditorTab,
    pub history: Vec<EditRecipe>,
    pub redo: Vec<EditRecipe>,
    pub transform: ViewTransform,
    pub compare: bool,
    pub drawing_crop: bool,
    pub crop_draft: Option<photos_library::edit::Crop>,
    pub crop_aspect: Option<f32>,
    pub info: bool,
    pub preview_generation: u64,
    pub preview_busy: bool,
    pub preview_dirty: bool,
    pub width_input: String,
    pub height_input: String,
    pub lock_aspect: bool,
    pub load_error: Option<String>,
}

pub(crate) struct Library {
    pub photos: Vec<Photo>,
    pub folder_counts: BTreeMap<PathBuf, usize>,
    pub visible: Vec<usize>,
    pub click_model: enzui_iced_core::list_selection::ClickModel<PhotoId>,
    pub groups: Vec<photos_library::grouping::PhotoGroup>,
    pub albums: Vec<AlbumTile>,
    pub recipes: BTreeMap<PhotoId, EditRecipe>,
    pub cache: RefCell<HandleCache<String>>,
    pub inflight: HashSet<String>,
    pub failed: HashSet<String>,
    pub geometry: Cell<ResolvedTileGrid>,
    pub tile_rows: RefCell<Vec<(f32, usize, usize)>>,
    pub scroll: ScrollTranslation,
    pub scroll_top: f32,
    pub viewport_height: f32,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            photos: Vec::new(),
            folder_counts: BTreeMap::new(),
            visible: Vec::new(),
            click_model: enzui_iced_core::list_selection::ClickModel::new([]),
            groups: Vec::new(),
            albums: Vec::new(),
            recipes: BTreeMap::new(),
            cache: RefCell::new(HandleCache::new(64 * 1024 * 1024)),
            inflight: HashSet::new(),
            failed: HashSet::new(),
            geometry: Cell::new(ResolvedTileGrid::default()),
            tile_rows: RefCell::new(Vec::new()),
            scroll: ScrollTranslation::default(),
            scroll_top: 0.0,
            viewport_height: 800.0,
        }
    }
}

pub(crate) struct AlbumTile {
    pub id: u64,
    pub name: String,
    pub count: usize,
    pub cover: Option<usize>,
}

impl ItemProvider<Option<Handle>> for Library {
    fn len(&self) -> u64 {
        self.visible.len() as u64
    }
    fn item(&self, index: u64) -> Option<Option<Handle>> {
        let photo = self
            .photos
            .get(*self.visible.get(usize::try_from(index).ok()?)?)?;
        Some(self.cache.borrow_mut().get(&crate::services::thumbnail_key(
            photo,
            self.recipes.get(&photo.id),
            crate::services::THUMBNAIL_LIMIT,
        )))
    }
}

impl KeyedProvider<Option<Handle>> for Library {
    fn key(&self, index: u64) -> Option<SequenceKey> {
        self.visible
            .get(usize::try_from(index).ok()?)
            .and_then(|index| self.photos.get(*index))
            .map(|photo| SequenceKey::new(&photo.id.0))
    }
    fn index_of(&self, key: &SequenceKey) -> Option<u64> {
        self.visible
            .iter()
            .position(|index| self.photos[*index].id.0 == key.as_str())
            .map(|index| index as u64)
    }
}
impl TileProvider<Option<Handle>> for Library {
    fn retention_hint(&self) -> Option<usize> {
        Some(0)
    }
}

pub(crate) struct Photos {
    pub(super) native_drops: Vec<super::native_drop::Pending>,
    pub launch: crate::platform::Launch,
    pub instance: enzui_iced_widgets::single_instance::SingleInstance,
    pub opening_file: bool,
    pub after_load: Option<(u64, super::context::PhotoAction)>,
    pub gallery_generation: u64,
    pub context: super::context::Context,
    pub paths: Paths,
    pub canonical_home: Option<PathBuf>,
    pub catalog: Catalog,
    pub cloud: super::cloud::State,
    pub library: Library,
    pub theme: ThemeIntegration,
    pub workers: BlockingTaskOwner,
    pub interactive: BlockingTaskOwner,
    pub thumbnails: BlockingTaskOwner,
    pub thumbnail_store: Arc<crate::services::thumbnail_cache::Cache>,
    pub thumbnail_usage: crate::services::thumbnail_cache::Usage,
    pub video_edit: crate::services::video::VideoEdit,
    pub nav: NavSplitState,
    pub destination: Destination,
    pub grouping: Grouping,
    pub period_filter: Option<(i64, i64, String)>,
    pub query: String,
    pub search_open: bool,
    /// Window height, beside `viewport_width`: settings choose a bottom sheet in a small window.
    pub window_height: Cell<f32>,
    pub viewport_width: Cell<f32>,
    pub density: u8,
    pub viewer: Option<Viewer>,
    pub selection: enzui_iced_core::list_selection::ListSelection<PhotoId>,
    pub load_generation: u64,
    pub scanning: bool,
    pub scan_generation: u64,
    pub scan_requested: bool,
    pub busy: bool,
    pub import_generation: u64,
    pub import_progress: Option<crate::services::import::ImportProgress>,
    /// The import's progress notice was closed; the import goes on and its
    /// result is still reported. The next import shows its progress again.
    pub import_progress_hidden: bool,
    pub import_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub dialog: Option<Dialog>,
    pub dialogs: DialogStack<Dialog>,
    pub dialog_text: String,
    pub shortcuts: enzui_iced_core::EditableShortcutCatalog<()>,
    pub shortcut_capture: Option<String>,
    pub shortcut_feedback: Option<(String, String)>,
    pub settings_page: String,
    /// A page is open over the page list in the narrow-window (stacked)
    /// settings navigation. Kept between opens, like the page itself.
    pub settings_page_open: bool,
    pub settings_menu: Option<String>,
    pub export_options: crate::services::ExportOptions,
    pub toasts: enzui_iced_widgets::toast_host::ToastHost,
    pub chooser: ChooserSession<PickRequest>,
    pub chooser_generation: u64,
    pub chooser_request: Option<(
        PickRequest,
        enzui_iced_widgets::file_chooser::PortalOperation,
        enzui_iced_widgets::file_chooser::ChooserOptions,
    )>,
    pub chooser_cancel: Option<enzui_iced_core::portal::cancel::PortalCancel>,
    pub parent_exports: enzui_iced_widgets::export_pool::ExportPoolOwner,
    pub window: Option<iced::window::Id>,
    pub notice: Option<(String, bool)>,
    pub revision: u64,
    pub saved_revision: u64,
    pub save_inflight: bool,
    pub persistence_blocked: bool,
    pub catalog_authority:
        Option<Arc<std::sync::Mutex<enzui_iced_core::state_authority::StateAuthority>>>,
    pub overflow_open: bool,
    pub close_requested: bool,
    pub pending_navigation: Option<Box<super::Message>>,
}

impl Photos {
    pub(crate) fn selected_photo(&self) -> Option<&Photo> {
        let id = &self.viewer.as_ref()?.id;
        self.library.photos.iter().find(|photo| &photo.id == id)
    }
}

impl Viewer {
    pub(crate) fn motion(&self) -> Option<&crate::services::formats::Motion> {
        self.animation
            .as_deref()
            .or_else(|| self.loaded.as_ref().and_then(|p| p.motion.as_ref()))
    }
}

pub(crate) type ParentHandoff = enzui_parent_lease::Handoff<
    enzui_iced_core::blocking_task::BlockingTaskSettlement<enzui_iced_wayland::ParentAdoption>,
>;
