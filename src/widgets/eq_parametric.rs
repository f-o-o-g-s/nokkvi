//! The Parametric view of the equalizer modal: the response curve, a status
//! line (automatic preamp, band count, add / import / export) and the band
//! table, every number of which is a text field.
//!
//! The band list itself lives on `EqState` (`Nokkvi.playback.eq_state`); the
//! view reads it through `EqModalViewData::bands` and the handlers in
//! `update/eq_modal.rs` write it. The editor keeps only what is its own: the
//! selected band and the text of the field being typed into
//! ([`FieldEdit`]).
//!
//! The value helpers (parse, format, nudge, field order) are pure and pinned
//! by the tests at the bottom.

use iced::{
    Alignment, Element, Length, Padding, Widget as _,
    widget::{
        Column, button, column, container, mouse_area, pick_list, row, scrollable, space, svg,
        text, text_input, tooltip,
    },
};
use nokkvi_data::{
    audio::eq::{EQ_MAX_BANDS, EqBand, EqBandKind, preamp_db},
    utils::noun::Noun,
};

use crate::{
    theme,
    widgets::{
        checkbox_glyph,
        eq_curve::{bands_key, eq_curve},
        eq_modal::{EqModalMessage, EqModalViewData},
        hover_overlay::HoverOverlay,
        remount::remount,
    },
};

// =============================================================================
// Fields
// =============================================================================

/// A band's numeric field in the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BandField {
    Freq,
    Gain,
    Q,
}

impl BandField {
    /// Reading order within a row.
    pub(crate) const ALL: [Self; 3] = [Self::Freq, Self::Gain, Self::Q];

    fn key(self) -> &'static str {
        match self {
            Self::Freq => "freq",
            Self::Gain => "gain",
            Self::Q => "q",
        }
    }

    /// Its place in [`Self::ALL`].
    fn index(self) -> usize {
        match self {
            Self::Freq => 0,
            Self::Gain => 1,
            Self::Q => 2,
        }
    }
}

/// A key the band table takes while the EQ modal shows it, read with the
/// focused widget's id (`update/hotkeys`): Tab / Shift+Tab move between the
/// fields, Up / Down nudge the focused one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    Next,
    Previous,
    Up,
    Down,
}

/// The field being typed into and its text as typed. The value already plays
/// whenever the text parses; the text stays as typed until Enter (or Tab, or
/// typing into another field) commits it, then the field shows the stored
/// value again.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldEdit {
    pub band: usize,
    pub field: BandField,
    pub text: String,
}

/// The text band `index`'s `field` hands its text field: what is typed while
/// that field is being edited, else the stored value formatted.
pub(crate) fn field_text(
    index: usize,
    band: &EqBand,
    field: BandField,
    edit: Option<&FieldEdit>,
) -> String {
    match edit {
        Some(edit) if (edit.band, edit.field) == (index, field) => edit.text.clone(),
        _ => format_value(field, band),
    }
}

/// What each band field's text field was last handed, and the key its
/// widget state is mounted under (`widgets::remount`).
///
/// iced's `text_input` redraws a value replaced from outside the field in
/// the system sans-serif while it stays mounted, so every field whose text
/// changes for any reason but its own typing gets a new key, which gives it
/// a fresh widget state. [`Self::sync`] runs after every message the EQ
/// modal handles and wherever the band list is replaced outside it.
#[derive(Debug, Default)]
pub(crate) struct FieldMounts {
    /// Per band, per field in [`BandField::ALL`] order: `(text, key)`.
    fields: Vec<[(String, u64); 3]>,
    /// The last key handed out; keys never repeat within a session, so a
    /// row that comes back can't meet a key its old widget held.
    last_key: u64,
}

impl FieldMounts {
    /// The key band `band`'s `field` is mounted under (0 before its first
    /// sync).
    pub(crate) fn key(&self, band: usize, field: BandField) -> u64 {
        self.fields
            .get(band)
            .map_or(0, |fields| fields[field.index()].1)
    }

    /// Record the text each field of `bands` will show and re-key every field
    /// whose text changed, except the one `edit` names: the user is typing
    /// there and its widget already holds that text (a re-key would drop
    /// its focus and caret).
    pub(crate) fn sync(&mut self, bands: &[EqBand], edit: Option<&FieldEdit>) {
        self.fields.truncate(bands.len());
        for (index, band) in bands.iter().enumerate() {
            if index == self.fields.len() {
                self.fields.push(Default::default());
            }
            for field in BandField::ALL {
                let text = field_text(index, band, field, edit);
                let typing = edit.is_some_and(|edit| (edit.band, edit.field) == (index, field));
                let slot = &mut self.fields[index][field.index()];
                if slot.0 != text {
                    slot.0 = text;
                    if !typing {
                        self.last_key += 1;
                        slot.1 = self.last_key;
                    }
                }
            }
        }
    }
}

/// The stable widget id of band `band`'s `field` input.
pub(crate) fn field_id(band: usize, field: BandField) -> iced::widget::Id {
    iced::widget::Id::from(format!("eq-band-{band}-{}", field.key()))
}

/// The band table's scrollable.
pub(crate) const TABLE_SCROLL_ID: &str = "eq-band-table";

/// The stable widget id of band `band`'s table row.
pub(crate) fn row_id(band: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("eq-band-{band}"))
}

/// Scroll band `band`'s row into the table's view (a no-op while it is not
/// laid out yet).
pub(crate) fn reveal_row<M: Send + 'static>(band: usize) -> iced::Task<M> {
    crate::widgets::scroll_into_view::center_in_scrollable(TABLE_SCROLL_ID, row_id(band))
}

/// Focus band `band`'s `field`, caret at the end, its row scrolled into view
/// (iced's scrollable doesn't follow focus).
pub(crate) fn focus_field<M: Send + 'static>(band: usize, field: BandField) -> iced::Task<M> {
    crate::widgets::focus_text_input_id(field_id(band, field)).chain(reveal_row(band))
}

/// The band field `id` names, if it is one of the first `len` bands'.
pub(crate) fn field_at(id: &iced::widget::Id, len: usize) -> Option<(usize, BandField)> {
    (0..len)
        .flat_map(|band| BandField::ALL.map(|field| (band, field)))
        .find(|&(band, field)| field_id(band, field) == *id)
}

/// Whether `field` takes input on `band`: a pass filter has no gain.
pub(crate) fn field_enabled(band: &EqBand, field: BandField) -> bool {
    field != BandField::Gain || band.kind.has_gain()
}

/// The field Tab (`forward`) or Shift+Tab reaches from `from`, in reading
/// order (row by row, frequency, gain, Q), skipping disabled gain fields and
/// wrapping at both ends. From no field (or one past the list) it is the
/// first, or the last going back.
pub(crate) fn adjacent_field(
    bands: &[EqBand],
    from: Option<(usize, BandField)>,
    forward: bool,
) -> Option<(usize, BandField)> {
    let order: Vec<(usize, BandField)> = (0..bands.len())
        .flat_map(|band| BandField::ALL.map(|field| (band, field)))
        .collect();
    let n = order.len();
    let start = from.and_then(|from| order.iter().position(|&slot| slot == from));
    (1..=n)
        .map(|step| match (start, forward) {
            (Some(at), true) => (at + step) % n,
            (Some(at), false) => (at + n - step) % n,
            (None, true) => step - 1,
            (None, false) => n - step,
        })
        .map(|i| order[i])
        .find(|&(band, field)| field_enabled(&bands[band], field))
}

// =============================================================================
// Values (pure)
// =============================================================================

/// Frequency step of an Up / Down nudge: one twelfth of an octave.
const FREQ_NUDGE: f64 = 1.059_463_094_359_295_3;
/// Gain step of a nudge, in dB.
const GAIN_NUDGE_DB: f64 = 0.5;
/// Q step of a nudge: 5 %.
const Q_NUDGE: f64 = 1.05;

/// `text` with units and spacing taken out, lowercased, a decimal comma read
/// as a point.
fn bare(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| {
            if c == ',' {
                '.'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

fn finite(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// A typed frequency in Hz: `1000`, `1k`, `1.5k`, `2.2 kHz`, `120 Hz`.
pub(crate) fn parse_freq(text: &str) -> Option<f64> {
    let text = bare(text);
    let text = text.strip_suffix("hz").unwrap_or(&text);
    let (number, scale) = match text.strip_suffix('k') {
        Some(number) => (number, 1_000.0),
        None => (text, 1.0),
    };
    finite(number)
        .map(|value| value * scale)
        .filter(|&hz| hz > 0.0)
}

/// A typed gain in dB: `-3.5`, `+6`, `2 dB`.
pub(crate) fn parse_gain(text: &str) -> Option<f64> {
    let text = bare(text);
    finite(text.strip_suffix("db").unwrap_or(&text))
}

/// A typed Q: `0.707`, `1.41`.
pub(crate) fn parse_q(text: &str) -> Option<f64> {
    finite(&bare(text)).filter(|&q| q > 0.0)
}

/// `text` read as `field`'s value, or `None` while it doesn't parse (a lone
/// `-`, `1.5x`).
pub(crate) fn parse_value(field: BandField, text: &str) -> Option<f64> {
    match field {
        BandField::Freq => parse_freq(text),
        BandField::Gain => parse_gain(text),
        BandField::Q => parse_q(text),
    }
}

/// `value` with at most `decimals` decimals, trailing zeros dropped.
fn trimmed(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// A frequency as the table shows it: `62.5`, `500`, `1k`, `1.5k`, `2.263k`.
pub(crate) fn format_freq(hz: f64) -> String {
    if hz >= 1_000.0 {
        format!("{}k", trimmed(hz / 1_000.0, 3))
    } else {
        trimmed(hz, 2)
    }
}

/// A gain as the table shows it: `+3.0`, `-3.5`, `-3.47`, `0.0`.
pub(crate) fn format_gain(db: f64) -> String {
    // `+ 0.0` turns a rounded -0.0 into 0.0, which prints without a sign.
    let db = (db * 100.0).round() / 100.0 + 0.0;
    let decimals = if ((db * 10.0).round() - db * 10.0).abs() < 1e-6 {
        1
    } else {
        2
    };
    if db > 0.0 {
        format!("+{db:.decimals$}")
    } else {
        format!("{db:.decimals$}")
    }
}

/// A Q as the table shows it: `0.707`, `1.41`, `1.0`.
pub(crate) fn format_q(q: f64) -> String {
    let text = trimmed(q, 3);
    if text.contains('.') {
        text
    } else {
        format!("{text}.0")
    }
}

/// `band`'s `field` as its text field shows it at rest (a pass filter's gain
/// is blank).
pub(crate) fn format_value(field: BandField, band: &EqBand) -> String {
    match field {
        BandField::Freq => format_freq(band.freq),
        BandField::Gain if band.kind.has_gain() => format_gain(band.gain),
        BandField::Gain => String::new(),
        BandField::Q => format_q(band.q),
    }
}

/// The automatic preamp as the status line shows it: `-5.4 dB`, `0.0 dB`.
pub(crate) fn format_preamp(db: f64) -> String {
    format!("{:.1} dB", (db * 10.0).round() / 10.0 + 0.0)
}

/// `band` with `field` set to `value`, held to the band limits. A pass
/// filter keeps its (unused) gain.
pub(crate) fn with_value(band: EqBand, field: BandField, value: f64) -> EqBand {
    match field {
        BandField::Freq => EqBand {
            freq: value,
            ..band
        },
        BandField::Gain if band.kind.has_gain() => EqBand {
            gain: value,
            ..band
        },
        BandField::Gain => band,
        BandField::Q => EqBand { q: value, ..band },
    }
    .clamped()
}

fn round_to(value: f64, scale: f64) -> f64 {
    (value * scale).round() / scale + 0.0
}

/// `value` to three significant digits (`1059.46` -> `1060`, `10.59` ->
/// `10.6`), so a run of frequency nudges lands on round numbers.
fn three_digits(value: f64) -> f64 {
    if value <= 0.0 || !value.is_finite() {
        return value;
    }
    let step = 10f64.powf(value.log10().floor() - 2.0);
    round_to((value / step).round() * step, 100.0)
}

/// `band` with `field` nudged one step up or down (Up / Down in its text
/// field): frequency by a twelfth of an octave, gain by 0.5 dB, Q by 5 %.
pub(crate) fn nudged(band: EqBand, field: BandField, up: bool) -> EqBand {
    match field {
        BandField::Freq => {
            let freq = if up {
                band.freq * FREQ_NUDGE
            } else {
                band.freq / FREQ_NUDGE
            };
            EqBand {
                freq: three_digits(freq),
                ..band
            }
        }
        BandField::Gain if band.kind.has_gain() => {
            let step = if up { GAIN_NUDGE_DB } else { -GAIN_NUDGE_DB };
            EqBand {
                gain: round_to(band.gain + step, 100.0),
                ..band
            }
        }
        BandField::Gain => band,
        BandField::Q => {
            let q = if up {
                band.q * Q_NUDGE
            } else {
                band.q / Q_NUDGE
            };
            EqBand {
                q: round_to(q, 1_000.0),
                ..band
            }
        }
    }
    .clamped()
}

// =============================================================================
// View
// =============================================================================

/// Curve height: kept low, so the modal with eight table rows fits a 720 px
/// window (about 640 px tall in all).
const CURVE_HEIGHT: f32 = 200.0;
/// One table row, and how many show before the table scrolls. The table's
/// height is fixed, so the modal doesn't change size as bands come and go.
const ROW_HEIGHT: f32 = 26.0;
const VISIBLE_ROWS: f32 = 8.0;
const TABLE_HEADER_HEIGHT: f32 = 14.0;
const TABLE_HEADER_GAP: f32 = 4.0;
const TABLE_BODY_HEIGHT: f32 = ROW_HEIGHT * VISIBLE_ROWS;
const TABLE_HEIGHT: f32 = TABLE_HEADER_HEIGHT + TABLE_HEADER_GAP + TABLE_BODY_HEIGHT;
/// Column widths, shared by the header and every row so they line up.
const NUMBER_WIDTH: f32 = 24.0;
const TOGGLE_WIDTH: f32 = 22.0;
const KIND_WIDTH: f32 = 128.0;
const FREQ_WIDTH: f32 = 84.0;
const GAIN_WIDTH: f32 = 80.0;
const Q_WIDTH: f32 = 72.0;
const REMOVE_SIZE: f32 = 22.0;
const COLUMN_GAP: f32 = 12.0;
/// The scrollbar's lane (embedded, so it never covers the remove buttons).
const SCROLLBAR_WIDTH: f32 = 4.0;
const SCROLLBAR_GAP: f32 = 6.0;
/// Text sizes: fields 13, the status line and labels 12, the header 11.
const FIELD_TEXT: f32 = 13.0;
const LABEL_TEXT: f32 = 12.0;
const HEADER_TEXT: f32 = 11.0;
/// A text input's inner padding: the header's numeric labels sit this far
/// from the column's right edge so they line up with the digits under them.
const FIELD_PAD_X: f32 = 6.0;

/// The Parametric view's body: curve, status line, band table.
pub(crate) fn parametric_body<'a>(data: &EqModalViewData<'a>) -> Element<'a, EqModalMessage> {
    let state = data.state;
    let bands = &data.bands;
    let selected = state.selected.filter(|&i| i < bands.len());

    let curve: Element<'a, EqModalMessage> = eq_curve(std::sync::Arc::clone(bands))
        .selected(selected)
        .dimmed(!data.enabled)
        .width(Length::Fill)
        .height(CURVE_HEIGHT)
        .on_select(EqModalMessage::PointPressed)
        .on_move(EqModalMessage::BandDragged)
        .on_release(|_| EqModalMessage::BandDragEnded)
        .on_q(EqModalMessage::BandQStepped)
        .on_add(EqModalMessage::AddBandAt)
        .into();

    let preamp = state.preamp_for(bands);
    let status = status_line(preamp, bands.len(), data.enabled);

    let table: Element<'a, EqModalMessage> = if bands.is_empty() {
        empty_table()
    } else {
        band_table(bands, selected, state.edit.as_ref(), &state.mounts)
    };

    column![curve, status, table]
        .spacing(10)
        .width(Length::Fill)
        .boxed()
}

/// AutoEq…: open the headphone search panel (`widgets/eq_autoeq.rs`).
const AUTOEQ_OPEN: EqModalMessage =
    EqModalMessage::AutoEq(crate::widgets::eq_autoeq::AutoEqMessage::Open);
const AUTOEQ_TOOLTIP: &str = "Search headphone profiles (downloads the list from GitHub)";

/// `Preamp -5.4 dB   6 of 32 bands   [Add band] [Import…] [AutoEq…] [Export…]`
fn status_line<'a>(preamp: f64, count: usize, enabled: bool) -> Element<'a, EqModalMessage> {
    let value_color = if enabled { theme::fg1() } else { theme::fg3() };
    let info = row![
        label("Preamp", theme::fg3()),
        label(format_preamp(preamp), value_color),
        space::horizontal().width(Length::Fixed(12.0)),
        label(
            format!(
                "{count} of {EQ_MAX_BANDS} {}",
                Noun::BAND.word(EQ_MAX_BANDS)
            ),
            theme::fg3(),
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let actions = row![
        with_tooltip(
            action_button("Add band", EqModalMessage::AddBand),
            "Add a peak at 1 kHz (or double-click the curve)",
        ),
        with_tooltip(
            action_button("Import…", EqModalMessage::Import),
            "Load an Equalizer APO or AutoEq ParametricEQ.txt",
        ),
        with_tooltip(action_button("AutoEq…", AUTOEQ_OPEN), AUTOEQ_TOOLTIP),
        with_tooltip(
            action_button("Export…", EqModalMessage::Export),
            "Save the bands as an Equalizer APO text file",
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    row![info, space::horizontal(), actions]
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .boxed()
}

/// The band list's empty state: what it is for and the ways to start.
fn empty_table<'a>() -> Element<'a, EqModalMessage> {
    let hint = column![
        text("No bands yet")
            .size(14.0)
            .font(theme::ui_font())
            .color(theme::fg2()),
        text(
            "Add one, start from the Graphic sliders, or import an AutoEq / Equalizer APO profile."
        )
        .size(LABEL_TEXT)
        .font(theme::ui_font())
        .color(theme::fg4()),
        row![
            action_button("Add band", EqModalMessage::AddBand),
            action_button("Copy from Graphic", EqModalMessage::CopyFromGraphic),
            action_button("Import…", EqModalMessage::Import),
            with_tooltip(action_button("AutoEq…", AUTOEQ_OPEN), AUTOEQ_TOOLTIP),
        ]
        .spacing(8),
    ]
    .spacing(10)
    .align_x(Alignment::Center);

    // The table's own height, so the modal keeps its size as the first
    // band comes or the last one goes.
    container(hint)
        .center_x(Length::Fill)
        .center_y(Length::Fixed(TABLE_HEIGHT))
        .boxed()
}

/// The header row and one row per band, scrolling past [`VISIBLE_ROWS`].
fn band_table<'a>(
    bands: &[EqBand],
    selected: Option<usize>,
    edit: Option<&FieldEdit>,
    mounts: &FieldMounts,
) -> Element<'a, EqModalMessage> {
    let header = row![
        header_cell("#", NUMBER_WIDTH, Alignment::End),
        space::horizontal().width(Length::Fixed(TOGGLE_WIDTH)),
        header_cell("Type", KIND_WIDTH, Alignment::Start),
        header_cell("Freq (Hz)", FREQ_WIDTH, Alignment::End),
        header_cell("Gain (dB)", GAIN_WIDTH, Alignment::End),
        header_cell("Q", Q_WIDTH, Alignment::End),
    ]
    .spacing(COLUMN_GAP)
    .padding(Padding::new(0.0).left(6.0))
    .height(Length::Fixed(TABLE_HEADER_HEIGHT))
    .align_y(Alignment::Center);

    let rows = Column::with_children(
        bands
            .iter()
            .enumerate()
            .map(|(i, band)| band_row(i, band, selected == Some(i), edit, mounts)),
    )
    .width(Length::Fill);

    let body = scrollable(rows)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH)
                .spacing(SCROLLBAR_GAP),
        ))
        .id(TABLE_SCROLL_ID)
        .width(Length::Fill)
        .height(Length::Fixed(TABLE_BODY_HEIGHT))
        .style(theme::settings_scrollable_style);

    column![header, body]
        .spacing(TABLE_HEADER_GAP)
        .width(Length::Fill)
        .height(Length::Fixed(TABLE_HEIGHT))
        .boxed()
}

/// One band: number, on/off, type, frequency, gain, Q, remove.
fn band_row<'a>(
    i: usize,
    band: &EqBand,
    is_selected: bool,
    edit: Option<&FieldEdit>,
    mounts: &FieldMounts,
) -> Element<'a, EqModalMessage> {
    let number_color = if is_selected {
        theme::accent_bright()
    } else if band.enabled {
        theme::fg3()
    } else {
        theme::fg4()
    };
    let number_weight = if is_selected {
        iced::font::Weight::Bold
    } else {
        iced::font::Weight::Normal
    };
    let number = container(
        text((i + 1).to_string())
            .size(LABEL_TEXT)
            .font(theme::weighted_ui_font(number_weight))
            .color(number_color),
    )
    .width(Length::Fixed(NUMBER_WIDTH))
    .align_x(Alignment::End);

    let toggle = tooltip(
        mouse_area(
            container(checkbox_glyph::element(band.enabled))
                .center_x(Length::Fixed(TOGGLE_WIDTH))
                .center_y(Length::Fixed(ROW_HEIGHT)),
        )
        .on_press(EqModalMessage::ToggleBand(i))
        .interaction(iced::mouse::Interaction::Pointer),
        tip(if band.enabled {
            "Switch this band off"
        } else {
            "Switch this band on"
        }),
        tooltip::Position::Top,
    )
    .gap(4)
    .style(theme::container_tooltip);

    let kind = pick_list(
        Some(band.kind),
        &EqBandKind::ALL[..],
        |kind: &EqBandKind| kind.label().to_owned(),
    )
    .on_select(move |kind| EqModalMessage::BandKindSelected(i, kind))
    .font(theme::ui_font())
    .text_size(FIELD_TEXT)
    .padding([3.0, FIELD_PAD_X])
    .width(Length::Fixed(KIND_WIDTH))
    .style(kind_pick_style)
    .menu_style(kind_menu_style);

    let value_field = |field: BandField, width: f32| {
        let enabled = field_enabled(band, field);
        let input = text_input(
            if enabled { "" } else { "—" },
            field_text(i, band, field, edit),
        )
        .id(field_id(i, field))
        .size(FIELD_TEXT)
        .padding([3.0, FIELD_PAD_X])
        .width(Length::Fixed(width))
        .font(theme::ui_font())
        .align_x(Alignment::End)
        .style(field_style(band.enabled));
        let input = if enabled {
            input
                .on_input(move |typed| EqModalMessage::FieldInput(i, field, typed))
                .on_submit(EqModalMessage::FieldSubmit(i, field))
        } else {
            input
        };
        // Keep this key: iced's text_input redraws a value replaced from
        // outside the field (`Editor::overwrite` sets default attributes, and
        // the font is re-applied only when the font changes) in the system
        // sans-serif while its state is kept. `FieldMounts` re-keys a field
        // at every outside change, so it gets a fresh state.
        remount(mounts.key(i, field), input)
    };

    let remove = tooltip(
        mouse_area(
            HoverOverlay::new(
                container(
                    crate::embedded_svg::svg_widget("assets/icons/x.svg")
                        .width(Length::Fixed(12.0))
                        .height(Length::Fixed(12.0))
                        .style(|_theme, _status| svg::Style {
                            color: Some(theme::fg3()),
                        }),
                )
                .width(Length::Fixed(REMOVE_SIZE))
                .height(Length::Fixed(REMOVE_SIZE))
                .center(Length::Fixed(REMOVE_SIZE)),
            )
            .border_radius(theme::ui_radius_xs()),
        )
        .on_press(EqModalMessage::RemoveBand(i))
        .interaction(iced::mouse::Interaction::Pointer),
        tip("Remove band"),
        tooltip::Position::Top,
    )
    .gap(4)
    .style(theme::container_tooltip);

    let cells = row![
        number,
        toggle,
        kind,
        value_field(BandField::Freq, FREQ_WIDTH),
        value_field(BandField::Gain, GAIN_WIDTH),
        value_field(BandField::Q, Q_WIDTH),
        space::horizontal(),
        remove,
    ]
    .spacing(COLUMN_GAP)
    .width(Length::Fill)
    .align_y(Alignment::Center);

    mouse_area(
        container(cells)
            .id(row_id(i))
            .width(Length::Fill)
            .height(Length::Fixed(ROW_HEIGHT))
            .padding(Padding::new(0.0).left(6.0).right(4.0))
            .align_y(Alignment::Center)
            .style(move |_theme| container::Style {
                background: is_selected.then(|| theme::bg1().into()),
                border: iced::Border {
                    radius: theme::ui_radius_sm(),
                    ..Default::default()
                },
                ..Default::default()
            }),
    )
    .on_press(EqModalMessage::SelectBand(i))
    .boxed()
}

fn label<'a>(content: impl text::IntoFragment<'a>, color: iced::Color) -> text::Text<'a> {
    text(content)
        .size(LABEL_TEXT)
        .font(theme::ui_font())
        .color(color)
}

fn tip<'a>(content: &'static str) -> Element<'a, EqModalMessage> {
    container(text(content).size(11.0).font(theme::ui_font()))
        .padding(4)
        .boxed()
}

pub(crate) fn with_tooltip<'a>(
    content: Element<'a, EqModalMessage>,
    hint: &'static str,
) -> Element<'a, EqModalMessage> {
    tooltip(content, tip(hint), tooltip::Position::Bottom)
        .gap(4)
        .style(theme::container_tooltip)
        .boxed()
}

fn header_cell<'a>(
    content: &'static str,
    width: f32,
    align: Alignment,
) -> Element<'a, EqModalMessage> {
    // Line the label up with the field text under it, inside the padding.
    let pad = FIELD_PAD_X + 1.0;
    let padding = match align {
        Alignment::End => Padding::new(0.0).right(pad),
        Alignment::Start | Alignment::Center => Padding::new(0.0).left(pad),
    };
    container(
        text(content)
            .size(HEADER_TEXT)
            .font(theme::ui_font())
            .color(theme::fg4()),
    )
    .width(Length::Fixed(width))
    .padding(padding)
    .align_x(align)
    .boxed()
}

/// A flat text button: `bg0_hard` with a 1 px border that turns
/// `accent_bright` under the cursor.
pub(crate) fn action_button<'a>(
    content: &'static str,
    on_press: EqModalMessage,
) -> Element<'a, EqModalMessage> {
    button(text(content).size(FIELD_TEXT).font(theme::ui_font()))
        .on_press(on_press)
        .padding([4, 10])
        .style(|_theme, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(if hot { theme::bg1() } else { theme::bg0_hard() }.into()),
                border: iced::Border {
                    color: if hot {
                        theme::accent_bright()
                    } else {
                        theme::border()
                    },
                    width: 1.0,
                    radius: theme::ui_radius_sm(),
                },
                text_color: if hot { theme::fg0() } else { theme::fg1() },
                ..Default::default()
            }
        })
        .boxed()
}

/// A numeric field: `bg0_hard`, 1 px border, `accent_bright` when hovered or
/// focused; a switched-off band's values read dimmer.
fn field_style(
    band_on: bool,
) -> impl Fn(&iced::Theme, text_input::Status) -> text_input::Style + 'static {
    move |_theme, status| text_input::Style {
        background: theme::bg0_hard().into(),
        border: iced::Border {
            color: match status {
                text_input::Status::Hovered | text_input::Status::Focused { .. } => {
                    theme::accent_bright()
                }
                text_input::Status::Active | text_input::Status::Disabled => theme::border(),
            },
            width: 1.0,
            radius: theme::ui_radius_xs(),
        },
        placeholder: theme::fg4(),
        value: if band_on { theme::fg1() } else { theme::fg3() },
        selection: theme::selection_color(),
    }
}

fn kind_pick_style(_theme: &iced::Theme, status: pick_list::Status) -> pick_list::Style {
    pick_list::Style {
        text_color: theme::fg1(),
        background: theme::bg0_hard().into(),
        border: iced::Border {
            color: match status {
                pick_list::Status::Active | pick_list::Status::Disabled => theme::border(),
                pick_list::Status::Hovered | pick_list::Status::Opened { .. } => {
                    theme::accent_bright()
                }
            },
            width: 1.0,
            radius: theme::ui_radius_xs(),
        },
        placeholder_color: theme::fg3(),
        handle_color: theme::fg3(),
    }
}

fn kind_menu_style(_theme: &iced::Theme) -> iced::widget::overlay::menu::Style {
    iced::widget::overlay::menu::Style {
        text_color: theme::fg0(),
        background: theme::bg1().into(),
        border: iced::Border {
            color: theme::accent_bright(),
            width: 1.0,
            radius: theme::ui_radius_sm(),
        },
        selected_text_color: theme::on_accent_fill(),
        selected_background: theme::accent_fill().into(),
        shadow: iced::Shadow::default(),
    }
}

/// Memo of the automatic preamp, keyed by the band list: `view` runs every
/// frame while anything on screen animates, and the preamp's peak search
/// evaluates every band at ~570 frequencies.
#[derive(Debug, Default)]
pub(crate) struct PreampMemo {
    key: std::sync::atomic::AtomicU64,
    db_bits: std::sync::atomic::AtomicU64,
    filled: std::sync::atomic::AtomicBool,
}

/// Sample rate the shown preamp is computed at (the curve's).
const PREAMP_RATE: f64 = 48_000.0;

impl PreampMemo {
    /// The preamp `bands` take, computed once per band list.
    pub(crate) fn get(&self, bands: &[EqBand]) -> f64 {
        use std::sync::atomic::Ordering::Relaxed;
        let key = bands_key(bands);
        if self.filled.load(Relaxed) && self.key.load(Relaxed) == key {
            return f64::from_bits(self.db_bits.load(Relaxed));
        }
        let db = preamp_db(bands, PREAMP_RATE);
        self.key.store(key, Relaxed);
        self.db_bits.store(db.to_bits(), Relaxed);
        self.filled.store(true, Relaxed);
        db
    }
}

#[cfg(test)]
mod tests {
    use nokkvi_data::audio::eq::{
        EQ_BAND_GAIN_DB_MAX, EQ_DEFAULT_Q, EQ_FREQ_MAX, EQ_FREQ_MIN, EQ_Q_MIN,
    };

    use super::*;

    fn peak(freq: f64, gain: f64, q: f64) -> EqBand {
        EqBand::new(EqBandKind::Peak, freq, gain, q)
    }

    #[test]
    fn frequencies_parse_with_or_without_units_and_a_k() {
        assert_eq!(parse_freq("1000"), Some(1_000.0));
        assert_eq!(parse_freq("1k"), Some(1_000.0));
        assert_eq!(parse_freq("1.5k"), Some(1_500.0));
        assert_eq!(parse_freq("1.5K"), Some(1_500.0));
        assert_eq!(parse_freq(" 2.2 kHz "), Some(2_200.0));
        assert_eq!(parse_freq("120 Hz"), Some(120.0));
        assert_eq!(parse_freq("62,5"), Some(62.5));
        for junk in ["", "k", "-", "1.5x", "abc", "inf", "NaN", "0", "-20"] {
            assert_eq!(parse_freq(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn gains_and_qs_parse_and_refuse_junk() {
        assert_eq!(parse_gain("-3.5"), Some(-3.5));
        assert_eq!(parse_gain("+6"), Some(6.0));
        assert_eq!(parse_gain("2 dB"), Some(2.0));
        assert_eq!(parse_gain("-1,25"), Some(-1.25));
        for junk in ["", "-", "+", ".", "3..5", "inf"] {
            assert_eq!(parse_gain(junk), None, "{junk:?}");
        }
        assert_eq!(parse_q("0.707"), Some(0.707));
        assert_eq!(parse_q("4"), Some(4.0));
        for junk in ["", "0", "-1", "q", "1.2.3"] {
            assert_eq!(parse_q(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn values_format_like_an_eq_table() {
        assert_eq!(format_freq(31.0), "31");
        assert_eq!(format_freq(62.5), "62.5");
        assert_eq!(format_freq(20.25), "20.25");
        assert_eq!(format_freq(1_000.0), "1k");
        assert_eq!(format_freq(1_500.0), "1.5k");
        assert_eq!(format_freq(2_263.0), "2.263k");
        assert_eq!(format_freq(22_000.0), "22k");
        assert_eq!(format_gain(3.0), "+3.0");
        assert_eq!(format_gain(-3.5), "-3.5");
        assert_eq!(format_gain(-3.47), "-3.47");
        assert_eq!(format_gain(0.0), "0.0");
        assert_eq!(format_gain(-0.001), "0.0");
        assert_eq!(format_q(EQ_DEFAULT_Q), "0.707");
        assert_eq!(format_q(1.0), "1.0");
        assert_eq!(format_q(1.41), "1.41");
        assert_eq!(format_preamp(-5.43), "-5.4 dB");
        assert_eq!(format_preamp(-0.01), "0.0 dB");
    }

    #[test]
    fn a_formatted_value_parses_back_to_itself() {
        for hz in [10.0, 31.0, 62.5, 999.0, 1_000.0, 1_500.0, 2_263.0, 22_000.0] {
            assert_eq!(parse_freq(&format_freq(hz)), Some(hz), "{hz}");
        }
        for db in [-30.0, -3.47, 0.0, 6.1, 30.0] {
            assert_eq!(parse_gain(&format_gain(db)), Some(db), "{db}");
        }
        for q in [0.1, 0.707, 1.0, 1.41, 20.0] {
            assert_eq!(parse_q(&format_q(q)), Some(q), "{q}");
        }
    }

    #[test]
    fn a_pass_filter_shows_and_takes_no_gain() {
        let cut = EqBand::new(EqBandKind::HighPass, 30.0, 4.0, EQ_DEFAULT_Q);
        assert_eq!(format_value(BandField::Gain, &cut), "");
        assert!(!field_enabled(&cut, BandField::Gain));
        assert!(field_enabled(&cut, BandField::Q));
        assert_eq!(with_value(cut, BandField::Gain, -6.0).gain, 4.0);
        assert_eq!(nudged(cut, BandField::Gain, true).gain, 4.0);
    }

    #[test]
    fn typed_values_are_held_to_the_band_limits() {
        let band = peak(1_000.0, 0.0, 1.0);
        assert_eq!(
            with_value(band, BandField::Freq, 50_000.0).freq,
            EQ_FREQ_MAX
        );
        assert_eq!(with_value(band, BandField::Freq, 1.0).freq, EQ_FREQ_MIN);
        assert_eq!(
            with_value(band, BandField::Gain, -99.0).gain,
            -EQ_BAND_GAIN_DB_MAX
        );
        assert_eq!(with_value(band, BandField::Q, 0.01).q, EQ_Q_MIN);
        assert_eq!(with_value(band, BandField::Q, 2.5).q, 2.5);
    }

    #[test]
    fn nudges_step_a_twelfth_octave_half_a_db_and_five_percent() {
        let band = peak(1_000.0, -3.47, 1.0);
        assert_eq!(nudged(band, BandField::Freq, true).freq, 1_060.0);
        assert_eq!(nudged(band, BandField::Freq, false).freq, 944.0);
        assert_eq!(
            nudged(peak(10.0, 0.0, 1.0), BandField::Freq, true).freq,
            10.6
        );
        assert_eq!(nudged(band, BandField::Gain, true).gain, -2.97);
        assert_eq!(nudged(band, BandField::Gain, false).gain, -3.97);
        assert_eq!(nudged(band, BandField::Q, true).q, 1.05);
        assert_eq!(nudged(band, BandField::Q, false).q, 0.952);
        // Held at the limits.
        let top = peak(EQ_FREQ_MAX, EQ_BAND_GAIN_DB_MAX, 1.0);
        assert_eq!(nudged(top, BandField::Freq, true).freq, EQ_FREQ_MAX);
        assert_eq!(nudged(top, BandField::Gain, true).gain, EQ_BAND_GAIN_DB_MAX);
        // Twelve steps up is about an octave.
        let mut octave = peak(100.0, 0.0, 1.0);
        for _ in 0..12 {
            octave = nudged(octave, BandField::Freq, true);
        }
        assert!((octave.freq - 200.0).abs() <= 2.0, "{}", octave.freq);
    }

    #[test]
    fn tab_walks_the_fields_in_reading_order_and_skips_a_pass_filters_gain() {
        let bands = [
            peak(100.0, 3.0, 1.0),
            EqBand::new(EqBandKind::LowPass, 15_000.0, 0.0, EQ_DEFAULT_Q),
        ];
        let mut at = None;
        let mut walk = Vec::new();
        for _ in 0..6 {
            at = adjacent_field(&bands, at, true);
            walk.push(at.expect("a field"));
        }
        assert_eq!(
            walk,
            [
                (0, BandField::Freq),
                (0, BandField::Gain),
                (0, BandField::Q),
                (1, BandField::Freq),
                (1, BandField::Q),
                // Wraps to the top.
                (0, BandField::Freq),
            ]
        );
        assert_eq!(
            adjacent_field(&bands, Some((0, BandField::Freq)), false),
            Some((1, BandField::Q)),
            "Shift+Tab from the first field wraps to the last"
        );
        assert_eq!(adjacent_field(&bands, None, false), Some((1, BandField::Q)));
        // A disabled field that still had focus (its band just became a pass
        // filter) steps on from its place.
        assert_eq!(
            adjacent_field(&bands, Some((1, BandField::Gain)), true),
            Some((1, BandField::Q))
        );
        assert_eq!(adjacent_field(&[], None, true), None);
    }

    #[test]
    fn row_and_field_ids_never_collide() {
        for band in 0..EQ_MAX_BANDS {
            for field in BandField::ALL {
                assert_ne!(row_id(band), field_id(band, field));
                assert_ne!(row_id(band), iced::widget::Id::new(TABLE_SCROLL_ID));
            }
        }
    }

    #[test]
    fn field_ids_round_trip_and_ignore_bands_past_the_list() {
        let id = field_id(3, BandField::Gain);
        assert_eq!(field_at(&id, 4), Some((3, BandField::Gain)));
        assert_eq!(field_at(&id, 3), None, "band 3 is gone");
        assert_eq!(field_at(&iced::widget::Id::new("search"), 4), None);
        assert_ne!(field_id(1, BandField::Q), field_id(11, BandField::Q));
    }

    #[test]
    fn a_field_shows_its_typed_text_while_edited_else_its_value() {
        let band = peak(2_500.0, -3.0, 1.41);
        let editing = |band, field, text: &str| FieldEdit {
            band,
            field,
            text: text.to_owned(),
        };
        assert_eq!(field_text(0, &band, BandField::Freq, None), "2.5k");
        let typed = editing(0, BandField::Freq, "2500");
        assert_eq!(field_text(0, &band, BandField::Freq, Some(&typed)), "2500");
        assert_eq!(
            field_text(0, &band, BandField::Gain, Some(&typed)),
            "-3.0",
            "another field of the band shows its value"
        );
        assert_eq!(
            field_text(1, &band, BandField::Freq, Some(&typed)),
            "2.5k",
            "the same field of another band too"
        );
        let cut = EqBand::new(EqBandKind::LowPass, 15_000.0, 0.0, EQ_DEFAULT_Q);
        assert_eq!(field_text(0, &cut, BandField::Gain, None), "");
    }

    #[test]
    fn mounts_rekey_changed_fields_except_the_one_typed_into() {
        let mut mounts = FieldMounts::default();
        let mut bands = vec![peak(100.0, 3.0, 1.0), peak(1_000.0, -2.0, 1.41)];
        mounts.sync(&bands, None);
        let first: Vec<u64> = (0..2)
            .flat_map(|band| BandField::ALL.map(|field| mounts.key(band, field)))
            .collect();
        assert!(first.iter().all(|&key| key != 0));
        let mut unique = first.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), first.len(), "every field its own key");

        // Nothing changed: nothing re-keyed.
        mounts.sync(&bands, None);
        assert_eq!(mounts.key(1, BandField::Q), first[5]);

        // A value from outside: that field only.
        bands[1].gain = -2.5;
        mounts.sync(&bands, None);
        assert_ne!(mounts.key(1, BandField::Gain), first[4]);
        assert_eq!(mounts.key(1, BandField::Freq), first[3]);

        // Typed text: the field's own widget holds it, no re-key; its value
        // changing under the typing doesn't re-key it either.
        let typing = FieldEdit {
            band: 0,
            field: BandField::Q,
            text: "2".to_owned(),
        };
        mounts.sync(&bands, Some(&typing));
        bands[0].q = 2.0;
        mounts.sync(&bands, Some(&typing));
        assert_eq!(mounts.key(0, BandField::Q), first[2]);
        // The commit shows `2.0`, not the `2` typed: re-keyed.
        mounts.sync(&bands, None);
        assert_ne!(mounts.key(0, BandField::Q), first[2]);

        // The list shrinks and grows back: the returning row takes new keys.
        let last = mounts.key(1, BandField::Freq);
        mounts.sync(&bands[..1], None);
        assert_eq!(mounts.key(1, BandField::Freq), 0, "gone");
        mounts.sync(&bands, None);
        assert!(mounts.key(1, BandField::Freq) > last);
    }

    #[test]
    fn the_preamp_memo_follows_the_band_list() {
        let memo = PreampMemo::default();
        let boost = [peak(1_000.0, 6.0, 1.0)];
        let flat = [peak(1_000.0, 0.0, 1.0)];
        let first = memo.get(&boost);
        assert!(first < -5.9, "a 6 dB boost takes about -6 dB: {first}");
        assert_eq!(memo.get(&boost), first);
        assert_eq!(memo.get(&flat), 0.0);
        assert_eq!(memo.get(&[]), 0.0);
    }
}
