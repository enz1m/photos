use enzui_iced_core::localization::text;
use enzui_iced_widgets::action_bar::ActionItem;
use enzui_iced_widgets::app_shell::{
    ShellLayout, ShellTokens, SidebarHeader, enz_app_shell_with_geometry,
};
use enzui_iced_widgets::search_field::enz_titlebar_search;
use enzui_iced_widgets::sidebar_content::enz_sidebar_content;
use enzui_iced_widgets::sidebar_row::enz_sidebar_row;
use enzui_iced_widgets::title_bar::enz_title_bar;
use enzui_iced_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::Space;
use iced::{Element, Length};

use crate::app::{Destination, Dialog, Grouping, Message, Photos};

pub(crate) const SEARCH_TRIGGER_ID: &str = "photos-search-open";
const WIDE_SEARCH_MIN: f32 = 900.0;

fn tokens() -> ShellTokens {
    ShellTokens {
        sidebar_width: 232.0,
        content_minimum: 600.0,
        sidebar_header: SidebarHeader::Inside,
        page_inset: 0.0,
        ..ShellTokens::inset_panes()
            .gap(4.0)
            .frame_material(enzui_iced_theme::MaterialStyle::FlatFrosted)
    }
}

fn search_minimum_width(app: &Photos) -> f32 {
    let labels = [
        text("photos-all"),
        text("photos-months"),
        text("photos-years"),
    ];
    let tabs = super::controls::glass_tabs_width(
        app,
        &[
            ("all", &labels[0]),
            ("months", &labels[1]),
            ("years", &labels[2]),
        ],
    );
    WIDE_SEARCH_MIN.max(
        app.theme.theme().sizing.search_field_width + 2.0 * (tabs + app.theme.theme().spacing.lg()),
    )
}

fn normal_geometry(app: &Photos) -> enzui_iced_widgets::app_shell::ShellGeometry {
    enzui_iced_widgets::app_shell::enz_app_shell::<Message>(|| Space::new().into())
        .layout(ShellLayout::FloatingTitlebarSidebar)
        .state(app.nav)
        .tokens(tokens())
        .breakpoints(app.theme.theme().breakpoints)
        .titlebar_island(enzui_iced_theme::MaterialStyle::Regular, || {
            Space::new().into()
        })
        .geometry_for(app.viewport_width.get())
}

pub(crate) fn wide_search(app: &Photos) -> bool {
    normal_geometry(app).content_width >= search_minimum_width(app)
}

pub(crate) fn view(app: &Photos) -> Element<'_, Message> {
    let theme = app.theme.theme();
    let normal = normal_geometry(app);
    let base = tokens().show_close(
        !app.viewer
            .as_ref()
            .is_some_and(|viewer| viewer.editing || viewer.video_editor.is_some()),
    );
    // Hiding the island must retain its shell-owned band and pane geometry.
    let search_tokens = ShellTokens {
        titlebar_height: normal.content_inset[0]
            - base.effective_edge_inset()
            - base.effective_pane_gutter(),
        window_radius: normal
            .content_corner_radii
            .into_iter()
            .fold(0.0_f32, f32::max),
        show_close: false,
        ..base
    };
    let searching = app.search_open || (app.cloud.open && app.cloud.search_open);
    let mut shell = enz_app_shell_with_geometry(move |geometry| {
        if app.cloud.open {
            titlebar_fade(app, super::cloud::view(app, geometry), geometry)
        } else if app.viewer.is_some() {
            super::file_target(app, super::viewer::view(app, geometry), None)
        } else if app.opening_file {
            iced::widget::container(
                enzui_iced_widgets::text::enz_text(text("photos-opening"))
                    .tone(enzui_iced_widgets::text::TextTone::Muted),
            )
            .center(Length::Fill)
            .into()
        } else {
            titlebar_fade(app, super::gallery::view(app, *geometry), geometry)
        }
    })
    .layout(ShellLayout::FloatingTitlebarSidebar)
    .tokens(if searching { search_tokens } else { base })
    .breakpoints(theme.breakpoints)
    .state(app.nav)
    .sidebar_label(text("photos-title"))
    .titlebar_with_geometry(move |geometry| titlebar(app, geometry.content_width))
    .titlebar_scrim(app.viewer.is_none() && !(app.cloud.open && app.cloud.preview.is_some()))
    .sidebar_toggle_tooltip(false)
    .sidebar_titlebar(move || enz_title_bar().title(text("photos-title")).into_element())
    .sidebar(move || sidebar(app))
    .on_sidebar_state(Message::Sidebar)
    .on_close(Message::CloseRequested)
    .on_window_gesture(Message::Window);
    let chrome_inset = enzui_iced_widgets::window_controls::trailing_inset_for_square_corner(
        search_tokens.titlebar_height,
        super::controls::CHROME_HEIGHT,
    );
    shell = shell.titlebar_background_with_geometry(move |geometry| {
        if geometry.sidebar != enzui_iced_widgets::nav_split::SidebarPresentation::Hidden
            || !geometry.shows_toggle
        {
            return Space::new().into();
        }
        let size = super::controls::CHROME_HEIGHT;
        iced::widget::container(
            enzui_iced_widgets::glass::enz_glass(Space::new().width(size).height(size))
                .style(enzui_iced_theme::MaterialStyle::Regular)
                .radius(size / 2.0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(if tokens().is_rtl() {
            iced::Padding::ZERO.right(chrome_inset)
        } else {
            iced::Padding::ZERO.left(chrome_inset)
        })
        .align_y(iced::Alignment::Center)
        .align_x(if tokens().is_rtl() {
            iced::alignment::Horizontal::Right
        } else {
            iced::alignment::Horizontal::Left
        })
        .into()
    });
    if normal.sidebar == enzui_iced_widgets::nav_split::SidebarPresentation::Hidden {
        let icon =
            enzui_iced_widgets::toolbar_icon::ToolbarIconTokens::from_palette(&theme.palette)
                .button_size;
        shell = shell.sidebar_toggle_side_inset(
            chrome_inset + (super::controls::CHROME_HEIGHT - icon) / 2.0,
        );
    }
    if searching {
        shell = shell.sidebar_available(
            normal.sidebar == enzui_iced_widgets::nav_split::SidebarPresentation::Inline,
        );
    } else {
        shell = shell.titlebar_island(enzui_iced_theme::MaterialStyle::Regular, || {
            action_island(app)
        });
    }
    shell.into_element()
}

fn titlebar(app: &Photos, width: f32) -> Element<'_, Message> {
    if app.cloud.open {
        return super::cloud::titlebar(app, width);
    }
    if app.search_open {
        return enz_title_bar()
            .leading(super::controls::glass_chrome(
                enz_toolbar_icon("arrow-left", Message::CloseSearch)
                    .id(iced::widget::Id::new("photos-search-back"))
                    .aria_label(text("photos-close-search")),
            ))
            .push(search_field(app, true))
            .fluid_content(120.0)
            .id("photos-window")
            .into_element();
    }
    if app.viewer.is_some() {
        return super::viewer::titlebar(app);
    }
    let bar = library_titlebar(app, width >= 700.0);
    if width < search_minimum_width(app) {
        return bar;
    }
    let field_width = app.theme.theme().sizing.search_field_width;
    let height = enzui_iced_widgets::title_bar::shell_band_height()
        .unwrap_or(enzui_iced_widgets::app_shell::DEFAULT_SHELL_TITLEBAR_HEIGHT);
    let field_height = enzui_iced_widgets::text_field::TextFieldSize::Md
        .tokens()
        .height;
    // The shell's island sits outside this slot, so centering uses the published pane width.
    iced::widget::stack![
        bar,
        iced::widget::pin(iced::widget::container(search_field(app, false)).width(field_width))
            .x((width - field_width) / 2.0)
            .y((height - field_height) / 2.0)
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn search_field(app: &Photos, focus_on_mount: bool) -> Element<'_, Message> {
    let field = enz_titlebar_search(&app.query, Message::Search)
        .size(enzui_iced_widgets::text_field::TextFieldSize::Md)
        .responsive(false)
        .chrome(enzui_iced_widgets::search_field::SearchFieldChrome::Ghost)
        .radius(
            app.theme
                .theme()
                .radius
                .step(app.theme.theme().shape.button_group),
        )
        .placeholder(text("photos-search"))
        .id(iced::widget::Id::new("photos-search"))
        .width(Length::Fill)
        .max_width(if focus_on_mount {
            f32::MAX
        } else {
            app.theme.theme().sizing.search_field_width
        })
        .focus_on_mount(focus_on_mount)
        .on_clear(Message::Search(String::new()))
        .into_element();
    iced::widget::container(super::controls::glass_chrome(field))
        .id(iced::widget::Id::new("photos-search-surface"))
        .width(Length::Fill)
        .into()
}

fn library_titlebar(app: &Photos, wide: bool) -> Element<'_, Message> {
    let mut bar = enz_title_bar();
    if let Some((_, _, label)) = &app.period_filter {
        return bar
            .leading(super::controls::glass_chrome(
                iced::widget::row![
                    enz_toolbar_icon("arrow-left", Message::ClearPeriod)
                        .id(iced::widget::Id::new("photos-period-back"))
                        .aria_label(if app.grouping == Grouping::Years {
                            "Back to years"
                        } else {
                            "Back to months"
                        }),
                    iced::widget::container(
                        enzui_iced_widgets::text::enz_text(label)
                            .weight(iced::font::Weight::Semibold)
                    )
                    .padding(iced::Padding::ZERO.right(12.0)),
                ]
                .spacing(8.0)
                .align_y(iced::Alignment::Center),
            ))
            .id("photos-window")
            .into_element();
    }
    if app.destination != Destination::Albums {
        let labels = [
            text(if wide {
                "photos-all"
            } else {
                "photos-all-compact"
            }),
            text("photos-months"),
            text("photos-years"),
        ];
        let selected = match app.grouping {
            Grouping::All => 0,
            Grouping::Months => 1,
            Grouping::Years => 2,
        };
        let on_select = |index| {
            Message::Grouping(match index {
                1 => Grouping::Months,
                2 => Grouping::Years,
                _ => Grouping::All,
            })
        };
        let grouping = if wide {
            super::controls::glass_tabs(
                app,
                &[
                    ("all", &labels[0]),
                    ("months", &labels[1]),
                    ("years", &labels[2]),
                ],
                selected,
                on_select,
                "Library grouping",
                false,
            )
        } else {
            let picker =
                enzui_iced_widgets::combo_box::enz_combo_box("", labels.to_vec(), None, |_| {
                    Message::Grouping(Grouping::All)
                })
                .selected_index(Some(selected))
                .on_select_index(on_select)
                .id(iced::widget::Id::new("photos-grouping"))
                .id_prefix("photos-grouping")
                .aria_label("Library grouping")
                .size(enzui_iced_widgets::text_field::TextFieldSize::Sm)
                .variant(enzui_iced_widgets::tokens::FieldVariant::Outlined)
                .radius(
                    app.theme
                        .theme()
                        .radius
                        .step(app.theme.theme().shape.button_group),
                )
                .width(84.0);
            enzui_iced_widgets::glass::enz_glass(
                iced::widget::container(picker)
                    .width(84.0)
                    .height(28.0)
                    .align_y(iced::Alignment::Center),
            )
            .style(enzui_iced_theme::MaterialStyle::Regular)
            .radius(14.0)
            .into_element()
        };
        bar = bar.leading(grouping);
    }
    bar.id("photos-window").into_element()
}

fn library_actions(app: &Photos) -> Vec<ActionItem<'_, Message>> {
    if app.cloud.open {
        use crate::app::cloud::Event;
        let state = &app.cloud;
        let mut actions = Vec::new();
        if state.preview.is_none()
            && state.albums.is_none()
            && normal_geometry(app).content_width < 900.0
        {
            actions.push(super::controls::icon_action(
                "photos-service-actions",
                "photos-service-search-open",
                "search",
                "Search this library",
                super::cloud::event(app, Event::FocusSearch),
                !state.busy && state.connected(),
            ));
        }
        if state.preview.is_some() || !state.selection.is_empty() {
            actions.push(super::controls::icon_action(
                "photos-service-actions",
                "photos-service-import",
                "download",
                if state.preview.is_some() {
                    "Download original"
                } else {
                    "Download selected originals"
                },
                super::cloud::event(app, Event::Import),
                !state.busy,
            ));
        }
        if state.busy {
            actions.push(super::controls::icon_action(
                "photos-service-actions",
                "photos-service-cancel",
                "x",
                "Cancel request",
                super::cloud::event(app, Event::Cancel),
                state.cancellable(),
            ));
        } else if state.preview.is_none() {
            actions.push(super::controls::icon_action(
                "photos-service-actions",
                "photos-service-refresh",
                "refresh-cw",
                "Refresh library",
                super::cloud::event(
                    app,
                    if state.albums.is_some() {
                        Event::Albums
                    } else {
                        Event::Refresh
                    },
                ),
                state.connected(),
            ));
        }
        if state.album.is_some() && state.preview.is_none() {
            actions.push(super::controls::icon_action(
                "photos-service-actions",
                "photos-service-import-album",
                "folder-down",
                "Download album",
                super::cloud::event(app, Event::ImportAlbum),
                !state.busy,
            ));
        }
        actions.push(super::controls::icon_action(
            "photos-service-actions",
            "photos-service-manage",
            "settings",
            "Service settings",
            super::cloud::event(app, Event::Configure),
            !state.busy,
        ));
        return actions;
    }
    [
        (
            "photos-backup",
            "cloud-upload",
            "Upload to service…".into(),
            Message::Cloud(crate::app::cloud::Event::Backup(match app.destination {
                Destination::Album(id) => Some(id),
                _ => None,
            })),
            !app.selection.is_empty() || matches!(app.destination, Destination::Album(_)),
        ),
        (
            "photos-import",
            "import",
            text("photos-import"),
            action_dialog(
                app,
                "photos-library-actions",
                "photos-import",
                Dialog::Import,
            ),
            true,
        ),
        (
            "photos-refresh",
            "refresh-cw",
            text("photos-rescan"),
            Message::Refresh,
            !app.scanning,
        ),
        (
            "photos-settings",
            "settings",
            text("photos-settings"),
            action_dialog(
                app,
                "photos-library-actions",
                "photos-settings",
                Dialog::Settings,
            ),
            true,
        ),
    ]
    .into_iter()
    .map(|(id, icon, label, message, enabled)| {
        super::controls::icon_action("photos-library-actions", id, icon, label, message, enabled)
    })
    .collect()
}

fn action_island(app: &Photos) -> Element<'_, Message> {
    use enzui_iced_widgets::action_bar::{Collector, CollectorPlacement, enz_action_bar};
    let (mut actions, prefix) = if app.cloud.open {
        (library_actions(app), "photos-service-actions")
    } else if app.viewer.is_some() {
        (super::viewer::actions(app), "photos-viewer-actions")
    } else {
        (library_actions(app), "photos-library-actions")
    };
    if !app.cloud.open && app.viewer.is_none() && !wide_search(app) {
        use enzui_iced_core::{
            Action,
            action_overflow::{Descriptor, Presentation},
        };
        let active = !app.query.is_empty();
        actions.insert(
            0,
            ActionItem::with_inline(
                Descriptor::new("photos-search-open")
                    .pinned()
                    .with_presentation(Presentation::Compact),
                Action::new("photos-search-open", Message::OpenSearch)
                    .with_label(text("photos-search"))
                    .with_icon(enzui_iced_core::Icon::named("search")),
                move |action, _| {
                    enz_toolbar_icon("search", action.message().clone())
                        .id(iced::widget::Id::new(SEARCH_TRIGGER_ID))
                        .label(action.label())
                        .active(active)
                        .into_element()
                },
            ),
        );
    }
    let control = enzui_iced_widgets::toolbar_icon::TitlebarAffordanceTokens::default()
        .effective_control_size();
    let width = actions.len() as f32 * control + actions.len().saturating_sub(1) as f32 * 4.0;
    iced::widget::container(
        enz_action_bar(actions, Message::ToggleOverflow)
            .collector(Collector::Kebab {
                icon: "ellipsis-vertical".into(),
                tokens: None,
            })
            .collector_placement(CollectorPlacement::TrailingCluster)
            .gap(4.0)
            .overflow_open(app.overflow_open)
            .on_dismiss(Message::CloseOverflow)
            .menu_id_prefix(prefix)
            .overflow_trigger_id(iced::widget::Id::from(format!("{prefix}-overflow")))
            .into_element(),
    )
    .width(
        Length::Fill.max(if app.cloud.open && app.viewport_width.get() < 700.0 {
            width.min(112.0)
        } else {
            width
        }),
    )
    .into()
}

fn sidebar(app: &Photos) -> Element<'_, Message> {
    let mut sidebar = enz_sidebar_content().spacing_scale(app.theme.theme().spacing);
    for (destination, label, icon, count) in [
        (
            Destination::Library,
            text("photos-library"),
            "images",
            app.library.photos.len(),
        ),
        (
            Destination::Favorites,
            text("photos-favorites"),
            "heart",
            app.catalog.favorites.len(),
        ),
        (
            Destination::Imports,
            text("photos-imports"),
            "import",
            app.catalog.imported.len(),
        ),
        (
            Destination::Albums,
            text("photos-albums"),
            "album",
            app.catalog.albums.len(),
        ),
    ] {
        let id = match destination {
            Destination::Library => "photos-nav-library",
            Destination::Favorites => "photos-nav-favorites",
            Destination::Imports => "photos-nav-imports",
            Destination::Albums => "photos-nav-albums",
            _ => unreachable!(),
        };
        sidebar = sidebar.push(
            enz_sidebar_row(label, Message::Navigate(destination.clone()))
                .id(iced::advanced::widget::Id::new(id))
                .id_prefix(id)
                .on_context({
                    let destination = destination.clone();
                    move |at| {
                        Message::OpenContext(
                            crate::app::context::Target::Sidebar(destination.clone()),
                            at,
                        )
                    }
                })
                .icon(icon)
                .count(nonzero_count(count))
                .selected(!app.cloud.open && app.destination == destination.clone())
                .reduced_motion(app.theme.reduced_motion()),
        );
    }
    sidebar = sidebar.push_section(text("photos-my-albums"));
    for album in &app.catalog.albums {
        sidebar = sidebar.push(super::file_target(
            app,
            enz_sidebar_row(&album.name, Message::Navigate(Destination::Album(album.id)))
                .id(iced::advanced::widget::Id::from(format!(
                    "photos-nav-album-{}",
                    album.id
                )))
                .id_prefix(format!("photos-nav-album-{}", album.id))
                .on_context(move |at| {
                    Message::OpenContext(
                        crate::app::context::Target::Sidebar(Destination::Album(album.id)),
                        at,
                    )
                })
                .icon("album")
                .count(nonzero_count(album.photos.len()))
                .selected(!app.cloud.open && app.destination == Destination::Album(album.id))
                .reduced_motion(app.theme.reduced_motion())
                .into(),
            Some(album.id),
        ));
    }
    sidebar = sidebar.push(
        enz_sidebar_row(text("photos-new-album"), Message::Dialog(Dialog::NewAlbum))
            .id(iced::advanced::widget::Id::new("photos-nav-new-album"))
            .icon("plus"),
    );
    sidebar = sidebar.push(
        iced::widget::container(
            iced::widget::row![
                enzui_iced_widgets::section_header::enz_section_header(text("photos-folders"))
                    .width(Length::Shrink),
                Space::new().width(Length::Fill),
                enz_toolbar_icon("folder-plus", Message::Dialog(Dialog::Sources))
                    .id(iced::widget::Id::new("photos-sources"))
                    .size(32.0, 16.0)
                    .aria_label(text("photos-sources")),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(
            iced::Padding::ZERO
                .top(16.0)
                .left(enzui_iced_widgets::sidebar_row::default_label_inset_x()),
        ),
    );
    sidebar = sidebar.push(super::folder_target(
        app,
        enz_sidebar_row(
            "Home / Photos",
            Message::Navigate(Destination::Source(app.paths.photos.clone())),
        )
        .id(iced::advanced::widget::Id::new("photos-nav-home"))
        .on_context(move |at| {
            Message::OpenContext(
                crate::app::context::Target::Sidebar(Destination::Source(app.paths.photos.clone())),
                at,
            )
        })
        .count(nonzero_count(app.folder_count(&app.paths.photos)))
        .id_prefix("photos-nav-home")
        .icon("folder-heart")
        .selected(
            !app.cloud.open && app.destination == Destination::Source(app.paths.photos.clone()),
        )
        .reduced_motion(app.theme.reduced_motion())
        .into(),
        app.paths.photos.clone(),
    ));
    for source in &app.catalog.sources {
        let label = source.file_name().unwrap_or_default().to_string_lossy();
        sidebar = sidebar.push(super::folder_target(
            app,
            enz_sidebar_row(
                label,
                Message::Navigate(Destination::Source(source.clone())),
            )
            .id(iced::advanced::widget::Id::from(format!(
                "photos-nav-source-{}",
                photos_library::catalog::PhotoId::for_path(source).0
            )))
            .on_context(move |at| {
                Message::OpenContext(
                    crate::app::context::Target::Sidebar(Destination::Source(source.clone())),
                    at,
                )
            })
            .count(nonzero_count(app.folder_count(source)))
            .id_prefix(format!(
                "photos-nav-source-{}",
                photos_library::catalog::PhotoId::for_path(source).0
            ))
            .icon("folder")
            .selected(!app.cloud.open && app.destination == Destination::Source(source.clone()))
            .reduced_motion(app.theme.reduced_motion())
            .into(),
            source.clone(),
        ));
    }
    sidebar = sidebar.push_section("Services");
    for account in &app.catalog.services {
        sidebar = sidebar.push(
            enz_sidebar_row(
                &account.name,
                Message::Cloud(crate::app::cloud::Event::Open(account.id)),
            )
            .id(iced::widget::Id::from(format!(
                "photos-nav-service-{}",
                account.id
            )))
            .icon("server")
            .selected(app.cloud.open && app.cloud.active == Some(account.id)),
        );
    }
    sidebar = sidebar.push(
        enz_sidebar_row("Add service", Message::Cloud(crate::app::cloud::Event::New))
            .id(iced::widget::Id::new("photos-nav-services"))
            .icon("plus"),
    );
    enzui_iced_widgets::scroll_view::enz_scroll_view(sidebar.into_element()).into()
}

pub(super) fn action_dialog(app: &Photos, prefix: &str, command: &str, dialog: Dialog) -> Message {
    let invoker = if app.overflow_open {
        iced::widget::Id::from(format!("{prefix}-overflow"))
    } else {
        enzui_iced_core::identity::OwnerId::of_kind(
            enzui_iced_widgets::action_bar::COMMAND_ID_KIND,
            prefix,
        )
        .id(command)
    };
    Message::DialogFrom(dialog, invoker)
}

fn titlebar_fade<'a>(
    app: &Photos,
    content: Element<'a, Message>,
    geometry: &enzui_iced_widgets::app_shell::ShellGeometry,
) -> Element<'a, Message> {
    let tint = iced::Color {
        a: 0.12,
        ..app.theme.theme().palette.background
    };
    let fade = iced::widget::container(Space::new())
        .width(Length::Fill)
        .height(geometry.content_inset[0] * 2.0)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Gradient(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                    .add_stop(0.0, tint)
                    .add_stop(0.5, tint)
                    .add_stop(1.0, iced::Color { a: 0.0, ..tint })
                    .into(),
            )),
            ..Default::default()
        });
    iced::widget::stack![content, fade]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// A sidebar count label, empty (no chip) for zero: a column of zeros is
/// noise, as Mail and macOS Photos show by leaving empty collections bare.
fn nonzero_count(n: impl Into<usize>) -> String {
    let n = n.into();
    if n == 0 { String::new() } else { n.to_string() }
}
