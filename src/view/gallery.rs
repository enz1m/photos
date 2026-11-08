use super::gallery_scroll::tiles;
use enzui_iced_core::localization::text;
use enzui_iced_widgets::button::{ButtonKind, ButtonSize, enz_button};
use enzui_iced_widgets::icon::enz_icon;
use enzui_iced_widgets::image::{EnzImageFit, enz_image_handle};
use enzui_iced_widgets::pressable::{PressableStyle, enz_pressable};
use enzui_iced_widgets::text::{TextSize, TextTone, enz_text};
use enzui_iced_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Length};

use crate::app::{Destination, Dialog, Grouping, Message, Photos};

pub(crate) const CONTROLS_INSET: f32 = 20.0;

pub(crate) fn view(
    app: &Photos,
    geometry: enzui_iced_widgets::app_shell::ShellGeometry,
) -> Element<'_, Message> {
    let sp = app.theme.theme().spacing;
    let count = app.library.visible.len();
    let content = if app.destination == Destination::Albums {
        albums(app, geometry)
    } else if count == 0 {
        empty(app)
    } else if app.grouping != Grouping::All && app.period_filter.is_none() {
        periods(app, geometry)
    } else {
        grid(app, geometry)
    };
    let body: Element<'_, Message> = container(content)
        .id("photos-gallery-pane")
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: sp.sm(),
        })
        .height(Length::Fill)
        .width(Length::Fill)
        .into();
    let body = super::file_target(
        app,
        body,
        match app.destination {
            Destination::Album(id) => Some(id),
            _ => None,
        },
    );
    if (app.grouping != Grouping::All && app.period_filter.is_none())
        || app.destination == Destination::Albums
    {
        return body;
    }
    let controls = super::controls::glass_chrome(
        row![
            enz_toolbar_icon("minus", Message::Density(-1))
                .id(iced::advanced::widget::Id::new("photos-density-smaller"))
                .aria_label(text("photos-smaller-thumbnails"))
                .enabled(app.density > 0),
            enz_toolbar_icon("plus", Message::Density(1))
                .id(iced::advanced::widget::Id::new("photos-density-larger"))
                .aria_label(text("photos-larger-thumbnails"))
                .enabled(app.density < 7)
        ]
        .spacing(4.0),
    );
    let selection: Element<'_, Message> = if app.selection.is_empty() {
        Space::new().into()
    } else {
        let actions = enzui_iced_widgets::glass::enz_glass(
            container(
                enzui_iced_widgets::view_that_fits::enz_view_that_fits(vec![
                    Box::new(move || {
                        container(selection_actions(app, false))
                            .width(Length::Fill.min(440.0))
                            .into()
                    }),
                    Box::new(move || selection_actions(app, true)),
                ])
                .id(iced::widget::Id::new("photos-selection-layout")),
            )
            .id("photos-selection-bar")
            .width(Length::Fill)
            .padding(8.0),
        )
        .style(enzui_iced_theme::MaterialStyle::Regular)
        .radius(
            app.theme
                .theme()
                .radius
                .step(app.theme.theme().shape.button_group),
        );
        container(
            enzui_iced_widgets::clamp::enz_clamp(
                actions,
                enzui_iced_widgets::clamp::ClampSize::Wide,
            )
            .max_width(540.0)
            .into_element(),
        )
        .center_x(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(iced::Padding {
            top: 12.0,
            right: 12.0,
            bottom: 20.0,
            left: 12.0,
        })
        .into()
    };
    let density: Element<'_, Message> = if app.selection.is_empty() {
        container(
            iced::widget::mouse_area(controls)
                .on_press(Message::Ignore)
                .on_release(Message::Ignore),
        )
        .align_right(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(CONTROLS_INSET)
        .into()
    } else {
        Space::new().into()
    };
    iced::widget::stack![body, selection, density].into()
}

fn selection_actions(app: &Photos, compact: bool) -> Element<'_, Message> {
    let clear = enz_toolbar_icon("x", Message::SelectionMode(false))
        .id(iced::advanced::widget::Id::new("photos-selection-mode"))
        .aria_label(text("photos-clear-selection"));
    let selected = app
        .selection
        .selected_in_order(app.library.click_model.order())
        .len();
    let count = enz_text(format!("{selected} selected"))
        .size(TextSize::Body)
        .weight(iced::font::Weight::Semibold);
    let favorite = enz_toolbar_icon("heart", Message::FavoriteSelection)
        .id(iced::advanced::widget::Id::new("photos-selection-favorite"))
        .aria_label(text("photos-favorite-selected-photos"))
        .enabled(selected > 0);
    let album = enz_button(
        text("photos-add-to-album"),
        Message::Dialog(Dialog::AddToAlbum),
    )
    .id(iced::advanced::widget::Id::new("photos-selection-album"))
    .icon("folder-plus")
    .kind(ButtonKind::Quiet)
    .size(enzui_iced_widgets::button::ButtonSize::Sm)
    .enabled(selected > 0);
    let backup = enz_toolbar_icon(
        "cloud-upload",
        Message::Cloud(crate::app::cloud::Event::Backup(None)),
    )
    .id(iced::widget::Id::new("photos-selection-backup"))
    .aria_label("Upload to service…")
    .tooltip("Upload to service…")
    .enabled(selected > 0);
    if compact {
        column![
            row![clear, count]
                .spacing(8.0)
                .align_y(Alignment::Center)
                .width(Length::Fill),
            row![Space::new().width(Length::Fill), favorite, backup, album]
                .spacing(8.0)
                .align_y(Alignment::Center)
                .width(Length::Fill)
        ]
        .spacing(4.0)
        .width(Length::Fill)
        .into()
    } else {
        row![
            clear,
            count,
            Space::new().width(Length::Fill),
            favorite,
            backup,
            album
        ]
        .spacing(8.0)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
    }
}

impl enzui_iced_core::virtual_sequence::ItemProvider<Option<iced::widget::image::Handle>>
    for Photos
{
    fn len(&self) -> u64 {
        if self.destination == Destination::Albums {
            self.library.albums.len() as u64
        } else {
            self.library.groups.len() as u64
        }
    }
    fn item(&self, index: u64) -> Option<Option<iced::widget::image::Handle>> {
        let index = usize::try_from(index).ok()?;
        let cover = if self.destination == Destination::Albums {
            self.library.albums.get(index)?.cover
        } else {
            self.library.groups.get(index)?.indices.first().copied()
        };
        let Some(photo) = cover.and_then(|index| self.library.photos.get(index)) else {
            return Some(None);
        };
        Some(
            self.library
                .cache
                .borrow_mut()
                .get(&crate::services::thumbnail_key(
                    photo,
                    self.library.recipes.get(&photo.id),
                    crate::services::THUMBNAIL_LIMIT,
                )),
        )
    }
}

impl enzui_iced_core::virtual_sequence::KeyedProvider<Option<iced::widget::image::Handle>>
    for Photos
{
    fn key(&self, index: u64) -> Option<enzui_iced_core::virtual_sequence::SequenceKey> {
        if self.destination == Destination::Albums {
            return self
                .library
                .albums
                .get(usize::try_from(index).ok()?)
                .map(|album| {
                    enzui_iced_core::virtual_sequence::SequenceKey::new(format!(
                        "album-{}",
                        album.id
                    ))
                });
        }
        self.library
            .groups
            .get(usize::try_from(index).ok()?)
            .map(|group| {
                enzui_iced_core::virtual_sequence::SequenceKey::new(group.start.to_string())
            })
    }
    fn index_of(&self, key: &enzui_iced_core::virtual_sequence::SequenceKey) -> Option<u64> {
        if self.destination == Destination::Albums {
            let id = key.as_str().strip_prefix("album-")?.parse::<u64>().ok()?;
            return self
                .library
                .albums
                .iter()
                .position(|album| album.id == id)
                .map(|index| index as u64);
        }
        let start = key.as_str().parse::<i64>().ok()?;
        self.library
            .groups
            .iter()
            .position(|group| group.start == start)
            .map(|index| index as u64)
    }
}
impl enzui_iced_core::virtual_grid::TileProvider<Option<iced::widget::image::Handle>> for Photos {
    fn retention_hint(&self) -> Option<usize> {
        Some(0)
    }
}

fn periods(
    app: &Photos,
    geometry: enzui_iced_widgets::app_shell::ShellGeometry,
) -> Element<'_, Message> {
    tiles(
        app,
        app,
        geometry,
        enzui_iced_core::virtual_grid::TileGrid::new(280.0, 240.0).with_spacing(16.0, 20.0),
        move |index, cover| {
            let Some(group) = app.library.groups.get(index as usize) else {
                return Space::new().into();
            };
            let image: Element<'_, Message> = if let Some(cover) = cover {
                enz_image_handle(cover.clone())
                    .radius(app.theme.theme().radius.step(app.theme.theme().shape.media))
                    .height(Length::Fixed(180.0))
                    .into()
            } else {
                container(enz_icon("images").size(32.0))
                    .center_x(Length::Fill)
                    .center_y(Length::Fixed(180.0))
                    .into()
            };
            enz_pressable(
                column![
                    image,
                    container(column![
                        enz_text(&group.label).size(TextSize::Headline),
                        enz_text(enzui_iced_core::localized!("photos-photo-count", "count" => group.indices.len()))
                            .tone(TextTone::Muted)
                            .size(TextSize::Footnote)
                    ].spacing(4.0).align_x(iced::Alignment::End))
                        .align_x(iced::Alignment::End)
                        .width(Length::Fill)
                        .padding([6.0, 12.0])
                ]
                .spacing(6.0)
                .into(),
                Message::Period(group.start, group.end, group.label.clone()),
            )
            .style(PressableStyle::Plain)
            .aria_label(format!("Browse {}", group.label))
            .id(iced::advanced::widget::Id::from(format!(
                "photos-period-{}",
                group.start
            )))
            .into()
        },
    )
}

fn grid(
    app: &Photos,
    geometry: enzui_iced_widgets::app_shell::ShellGeometry,
) -> Element<'_, Message> {
    let side =
        [64.0, 88.0, 112.0, 144.0, 176.0, 224.0, 280.0, 352.0][usize::from(app.density.min(7))];
    let grid = enzui_iced_core::virtual_grid::TileGrid::new(side, side).with_spacing(12.0, 16.0);
    let source = &app.library;
    let grid = tiles(app, source, geometry, grid, move |index, thumbnail| {
        let Some(photo) = app
            .library
            .visible
            .get(index as usize)
            .and_then(|index| app.library.photos.get(*index))
        else {
            return Space::new().into();
        };
        let thumbnail = thumbnail.clone();
        let selected = app.selection.contains(&photo.id);
        let tile = super::media_tile::view(
            app,
            super::media_tile::Tile {
                paths: if selected {
                    app.library
                        .photos
                        .iter()
                        .filter(|photo| app.selection.contains(&photo.id))
                        .map(|photo| photo.path.clone())
                        .collect()
                } else {
                    vec![photo.path.clone()]
                },
                key: photo.id.0.clone(),
                label: photo.name.clone(),
                thumbnail,
                loading: false,
                side,
                selected,
                video: photos_library::scan::is_video_path(&photo.path),
                open: Message::Open(photo.id.clone()),
                select: Message::TileClicked(
                    photo.id.clone(),
                    enzui_iced_core::list_selection::ClickModifiers::TOGGLE,
                ),
            },
            move |m| {
                Message::TileClicked(
                    photo.id.clone(),
                    enzui_iced_core::list_selection::ClickModifiers {
                        toggle: m.control(),
                        range: m.shift(),
                    },
                )
            },
        );
        enzui_iced_widgets::context_target::enz_context_target(tile, move |at| {
            Message::OpenContext(crate::app::context::Target::Photo(photo.id.clone()), at)
        })
        .id(iced::advanced::widget::Id::from(format!(
            "photos-context-{}",
            photo.id.0
        )))
        .label(format!("Actions for {}", photo.name))
        .into()
    });
    enzui_iced_widgets::drag_autoscroll_host::enz_drag_autoscroll_host(
        super::selection_surface::surface(app, grid),
        iced::widget::Id::new("photos-grid-scroll"),
        enzui_iced_core::drag_autoscroll::AutoScroll::new(32.0, 480.0),
    )
    .on_scrolled(Message::AutoScrolled)
    .width(Length::Fill)
    .height(Length::Fill)
    .into_element()
}

fn empty(app: &Photos) -> Element<'_, Message> {
    let (heading, body) = if !app.query.is_empty() {
        (
            "No matching photos",
            "Try a different filename, folder, or camera.",
        )
    } else {
        match app.destination {
            Destination::Favorites => (
                "Your favorites, all together",
                "Open a photo and choose the heart to keep it here.",
            ),
            Destination::Album(_) => (
                "This album is ready for photos",
                "Open any photo and use Add to album to collect it here.",
            ),
            Destination::Imports => (
                "Bring your photos home",
                "Import a folder to copy its photos into your home Photos folder.",
            ),
            _ => (
                "A home for your photos",
                "Photos reads your home Photos folder. Import pictures or add a folder to get started.",
            ),
        }
    };
    container(
        column![
            enz_icon("images").size(48.0),
            enz_text(heading).size(TextSize::Title2),
            enz_text(body).tone(TextTone::Muted),
            enz_button(text("photos-import"), Message::Dialog(Dialog::Import))
                .id(iced::advanced::widget::Id::new("photos-empty-import"))
                .icon("import")
                .size(ButtonSize::Sm),
        ]
        .spacing(16.0)
        .align_x(Alignment::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn albums(
    app: &Photos,
    geometry: enzui_iced_widgets::app_shell::ShellGeometry,
) -> Element<'_, Message> {
    if app.library.albums.is_empty() {
        return container(
            enz_text(if app.query.trim().is_empty() {
                "Create an album to collect your favorite moments."
            } else {
                "No matching albums. Try another name."
            })
            .tone(TextTone::Muted),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(24.0)
        .into();
    }
    tiles(
        app,
        app,
        geometry,
        enzui_iced_core::virtual_grid::TileGrid::new(240.0, 238.0).with_spacing(16.0, 20.0),
        move |index, cover| {
            let Some(album) = app.library.albums.get(index as usize) else {
                return Space::new().into();
            };
            let image: Element<'_, Message> = if let Some(handle) = cover {
                enz_image_handle(handle.clone())
                    .fit(EnzImageFit::Cover)
                    .radius(app.theme.theme().radius.step(app.theme.theme().shape.media))
                    .height(Length::Fixed(180.0))
                    .into()
            } else {
                container(enz_icon("album").size(40.0))
                    .center_x(Length::Fill)
                    .center_y(Length::Fixed(180.0))
                    .into()
            };
            enz_pressable(
                column![
                    image,
                    enz_text(&album.name).size(TextSize::Headline).lines(1),
                    enz_text(
                        enzui_iced_core::localized!("photos-photo-count", "count" => album.count)
                    )
                    .tone(TextTone::Muted)
                    .size(TextSize::Footnote)
                ]
                .spacing(6.0)
                .into(),
                Message::Navigate(Destination::Album(album.id)),
            )
            .style(PressableStyle::Plain)
            .aria_label(format!("Open album {}", album.name))
            .id(iced::advanced::widget::Id::from(format!(
                "photos-album-{}",
                album.id
            )))
            .into()
        },
    )
}
