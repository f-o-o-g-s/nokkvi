//! Composable metadata pill builders
//!
//! Reusable row-level building blocks for artwork panel metadata overlays.
//! Each function returns `Option<Element>` — `None` when there's nothing to show,
//! letting callers chain with `if let Some(row) = ... { col = col.push(row); }`.

use iced::{Element, Widget as _, widget::text};

use crate::theme;

/// "Favorited ♥ • Rated ★★★★★" row.
///
/// Returns `None` when `!is_starred && rating <= 0`.
pub(crate) fn auth_status_row<'a, M: 'a>(
    is_starred: bool,
    rating: Option<u32>,
) -> Option<Element<'a, M>> {
    let mut items: Vec<Element<'a, M>> = Vec::new();

    if is_starred {
        let heart = crate::embedded_svg::svg_widget("assets/icons/heart-filled.svg")
            .width(13)
            .height(13)
            .style(|_, _| iced::widget::svg::Style {
                color: Some(theme::love_color()),
            });
        items.push(
            iced::widget::row![
                text("Favorited ")
                    .size(13)
                    .color(theme::fg2())
                    .font(theme::ui_font()),
                heart
            ]
            .align_y(iced::Alignment::Center)
            .boxed(),
        );
    }

    if let Some(r) = rating
        && r > 0
    {
        let mut stars_row = iced::widget::row![
            text("Rated ")
                .size(13)
                .color(theme::fg2())
                .font(theme::ui_font())
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center);

        for _ in 0..r {
            stars_row = stars_row.push(
                crate::embedded_svg::svg_widget("assets/icons/star-filled.svg")
                    .width(13)
                    .height(13)
                    .style(|_, _| iced::widget::svg::Style {
                        color: Some(theme::rating_color()),
                    })
                    .boxed(),
            );
        }
        items.push(stars_row.boxed());
    }

    if items.is_empty() {
        return None;
    }

    let mut row = iced::widget::Row::<Element<'_, _>>::new()
        .spacing(12)
        .align_y(iced::Alignment::Center);
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            row = row.push(
                text("•")
                    .size(13)
                    .font(theme::ui_font())
                    .color(theme::fg3())
                    .boxed(),
            );
        }
        row = row.push(item);
    }
    Some(row.boxed())
}

/// "N plays • Last played: YYYY-MM-DD" row.
///
/// Returns `None` when both fields are absent.
pub(crate) fn play_stats_row<'a, M: 'a>(
    play_count: Option<u32>,
    play_date: Option<&str>,
) -> Option<Element<'a, M>> {
    let mut items = Vec::new();
    if let Some(plays) = play_count {
        items.push(format!("{plays} plays"));
    }
    if let Some(date) = play_date {
        let ymd = date.split('T').next().unwrap_or(date);
        items.push(format!("Last played: {ymd}"));
    }
    if items.is_empty() {
        return None;
    }
    Some(
        text(items.join(" • "))
            .size(13)
            .color(theme::fg2())
            .font(theme::ui_font())
            .boxed(),
    )
}

/// "FLAC • 16-bit • 44.1 kHz • 900 kbps • 120 BPM" tech specs row.
///
/// Returns `None` when all fields are absent or zero.
pub(crate) fn tech_specs_row<'a, M: 'a>(
    suffix: Option<&str>,
    bit_depth: Option<u32>,
    sample_rate: Option<u32>,
    bitrate: Option<u32>,
    bpm: Option<u32>,
) -> Option<Element<'a, M>> {
    let mut specs = Vec::new();
    if let Some(s) = suffix {
        specs.push(s.to_uppercase());
    }
    if let Some(depth) = bit_depth
        && depth > 0
    {
        specs.push(format!("{depth}-bit"));
    }
    if let Some(rate) = sample_rate
        && rate > 0
    {
        specs.push(format!("{} kHz", rate as f32 / 1000.0));
    }
    if let Some(br) = bitrate
        && br > 0
    {
        specs.push(format!("{br} kbps"));
    }
    if let Some(b) = bpm {
        specs.push(format!("{b} BPM"));
    }
    if specs.is_empty() {
        return None;
    }
    Some(
        text(specs.join(" • "))
            .size(12)
            .color(theme::fg3())
            .font(theme::ui_font())
            .boxed(),
    )
}

/// Generic dot-separated text row at a given size and color.
///
/// Returns `None` when `items` is empty.
pub(crate) fn dot_row<'a, M: 'a>(
    items: Vec<String>,
    size: f32,
    color: iced::Color,
) -> Option<Element<'a, M>> {
    if items.is_empty() {
        return None;
    }
    Some(
        text(items.join(" • "))
            .size(size)
            .color(color)
            .font(theme::ui_font())
            .boxed(),
    )
}

/// How much of an agent's text (an artist's biography, an album's notes) a
/// pill shows before it trails off, in characters.
pub(crate) const PILL_TEXT_PREVIEW_CHARS: usize = 350;

/// `body` as a pill shows it: whole when it fits the preview length, else its
/// opening with "..." where it was cut.
pub(crate) fn pill_text_preview(body: &str) -> String {
    match body.char_indices().nth(PILL_TEXT_PREVIEW_CHARS) {
        Some((cut, _)) => format!("{}...", body[..cut].trim_end()),
        None => body.to_string(),
    }
}

/// A pill link's label: it names where the link goes ("Read more on
/// last.fm"), since Navidrome fills the link from whichever agent answered
/// and it is not always Last.fm. "Read more" when the link has no host.
pub(crate) fn read_more_label(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| {
            let host = parsed.host_str()?;
            let host = host.strip_prefix("www.").unwrap_or(host);
            (!host.is_empty()).then(|| format!("Read more on {host}"))
        })
        .unwrap_or_else(|| "Read more".to_string())
}

/// `url` when it is a web link. A pill's link is handed to `xdg-open`, so
/// nothing but http(s) is offered.
pub(crate) fn web_link(url: Option<&str>) -> Option<&str> {
    url.filter(|url| nokkvi_data::utils::server_url::has_http_scheme(url))
}

/// An agent's text under a pill's own rows (an artist's biography, an
/// album's notes): its opening, centred, then a link to the rest when the
/// agent gave a page. `on_link` gets the page's address.
pub(crate) fn agent_text_block<'a, M: Clone + 'a>(
    body: &str,
    link: Option<&str>,
    on_link: impl FnOnce(String) -> M,
) -> Element<'a, M> {
    use iced::widget::{button, column};

    let mut block = column![
        text(pill_text_preview(body))
            .size(13)
            .color(theme::fg1())
            .font(theme::ui_font())
            .center()
    ]
    .spacing(4)
    .align_x(iced::Alignment::Center);

    if let Some(url) = link {
        let read_more = button(
            text(read_more_label(url))
                .size(11)
                .color(theme::accent_bright())
                .font(theme::weighted_ui_font(iced::font::Weight::Bold)),
        )
        .on_press(on_link(url.to_string()))
        .padding(iced::Padding {
            top: 2.0,
            bottom: 2.0,
            left: 6.0,
            right: 6.0,
        })
        .style(|_theme, status| {
            let opacity =
                match status {
                    iced::widget::button::Status::Hovered
                    | iced::widget::button::Status::Pressed => 0.75,
                    iced::widget::button::Status::Active
                    | iced::widget::button::Status::Disabled => 1.0,
                };
            let mut color = theme::accent_bright();
            color.a = opacity;
            iced::widget::button::Style {
                background: None,
                border: iced::Border {
                    width: 0.0,
                    ..Default::default()
                },
                text_color: color,
                ..Default::default()
            }
        });
        block = block.push(read_more.boxed());
    }

    block.boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_that_fits_is_shown_whole() {
        assert_eq!(pill_text_preview("A short note."), "A short note.");
        let exact = "x".repeat(PILL_TEXT_PREVIEW_CHARS);
        assert_eq!(pill_text_preview(&exact), exact);
    }

    #[test]
    fn a_long_text_is_cut_at_the_preview_length_with_an_ellipsis() {
        let long = format!("{} tail", "x".repeat(PILL_TEXT_PREVIEW_CHARS));
        assert_eq!(
            pill_text_preview(&long),
            format!("{}...", "x".repeat(PILL_TEXT_PREVIEW_CHARS))
        );
        // A cut that lands after a space leaves no gap before the dots.
        let spaced = format!("{} y and more", "x".repeat(PILL_TEXT_PREVIEW_CHARS - 1));
        assert_eq!(
            pill_text_preview(&spaced),
            format!("{}...", "x".repeat(PILL_TEXT_PREVIEW_CHARS - 1))
        );
    }

    /// The length is counted in characters: a cut inside a multi-byte
    /// character would panic.
    #[test]
    fn the_preview_counts_characters_not_bytes() {
        let cyrillic = "ж".repeat(PILL_TEXT_PREVIEW_CHARS + 10);
        assert_eq!(
            pill_text_preview(&cyrillic),
            format!("{}...", "ж".repeat(PILL_TEXT_PREVIEW_CHARS))
        );
    }

    #[test]
    fn the_link_label_names_the_site_it_opens() {
        assert_eq!(
            read_more_label("https://www.last.fm/music/Sia"),
            "Read more on last.fm"
        );
        assert_eq!(
            read_more_label("https://isissemajhall.com/"),
            "Read more on isissemajhall.com"
        );
        assert_eq!(read_more_label("not a url"), "Read more");
    }

    #[test]
    fn only_a_web_link_is_offered() {
        assert_eq!(web_link(Some("file:///etc/passwd")), None);
        assert_eq!(web_link(Some("javascript:alert(1)")), None);
        assert_eq!(
            web_link(Some("HTTPS://example.org/a")),
            Some("HTTPS://example.org/a")
        );
        assert_eq!(web_link(None), None);
    }
}
