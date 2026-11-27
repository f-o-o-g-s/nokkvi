//! Equalizer Modal Widget
//!
//! A modal overlay with the equalizer's two layouts: the 10-band Graphic
//! sliders and the Parametric band editor (`eq_parametric.rs`), switched by
//! the segmented control under the title. The switch picks the layout that
//! plays (`EqState::set_mode`).
//! Triggered via hotkey [Q] or from the player bar EQ button.

use std::sync::Arc;

use iced::{
    Alignment, Element, Length, Widget as _,
    widget::{button, column, container, mouse_area, pick_list, row, space, svg, text, tooltip},
};
use nokkvi_data::audio::eq::{EQ_BAND_COUNT, EqBand, EqBandKind, EqMode, EqProfile, bands_match};

pub(crate) use crate::widgets::eq_parametric::{BandField, FieldEdit, FieldKey};
use crate::{
    theme,
    widgets::{
        eq_autoeq::{self, AutoEqMessage, AutoEqPanel},
        eq_parametric::{self, FieldMounts, PreampMemo},
        eq_slider::eq_slider,
        hover_overlay::HoverOverlay,
        pill_segmented_button::{PillOption, PillRowParams, PillVariant, pill_segmented_button},
        text_field::text_field,
    },
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
    /// Whether the modal is in "save" mode (showing the name input): a
    /// preset in the Graphic view, a profile in the Parametric view.
    pub save_mode: bool,
    /// Text input content for the preset or profile name being saved.
    pub save_name: String,
    /// Cached custom EQ presets (loaded from redb, kept in sync on save/delete).
    pub custom_presets: Vec<nokkvi_data::audio::eq::CustomEqPreset>,
    /// Saved Parametric profiles (restored from the settings on load, kept in
    /// sync on save/delete).
    pub profiles: Vec<EqProfile>,
    /// The name the next profile Save starts with: the last imported file's
    /// name without its extension, kept while the band list is still the one
    /// that import put in (dropped by the first edit, a profile selected or
    /// saved).
    pub(crate) imported: Option<ImportedProfile>,
    /// Parametric view: the band whose point and table row are highlighted.
    /// An index into `EqState::parametric_bands`; one past the end (the list
    /// shrank under it, a config.toml reload) highlights nothing.
    pub selected: Option<usize>,
    /// Parametric view: the field being typed into, with its text as typed.
    pub edit: Option<FieldEdit>,
    /// The band list changed live (a drag, typing, a nudge) since it was last
    /// persisted; the next commit, drag release or Close writes it.
    pub bands_unsaved: bool,
    /// A Graphic slider moved since the gains were last persisted; the drag's
    /// release, the wheel step's commit or Close writes them.
    pub gains_unsaved: bool,
    /// The preamp the status line shows, memoized per band list.
    pub(crate) preamp: PreampMemo,
    /// The band table's text fields: what each was last handed and the key
    /// its widget state is mounted under.
    pub(crate) mounts: FieldMounts,
    /// Parametric view: the AutoEq search panel, shown in place of the
    /// editor while open (`widgets/eq_autoeq.rs`).
    pub(crate) autoeq: Option<AutoEqPanel>,
    /// Stamps every AutoEq request, so a result for a panel since closed,
    /// reopened or asked again is dropped. Beside the panel, not in it: it
    /// must outlive a close and reopen (gotchas/async-state.md).
    pub(crate) autoeq_generation: crate::state::StaleDropGen,
}

/// An imported file's name (extension dropped) and the band list it put in.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ImportedProfile {
    pub name: String,
    pub bands: Vec<EqBand>,
}

impl EqModalState {
    /// Logout: everything goes back to its default except the AutoEq
    /// stale-drop counter, which is carried forward (a request from the old
    /// session must not match the next one's).
    pub(crate) fn reset_for_session(&mut self) {
        *self = Self {
            autoeq_generation: self.autoeq_generation.carried_forward(),
            ..Self::default()
        };
    }

    /// The automatic preamp `bands` take (dB, never above 0).
    pub(crate) fn preamp_for(&self, bands: &[EqBand]) -> f64 {
        self.preamp.get(bands)
    }

    /// Bring the band table's mount keys up to date with `bands` (what
    /// plays) and drop an import name the list has moved away from. Runs
    /// after every EQ modal message and wherever the band list is replaced
    /// outside the modal.
    pub(crate) fn sync_fields(&mut self, bands: &[EqBand]) {
        self.mounts.sync(bands, self.edit.as_ref());
        if self
            .imported
            .as_ref()
            .is_some_and(|imported| imported.bands != bands)
        {
            self.imported = None;
        }
    }

    /// The saved profile the band list is, compared at the stored precision
    /// (a reload reads back rounded values).
    pub(crate) fn matching_profile(&self, bands: &[EqBand]) -> Option<usize> {
        self.profiles
            .iter()
            .position(|profile| bands_match(&profile.bands, bands))
    }
}

#[derive(Debug, Clone)]
pub enum EqModalMessage {
    Open,
    Close,
    Toggle,
    ToggleEnabled,
    /// Graphic: slider `band` moved to this gain (dB). Plays at once; the
    /// write waits for `GainCommitted`.
    GainChanged(usize, f32),
    /// Graphic: a slider drag ended, or a wheel step landed: write the gains.
    GainCommitted,
    PresetSelected(PresetChoice),
    /// Graphic: every slider to 0 dB. Parametric: every band's gain to 0 dB,
    /// the bands kept.
    ResetAll,
    /// Enter save mode — show inline name input (a preset in the Graphic
    /// view, a profile in the Parametric view)
    SavePreset,
    /// Text input changed while saving
    SavePresetNameChanged(String),
    /// Confirm save with current name
    SavePresetConfirm,
    /// Cancel save mode
    CancelSave,
    /// Delete a custom preset by its index in the custom list
    DeletePreset(usize),
    /// Parametric: a saved profile picked (its index in the list).
    ProfileSelected(usize),
    /// Parametric: delete the saved profile at this index (the bands stay).
    DeleteProfile(usize),
    /// The view switch: the layout that plays (and that the modal shows).
    ModeSelected(EqMode),
    /// A band's table row was pressed.
    SelectBand(usize),
    /// A band's curve point was pressed: selected, its row scrolled into
    /// view.
    PointPressed(usize),
    /// A curve drag moved band `.0` to `(freq Hz, gain dB)`: plays at once,
    /// persisted at the drag's end.
    BandDragged(usize, f64, f64),
    /// A curve drag that moved a band ended.
    BandDragEnded,
    /// The wheel over a curve point: band `.0`'s new Q.
    BandQStepped(usize, f64),
    /// Add band: a peak at 1 kHz, 0 dB, Q 1.0.
    AddBand,
    /// A double-click on empty curve space: a peak at `(freq Hz, gain dB)`.
    AddBandAt(f64, f64),
    RemoveBand(usize),
    /// A band's on/off box.
    ToggleBand(usize),
    BandKindSelected(usize, EqBandKind),
    /// Text typed into band `.0`'s `.1` field.
    FieldInput(usize, BandField, String),
    /// Enter in band `.0`'s `.1` field.
    FieldSubmit(usize, BandField),
    /// Tab / Shift+Tab / Up / Down while the band table shows, with the
    /// widget that had focus (`None`: nothing did).
    FieldKey(Option<iced::widget::Id>, FieldKey),
    /// Fill the empty band list with the ten Graphic sliders.
    CopyFromGraphic,
    /// Import…: pick an Equalizer APO / AutoEq text.
    Import,
    /// The picked file, read and parsed off the UI thread: `(file name,
    /// bands)`, or the toast that says why not. A cancelled dialog sends
    /// nothing.
    ImportLoaded(Result<(String, Vec<EqBand>), String>),
    /// Export…: write the bands as an Equalizer APO text.
    Export,
    /// The export finished: `(file name, filters written)`, or why not.
    ExportWritten(Result<(String, usize), String>),
    /// The AutoEq search panel (`widgets/eq_autoeq.rs`).
    AutoEq(AutoEqMessage),
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

/// Id of the save prompt's name field, so opening the prompt can focus it
/// (Save, type or accept the prefilled name, Enter).
pub(crate) const SAVE_NAME_INPUT_ID: &str = "eq_modal_save_name";
/// Widest the Parametric header's profile picker grows: with the delete
/// button showing, the title, Enabled and four icon buttons leave about
/// 210 px of the modal's 592 px content row.
const PROFILE_PICKER_MAX_WIDTH: f32 = 200.0;
const BAND_FREQS: [&str; 10] = [
    "32", "64", "125", "250", "500", "1K", "2K", "4K", "8K", "16K",
];

// Compile-time guard: the visible band labels must stay 1:1 with the DSP
// center-frequency array (`nokkvi_data::audio::eq::EQ_BANDS_HZ`). Adding or
// removing a band in one array without the other now fails the build.
const _: () = assert!(
    BAND_FREQS.len() == nokkvi_data::audio::eq::EQ_BANDS_HZ.len(),
    "EQ label array length must match DSP band-frequency array length",
);

/// What the modal draws, read from the app state each render.
pub(crate) struct EqModalViewData<'a> {
    pub state: &'a EqModalState,
    /// The master switch.
    pub enabled: bool,
    /// The layout that plays, which is the view the modal shows.
    pub mode: EqMode,
    /// The Graphic sliders as shown: 0 dB while the equalizer is off.
    pub gains: [f32; EQ_BAND_COUNT],
    /// The Parametric layout's band list, shown as it is (dimmed while off).
    pub bands: Arc<Vec<EqBand>>,
}

pub(crate) fn eq_modal_overlay<'a>(
    data: EqModalViewData<'a>,
) -> Option<Element<'a, EqModalMessage>> {
    let state = data.state;
    if !state.open {
        return None;
    }
    let eq_enabled = data.enabled;
    let eq_gains = data.gains;
    let custom_presets = state.custom_presets.as_slice();
    let parametric = data.mode == EqMode::Parametric;
    // A preset in the Graphic view, a profile in the Parametric view (the
    // view switch ends the prompt).
    let save_mode = state.save_mode;
    let save_name = state.save_name.as_str();
    // What the Graphic view calls a preset, the Parametric view calls a
    // profile.
    let save_title = if parametric {
        "Save Profile"
    } else {
        "Save Preset"
    };

    // ── Header ──────────────────────────────────────────────────
    let title_text = text("Equalizer")
        .size(20.0)
        .font(theme::weighted_ui_font(iced::font::Weight::Bold))
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
        .boxed()
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
            .boxed()
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
        container(text(save_title).size(11.0).font(theme::ui_font())).padding(4),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .style(theme::container_tooltip)
    .boxed();

    let reset_tip = if parametric {
        "Reset gains: every band to 0 dB, the bands kept"
    } else {
        "Reset"
    };
    let reset_button: Element<'_, EqModalMessage> = tooltip(
        svg_btn(
            "assets/icons/rotate-ccw.svg",
            theme::fg4(),
            28.0,
            EqModalMessage::ResetAll,
        ),
        container(text(reset_tip).size(11.0).font(theme::ui_font())).padding(4),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .style(theme::container_tooltip)
    .boxed();

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
            .find(|(_, p)| gains_match(&p.gains, &eq_gains))
            .map(|(i, _)| PresetChoice::Builtin(i))
            .or_else(|| {
                custom_presets
                    .iter()
                    .enumerate()
                    .find(|(_, p)| gains_match(&p.gains, &eq_gains))
                    .map(|(i, _)| PresetChoice::Custom(i))
            })
    } else {
        None
    };

    // Custom display for pick_list items showing custom preset names
    let custom_presets_owned: Vec<nokkvi_data::audio::eq::CustomEqPreset> = custom_presets.to_vec();

    let preset_picker = pick_list(
        current_choice.clone(),
        choices,
        move |choice: &PresetChoice| match choice {
            PresetChoice::Builtin(i) => nokkvi_data::audio::eq::BUILTIN_PRESETS
                .get(*i)
                .map_or_else(|| format!("Preset {i}"), |p| p.name.to_string()),
            PresetChoice::Custom(i) => custom_presets_owned
                .get(*i)
                .map_or_else(|| format!("Custom #{}", i + 1), |p| format!("★ {}", p.name)),
        },
    )
    .on_select(EqModalMessage::PresetSelected)
    .font(theme::ui_font())
    .text_size(14.0)
    .placeholder(if eq_enabled { "Custom" } else { "EQ Off" })
    .padding([4, 8])
    .style(header_pick_style)
    .menu_style(header_menu_style);

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

    // ── Profile Picker (Parametric) ─────────────────────────────
    // The Graphic picker's twin over the saved band lists: a profile shows
    // as selected while the equalizer is on and the band list is that
    // profile's (at the stored precision).
    let current_profile = if eq_enabled {
        state.matching_profile(&data.bands)
    } else {
        None
    };
    let profile_names: Vec<String> = state
        .profiles
        .iter()
        .map(|profile| profile_label(&profile.name))
        .collect();
    let profile_placeholder = if !eq_enabled {
        "EQ Off"
    } else if profile_names.is_empty() {
        "No profiles"
    } else {
        "Custom"
    };
    let profile_picker = container(
        pick_list(
            current_profile,
            (0..profile_names.len()).collect::<Vec<usize>>(),
            move |index: &usize| profile_names.get(*index).cloned().unwrap_or_default(),
        )
        .on_select(EqModalMessage::ProfileSelected)
        .font(theme::ui_font())
        .text_size(14.0)
        .placeholder(profile_placeholder)
        .padding([4, 8])
        .style(header_pick_style)
        .menu_style(header_menu_style),
    )
    // A profile is often named after a long file name ("Sennheiser HD 650
    // ParametricEQ"): the labels are cut short (`profile_label`) and the
    // picker capped, so the header's buttons keep their place in the 640 px
    // modal. A bounded width on the pick_list itself would lose its
    // measured width (it measures its labels only under `Shrink`).
    .width(Length::Shrink.max(PROFILE_PICKER_MAX_WIDTH));
    let delete_profile_button: Option<Element<'_, EqModalMessage>> = current_profile.map(|idx| {
        svg_btn(
            "assets/icons/trash-2.svg",
            theme::fg3(),
            28.0,
            EqModalMessage::DeleteProfile(idx),
        )
    });

    // Build header row — two modes: normal vs save
    let header: Element<'_, EqModalMessage> = if save_mode {
        // Save mode: name input + OK + Cancel
        let name_placeholder = if parametric {
            "Profile name..."
        } else {
            "Preset name..."
        };
        let name_input = text_field(name_placeholder, save_name, theme::ui_font(), |input| {
            input
                .id(SAVE_NAME_INPUT_ID)
                .on_input(EqModalMessage::SavePresetNameChanged)
                .on_submit(EqModalMessage::SavePresetConfirm)
                .size(14.0)
                .padding([4, 8])
                .width(Length::Fixed(200.0))
                .style(move |_theme, _status| iced::widget::text_input::Style {
                    background: theme::bg0_hard().into(),
                    border: iced::Border {
                        color: theme::accent_bright(),
                        width: 1.0,
                        radius: theme::ui_border_radius(),
                    },
                    placeholder: theme::fg4(),
                    value: theme::fg0(),
                    selection: theme::selection_color(),
                })
        });

        let ok_button = text_btn("OK", theme::success(), EqModalMessage::SavePresetConfirm);

        let cancel_button = text_btn("Cancel", theme::fg4(), EqModalMessage::CancelSave);

        row![
            text(save_title)
                .size(16.0)
                .color(theme::accent_bright())
                .font(theme::weighted_ui_font(iced::font::Weight::Bold)),
            name_input,
            ok_button,
            cancel_button,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .boxed()
    } else {
        // Normal mode: title + picker + buttons, the same row in both views:
        // the Graphic view picks presets, the Parametric view profiles.
        let mut header_items: Vec<Element<'_, EqModalMessage>> =
            vec![title_text.boxed(), space::horizontal().boxed()];
        if parametric {
            header_items.push(profile_picker.boxed());
            if let Some(del_btn) = delete_profile_button {
                header_items.push(del_btn);
            }
        } else {
            header_items.push(preset_picker.boxed());
            if let Some(del_btn) = delete_button {
                header_items.push(del_btn);
            }
        }
        header_items.push(enable_button);
        header_items.push(save_button);
        header_items.push(reset_button);
        header_items.push(close_button);

        iced::widget::Row::with_children(header_items)
            .spacing(12)
            .align_y(Alignment::Center)
            .boxed()
    };

    // The view switch sits under the title, on a line of its own: beside the
    // Graphic header's picker and buttons it would not fit the 640 px modal.
    let header = column![header, mode_switch(data.mode)].spacing(12);

    let header_sep = theme::modal_header_separator();

    // ── Equalizer Bands ──────────────────────────────────────────
    let mut sliders = Vec::new();
    for (i, &gain) in eq_gains.iter().enumerate() {
        // Slider
        let slider = eq_slider(
            gain,
            move |val| EqModalMessage::GainChanged(i, val),
            EqModalMessage::GainCommitted,
        );

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

        sliders.push(band_col);
    }

    let eq_row = row(sliders)
        .width(Length::Fill)
        .spacing(8)
        .align_y(Alignment::Center);

    let body: Element<'_, EqModalMessage> = if parametric {
        // The AutoEq panel takes the editor's place while it is open.
        match state.autoeq.as_ref() {
            Some(panel) => eq_autoeq::autoeq_panel(panel),
            None => eq_parametric::parametric_body(&data),
        }
    } else {
        container(eq_row)
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .boxed()
    };

    let content = column![header, header_sep, body]
        .spacing(24)
        .padding(24)
        .width(Length::Fixed(MODAL_WIDTH));

    // Shared modal frame: bg0_hard fill + 1 px accent_bright outline +
    // ui_radius_lg corners. Five overlay modals route through this helper.
    let dialog_box = container(content)
        .style(theme::modal_frame_style)
        .width(Length::Shrink);

    // ── Backdrop ─────────────────────────────────────────────────
    Some(theme::modal_scaffold(
        dialog_box.boxed(),
        EqModalMessage::Close,
        theme::MODAL_BACKDROP_ALPHA,
    ))
}

/// Most characters a profile name shows in the picker before it is cut
/// with an ellipsis: about 180 px at 14 px, clear of the picker's arrow.
const PROFILE_LABEL_MAX_CHARS: usize = 24;

/// A saved profile's name as the picker lists it.
fn profile_label(name: &str) -> String {
    if name.chars().count() <= PROFILE_LABEL_MAX_CHARS {
        name.to_owned()
    } else {
        let mut label: String = name.chars().take(PROFILE_LABEL_MAX_CHARS - 1).collect();
        label.push('…');
        label
    }
}

/// The header pickers' look (presets and profiles alike): flat `bg0_hard`,
/// an `accent_bright` border under the cursor or while open.
fn header_pick_style(_theme: &iced::Theme, status: pick_list::Status) -> pick_list::Style {
    pick_list::Style {
        text_color: theme::fg1(),
        background: theme::bg0_hard().into(),
        border: iced::Border {
            color: match status {
                pick_list::Status::Active | pick_list::Status::Disabled => theme::bg0_hard(),
                pick_list::Status::Hovered | pick_list::Status::Opened { .. } => {
                    theme::accent_bright()
                }
            },
            width: 1.0,
            radius: theme::ui_border_radius(),
        },
        placeholder_color: theme::fg3(),
        handle_color: theme::fg3(),
    }
}

/// The header pickers' open menu.
fn header_menu_style(_theme: &iced::Theme) -> iced::widget::overlay::menu::Style {
    iced::widget::overlay::menu::Style {
        text_color: theme::fg0(),
        background: theme::bg1().into(),
        border: iced::Border {
            color: theme::accent_bright(),
            width: 1.0,
            radius: theme::ui_border_radius(),
        },
        selected_text_color: theme::on_accent_fill(),
        selected_background: theme::accent_fill().into(),
        shadow: iced::Shadow::default(),
    }
}

/// The `GRAPHIC | PARAMETRIC` switch: the Settings pill chips (accent fill on
/// the layout that plays).
fn mode_switch<'a>(mode: EqMode) -> Element<'a, EqModalMessage> {
    let options: Vec<PillOption> = EqMode::ALL
        .into_iter()
        .map(|option| PillOption {
            display: option.label().to_owned(),
            key: option.label().to_owned(),
            on: option == mode,
        })
        .collect();
    pill_segmented_button(
        &options,
        PillVariant::Single,
        PillRowParams {
            font_size: 11.0,
            is_center: true,
            opacity: 1.0,
        },
        |key: String| {
            EqModalMessage::ModeSelected(
                EqMode::ALL
                    .into_iter()
                    .find(|mode| mode.label() == key)
                    .unwrap_or_default(),
            )
        },
    )
}

/// Compare two 10-band EQ gain arrays with a 1e-3 dB epsilon.
///
/// `toml::to_string_pretty` serializes top-level `eq_gains` with ~4-decimal
/// precision but writes nested `[[custom_eq_presets]].gains` at full f32
/// precision. After a reload the rounded bits no longer match the
/// full-precision bits, so the dropdown's exact-equality lookup would fall
/// through to the "Custom" placeholder. 1e-3 sits well above the ~5e-5
/// rounding noise and well below any user-perceivable gain step in the
/// -12.0..=12.0 dB range.
fn gains_match(a: &[f32; 10], b: &[f32; 10]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-3)
}

#[cfg(test)]
mod tests {
    use super::{BAND_FREQS, PROFILE_LABEL_MAX_CHARS, gains_match, profile_label};

    #[test]
    fn long_profile_names_are_cut_with_an_ellipsis() {
        assert_eq!(profile_label("HD 650"), "HD 650");
        let exact = "x".repeat(PROFILE_LABEL_MAX_CHARS);
        assert_eq!(profile_label(&exact), exact);
        let cut = profile_label("Sennheiser HD 650 ParametricEQ");
        assert_eq!(cut, "Sennheiser HD 650 Param…");
        assert_eq!(cut.chars().count(), PROFILE_LABEL_MAX_CHARS);
        // Cut on characters, never inside one.
        let wide = profile_label(&"ø".repeat(30));
        assert_eq!(wide.chars().count(), PROFILE_LABEL_MAX_CHARS);
    }

    /// Pins each visible band label to its DSP center frequency, encoding the
    /// intentional ISO-rounding divergences (label `32` for the 31 Hz band,
    /// `64` for 62 Hz) and the `K` thousands-suffix as the spec. The
    /// `const _: ()` assert beside `BAND_FREQS` already guards *count* drift;
    /// this test guards a future *value* change in `EQ_BANDS_HZ` — any edit to
    /// either array now forces a deliberate review of this label mapping.
    #[test]
    fn band_freq_labels_pin_to_dsp_frequencies() {
        // Length parity with the DSP center-frequency array (mirrors the
        // compile-time `const _: ()` assert, asserted here at runtime too so a
        // count change is caught even if the const-block is ever removed).
        assert_eq!(
            BAND_FREQS.len(),
            nokkvi_data::audio::eq::EQ_BANDS_HZ.len(),
            "EQ label array length must match DSP band-frequency array length",
        );

        // (DSP center frequency in Hz, intended display label). The 31->32 and
        // 62->64 rows are the intentional ISO-power-of-two label rounding; the
        // rest are exact, with `K` standing in for `000`.
        let expected: [(f32, &str); 10] = [
            (31.0, "32"),
            (62.0, "64"),
            (125.0, "125"),
            (250.0, "250"),
            (500.0, "500"),
            (1000.0, "1K"),
            (2000.0, "2K"),
            (4000.0, "4K"),
            (8000.0, "8K"),
            (16000.0, "16K"),
        ];

        for (i, (freq, label)) in expected.iter().enumerate() {
            assert_eq!(
                nokkvi_data::audio::eq::EQ_BANDS_HZ[i],
                *freq,
                "DSP center frequency for band {i} changed; review the label mapping",
            );
            assert_eq!(
                BAND_FREQS[i], *label,
                "display label for band {i} changed; review the DSP frequency mapping",
            );
        }
    }

    /// Pair lifted from a real config.toml: rounded top-level `eq_gains`
    /// vs full-precision nested `[[custom_eq_presets]].gains`.
    #[test]
    fn gains_match_within_epsilon() {
        let rounded: [f32; 10] = [
            0.0, -3.1139, 8.5257, -8.3076, 5.8904, 0.0, 1.5, 3.0, 4.0, 5.0,
        ];
        // Clippy truncates these to their actual f32 bit representations
        // (`-3.113_941`, `8.525_743_5`, `-8.307_606`, `5.890_437`); the
        // longer TOML strings (`-3.113941192626953`, etc.) parse to the
        // same bits.
        let full_precision: [f32; 10] = [
            0.0,
            -3.113_941,
            8.525_743_5,
            -8.307_606,
            5.890_437,
            0.0,
            1.5,
            3.0,
            4.0,
            5.0,
        ];
        assert!(gains_match(&rounded, &full_precision));
        assert!(gains_match(&full_precision, &rounded));
    }

    /// A real 0.01 dB delta (10x the epsilon) in any single band must
    /// reject the match, so genuinely distinct presets stay distinguishable.
    #[test]
    fn gains_match_rejects_actual_difference() {
        let base: [f32; 10] = [
            0.0, -3.1139, 8.5257, -8.3076, 5.8904, 0.0, 1.5, 3.0, 4.0, 5.0,
        ];
        for band in 0..10 {
            let mut perturbed = base;
            perturbed[band] += 0.01;
            assert!(
                !gains_match(&base, &perturbed),
                "expected mismatch when band {band} differs by 0.01 dB",
            );
        }
    }

    /// NaN / ±Inf should never appear in real config but must not panic
    /// the comparison if they ever do.
    #[test]
    fn gains_match_handles_nan_and_inf_without_panic() {
        let zeros = [0.0f32; 10];
        let nans = [f32::NAN; 10];
        let infs = [f32::INFINITY; 10];
        let neg_infs = [f32::NEG_INFINITY; 10];

        assert!(!gains_match(&nans, &nans));
        assert!(!gains_match(&infs, &infs));
        assert!(!gains_match(&neg_infs, &neg_infs));
        assert!(!gains_match(&nans, &zeros));
        assert!(!gains_match(&infs, &zeros));
        assert!(!gains_match(&neg_infs, &zeros));
    }
}
