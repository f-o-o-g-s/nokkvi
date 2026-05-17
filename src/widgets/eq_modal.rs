//! Equalizer Modal Widget
//!
//! A modal overlay displaying the 10-band graphic equalizer.
//! Triggered via hotkey [Q] or from the player bar EQ button.

use iced::{
    Alignment, Element, Length,
    widget::{
        button, column, container, mouse_area, opaque, pick_list, row, space, svg, text, tooltip,
    },
};

use crate::{
    theme,
    widgets::{eq_slider::eq_slider, hover_overlay::HoverOverlay},
};

// =============================================================================
// State & Messages
// =============================================================================

/// State for the equalizer modal overlay.
///
/// Mirrors the sibling pattern established by `InfoModalState` and
/// `AboutModalState` — modal state lives next to its widget rendering
/// code, not in the catch-all `WindowState` struct. Lift target for
/// the four EQ-modal fields that previously lived on `WindowState`.
#[derive(Debug, Default)]
pub struct EqModalState {
    /// Whether the EQ modal overlay is currently visible.
    pub open: bool,
    /// Whether the modal is in "save preset" mode (showing name input).
    pub save_mode: bool,
    /// Text input content for the preset name being saved.
    pub save_name: String,
    /// Cached custom EQ presets (loaded from redb, kept in sync on save/delete).
    pub custom_presets: Vec<nokkvi_data::audio::eq::CustomEqPreset>,
}

#[derive(Debug, Clone)]
pub enum EqModalMessage {
    Open,
    Close,
    Toggle,
    ToggleEnabled,
    GainChanged(usize, f32),
    PresetSelected(PresetChoice),
    ResetAll,
    /// Enter save mode — show inline name input
    SavePreset,
    /// Text input changed while saving
    SavePresetNameChanged(String),
    /// Confirm save with current name
    SavePresetConfirm,
    /// Cancel save mode
    CancelSave,
    /// Delete a custom preset by its index in the custom list
    DeletePreset(usize),
}

/// Unified preset choice for the pick_list — can be builtin or custom.
#[derive(Debug, Clone, PartialEq)]
pub enum PresetChoice {
    Builtin(usize), // index into BUILTIN_PRESETS
    Custom(usize),  // index into custom_eq_presets
}

impl std::fmt::Display for PresetChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Builtin(i) => {
                if let Some(preset) = nokkvi_data::audio::eq::BUILTIN_PRESETS.get(*i) {
                    write!(f, "{}", preset.name)
                } else {
                    write!(f, "Preset {i}")
                }
            }
            Self::Custom(i) => write!(f, "★ Custom #{}", i + 1),
        }
    }
}

// =============================================================================
// View
// =============================================================================

const MODAL_WIDTH: f32 = 640.0;
const BAND_FREQS: [&str; 10] = [
    "32", "64", "125", "250", "500", "1K", "2K", "4K", "8K", "16K",
];

pub(crate) fn eq_modal_overlay<'a>(
    visible: bool,
    eq_enabled: bool,
    eq_gains: [f32; 10],
    custom_presets: &[nokkvi_data::audio::eq::CustomEqPreset],
    save_mode: bool,
    save_name: &str,
) -> Option<Element<'a, EqModalMessage>> {
    if !visible {
        return None;
    }

    // ── Header ──────────────────────────────────────────────────
    let title_text = text("Equalizer")
        .size(20.0)
        .font(iced::font::Font {
            weight: iced::font::Weight::Bold,
            ..theme::ui_font()
        })
        .color(theme::accent_bright());
    // Icon-only button helper — view header pattern:
    // mouse_area(HoverOverlay(container(svg).center()))
    let svg_btn = |icon_path: &'static str,
                   icon_color: iced::Color,
                   size: f32,
                   msg: EqModalMessage|
     -> Element<'a, EqModalMessage> {
        let icon = crate::embedded_svg::svg_widget(icon_path)
            .width(Length::Fixed(size * 0.5))
            .height(Length::Fixed(size * 0.5))
            .style(move |_theme, _status| svg::Style {
                color: Some(icon_color),
            });

        mouse_area(
            HoverOverlay::new(
                container(icon)
                    .width(Length::Fixed(size))
                    .height(Length::Fixed(size))
                    .style(|_theme| container::Style {
                        background: Some(theme::bg0_hard().into()),
                        border: iced::Border {
                            radius: theme::ui_border_radius(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .center(Length::Fixed(size)),
            )
            .border_radius(theme::ui_border_radius()),
        )
        .on_press(msg)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
    };

    let text_btn = |label: &'static str,
                    color: iced::Color,
                    msg: EqModalMessage|
     -> Element<'a, EqModalMessage> {
        button(text(label).size(13.0).font(theme::ui_font()))
            .on_press(msg)
            .padding([4, 10])
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => theme::bg2(),
                    _ => theme::bg0_hard(),
                };
                button::Style {
                    background: Some(bg.into()),
                    border: iced::Border {
                        color,
                        width: 1.0,
                        radius: theme::ui_border_radius(),
                    },
                    text_color: color,
                    ..Default::default()
                }
            })
            .into()
    };

    let enable_btn_text = if eq_enabled { "Enabled" } else { "Disabled" };
    let enable_btn_color = if eq_enabled {
        theme::success()
    } else {
        theme::fg4()
    };

    let enable_button = text_btn(
        enable_btn_text,
        enable_btn_color,
        EqModalMessage::ToggleEnabled,
    );

    let save_button: Element<'_, EqModalMessage> = tooltip(
        svg_btn(
            "assets/icons/save.svg",
            theme::accent_bright(),
            28.0,
            EqModalMessage::SavePreset,
        ),
        container(text("Save Preset").size(11.0).font(theme::ui_font())).padding(4),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .style(theme::container_tooltip)
    .into();

    let reset_button: Element<'_, EqModalMessage> = tooltip(
        svg_btn(
            "assets/icons/rotate-ccw.svg",
            theme::fg4(),
            28.0,
            EqModalMessage::ResetAll,
        ),
        container(text("Reset").size(11.0).font(theme::ui_font())).padding(4),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .style(theme::container_tooltip)
    .into();

    let close_button: Element<'_, EqModalMessage> = svg_btn(
        "assets/icons/x.svg",
        theme::fg3(),
        28.0,
        EqModalMessage::Close,
    );

    // ── Preset Picker ───────────────────────────────────────────
    // Build combined choices: builtins + customs
    let mut choices: Vec<PresetChoice> = (0..nokkvi_data::audio::eq::BUILTIN_PRESETS.len())
        .map(PresetChoice::Builtin)
        .collect();
    for i in 0..custom_presets.len() {
        choices.push(PresetChoice::Custom(i));
    }

    // Find current match — hide selection when EQ is disabled so the
    // placeholder ("EQ Off") shows instead of a misleading preset name.
    let current_choice = if eq_enabled {
        nokkvi_data::audio::eq::BUILTIN_PRESETS
            .iter()
            .enumerate()
            .find(|(_, p)| p.gains == eq_gains)
            .map(|(i, _)| PresetChoice::Builtin(i))
            .or_else(|| {
                custom_presets
                    .iter()
                    .enumerate()
                    .find(|(_, p)| p.gains == eq_gains)
                    .map(|(i, _)| PresetChoice::Custom(i))
            })
    } else {
        None
    };

    // Custom display for pick_list items showing custom preset names
    let custom_presets_owned: Vec<nokkvi_data::audio::eq::CustomEqPreset> = custom_presets.to_vec();

    let preset_picker = pick_list(current_choice.clone(), choices, {
        let presets = custom_presets_owned.clone();
        move |choice: &PresetChoice| match choice {
            PresetChoice::Builtin(i) => nokkvi_data::audio::eq::BUILTIN_PRESETS
                .get(*i)
                .map_or_else(|| format!("Preset {i}"), |p| p.name.to_string()),
            PresetChoice::Custom(i) => presets
                .get(*i)
                .map_or_else(|| format!("Custom #{}", i + 1), |p| format!("★ {}", p.name)),
        }
    })
    .on_select(EqModalMessage::PresetSelected)
    .font(theme::ui_font())
    .text_size(14.0)
    .placeholder(if eq_enabled { "Custom" } else { "EQ Off" })
    .padding([4, 8])
    .style(move |_theme, status| pick_list::Style {
        text_color: theme::fg1(),
        background: theme::bg0_hard().into(),
        border: iced::Border {
            color: match status {
                pick_list::Status::Active | pick_list::Status::Disabled => theme::bg0_hard(),
                pick_list::Status::Hovered => theme::accent_bright(),
                pick_list::Status::Opened { .. } => theme::accent_bright(),
            },
            width: 1.0,
            radius: theme::ui_border_radius(),
        },
        placeholder_color: theme::fg3(),
        handle_color: theme::fg3(),
    })
    .menu_style(move |_theme| iced::widget::overlay::menu::Style {
        text_color: theme::fg0(),
        background: theme::bg1().into(),
        border: iced::Border {
            color: theme::accent_bright(),
            width: 1.0,
            radius: theme::ui_border_radius(),
        },
        selected_text_color: theme::bg0_hard(),
        selected_background: theme::accent_bright().into(),
        shadow: iced::Shadow::default(),
    });

    // Delete button — only visible when a custom preset is currently active
    let delete_button: Option<Element<'_, EqModalMessage>> = current_choice
        .as_ref()
        .and_then(|choice| {
            if let PresetChoice::Custom(idx) = choice {
                Some(*idx)
            } else {
                None
            }
        })
        .map(|idx| {
            svg_btn(
                "assets/icons/trash-2.svg",
                theme::fg3(),
                28.0,
                EqModalMessage::DeletePreset(idx),
            )
        });

    // Build header row — two modes: normal vs save
    let header: Element<'_, EqModalMessage> = if save_mode {
        // Save mode: name input + OK + Cancel
        let name_input = iced::widget::text_input("Preset name...", save_name)
            .on_input(EqModalMessage::SavePresetNameChanged)
            .on_submit(EqModalMessage::SavePresetConfirm)
            .size(14.0)
            .padding([4, 8])
            .width(Length::Fixed(200.0))
            .font(theme::ui_font())
            .style(move |_theme, _status| iced::widget::text_input::Style {
                background: theme::bg0_hard().into(),
                border: iced::Border {
                    color: theme::accent_bright(),
                    width: 1.0,
                    radius: theme::ui_border_radius(),
                },
                icon: theme::fg3(),
                placeholder: theme::fg4(),
                value: theme::fg0(),
                selection: theme::accent_bright(),
            });

        let ok_button = text_btn("OK", theme::success(), EqModalMessage::SavePresetConfirm);

        let cancel_button = text_btn("Cancel", theme::fg4(), EqModalMessage::CancelSave);

        row![
            text("Save Preset")
                .size(16.0)
                .color(theme::accent_bright())
                .font(iced::font::Font {
                    weight: iced::font::Weight::Bold,
                    ..theme::ui_font()
                }),
            name_input,
            ok_button,
            cancel_button,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    } else {
        // Normal mode: title + preset picker + buttons
        let mut header_items: Vec<Element<'_, EqModalMessage>> = vec![
            title_text.into(),
            space::horizontal().into(),
            preset_picker.into(),
        ];
        if let Some(del_btn) = delete_button {
            header_items.push(del_btn);
        }
        header_items.push(enable_button);
        header_items.push(save_button);
        header_items.push(reset_button);
        header_items.push(close_button);

        iced::widget::Row::with_children(header_items)
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
    };

    let header_sep = separator_line();

    // ── Equalizer Bands ──────────────────────────────────────────
    let mut sliders = Vec::new();
    for (i, &gain) in eq_gains.iter().enumerate() {
        // Slider
        let slider = eq_slider(gain, move |val| EqModalMessage::GainChanged(i, val));

        // Value label (+X.X dB)
        let val_text = if gain > 0.0 {
            format!("+{gain:.1}")
        } else {
            format!("{gain:.1}")
        };

        let val_color = if gain.abs() < 0.1 {
            theme::fg3()
        } else {
            theme::accent_bright()
        };

        let val_label = text(val_text)
            .size(12.0)
            .color(val_color)
            .font(theme::ui_font());

        // Frequency label (e.g. 1K)
        let freq_label = text(BAND_FREQS[i])
            .size(12.0)
            .color(theme::fg3())
            .font(theme::ui_font());

        let band_col = column![val_label, slider, freq_label]
            .spacing(12)
            .align_x(Alignment::Center)
            .width(Length::Fill);

        sliders.push(band_col.into());
    }

    let eq_row = row(sliders)
        .width(Length::Fill)
        .spacing(8)
        .align_y(Alignment::Center);

    let content = column![
        header,
        header_sep,
        container(eq_row)
            .width(Length::Fill)
            .align_x(Alignment::Center),
    ]
    .spacing(24)
    .padding(24)
    .width(Length::Fixed(MODAL_WIDTH));

    // ── Dialog Box ───────────────────────────────────────────────
    let dialog_box = container(content)
        .style(|_theme| container::Style {
            background: Some(theme::bg1().into()),
            border: iced::Border {
                color: theme::accent_bright(),
                width: 1.0,
                radius: theme::ui_border_radius(),
            },
            ..Default::default()
        })
        .width(Length::Shrink);

    // ── Backdrop ─────────────────────────────────────────────────
    let backdrop = mouse_area(
        container(opaque(dialog_box))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(|_theme| {
                let mut bg = theme::bg0_hard();
                bg.a = 0.6;
                container::Style {
                    background: Some(bg.into()),
                    ..Default::default()
                }
            }),
    )
    .on_press(EqModalMessage::Close);

    Some(opaque(backdrop))
}

fn separator_line<'a>() -> Element<'a, EqModalMessage> {
    container(space::horizontal())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_theme| {
            let mut c = theme::fg4();
            c.a = 0.2;
            container::Style {
                background: Some(c.into()),
                ..Default::default()
            }
        })
        .into()
}
