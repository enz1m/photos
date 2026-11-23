use enzui_iced_theme::Palette;
use enzui_iced_widgets::tab_pill::{
    TabLabelWeight, TabListWidth, TabPillItem, TabPillTokens, TabSelectionStyle, enz_tab_list,
};
use iced::{Element, Length};

use crate::app::{Message, Photos};

pub(crate) fn glass_tabs<'a>(
    app: &Photos,
    labels: &[(&str, &str)],
    selected: usize,
    on_select: impl Fn(usize) -> Message + 'a,
    name: &str,
    stretch: bool,
) -> Element<'a, Message> {
    let cell_minimum = tab_cell_minimum(app, labels);
    let tabs = enz_tab_list(
        labels
            .iter()
            .map(|(key, label)| TabPillItem::new(*label).key(*key)),
        selected,
        on_select,
    )
    // FilledAccent keeps the pinned selection colors when glass supplies its draw-time palette.
    .selection_indicator(TabSelectionStyle::FilledAccent)
    .tokens(neutral_tab_tokens(app))
    .cell_min_width(cell_minimum)
    .label_weight(TabLabelWeight::AlwaysSemibold)
    .width_policy(if stretch {
        TabListWidth::Fill
    } else {
        TabListWidth::Hug
    })
    .aria_label(name)
    .id(iced::advanced::widget::Id::from(format!(
        "photos-tabs-{name}"
    )));
    // The selected pill sits inset in the track by an even gap, with
    // concentric corners (see `neutral_tab_tokens`).
    enzui_iced_widgets::glass::enz_glass(iced::widget::container(tabs).padding(TRACK_INSET).width(
        if stretch {
            Length::Fill
        } else {
            Length::Shrink
        },
    ))
    .style(enzui_iced_theme::MaterialStyle::Regular)
    .radius(
        app.theme
            .theme()
            .radius
            .step(app.theme.theme().shape.button_group),
    )
    .into_element()
}

pub(crate) fn icon_action<'a>(
    prefix: &'static str,
    id: &'static str,
    icon: &'static str,
    label: impl Into<String>,
    message: Message,
    enabled: bool,
) -> enzui_iced_widgets::action_bar::ActionItem<'a, Message> {
    use enzui_iced_core::{
        Action,
        action_overflow::{Descriptor, Presentation},
    };
    enzui_iced_widgets::action_bar::ActionItem::with_inline(
        Descriptor::new(id)
            .with_presentation(Presentation::Compact)
            .with_enabled(enabled),
        Action::new(id, message)
            .with_label(label)
            .with_icon(enzui_iced_core::Icon::named(icon))
            .with_enabled(enabled),
        move |action, _| {
            enzui_iced_widgets::toolbar_icon::enz_toolbar_icon(icon, action.message().clone())
                .label(action.label())
                .enabled(action.is_enabled())
                .id(enzui_iced_core::identity::OwnerId::of_kind(
                    enzui_iced_widgets::action_bar::COMMAND_ID_KIND,
                    prefix,
                )
                .id(id))
                .into_element()
        },
    )
}

pub(crate) const CHROME_HEIGHT: f32 =
    enzui_iced_widgets::window_controls::DEFAULT_CONTROL_SIZE + 8.0;

pub(crate) fn glass_chrome<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    enzui_iced_widgets::glass::enz_glass(
        iced::widget::container(content)
            .height(CHROME_HEIGHT)
            .align_y(iced::Alignment::Center)
            .padding([0.0, 4.0]),
    )
    .style(enzui_iced_theme::MaterialStyle::Regular)
    .radius(CHROME_HEIGHT / 2.0)
    .into_element()
}

fn tab_cell_minimum(app: &Photos, labels: &[(&str, &str)]) -> f32 {
    let geometry = TabPillTokens::from_palette(&app.theme.theme().palette);
    let typography = enzui_iced_widgets::text_measure::MeasureTypography::new(
        enzui_iced_widgets::text::TextSize::Body,
    )
    .pin_size_px(geometry.label_size)
    .weight(iced::font::Weight::Semibold);
    labels
        .iter()
        .map(|(_, label)| {
            enzui_iced_widgets::text_measure::enz_text_measure_width(label, typography)
                + 2.0 * geometry.pill_padding_x
        })
        .fold(0.0_f32, f32::max)
}

pub(super) fn glass_tabs_width(app: &Photos, labels: &[(&str, &str)]) -> f32 {
    let geometry = TabPillTokens::from_palette(&app.theme.theme().palette);
    labels.len() as f32 * tab_cell_minimum(app, labels)
        + labels.len().saturating_sub(1) as f32 * geometry.strip_gap
        + 2.0 * TRACK_INSET
}

const TRACK_INSET: f32 = 3.0;

fn neutral_tab_tokens(app: &Photos) -> TabPillTokens {
    let theme = app.theme.theme();
    let palette = &theme.palette;
    let increased_contrast = palette.is_high_contrast()
        || enzui_iced_widgets::theme_lookup::material_policy(&app.theme()).increased_contrast;
    let selected_bg = palette.solid_backdrop(palette.surface_selected);
    let contrast_floor = if increased_contrast { 7.0 } else { 4.5 };

    TabPillTokens {
        selected_bg,
        selected_fg: Palette::brand_text_on_at(selected_bg, palette.text, contrast_floor),
        selected_edge: if increased_contrast {
            palette.text
        } else {
            iced::Color::TRANSPARENT
        },
        // Pinned tab tokens default to 9 px, so follow the surrounding track's shape.
        pill_radius: theme.radius.step(theme.shape.button),
        ..TabPillTokens::from_palette(palette)
    }
}
