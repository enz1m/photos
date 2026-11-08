use enzui_iced_core::localization::text;
use enzui_iced_widgets::button::{ButtonKind, ButtonSize, enz_button};
use enzui_iced_widgets::slider::enz_slider;
use enzui_iced_widgets::text::{TextSize, TextTone, enz_text};
use enzui_iced_widgets::text_field::enz_text_field;
use enzui_iced_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{Column, Space, column, container, row};
use iced::{Element, Length};

use crate::app::{Adjustment, EditorTab, Message, Photos};

fn ratio_shape(
    app: &Photos,
    preset: u8,
    label: &'static str,
    ratio: f32,
) -> Element<'static, Message> {
    let (width, height) = if ratio >= 1.0 {
        (72.0, (72.0 / ratio).max(32.0))
    } else {
        (64.0 * ratio, 64.0)
    };
    let mut tokens =
        enzui_iced_widgets::pressable::PressableTokens::from_palette(&app.theme.theme().palette);
    tokens.card_bg = iced::Color::TRANSPARENT;
    tokens.card_border = iced::Color {
        a: 0.45,
        ..tokens.text
    };
    let shape = enzui_iced_widgets::pressable::enz_pressable(
        container(enz_text(label).size(TextSize::Footnote))
            .center_x(width)
            .center_y(height)
            .into(),
        Message::CropPreset(preset),
    )
    .id(iced::advanced::widget::Id::from(format!(
        "photos-crop-preset-{preset}"
    )))
    .aria_label(format!("Crop {label}"))
    .style(enzui_iced_widgets::pressable::PressableStyle::Card)
    .tokens(tokens)
    .padding(0.0)
    .width(Length::Fixed(width))
    .radius(app.theme.theme().radius.step(app.theme.theme().shape.card));
    container(shape)
        .center_x(Length::Fill)
        .center_y(80.0)
        .into()
}

pub(crate) fn view(app: &Photos, compact: bool) -> Element<'_, Message> {
    let Some(viewer) = &app.viewer else {
        return Space::new().into();
    };
    let switcher = super::controls::glass_tabs(
        app,
        &[("adjust", "Adjust"), ("crop", "Crop"), ("resize", "Resize")],
        match viewer.tab {
            EditorTab::Adjust => 0,
            EditorTab::Crop => 1,
            EditorTab::Resize => 2,
        },
        |index| {
            Message::EditorTab(match index {
                1 => EditorTab::Crop,
                2 => EditorTab::Resize,
                _ => EditorTab::Adjust,
            })
        },
        "Editing tools",
        true,
    );
    let mut controls = Column::new().spacing(12.0);
    match viewer.tab {
        EditorTab::Adjust => {
            for (adjustment, label) in [
                (Adjustment::Exposure, "Exposure"),
                (Adjustment::Brightness, "Brightness"),
                (Adjustment::Contrast, "Contrast"),
                (Adjustment::Highlights, "Highlights"),
                (Adjustment::Shadows, "Shadows"),
                (Adjustment::Saturation, "Saturation"),
                (Adjustment::Temperature, "Warmth"),
                (Adjustment::Tint, "Tint"),
                (Adjustment::Sharpness, "Sharpness"),
            ] {
                let range = if adjustment == Adjustment::Exposure {
                    -3.0..=3.0
                } else if adjustment == Adjustment::Sharpness {
                    0.0..=100.0
                } else {
                    -100.0..=100.0
                };
                controls = controls.push(
                    enz_slider(range, adjustment.value(&viewer.recipe), move |value| {
                        Message::Adjust(adjustment, value)
                    })
                    .label(label)
                    .show_value(true)
                    .value_format(move |v| {
                        if adjustment == Adjustment::Exposure {
                            format!("{v:+.1} EV")
                        } else {
                            format!("{v:+.0}")
                        }
                    })
                    .step_marks(false)
                    .track_height(4.0)
                    .thumb_width(16.0)
                    .thumb_height(16.0),
                );
            }
            controls = controls.push(
                enz_button(
                    if viewer.recipe.monochrome {
                        "Black & white · On"
                    } else {
                        "Black & white"
                    },
                    Message::Adjust(
                        Adjustment::Monochrome,
                        if viewer.recipe.monochrome { 0.0 } else { 1.0 },
                    ),
                )
                .kind(ButtonKind::Quiet)
                .id(iced::advanced::widget::Id::new("photos-monochrome"))
                .size(ButtonSize::Sm),
            );
        }
        EditorTab::Crop => {
            let original_ratio = viewer.loaded.as_ref().map_or(1.5, |source| {
                let (w, h) = source.pixels.dimensions();
                if viewer.recipe.quarter_turns % 2 == 1 {
                    h as f32 / w.max(1) as f32
                } else {
                    w as f32 / h.max(1) as f32
                }
            });
            controls = controls
                .push(
                    row![
                        enz_button(text("photos-rotate"), Message::Rotate)
                            .fill_width(true)
                            .id(iced::advanced::widget::Id::new("photos-rotate"))
                            .icon("rotate-cw")
                            .kind(ButtonKind::Quiet)
                            .size(ButtonSize::Sm),
                        enz_button(text("photos-flip"), Message::Flip)
                            .fill_width(true)
                            .id(iced::advanced::widget::Id::new("photos-flip"))
                            .icon("flip-horizontal-2")
                            .kind(ButtonKind::Quiet)
                            .size(ButtonSize::Sm)
                    ]
                    .spacing(8.0),
                )
                .push(
                    enz_text(text("photos-aspect-ratio"))
                        .size(TextSize::Footnote)
                        .tone(TextTone::Muted),
                );
            controls = controls
                .push(
                    row![
                        ratio_shape(app, 0, "Original", original_ratio),
                        ratio_shape(app, 1, "1:1", 1.0),
                        ratio_shape(app, 2, "4:3", 4.0 / 3.0)
                    ]
                    .spacing(8.0),
                )
                .push(
                    row![
                        ratio_shape(app, 3, "16:9", 16.0 / 9.0),
                        ratio_shape(app, 4, "3:4", 3.0 / 4.0),
                        Space::new().width(Length::Fill)
                    ]
                    .spacing(8.0),
                );
            controls = controls.push(
                enz_button(text("photos-free-crop"), Message::DrawCrop)
                    .icon("crop")
                    .id(iced::advanced::widget::Id::new("photos-free-crop"))
                    .kind(ButtonKind::Quiet)
                    .size(ButtonSize::Sm)
                    .fill_width(true),
            );
            if viewer.drawing_crop {
                controls = controls.push(
                    row![
                        Space::new().width(Length::Fill),
                        enz_button(text("photos-cancel"), Message::CancelCrop)
                            .id(iced::advanced::widget::Id::new("photos-crop-cancel"))
                            .kind(ButtonKind::Quiet)
                            .size(ButtonSize::Sm),
                        enz_button(text("photos-apply-crop"), Message::ApplyCrop)
                            .icon("check")
                            .kind(ButtonKind::Quiet)
                            .id(iced::advanced::widget::Id::new("photos-crop-apply"))
                            .size(ButtonSize::Sm),
                    ]
                    .spacing(8.0),
                );
            }
        }
        EditorTab::Resize => {
            controls = controls
                .push(
                    enz_text(text("photos-output-size"))
                        .size(TextSize::Footnote)
                        .weight(iced::font::Weight::Semibold),
                )
                .push(
                    row![
                        enz_text_field(
                            "Width · px",
                            "Width",
                            &viewer.width_input,
                            Message::ResizeWidth
                        )
                        .id(iced::advanced::widget::Id::new("photos-resize-width")),
                        container(
                            enz_toolbar_icon(
                                if viewer.lock_aspect { "link" } else { "unlink" },
                                Message::LockAspect(!viewer.lock_aspect)
                            )
                            .id(iced::advanced::widget::Id::new("photos-resize-lock"))
                            .aria_label(if viewer.lock_aspect {
                                "Unlock aspect ratio"
                            } else {
                                "Lock aspect ratio"
                            })
                        )
                        .padding(iced::Padding::ZERO.top(22.0)),
                        enz_text_field(
                            "Height · px",
                            "Height",
                            &viewer.height_input,
                            Message::ResizeHeight
                        )
                        .id(iced::advanced::widget::Id::new("photos-resize-height")),
                    ]
                    .spacing(6.0)
                    .align_y(iced::Alignment::Center),
                )
                .push(
                    container(
                        enz_button(text("photos-apply-size"), Message::ApplyResize)
                            .id(iced::advanced::widget::Id::new("photos-resize-apply"))
                            .icon("check")
                            .kind(ButtonKind::Quiet)
                            .size(ButtonSize::Sm),
                    )
                    .align_right(Length::Fill),
                );
        }
    }
    let reset = enz_button(text("photos-reset-edits"), Message::ResetEdit)
        .id(iced::advanced::widget::Id::new("photos-edit-reset"))
        .icon("rotate-ccw")
        .kind(ButtonKind::Quiet)
        .size(ButtonSize::Sm);
    let body = column![
        switcher,
        enzui_iced_widgets::scroll_view::enz_scroll_view(controls).height(Length::Fill),
        row![Space::new().width(Length::Fill), reset],
    ]
    .spacing(20.0);
    container(body)
        .padding(20.0)
        .width(if compact {
            Length::Fill
        } else {
            Length::Fixed(310.0)
        })
        .height(if compact {
            Length::Fixed(300.0)
        } else {
            Length::Fill
        })
        .into()
}
