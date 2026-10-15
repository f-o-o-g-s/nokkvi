//! Cover placeholders (Navidrome 0.64+): the ThumbHash a row carries is
//! decoded when its 80px cover is sent for, so the row, and the big panel
//! over it, draw a blurred stand-in until the cover lands.

use nokkvi_data::{backend::albums::AlbumUIViewData, types::album::Album};

use crate::test_helpers::{make_artist, test_app};

/// Two real covers' ThumbHashes (The Division Bell, Animals).
const DIVISION_BELL: &str = "3hcKHwRmJ6d+eYmAh7h4iIiIhQJ4SYAK";
const ANIMALS: &str = "VfgNHYaIiXeGdodweEeZe6iAlgqI";

fn album(id: &str, thumb_hash: Option<&str>) -> AlbumUIViewData {
    let mut v = serde_json::json!({ "id": id, "name": id, "updatedAt": "T1" });
    if let (Some(obj), Some(hash)) = (v.as_object_mut(), thumb_hash) {
        obj.insert("thumbHash".into(), hash.into());
    }
    let album: Album = serde_json::from_value(v).expect("album json");
    AlbumUIViewData::from_album(&album, "http://srv", "u=x&s=y&t=z")
}

fn placeholder_hash(app: &crate::Nokkvi, id: &str) -> Option<String> {
    app.artwork
        .placeholders
        .peek(&id.to_string())
        .map(|p| p.thumb_hash.clone())
}

#[test]
fn claiming_a_mini_decodes_its_placeholder() {
    let mut app = test_app();
    app.library
        .albums
        .set_from_vec(vec![album("a1", Some(DIVISION_BELL))]);

    app.claim_minis(["a1".to_string()]);

    assert!(
        app.artwork.album_art_pending.contains("a1"),
        "still claimed"
    );
    assert_eq!(placeholder_hash(&app, "a1").as_deref(), Some(DIVISION_BELL));
}

#[test]
fn a_cover_without_a_thumb_hash_gets_no_placeholder() {
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![album("a1", None)]);

    app.claim_minis(["a1".to_string()]);

    assert!(app.artwork.album_art_pending.contains("a1"));
    assert_eq!(placeholder_hash(&app, "a1"), None);
}

#[test]
fn an_unchanged_thumb_hash_keeps_its_picture() {
    let mut app = test_app();
    app.library
        .albums
        .set_from_vec(vec![album("a1", Some(DIVISION_BELL))]);
    app.claim_minis(["a1".to_string()]);
    let first = app
        .artwork
        .placeholders
        .peek(&"a1".to_string())
        .map(|p| p.handle.id());

    app.artwork.album_art_pending.clear();
    app.claim_minis(["a1".to_string()]);

    let second = app
        .artwork
        .placeholders
        .peek(&"a1".to_string())
        .map(|p| p.handle.id());
    assert!(first.is_some());
    assert_eq!(
        first, second,
        "no second decode, so the picture stays on the GPU"
    );
}

#[test]
fn a_changed_thumb_hash_is_decoded_again() {
    let mut app = test_app();
    app.library
        .albums
        .set_from_vec(vec![album("a1", Some(DIVISION_BELL))]);
    app.claim_minis(["a1".to_string()]);

    // The cover was replaced on the server: the reload carries a new hash.
    app.library
        .albums
        .set_from_vec(vec![album("a1", Some(ANIMALS))]);
    app.artwork.album_art_pending.clear();
    app.claim_minis(["a1".to_string()]);

    assert_eq!(placeholder_hash(&app, "a1").as_deref(), Some(ANIMALS));
}

#[test]
fn artists_and_expanded_albums_get_placeholders() {
    let mut app = test_app();
    let mut artist = make_artist("ar1", "Pink Floyd");
    artist.image.thumb_hash = Some(ANIMALS.to_string());
    app.library.artists.set_from_vec(vec![artist]);
    // An album only the expanded artist lists, in no album buffer.
    app.artists_page.expansion.expanded_id = Some("ar1".to_string());
    app.artists_page.expansion.children = vec![album("a9", Some(DIVISION_BELL))];

    app.claim_minis(["ar1".to_string(), "a9".to_string()]);

    assert_eq!(placeholder_hash(&app, "ar1").as_deref(), Some(ANIMALS));
    assert_eq!(placeholder_hash(&app, "a9").as_deref(), Some(DIVISION_BELL));
}

/// Absent art keeps nokkvi's plain square, even when a server sends a
/// ThumbHash beside the flag.
#[test]
fn absent_art_gets_no_placeholder() {
    let mut app = test_app();
    let mut absent = album("a1", Some(DIVISION_BELL));
    absent.image.image_absent = true;
    app.library.albums.set_from_vec(vec![absent]);

    app.claim_minis(["a1".to_string()]);

    assert_eq!(placeholder_hash(&app, "a1"), None);
}

/// The source with code only: `//` comments dropped, then every whitespace
/// character, so a claim rustfmt splits over lines reads as one.
fn squashed_code(text: &str) -> String {
    text.lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .flat_map(|code| code.chars().filter(|c| !c.is_whitespace()))
        .collect()
}

/// Every 80px fetch claims through `claim_minis`, the one place that decodes
/// the placeholder; a path that claims `album_art_pending` by hand would draw
/// its rows' plain square while they load. Catches a call split over lines
/// and a `&mut` borrow of the set handed elsewhere.
#[test]
fn every_mini_claim_goes_through_claim_minis() {
    const SET: &str = concat!("album_art_", "pending");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = root.join("src");
    // `claim_minis` lives here, and claims once.
    let home = src.join("update").join("albums.rs");
    let allowed = format!("self.artwork.{SET}.extend(ids)");
    // Handler tests may stand in for an earlier pass's claim.
    let tests = src.join("update").join("tests");
    let mut stray = Vec::new();
    let mut dirs = vec![src];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path == tests {
                continue;
            }
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let code = squashed_code(&std::fs::read_to_string(&path).unwrap());
            let mut claims = code.matches(&format!("{SET}.extend(")).count()
                + code.matches(&format!("{SET}.insert(")).count();
            if path == home {
                assert_eq!(code.matches(&allowed).count(), 1, "claim_minis claims once");
                claims -= 1;
            }
            let borrows = code
                .match_indices("&mut")
                .filter(|(at, _)| {
                    code[at + 4..]
                        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                        .next()
                        .is_some_and(|place| place.ends_with(SET))
                })
                .count();
            if claims + borrows > 0 {
                stray.push(format!(
                    "{}: {claims} claim(s), {borrows} &mut borrow(s)",
                    path.display()
                ));
            }
        }
    }
    stray.sort();
    assert!(
        stray.is_empty(),
        "claim 80px fetches through Nokkvi::claim_minis:\n{}",
        stray.join("\n")
    );
}
