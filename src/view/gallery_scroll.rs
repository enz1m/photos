use std::rc::Rc;

use enzui_iced_core::virtual_grid::TileGrid;
use enzui_iced_core::virtual_sequence::{ItemProvider, KeyedProvider, SequenceKey};
use enzui_iced_widgets::text::{TextSize, enz_text};
use enzui_iced_widgets::virtual_list::enz_virtual_list_variable;
use iced::widget::{Row, Space, container, image, responsive};
use iced::{Element, Length};
use photos_library::catalog::GallerySections;

use crate::app::{Destination, Grouping, Message, Photos};

pub(crate) const TOP_INSET: f32 =
    enzui_iced_widgets::app_shell::DEFAULT_SHELL_TITLEBAR_HEIGHT + 8.0;
const HEADER_HEIGHT: f32 = 40.0;
type Tile = Option<image::Handle>;

#[derive(Clone)]
enum RowKind {
    Inset,
    Heading(String),
    Tiles { start: usize, count: usize },
}

#[derive(Clone)]
struct RowItem {
    kind: RowKind,
    tiles: Vec<(u64, Tile)>,
}

struct Rows<'a, P> {
    source: &'a P,
    plan: Vec<RowKind>,
}

impl<P: KeyedProvider<Tile>> ItemProvider<RowItem> for Rows<'_, P> {
    fn len(&self) -> u64 {
        self.plan.len() as u64
    }

    fn item(&self, index: u64) -> Option<RowItem> {
        let kind = self.plan.get(index as usize)?.clone();
        let tiles = if let RowKind::Tiles { start, count } = &kind {
            (*start..*start + *count)
                .filter_map(|index| {
                    self.source
                        .item(index as u64)
                        .map(|tile| (index as u64, tile))
                })
                .collect()
        } else {
            Vec::new()
        };
        Some(RowItem { kind, tiles })
    }
}

impl<P: KeyedProvider<Tile>> KeyedProvider<RowItem> for Rows<'_, P> {
    fn key(&self, index: u64) -> Option<SequenceKey> {
        match self.plan.get(index as usize)? {
            RowKind::Inset => Some(SequenceKey::new("gallery-top-inset")),
            RowKind::Heading(label) => Some(SequenceKey::new(format!("gallery-date-{label}"))),
            RowKind::Tiles { start, .. } => self.source.key(*start as u64),
        }
    }

    fn index_of(&self, key: &SequenceKey) -> Option<u64> {
        (0..self.len()).find(|index| self.key(*index).as_ref() == Some(key))
    }
}

fn section_label(timestamp: i64, sections: GallerySections) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0).map_or_else(
        || "Undated".into(),
        |date| {
            date.format(if sections == GallerySections::Day {
                "%e %B %Y"
            } else {
                "%B %Y"
            })
            .to_string()
            .trim()
            .to_owned()
        },
    )
}

fn plan(app: &Photos, length: usize, columns: usize) -> Vec<RowKind> {
    let sections = if app.grouping == Grouping::All && app.destination != Destination::Albums {
        app.catalog.settings.sections
    } else {
        GallerySections::None
    };
    let mut rows = vec![RowKind::Inset];
    let mut start = 0;
    while start < length {
        let end = if sections == GallerySections::None {
            length
        } else {
            let label = section_label(
                app.library.photos[app.library.visible[start]].captured_at,
                sections,
            );
            rows.push(RowKind::Heading(label.clone()));
            (start + 1..length)
                .find(|index| {
                    section_label(
                        app.library.photos[app.library.visible[*index]].captured_at,
                        sections,
                    ) != label
                })
                .unwrap_or(length)
        };
        for row in (start..end).step_by(columns) {
            rows.push(RowKind::Tiles {
                start: row,
                count: columns.min(end - row),
            });
        }
        start = end;
    }
    rows
}

pub(crate) fn tiles<'a, P, F>(
    app: &'a Photos,
    source: &'a P,
    shell_geometry: enzui_iced_widgets::app_shell::ShellGeometry,
    grid: TileGrid,
    build: F,
) -> Element<'a, Message>
where
    P: KeyedProvider<Tile> + 'a,
    F: Fn(u64, &Tile) -> Element<'a, Message> + 'a,
{
    let build = Rc::new(build);
    let scrollbar = app.theme.theme().scrollbar.normalized();
    let mut scrollbar_insets = shell_geometry.scrollbar_safe_area();
    scrollbar_insets.top += app.theme.theme().spacing.sm();
    scrollbar_insets.bottom = scrollbar_insets.bottom.max(
        super::gallery::CONTROLS_INSET
            + super::controls::CHROME_HEIGHT
            + app.theme.theme().spacing.sm(),
    );
    responsive(move |size| {
        let geometry = grid.stretch_for_width((size.width - scrollbar.hit_width).max(1.0));
        app.library.geometry.set(geometry);
        let plan = plan(app, source.len() as usize, geometry.columns.max(1));
        let pitch = geometry.tile_height + geometry.gap_y;
        let mut y = 0.0;
        let mut tile_rows = Vec::new();
        let heights: Vec<_> = plan
            .iter()
            .map(|kind| {
                let height = match kind {
                    RowKind::Inset => TOP_INSET,
                    RowKind::Heading(_) => HEADER_HEIGHT,
                    RowKind::Tiles { start, count } => {
                        tile_rows.push((y, *start, *count));
                        pitch
                    }
                };
                y += height;
                height
            })
            .collect();
        *app.library.tile_rows.borrow_mut() = tile_rows;
        let source = Rc::new(Rows { source, plan });
        let build = Rc::clone(&build);
        enz_virtual_list_variable(source, heights, move |_, item| match &item.kind {
            RowKind::Inset => Space::new().height(TOP_INSET).into(),
            RowKind::Heading(label) => container(
                enz_text(label)
                    .size(TextSize::Body)
                    .weight(iced::font::Weight::Semibold),
            )
            .center_y(HEADER_HEIGHT)
            .padding(iced::Padding::ZERO.left(4.0))
            .width(Length::Fill)
            .into(),
            RowKind::Tiles { .. } => {
                let mut row = Row::new().spacing(geometry.gap_x);
                for (index, tile) in &item.tiles {
                    row = row.push(
                        container(build(*index, tile))
                            .width(geometry.tile_width)
                            .height(geometry.tile_height),
                    );
                }
                container(row).width(Length::Fill).height(pitch).into()
            }
        })
        .viewport_height(size.height)
        .self_scrolling(app.library.scroll.clone())
        .id(iced::widget::Id::new("photos-grid-scroll"))
        .on_scroll(Message::GridScroll)
        .scrollbar_tokens(scrollbar)
        .scrollbar_insets(scrollbar_insets)
        .cache_capacity(0)
        .into_element()
    })
    .into()
}
