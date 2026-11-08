use crate::app::{Message, Photos};
use enzui_iced_widgets::icon::enz_icon;
use enzui_iced_widgets::image::{EnzImageFit, enz_image_handle};
use enzui_iced_widgets::pressable::{PressableStyle, enz_pressable};
use enzui_iced_widgets::text::{TextSize, enz_text};
use enzui_iced_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{container, image::Handle};
use iced::{Element, Length};

pub(super) struct Tile {
    pub key: String,
    pub label: String,
    pub paths: Vec<std::path::PathBuf>,
    pub thumbnail: Option<Handle>,
    pub loading: bool,
    pub side: f32,
    pub selected: bool,
    pub video: bool,
    pub open: Message,
    pub select: Message,
}

pub(super) fn view<'a>(
    app: &'a Photos,
    tile: Tile,
    activate: impl Fn(iced::keyboard::Modifiers) -> Message + 'a,
) -> Element<'a, Message> {
    let activate = std::rc::Rc::new(activate);
    enzui_iced_widgets::hover_group::enz_hover_group_with(move |hovered| -> Element<'a, Message> {
        let picture: Element<'a, Message> = if let Some(handle) = &tile.thumbnail {
            enz_image_handle(handle.clone())
                .fit(if app.catalog.settings.contain_thumbnails {
                    EnzImageFit::Contain
                } else {
                    EnzImageFit::Cover
                })
                .radius(app.theme.theme().radius.step(app.theme.theme().shape.media))
                .width(Length::Fill)
                .height(Length::Fixed(tile.side))
                .alt_text(tile.label.clone())
                .into()
        } else if tile.loading {
            use enzui_iced_widgets::skeleton::{SkeletonKind, SkeletonTokens, enz_skeleton};
            let mut tokens = SkeletonTokens::from_palette(&app.theme.theme().palette);
            tokens.card_height = tile.side;
            tokens.text_min_width = 1.0;
            tokens.card_radius = app.theme.theme().radius.step(app.theme.theme().shape.media);
            enz_skeleton(SkeletonKind::Card)
                .tokens(tokens)
                .label(format!("Loading {}", tile.label))
                .into_element()
        } else {
            container(enz_icon(if tile.video { "video" } else { "image" }).size(32.0))
                .center_x(Length::Fill)
                .center_y(tile.side)
                .into()
        };
        let visible = hovered && app.catalog.settings.show_names;
        let caption = container(
            enz_text(tile.label.clone())
                .size(TextSize::Footnote)
                .lines(1)
                .color(if visible {
                    iced::Color::WHITE
                } else {
                    iced::Color::TRANSPARENT
                }),
        )
        .width(Length::Fill)
        .height(64.0)
        .align_bottom(64.0)
        .padding([10.0, 8.0])
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Gradient(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                    .add_stop(0.0, iced::Color::TRANSPARENT)
                    .add_stop(0.25, iced::Color::TRANSPARENT)
                    .add_stop(
                        0.7,
                        iced::Color::from_rgba(0.0, 0.0, 0.0, if visible { 0.22 } else { 0.0 }),
                    )
                    .add_stop(
                        1.0,
                        iced::Color::from_rgba(0.0, 0.0, 0.0, if visible { 0.55 } else { 0.0 }),
                    )
                    .into(),
            )),
            ..Default::default()
        });
        let mut tokens = enzui_iced_widgets::toolbar_icon::ToolbarIconTokens::from_palette(
            &app.theme.theme().palette,
        );
        tokens.icon_color = iced::Color::WHITE;
        tokens.hover_color = iced::Color::WHITE;
        tokens.pressed_color = iced::Color::WHITE;
        tokens.active_color = iced::Color::WHITE;
        tokens.active_background = iced::Color {
            a: 0.38,
            ..app.theme.theme().palette.accent
        };
        tokens.hover_background = iced::Color::from_rgba(0.0, 0.0, 0.0, 0.22);
        let check = enz_toolbar_icon(
            if tile.selected {
                "circle-check"
            } else {
                "circle"
            },
            tile.select.clone(),
        )
        .size(28.0, 22.0)
        .tokens(tokens)
        .active(tile.selected)
        .aria_label(format!(
            "{} {}",
            if tile.selected { "Deselect" } else { "Select" },
            tile.label
        ))
        .id(iced::widget::Id::from(format!("photos-check-{}", tile.key)));
        let activate = activate.clone();
        let open = tile.open.clone();
        let base = enz_pressable(picture, tile.open.clone())
            .on_activate_with_modifiers(move |m| activate(m))
            .on_enter(move || open.clone())
            .style(PressableStyle::Plain)
            .selected(tile.selected)
            .padding(0.0)
            .aria_label(format!("Open {}", tile.label))
            .id(iced::widget::Id::from(format!("photos-tile-{}", tile.key)));
        let mut layers = iced::widget::stack![
            base,
            container(caption)
                .align_bottom(Length::Fill)
                .height(Length::Fixed(tile.side))
        ];
        if tile.video {
            layers = layers.push(
                container(enz_icon("play").size(18.0))
                    .padding(8.0)
                    .align_bottom(Length::Fixed(tile.side))
                    .align_right(Length::Fill),
            );
        }
        if hovered || tile.selected {
            layers = layers.push(container(check).padding(4.0));
        }
        enzui_iced_widgets::file_drag::enz_file_drag_source(layers, tile.paths.clone())
            .key(&tile.key)
            .allows_captured_press(true)
            .into()
    })
    .style(|_, _| enzui_iced_widgets::hover_group::HoverGroupStyle::NONE)
    .into()
}
