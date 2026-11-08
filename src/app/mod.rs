mod chooser;
pub(crate) mod cloud;
pub(crate) mod cloud_sync;
pub(crate) mod context;
mod message;
mod native_drop;
mod reduce;
#[cfg(test)]
mod render_tests;
pub(crate) mod shortcuts;
mod state;
#[cfg(test)]
mod tests;
mod video;
pub(crate) mod viewer_actions;
mod work;

pub(crate) use message::Message;
pub(crate) use state::{
    Adjustment, Destination, Dialog, EditorTab, Grouping, Photos, PickPurpose, Viewer,
};

use std::num::NonZeroUsize;

use enzui_iced_core::blocking_task::BlockingTaskOwner;
use enzui_iced_core::theme_file::integration::ThemeIntegration;
use enzui_iced_widgets::file_chooser::ChooserSession;
use iced::{Element, Subscription, Task};

impl Photos {
    #[cfg(test)]
    pub(crate) fn new_at(paths: crate::platform::Paths) -> (Self, Task<Message>) {
        Self::new_launch(paths, Default::default())
    }

    pub(crate) fn new_launch(
        paths: crate::platform::Paths,
        launch: crate::platform::Launch,
    ) -> (Self, Task<Message>) {
        let (theme, theme_task) = ThemeIntegration::boot();
        let workers = match BlockingTaskOwner::new(
            "photos",
            NonZeroUsize::MIN,
            NonZeroUsize::new(16).expect("nonzero capacity"),
        ) {
            Ok(workers) => workers,
            Err(error) => {
                tracing::error!(%error, "cannot start photo worker");
                std::process::exit(1);
            }
        };
        let interactive = BlockingTaskOwner::new(
            "photos-viewer",
            NonZeroUsize::MIN,
            NonZeroUsize::new(4).expect("nonzero capacity"),
        )
        .unwrap_or_else(|error| {
            tracing::error!(%error, "cannot start viewer worker");
            std::process::exit(1);
        });
        let thumbnails = BlockingTaskOwner::new(
            "photos-thumbnails",
            NonZeroUsize::new(2).expect("two thumbnail workers"),
            NonZeroUsize::new(16).expect("nonzero capacity"),
        )
        .unwrap_or_else(|error| {
            tracing::error!(%error, "cannot start thumbnail workers");
            std::process::exit(1);
        });
        let thumbnail_store = std::sync::Arc::new(crate::services::thumbnail_cache::Cache::new(
            paths.thumbnail_cache(),
        ));
        let catalog_path = paths.catalog.clone();
        let add_folder = launch.add_folder.clone();
        let load = workers.submit(0, move |_| {
            (|| -> anyhow::Result<_> {
                let mut session = crate::services::persistence::open(&catalog_path)?;
                if let Some(path) = add_folder {
                    let folder = crate::services::validate_source(&path)?;
                    if !session.catalog.sources.contains(&folder) {
                        session.catalog.sources.push(folder);
                        crate::services::persistence::publish(
                            &session.authority,
                            &session.catalog,
                        )?;
                    }
                }
                Ok(session)
            })()
            .map_err(|e| e.to_string())
        });
        let file = launch.file.clone();
        let app = Self {
            instance: enzui_iced_widgets::single_instance::SingleInstance::standalone(),
            opening_file: file.is_some(),
            launch,
            after_load: None,
            gallery_generation: 0,
            context: Default::default(),
            paths,
            canonical_home: None,
            catalog: Default::default(),
            cloud: cloud::State::new(),
            library: Default::default(),
            theme,
            workers,
            interactive,
            thumbnails,
            thumbnail_store,
            thumbnail_usage: Default::default(),
            video_edit: Default::default(),
            nav: enzui_iced_widgets::nav_split::NavSplitState {
                preference: if file.is_some() {
                    enzui_iced_widgets::nav_split::SidebarPreference::Hidden
                } else {
                    Default::default()
                },
                ..Default::default()
            },
            destination: Destination::Library,
            grouping: Grouping::All,
            period_filter: None,
            query: String::new(),
            search_open: false,
            viewport_width: std::cell::Cell::new(iced::window::Settings::default().size.width),
            window_height: std::cell::Cell::new(iced::window::Settings::default().size.height),
            density: 4,
            viewer: None,
            selection: Default::default(),
            load_generation: 0,
            scanning: false,
            scan_generation: 0,
            scan_requested: false,
            busy: false,
            import_generation: 0,
            import_progress: None,
            import_progress_hidden: false,
            import_cancel: None,
            dialog: None,
            dialogs: Default::default(),
            dialog_text: String::new(),
            shortcuts: shortcuts::restore("").expect("empty shortcut overrides"),
            shortcut_capture: None,
            shortcut_feedback: None,
            settings_page: "gallery".into(),
            settings_page_open: false,
            settings_menu: None,
            export_options: Default::default(),
            toasts: enzui_iced_widgets::toast_host::ToastHost::new()
                .max_visible(2)
                .id_prefix("photos-toast"),
            chooser: ChooserSession::new().with_fallback(),
            chooser_generation: 0,
            chooser_request: None,
            chooser_cancel: None,
            parent_exports: enzui_iced_widgets::export_pool::ExportPoolOwner::start()
                .unwrap_or_else(|_| {
                    enzui_iced_widgets::export_pool::ExportPoolOwner::unavailable()
                }),
            window: None,
            notice: None,
            revision: 0,
            saved_revision: 0,
            save_inflight: false,
            persistence_blocked: false,
            catalog_authority: None,
            overflow_open: false,
            close_requested: false,
            pending_navigation: None,
            native_drops: Vec::new(),
        };
        let load = Task::perform(load, |settled| {
            Message::CatalogLoaded(
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            )
        });
        let opening = file.map_or_else(Task::none, |path| {
            let work = app.interactive.submit(0, move |context| {
                (|| -> anyhow::Result<_> {
                    anyhow::ensure!(!context.is_cancelled(), "opening cancelled");
                    let photo = photos_library::scan::inspect_file(&path)
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    let loaded =
                        crate::services::load_photo(&photo.path, || context.is_cancelled())?;
                    Ok((photo, std::sync::Arc::new(loaded)))
                })()
                .map_err(|e| e.to_string())
            });
            Task::perform(work, |settled| {
                Message::FileOpened(
                    0,
                    settled
                        .into_result()
                        .map_err(|e| e.to_string())
                        .and_then(|r| r),
                )
            })
        });
        (
            app,
            Task::batch([theme_task.map(Message::Theme), load, opening]),
        )
    }

    pub(crate) fn title(&self) -> String {
        if self.cloud.open {
            let name = self
                .cloud
                .preview
                .as_ref()
                .and_then(|id| {
                    self.cloud
                        .page
                        .as_ref()?
                        .assets
                        .iter()
                        .find(|(a, _)| &a.id == id)
                })
                .map(|(a, _)| a.label())
                .unwrap_or_else(|| self.cloud.name.clone());
            return format!("{name} — Photos");
        }
        self.selected_photo().map_or_else(
            || enzui_iced_core::localization::text("photos-title"),
            |photo| {
                format!(
                    "{} — {}",
                    photo.name,
                    enzui_iced_core::localization::text("photos-title")
                )
            },
        )
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        crate::view::view(self)
    }

    pub(crate) fn theme(&self) -> iced::Theme {
        self.theme.iced_theme()
    }

    pub(crate) fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            if !self
                .catalog
                .transfer_rules
                .values()
                .any(|rule| rule.enabled)
            {
                Subscription::none()
            } else {
                iced::time::every(std::time::Duration::from_secs(60))
                    .map(|_| Message::Cloud(cloud::Event::Sync(cloud_sync::Event::Tick)))
            },
            self.instance.subscription(|_| Message::InstanceWake),
            self.theme.subscription().map(Message::Theme),
            self.viewer
                .as_ref()
                .and_then(|v| v.stream.as_ref())
                .map_or_else(Subscription::none, |stream| {
                    let generation = self.load_generation;
                    stream
                        .player
                        .subscription()
                        .with(generation)
                        .map(|(generation, _)| Message::VideoWake(generation))
                }),
            if self
                .viewer
                .as_ref()
                .is_some_and(|v| v.motion_playing && !v.editing)
            {
                iced::time::every(std::time::Duration::from_secs_f32(
                    1.0 / crate::services::formats::MOTION_FPS,
                ))
                .map(|_| Message::MotionTick)
            } else {
                Subscription::none()
            },
            // Only an expiring toast needs ticks; an error waits to be closed.
            if self.toasts.next_expiry().is_none() {
                Subscription::none()
            } else {
                iced::time::every(std::time::Duration::from_millis(100)).map(|now| {
                    Message::Toast(enzui_iced_widgets::toast_host::ToastEvent::Tick(now))
                })
            },
            self.chooser.subscription().map(|_| Message::ChooserWake),
            iced::window::close_requests().map(|_| Message::CloseRequested),
            iced::event::listen_with(|event, status, window| {
                if matches!(
                    event,
                    iced::Event::Window(
                        iced::window::Event::Opened { .. }
                            | iced::window::Event::Closed
                            | iced::window::Event::Resized(_)
                    )
                ) || (status == iced::event::Status::Ignored
                    && matches!(
                        event,
                        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { .. })
                    ))
                {
                    Some(Message::Event(event, status, window))
                } else {
                    None
                }
            }),
        ])
    }
}
