//! Plain text from the HTML Navidrome's metadata agents hand back: an
//! artist's biography and an album's notes come from Last.fm or another
//! agent and can carry tags, mostly a trailing `<a>` link.

/// `html` as plain text: tags dropped, `<a href="URL">text</a>` written as
/// `text (URL)`, the common entities decoded. A dropped paragraph or line
/// break leaves one space, so the sentences either side do not run together.
///
/// A `<` only starts a tag when a letter, `/` or `!` follows it and a `>`
/// closes it, so prose like "<3" or "a < b" is kept as written.
pub fn strip_html_tags(html: &str) -> String {
    decode_entities(&strip_tags(html))
}

/// [`strip_html_tags`] without the entity pass, which runs once over the
/// whole result: an escaped `&lt;b&gt;` must come out as the text `<b>`, not
/// be stripped as a tag.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    // A dropped block tag owes one space before the next text.
    let mut owes_space = false;
    let mut rest = html;

    while let Some(lt) = rest.find('<') {
        push_text(&mut out, &rest[..lt], &mut owes_space);
        let after = &rest[lt + 1..];
        let opens_tag =
            after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        let Some(end) = after.find('>').filter(|_| opens_tag) else {
            push_text(&mut out, "<", &mut owes_space);
            rest = after;
            continue;
        };
        let tag = &after[..end];
        rest = &after[end + 1..];

        if is_anchor_open(tag) {
            let (inner, tail) = split_at_anchor_close(rest);
            rest = tail;
            let text = strip_tags(inner);
            let text = text.trim();
            match extract_href(tag) {
                Some(url) if text.is_empty() || text == url => {
                    push_text(&mut out, url, &mut owes_space);
                }
                Some(url) => {
                    push_text(&mut out, &format!("{text} ({url})"), &mut owes_space);
                }
                None => push_text(&mut out, text, &mut owes_space),
            }
        } else if is_block_tag(tag) {
            owes_space = true;
        }
    }
    push_text(&mut out, rest, &mut owes_space);
    out
}

/// Append `text`, first paying the space a dropped block tag owes unless
/// whitespace already separates the two sides.
fn push_text(out: &mut String, text: &str, owes_space: &mut bool) {
    if text.is_empty() {
        return;
    }
    if std::mem::take(owes_space)
        && !out.is_empty()
        && !out.ends_with(char::is_whitespace)
        && !text.starts_with(char::is_whitespace)
    {
        out.push(' ');
    }
    out.push_str(text);
}

/// Whether `tag` (the text between `<` and `>`) opens a link: `a` and then
/// its attributes.
fn is_anchor_open(tag: &str) -> bool {
    let mut chars = tag.chars();
    chars.next().is_some_and(|c| c.eq_ignore_ascii_case(&'a'))
        && chars.next().is_some_and(char::is_whitespace)
}

/// Whether `tag` opens or closes a block: text either side of it is on
/// separate lines in a browser.
fn is_block_tag(tag: &str) -> bool {
    const BLOCK_TAGS: [&str; 14] = [
        "br",
        "p",
        "div",
        "li",
        "ul",
        "ol",
        "tr",
        "blockquote",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
    ];
    let name = tag.trim_start_matches('/');
    let name = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or_default();
    BLOCK_TAGS
        .iter()
        .any(|block| name.eq_ignore_ascii_case(block))
}

/// Split `rest` at the `</a>` that closes a link: the link's text, then what
/// follows the closing tag. A link left unclosed takes the rest as its text.
fn split_at_anchor_close(rest: &str) -> (&str, &str) {
    // ASCII lowercasing keeps every byte where it was, so an offset found
    // in the copy is the same offset in `rest`.
    let lower = rest.to_ascii_lowercase();
    for (at, _) in lower.match_indices("</a") {
        let after_name = &lower[at + 3..];
        let closing = after_name.trim_start();
        if closing.starts_with('>') {
            let tail = at + 3 + (after_name.len() - closing.len()) + 1;
            return (&rest[..at], &rest[tail..]);
        }
    }
    (rest, "")
}

/// The `href` value in an anchor tag's attributes.
fn extract_href(tag_attrs: &str) -> Option<&str> {
    // An offset from an ASCII-lowercased copy is valid in the original. A
    // full `to_lowercase` is not: some capitals change byte length (İ, the
    // Kelvin sign), and slicing at its offset once cut a character in half.
    let href_pos = tag_attrs.to_ascii_lowercase().find("href=")?;
    let value = tag_attrs[href_pos + 5..].trim_start();

    if let Some(quoted) = value.strip_prefix('"') {
        quoted.find('"').map(|end| &quoted[..end])
    } else if let Some(quoted) = value.strip_prefix('\'') {
        quoted.find('\'').map(|end| &quoted[..end])
    } else {
        let end = value
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(value.len());
        Some(&value[..end])
    }
}

/// The longest entity body read (`#x10FFFF`); a `&` with no `;` this close
/// is an ampersand in prose.
const MAX_ENTITY_LEN: usize = 8;

/// Decode the entities agents' text carries: the five XML ones, `&nbsp;`,
/// and numeric references. Anything else after a `&` is kept as written.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let decoded = after
            .find(';')
            .filter(|&end| end <= MAX_ENTITY_LEN)
            .and_then(|end| Some((entity_char(&after[..end])?, end)));
        match decoded {
            Some((ch, end)) => {
                out.push(ch);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The character an entity body names (`amp`, `#39`, `#x2014`).
fn entity_char(body: &str) -> Option<char> {
    match body {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            let number = body.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code).filter(|c| !c.is_control())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_kept_as_it_is() {
        assert_eq!(
            strip_html_tags("Sixth album, released in 2009. \"Cellz\" opens it."),
            "Sixth album, released in 2009. \"Cellz\" opens it."
        );
    }

    #[test]
    fn a_link_becomes_its_text_and_address() {
        assert_eq!(
            strip_html_tags(
                r#"A record. <a href="https://www.last.fm/music/A/B">Read more on Last.fm</a>"#
            ),
            "A record. Read more on Last.fm (https://www.last.fm/music/A/B)"
        );
        assert_eq!(
            strip_html_tags(r#"See <a href='https://example.org'>https://example.org</a>."#),
            "See https://example.org."
        );
    }

    #[test]
    fn other_tags_are_dropped_and_their_text_kept() {
        assert_eq!(
            strip_html_tags("A <b>bold</b> and <i class=\"x\">slanted</i> word."),
            "A bold and slanted word."
        );
        assert_eq!(
            strip_html_tags(r#"<a href="https://example.org"><b>Bold</b> link</a>"#),
            "Bold link (https://example.org)"
        );
    }

    /// A paragraph or line break that is dropped must not run two sentences
    /// together.
    #[test]
    fn a_dropped_block_tag_leaves_one_space_between_sentences() {
        assert_eq!(
            strip_html_tags("<p>One <b>bold</b> word.</p><br/>Next line."),
            "One bold word. Next line."
        );
        assert_eq!(
            strip_html_tags("<p>First.</p><p>Second.</p>"),
            "First. Second."
        );
        assert_eq!(
            strip_html_tags("Line one.<br>Line two. </p> Line three."),
            "Line one. Line two.  Line three."
        );
    }

    /// A `<` in prose is not a tag: "<3", a comparison, a stray bracket.
    #[test]
    fn a_less_than_sign_in_prose_is_kept() {
        assert_eq!(
            strip_html_tags("Rated <3 by fans. The rest."),
            "Rated <3 by fans. The rest."
        );
        assert_eq!(strip_html_tags("a < b and c > d"), "a < b and c > d");
        assert_eq!(strip_html_tags("ends with <"), "ends with <");
        assert_eq!(strip_html_tags("an unclosed <b tag"), "an unclosed <b tag");
    }

    /// Some capitals change byte length when lowercased (İ, the Kelvin
    /// sign): an offset found in a lowercased copy of the tag once sliced
    /// the original inside a character and panicked.
    #[test]
    fn a_tag_whose_letters_change_length_in_lowercase_is_read_whole() {
        assert_eq!(strip_html_tags("<a title=\"İ\" href=ž>x</a>"), "x (ž)");
        assert_eq!(
            strip_html_tags("<a title=\"İK\" HREF=\"https://e.org\">site</a>"),
            "site (https://e.org)"
        );
    }

    #[test]
    fn common_entities_are_decoded() {
        assert_eq!(
            strip_html_tags("Simon &amp; Garfunkel&#39;s &quot;Bookends&quot;"),
            "Simon & Garfunkel's \"Bookends\""
        );
        assert_eq!(strip_html_tags("5 &lt; 6 &gt; 4"), "5 < 6 > 4");
        assert_eq!(strip_html_tags("&#x2014; and &#8212;"), "— and —");
        assert_eq!(
            strip_html_tags(r#"<a href="https://e.org/?a=1&amp;b=2">E</a>"#),
            "E (https://e.org/?a=1&b=2)"
        );
        // Not entities: kept as written.
        assert_eq!(
            strip_html_tags("AT&T, R&B; and &bogus;"),
            "AT&T, R&B; and &bogus;"
        );
    }

    /// An escaped tag is text, not markup: it is shown, never stripped.
    #[test]
    fn an_escaped_tag_is_shown_as_text() {
        assert_eq!(
            strip_html_tags("use &lt;b&gt; for bold"),
            "use <b> for bold"
        );
    }
}
