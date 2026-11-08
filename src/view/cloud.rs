use enzui_iced_widgets::button::{ButtonKind, ButtonSize, enz_button};
use enzui_iced_widgets::icon::enz_icon;
use enzui_iced_widgets::pressable::{PressableStyle, enz_pressable};
use enzui_iced_widgets::text::{TextSize, TextTone, enz_text};
use enzui_iced_widgets::text_field::enz_text_field;
use enzui_iced_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{Space, column, container, row};
use iced::{Element, Length};

use crate::app::cloud::{Event, SettingsSection};
use crate::app::{Message, Photos};
use crate::services::remote::ApiKey;
use photos_library::catalog::SyncDirection;

fn button<'a>(
    state: &crate::app::cloud::State,
    label: impl Into<String>,
    event: Event,
    enabled: bool,
) -> Element<'a, Message> {
    let id = match &event {
        Event::Open(id) => format!("account-{id}"),
        Event::BackupServer(id) => format!("backup-server-{id}"),
        Event::Section(section) => format!("section-{section:?}"),
        Event::Select(id) => format!("select-{id}"),
        Event::Albums => "albums".into(),
        Event::Album(Some(album)) => format!("album-{}", album.id),
        Event::Album(None) => "timeline".into(),
        Event::ImportAlbum => "import-album".into(),
        Event::ExportAlbum(id) => format!("export-album-{id}"),
        Event::Sync(event) => format!("sync-{event:?}"),
        Event::SelectAll => "select-page".into(),
        Event::Preview(Some(id)) => format!("preview-{id}"),
        Event::Preview(None) => "preview-back".into(),
        Event::Connect => "connect".into(),
        Event::Refresh => "refresh".into(),
        Event::New => "new".into(),
        Event::Import => "import".into(),
        Event::Upload => "upload".into(),
        Event::RetryMore => "retry-loading".into(),
        Event::Cancel => "cancel".into(),
        _ => "command".into(),
    };
    let primary = matches!(
        event,
        Event::Upload | Event::ExportAlbum(_) | Event::Connect
    );
    let label = label.into();
    let accessible_name = match &event {
        Event::Select(id) | Event::Preview(Some(id)) => state
            .page
            .as_ref()
            .and_then(|page| page.assets.iter().find(|(asset, _)| &asset.id == id))
            .map(|(asset, _)| format!("{label}: {} · {}", asset.label(), asset.taken))
            .unwrap_or_else(|| label.clone()),
        _ => label.clone(),
    };
    enz_button(
        label,
        Message::Cloud(Event::Owned(state.generation, Box::new(event))),
    )
    .id(iced::widget::Id::from(format!("photos-service-{id}")))
    .aria_label(accessible_name)
    .size(ButtonSize::Sm)
    .kind(if primary {
        ButtonKind::Primary
    } else {
        ButtonKind::Quiet
    })
    .enabled(enabled)
    .into_element()
}

fn hint(text: impl Into<String>) -> Element<'static, Message> {
    enz_text(text.into())
        .size(TextSize::Footnote)
        .tone(TextTone::Muted)
        .into()
}

fn icon<'a>(
    app: &Photos,
    id: impl Into<String>,
    icon: &'static str,
    label: impl Into<String>,
    event: Event,
    enabled: bool,
) -> Element<'a, Message> {
    let label = label.into();
    enz_toolbar_icon(icon, self::event(app, event))
        .aria_label(label.clone())
        .tooltip(label)
        .enabled(enabled)
        .id(iced::widget::Id::from(format!(
            "photos-service-{}",
            id.into()
        )))
        .into_element()
}

fn server_label(app: &Photos, account: &photos_library::catalog::ServiceAccount) -> String {
    if app
        .catalog
        .services
        .iter()
        .filter(|other| other.name == account.name)
        .count()
        < 2
    {
        return account.name.clone();
    }
    let label = format!("{} · {}", account.name, account.url);
    if app
        .catalog
        .services
        .iter()
        .filter(|other| other.name == account.name && other.url == account.url)
        .count()
        > 1
    {
        format!("{label} ({})", account.id)
    } else {
        label
    }
}

fn server_choice(app: &Photos) -> Element<'_, Message> {
    let current = app
        .cloud
        .active
        .and_then(|id| app.catalog.services.iter().find(|account| account.id == id))
        .map_or_else(
            || "Choose server".into(),
            |account| server_label(app, account),
        );
    super::settings::choice(
        app,
        "service-server",
        "Server",
        &current,
        app.catalog
            .services
            .iter()
            .map(|account| {
                (
                    server_label(app, account),
                    event(app, Event::BackupServer(account.id)),
                )
            })
            .collect(),
    )
}

pub(crate) fn settings(app: &Photos) -> Element<'_, Message> {
    use enzui_iced_widgets::view_switcher::{SwitcherPage, SwitcherTokens, enz_view_switcher};
    let state = &app.cloud;
    let navigation = enz_view_switcher(
        vec![
            SwitcherPage::new("connection", "Connections"),
            SwitcherPage::new("upload", "Upload"),
            SwitcherPage::new("download", "Download"),
        ],
        match state.section {
            SettingsSection::Connection => 0,
            SettingsSection::Upload => 1,
            SettingsSection::Download => 2,
        },
        |index| {
            event(
                app,
                Event::Section(match index {
                    1 => SettingsSection::Upload,
                    2 => SettingsSection::Download,
                    _ => SettingsSection::Connection,
                }),
            )
        },
    )
    .tokens(SwitcherTokens {
        auto_floor: 0.0,
        ..Default::default()
    })
    .aria_label("Service settings sections");
    let content = match state.section {
        SettingsSection::Connection => connection(app),
        SettingsSection::Upload => {
            if state.backup_setup {
                backup(app)
            } else {
                column![
                    super::transfers::view(app, SyncDirection::Upload),
                    button(
                        state,
                        "Upload photos once…",
                        Event::Backup(None),
                        !state.busy
                    )
                ]
                .spacing(20.0)
                .into()
            }
        }
        SettingsSection::Download => column![
            super::transfers::view(app, SyncDirection::Download),
            download(app)
        ]
        .spacing(20.0)
        .into(),
    };
    let mut body = column![navigation, content].spacing(12.0);
    if state.busy {
        body = body.push(
            row![
                pulse(),
                icon(
                    app,
                    "request-cancel",
                    "x",
                    "Cancel request",
                    Event::Cancel,
                    state.cancellable()
                )
            ]
            .align_y(iced::Alignment::Center),
        );
    }
    body.width(Length::Fill).into()
}

fn connection(app: &Photos) -> Element<'_, Message> {
    use enzui_iced_widgets::preferences_row::enz_preferences_row;
    let state = &app.cloud;
    let idle = !state.busy;
    let add: Element<'_, Message> = enz_button("Add", event(app, Event::New))
        .icon("plus")
        .kind(ButtonKind::Quiet)
        .size(ButtonSize::Sm)
        .enabled(idle)
        .id(iced::widget::Id::new("photos-service-add"))
        .aria_label("Add server")
        .into_element();
    let mut body = column![super::settings::heading("Servers", "service-servers", "Saved servers appear in the library sidebar. Removing a connection never deletes local or remote photos.", Some(add))].spacing(8.0);
    let mut rows = column![].spacing(2.0);
    for account in &app.catalog.services {
        rows = rows.push(
            enz_preferences_row(account.name.clone())
                .subtitle(account.url.clone())
                .subtitle_wrap(false)
                .control(
                    row![
                        icon(
                            app,
                            format!("account-{}-key", account.id),
                            "key-round",
                            format!("Change API key for {}", account.name),
                            Event::EditKey(account.id),
                            idle
                        ),
                        icon(
                            app,
                            format!("account-{}-remove", account.id),
                            "trash-2",
                            format!("Remove {} connection", account.name),
                            Event::ForgetAccount(account.id),
                            idle
                        ),
                    ]
                    .spacing(4.0),
                )
                .accessory_width(68.0)
                .label_min_width(120.0)
                .id(iced::widget::Id::from(format!(
                    "photos-service-account-{}",
                    account.id
                ))),
        );
    }
    if !app.catalog.services.is_empty() {
        body = body.push(
            enzui_iced_widgets::preferences_group::enz_preferences_group(rows)
                .bordered(true)
                .aria_label("Saved servers"),
        );
    } else if state.form.is_none() {
        body = body.push(hint("Add your first photo server."));
    }
    if let Some(form) = &state.form {
        let title = if form.account.is_some() {
            "Change API key"
        } else {
            "New Immich server"
        };
        let cancel = icon(
            app,
            "form-cancel",
            "x",
            "Cancel connection changes",
            Event::CancelForm,
            idle,
        );
        let mut fields = column![super::settings::heading(title, "service-key", "Create an API key in Immich account settings. Grant user.read, asset.read, asset.view, asset.download, asset.upload, album.read, album.create and albumAsset.create. Keys are kept in the system keyring when available.", Some(cancel))].spacing(12.0);
        if form.account.is_none() {
            fields = fields.push(
                enz_text_field("Server name", "My Immich", &form.name, |value| {
                    event(app, Event::Name(value))
                })
                .id(iced::widget::Id::new("photos-service-name"))
                .enabled(idle),
            );
            fields = fields.push(
                enz_text_field(
                    "Server URL",
                    "https://photos.example.com",
                    &form.url,
                    |value| event(app, Event::Url(value)),
                )
                .id(iced::widget::Id::new("photos-service-url"))
                .enabled(idle),
            );
        }
        fields = fields.push(
            enz_text_field(
                "API key",
                "Paste an Immich API key",
                form.key.text(),
                |value| event(app, Event::Key(ApiKey::new(value))),
            )
            .secure(true)
            .id(iced::widget::Id::new("photos-service-key"))
            .enabled(idle),
        );
        if !state.status.is_empty() && idle {
            fields = fields.push(enz_text(&state.status).size(TextSize::Footnote));
        }
        fields = fields.push(button(
            state,
            if form.account.is_some() {
                "Save key"
            } else {
                "Connect"
            },
            Event::Connect,
            idle,
        ));
        body = body.push(fields);
    }
    body.into()
}

fn backup(app: &Photos) -> Element<'_, Message> {
    let state = &app.cloud;
    let mut body = column![super::settings::heading("Upload once", "service-backup", "Copies originals without overwriting or deleting anything. Immich reuses duplicates. Album upload creates a new remote album. Local edit recipes stay in Photos; export an edited copy first to upload it.", Some(icon(app, "close-upload-options", "x", "Close one-time upload options", Event::CloseUpload, true)))].spacing(8.0);
    if app.catalog.services.is_empty() {
        return body
            .push(button(state, "Add server", Event::New, !state.busy))
            .into();
    }
    body = body.push(server_choice(app));
    let current = state
        .backup_album
        .and_then(|id| app.catalog.albums.iter().find(|album| album.id == id));
    let mut sources = vec![(
        format!("Selected photos ({})", state.upload_paths.len()),
        event(app, Event::Backup(None)),
    )];
    sources.extend(app.catalog.albums.iter().map(|album| {
        (
            album.name.clone(),
            event(app, Event::Backup(Some(album.id))),
        )
    }));
    let source = current.map_or_else(
        || format!("Selected photos ({})", state.upload_paths.len()),
        |album| album.name.clone(),
    );
    body = body.push(super::settings::choice(
        app,
        "backup-source",
        "Source",
        &source,
        sources,
    ));
    let scope = if current.is_some() {
        "New album".to_owned()
    } else {
        state
            .album
            .as_ref()
            .map_or_else(|| "Timeline".into(), |album| album.name.clone())
    };
    body = body.push(
        enzui_iced_widgets::preferences_row::enz_preferences_row("Destination")
            .control(enz_text(scope).size(TextSize::Footnote))
            .accessory_width(150.0)
            .label_min_width(90.0)
            .id(iced::widget::Id::new("photos-backup-destination")),
    );
    let count = current.map_or(state.upload_paths.len(), |album| album.photos.len());
    let action = current.map_or(Event::Upload, |album| Event::ExportAlbum(album.id));
    if count == 0 {
        body = body.push(hint("Select local photos or choose an album to upload."));
    }
    body = body.push(row![
        Space::new().width(Length::Fill),
        button(
            state,
            if current.is_some() {
                "Upload album"
            } else {
                "Upload photos"
            },
            action,
            !state.busy && state.connected() && count > 0
        )
    ]);
    body.into()
}

fn download(app: &Photos) -> Element<'_, Message> {
    column![
        super::settings::heading("Download once", "download-once", "Select media in a server library, then download new copies. Existing photos are never overwritten. For an entire album, open it and use Download album in its titlebar.", None),
        row![enz_text(format!("{} selected", app.cloud.selection.len())), Space::new().width(Length::Fill),
            button(&app.cloud, "Download photos", Event::Import, !app.cloud.busy && app.cloud.connected() && !app.cloud.selection.is_empty())]
            .align_y(iced::Alignment::Center)
    ].spacing(8.0).into()
}

fn pulse<'a>() -> Element<'a, Message> {
    use enzui_iced_widgets::skeleton::{SkeletonKind, enz_skeleton};
    container(
        enz_skeleton(SkeletonKind::Circle)
            .label("Loading photo service")
            .into_element(),
    )
    .center_x(Length::Fill)
    .padding(24.0)
    .into()
}

pub(crate) fn event(app: &Photos, event: Event) -> Message {
    Message::Cloud(Event::Owned(app.cloud.generation, Box::new(event)))
}

pub(crate) fn titlebar(app: &Photos, width: f32) -> Element<'_, Message> {
    use enzui_iced_widgets::title_bar::enz_title_bar;
    let state = &app.cloud;
    if state.search_open {
        return enz_title_bar()
            .leading(super::controls::glass_chrome(
                enz_toolbar_icon("arrow-left", event(app, Event::SearchClose))
                    .aria_label("Close service search")
                    .id(iced::widget::Id::new("photos-service-search-back")),
            ))
            .push(search(app, true))
            .fluid_content(120.0)
            .id("photos-window")
            .into_element();
    }
    let leading: Element<'_, Message> = if let Some(id) = &state.preview {
        let position = state
            .page
            .as_ref()
            .and_then(|p| p.assets.iter().position(|(a, _)| &a.id == id));
        super::controls::glass_chrome(
            row![
                enz_toolbar_icon("arrow-left", event(app, Event::Back))
                    .aria_label("Back to service library")
                    .id(iced::widget::Id::new("photos-service-back")),
                enz_toolbar_icon("chevron-left", event(app, Event::Step(-1)))
                    .aria_label("Previous service photo")
                    .enabled(position.is_some_and(|i| i > 0))
                    .id(iced::widget::Id::new("photos-service-photo-previous")),
                enz_toolbar_icon("chevron-right", event(app, Event::Step(1)))
                    .aria_label("Next service photo")
                    .enabled(position.is_some_and(|i| {
                        state
                            .page
                            .as_ref()
                            .is_some_and(|p| i + 1 < p.assets.len() || p.next.is_some())
                    }))
                    .id(iced::widget::Id::new("photos-service-photo-next")),
            ]
            .spacing(4.0),
        )
    } else if let Some(album) = &state.album {
        super::controls::glass_chrome(
            container(
                row![
                    enz_toolbar_icon("arrow-left", event(app, Event::Back))
                        .aria_label("Back to service albums")
                        .id(iced::widget::Id::new("photos-service-albums-back")),
                    enz_text(&album.name).lines(1),
                ]
                .spacing(4.0)
                .align_y(iced::Alignment::Center),
            )
            .padding(iced::Padding::ZERO.right(12.0)),
        )
    } else {
        super::controls::glass_tabs(
            app,
            &[("photos", "Photos"), ("albums", "Albums")],
            usize::from(state.albums.is_some()),
            |index| {
                event(
                    app,
                    if index == 1 {
                        Event::Albums
                    } else {
                        Event::Album(None)
                    },
                )
            },
            "Service library",
            false,
        )
    };
    let bar = enz_title_bar()
        .leading(leading)
        .id("photos-window")
        .into_element();
    if width < 900.0 || state.preview.is_some() || state.albums.is_some() {
        return bar;
    }
    let field_width = app.theme.theme().sizing.search_field_width;
    let height = enzui_iced_widgets::title_bar::shell_band_height()
        .unwrap_or(enzui_iced_widgets::app_shell::DEFAULT_SHELL_TITLEBAR_HEIGHT);
    let field_height = enzui_iced_widgets::text_field::TextFieldSize::Md
        .tokens()
        .height;
    iced::widget::stack![
        bar,
        iced::widget::pin(container(search(app, false)).width(field_width))
            .x((width - field_width) / 2.0)
            .y((height - field_height) / 2.0)
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn search(app: &Photos, focus: bool) -> Element<'_, Message> {
    let generation = app.cloud.generation;
    super::controls::glass_chrome(
        row![
            enzui_iced_widgets::search_field::enz_titlebar_search(&app.cloud.query, move |q| {
                Message::Cloud(Event::Owned(generation, Box::new(Event::Query(q))))
            })
            .size(enzui_iced_widgets::text_field::TextFieldSize::Md)
            .responsive(false)
            .chrome(enzui_iced_widgets::search_field::SearchFieldChrome::Ghost)
            .placeholder(
                if app.cloud.search_kind == crate::services::remote::SearchKind::Smart {
                    "Search image content…"
                } else {
                    "Search filenames…"
                }
            )
            .id(iced::widget::Id::new("photos-service-search"))
            .width(Length::Fill)
            .max_width(if focus {
                f32::MAX
            } else {
                app.theme.theme().sizing.search_field_width
            })
            .focus_on_mount(focus)
            .on_submit(event(app, Event::Refresh))
            .on_clear(event(app, Event::ClearQuery))
            .into_element(),
            enz_toolbar_icon(
                "sparkles",
                event(
                    app,
                    Event::SearchKind(
                        if app.cloud.search_kind == crate::services::remote::SearchKind::Smart {
                            crate::services::remote::SearchKind::Filename
                        } else {
                            crate::services::remote::SearchKind::Smart
                        }
                    )
                )
            )
            .active(app.cloud.search_kind == crate::services::remote::SearchKind::Smart)
            .enabled(app.cloud.smart_search)
            .aria_label("AI search")
            .tooltip(if !app.cloud.smart_search {
                "Enable Smart Search on your Immich server to search image content."
            } else if app.cloud.search_kind == crate::services::remote::SearchKind::Smart {
                "AI search is on. Click to search filenames instead."
            } else {
                "Search image content with Immich AI."
            })
            .id(iced::widget::Id::new("photos-service-ai-search")),
        ]
        .align_y(iced::Alignment::Center),
    )
}

pub(crate) fn view<'a>(
    app: &'a Photos,
    geometry: &enzui_iced_widgets::app_shell::ShellGeometry,
) -> Element<'a, Message> {
    let state = &app.cloud;
    if let Some((asset, thumbnail)) = state.preview.as_ref().and_then(|id| {
        state
            .page
            .as_ref()?
            .assets
            .iter()
            .find(|(a, _)| &a.id == id)
    }) {
        let handle = state
            .preview_image
            .as_ref()
            .filter(|(id, _)| id == &asset.id)
            .map(|(_, h)| h)
            .or(thumbnail.as_ref());
        if let Some(handle) = handle {
            let generation = state.generation;
            let mut tokens = enzui_iced_widgets::image_viewer::ImageViewerTokens::from_palette(
                &app.theme.theme().palette,
            );
            tokens.radius = geometry
                .content_corner_radii
                .into_iter()
                .fold(0.0_f32, f32::max);
            let viewer =
                enzui_iced_widgets::image_viewer::enz_image_viewer(handle.clone(), state.transform)
                    .tokens(tokens)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .on_transform(move |t| {
                        Message::Cloud(Event::Owned(generation, Box::new(Event::Transform(t))))
                    })
                    .alt_text(format!(
                        "{} · {} × {}",
                        asset.label(),
                        asset.width,
                        asset.height
                    ))
                    .id(iced::widget::Id::new("photos-service-viewer"))
                    .into();
            let viewer = super::viewer_input::surface(app, viewer);
            return if state.busy {
                iced::widget::stack![viewer, container(pulse()).center_y(Length::Fill)].into()
            } else {
                viewer
            };
        }
    }
    let scrolling: Element<'a, Message> = if state.albums.is_some() || state.page.is_some() {
        library_grid(app, geometry)
    } else if state.busy {
        container(pulse()).center_y(Length::Fill).into()
    } else {
        container(enz_text(&state.status).tone(TextTone::Muted))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    };
    let mut overlay = row![].spacing(8.0).align_y(iced::Alignment::Center);
    if !state.selection.is_empty() {
        overlay = overlay
            .push(
                enz_toolbar_icon("x", event(app, Event::ClearSelection))
                    .aria_label("Clear service selection")
                    .id(iced::widget::Id::new("photos-service-clear-selection")),
            )
            .push(enz_text(format!("{} selected", state.selection.len())));
    }
    let selection: Element<'a, Message> = if !state.selection.is_empty() {
        container(super::controls::glass_chrome(overlay))
            .center_x(Length::Fill)
            .align_bottom(Length::Fill)
            .padding(16.0)
            .into()
    } else {
        Space::new().into()
    };
    let density: Element<'a, Message> = if state.busy {
        Space::new().into()
    } else {
        container(super::controls::glass_chrome(
            row![
                enz_toolbar_icon("minus", Message::Density(-1))
                    .aria_label("Smaller thumbnails")
                    .enabled(app.density > 0)
                    .id(iced::widget::Id::new("photos-service-smaller")),
                enz_toolbar_icon("plus", Message::Density(1))
                    .aria_label("Larger thumbnails")
                    .enabled(app.density < 7)
                    .id(iced::widget::Id::new("photos-service-larger")),
            ]
            .spacing(4.0),
        ))
        .align_right(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(20.0)
        .into()
    };
    let progress: Element<'a, Message> = if state.busy && state.page.is_some() {
        container(pulse()).center_y(Length::Fill).into()
    } else {
        Space::new().into()
    };
    iced::widget::stack![scrolling, selection, density, progress].into()
}

struct RemoteRows<'a> {
    state: &'a crate::app::cloud::State,
    columns: usize,
}
impl RemoteRows<'_> {
    fn count(&self) -> usize {
        self.state.albums.as_ref().map_or_else(
            || self.state.page.as_ref().map_or(0, |page| page.assets.len()),
            Vec::len,
        )
    }
}
impl enzui_iced_core::virtual_sequence::ItemProvider<usize> for RemoteRows<'_> {
    fn len(&self) -> u64 {
        (self.count().div_ceil(self.columns) + 2) as u64
    }
    fn item(&self, index: u64) -> Option<usize> {
        (index < self.len()).then_some(index as usize)
    }
}
impl enzui_iced_core::virtual_sequence::KeyedProvider<usize> for RemoteRows<'_> {
    fn key(&self, index: u64) -> Option<enzui_iced_core::virtual_sequence::SequenceKey> {
        use enzui_iced_core::virtual_sequence::ItemProvider;
        if index >= self.len() {
            return None;
        }
        let id = if index == 0 {
            "service-top".into()
        } else if index + 1 == self.len() {
            "service-end".into()
        } else {
            let first = (index as usize - 1) * self.columns;
            self.state
                .albums
                .as_ref()
                .and_then(|albums| albums.get(first))
                .map(|album| format!("album-{}", album.id))
                .or_else(|| {
                    self.state
                        .page
                        .as_ref()
                        .and_then(|page| page.assets.get(first))
                        .map(|(asset, _)| format!("asset-{}", asset.id))
                })?
        };
        Some(enzui_iced_core::virtual_sequence::SequenceKey::new(id))
    }
    fn index_of(&self, key: &enzui_iced_core::virtual_sequence::SequenceKey) -> Option<u64> {
        use enzui_iced_core::virtual_sequence::ItemProvider;
        (0..self.len()).find(|index| self.key(*index).as_ref() == Some(key))
    }
}

fn library_grid<'a>(
    app: &'a Photos,
    geometry: &enzui_iced_widgets::app_shell::ShellGeometry,
) -> Element<'a, Message> {
    let side =
        [64.0, 88.0, 112.0, 144.0, 176.0, 224.0, 280.0, 352.0][usize::from(app.density.min(7))];
    let top = geometry.content_inset[0] + 8.0;
    let mut insets = geometry.scrollbar_safe_area();
    insets.top += 8.0;
    insets.bottom = insets.bottom.max(68.0);
    iced::widget::responsive(move |size| {
        use enzui_iced_core::virtual_sequence::ItemProvider;
        let available = (size.width - 40.0).max(1.0);
        let columns = (((available + 12.0) / (side + 12.0)).floor() as usize).clamp(1, 24);
        let cell = ((available - 12.0 * (columns - 1) as f32) / columns as f32).max(1.0);
        let albums = app.cloud.albums.is_some();
        let pitch = side + if albums { 34.0 } else { 16.0 };
        let source = std::rc::Rc::new(RemoteRows {
            state: &app.cloud,
            columns,
        });
        let rows = source.len() as usize;
        let heights: Vec<_> = (0..rows)
            .map(|index| {
                if index == 0 {
                    top
                } else if index + 1 == rows {
                    88.0
                } else {
                    pitch
                }
            })
            .collect();
        let generation = app.cloud.generation;
        enzui_iced_widgets::virtual_list::enz_virtual_list_variable(
            source,
            heights,
            move |_, index| {
                if *index == 0 {
                    return Space::new().height(top).into();
                }
                if *index + 1 == rows {
                    return load_footer(app);
                }
                let start = (*index - 1) * columns;
                let mut line = row![].spacing(12.0);
                if let Some(albums) = &app.cloud.albums {
                    for album in albums.iter().skip(start).take(columns) {
                        line = line.push(container(album_card(app, album, side)).width(cell));
                    }
                } else if let Some(page) = &app.cloud.page {
                    for (asset, thumbnail) in page.assets.iter().skip(start).take(columns) {
                        let id = asset.id.clone();
                        let tile = super::media_tile::view(
                            app,
                            super::media_tile::Tile {
                                paths: vec![],
                                key: format!("remote-{}", asset.id),
                                label: asset.label(),
                                thumbnail: thumbnail.clone(),
                                loading: !app.cloud.failed_previews.contains(&asset.id),
                                side,
                                selected: app.cloud.selection.contains(&asset.id),
                                video: asset.video,
                                open: event(app, Event::Activate(id.clone(), false)),
                                select: event(app, Event::Select(id.clone())),
                            },
                            move |m| {
                                Message::Cloud(Event::Owned(
                                    generation,
                                    Box::new(Event::Activate(id.clone(), m.control() || m.shift())),
                                ))
                            },
                        );
                        line = line.push(container(tile).width(cell));
                    }
                }
                container(line)
                    .padding(iced::Padding::ZERO.left(16.0).right(24.0))
                    .height(pitch)
                    .width(Length::Fill)
                    .into()
            },
        )
        .viewport_height(size.height)
        .self_scrolling(app.cloud.scroll.clone())
        .on_scroll(move |viewport| {
            let offset = (viewport.absolute_offset().y - top).max(0.0);
            let first = (offset / pitch).floor() as usize;
            let end = ((offset + viewport.bounds().height) / pitch).ceil() as usize + 2;
            Message::Cloud(Event::Owned(
                generation,
                Box::new(Event::Window(
                    first.saturating_sub(2) * columns,
                    end * columns,
                )),
            ))
        })
        .scrollbar_tokens(app.theme.theme().scrollbar)
        .scrollbar_insets(insets)
        .cache_capacity(0)
        .id(iced::widget::Id::new("photos-service-scroll"))
        .into_element()
    })
    .into()
}

fn album_card<'a>(
    app: &'a Photos,
    album: &'a crate::services::remote::RemoteAlbum,
    side: f32,
) -> Element<'a, Message> {
    let cover: Element<'_, Message> = if let Some(handle) = app.cloud.album_covers.get(&album.id) {
        enzui_iced_widgets::image::enz_image_handle(handle.clone())
            .width(Length::Fill)
            .height(Length::Fixed(side - 48.0))
            .fit(enzui_iced_widgets::image::EnzImageFit::Cover)
            .radius(app.theme.theme().radius.step(app.theme.theme().shape.media))
            .alt_text(format!("Cover of {}", album.name))
            .into()
    } else {
        container(enz_icon("album").size((side - 48.0).clamp(16.0, 48.0)))
            .center_x(Length::Fill)
            .center_y(side - 48.0)
            .into()
    };
    let card = container(
        column![
            cover,
            enz_text(&album.name).lines(1),
            enz_text(format!("{} items", album.count))
                .size(TextSize::Footnote)
                .tone(TextTone::Muted)
        ]
        .spacing(4.0),
    )
    .padding(12.0);
    enz_pressable(card.into(), event(app, Event::Album(Some(album.clone()))))
        .style(PressableStyle::Plain)
        .aria_label(format!("Open album {}", album.name))
        .id(iced::widget::Id::from(format!(
            "photos-service-album-{}",
            album.id
        )))
        .into()
}

fn load_footer(app: &Photos) -> Element<'_, Message> {
    if let Some(error) = &app.cloud.load_error {
        return container(
            row![
                enz_text(error).size(TextSize::Footnote).width(Length::Fill),
                enz_toolbar_icon("refresh-cw", event(app, Event::RetryMore))
                    .aria_label("Retry loading")
                    .tooltip("Retry loading")
                    .id(iced::widget::Id::new("photos-service-retry-loading"))
            ]
            .spacing(8.0),
        )
        .padding(16.0)
        .into();
    }
    if let Some(error) = &app.cloud.preview_error {
        return container(
            row![
                enz_text("Some previews unavailable")
                    .size(TextSize::Footnote)
                    .width(Length::Fill),
                super::settings::info("service-preview-error", "preview error", error),
                enz_toolbar_icon("refresh-cw", event(app, Event::RetryPreviews))
                    .aria_label("Retry previews")
                    .tooltip("Retry previews")
                    .id(iced::widget::Id::new("photos-service-retry-previews"))
            ]
            .spacing(4.0),
        )
        .padding(16.0)
        .into();
    }
    if app.cloud.loading_more {
        return pulse();
    }
    if app
        .cloud
        .page
        .as_ref()
        .is_some_and(|page| page.assets.is_empty())
        || app.cloud.albums.as_ref().is_some_and(Vec::is_empty)
    {
        return container(enz_text("No matching photos or albums").tone(TextTone::Muted))
            .center_x(Length::Fill)
            .padding(16.0)
            .into();
    }
    Space::new().into()
}
