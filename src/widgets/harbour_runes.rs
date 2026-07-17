//! Long-branch Younger Futhark stroke skeletons for the harbour scene's
//! moon-dream verses. The scene draws these as canvas geometry — the same
//! stroke vocabulary as its stars, notes, and fish — so no runic font ever
//! ships (the app's UI face has no Runic coverage and a system-font
//! fallback would tofu on most machines).
//!
//! Geometry contract: every glyph is a list of straight segments
//! `[x1, y1, x2, y2]` in a stave-anchored box — y runs 0.0 (top) to 1.0
//! (bottom) in stave-heights, the stave (where one exists — ᛋ sól is a
//! stave-less bolt) sits at x = 0, and branches extend right. Four runes
//! are genuinely two-sided (ᚾ ᛅ ᛏ ᛘ) and reach negative x, which is why
//! layout goes through [`left_bearing`] instead of assuming ink starts at
//! the pen. `width` is the pen advance, side bearings included.
//!
//! Forms follow the long-branch (Danish) row as drawn by the standard
//! scholarly convention, deliberately angularized where reference fonts
//! curve (ᚢ ᚦ ᚴ ᛘ ᚠ) — carved staves, not typography. Two forms carry a
//! declared legibility stylization: the crossing diagonals of ᚾ/ᛅ are
//! steepened from the sources' ~21–25° to ~29° so they read at drift
//! size instead of smudging near-horizontal.

/// One rune's stroke skeleton: straight `[x1, y1, x2, y2]` segments in
/// the stave box, plus the pen advance in stave-height units.
pub(crate) struct RuneGlyph {
    pub(crate) segments: &'static [[f32; 4]],
    pub(crate) width: f32,
}

/// ᚠ (U+16A0)
const FE: RuneGlyph = RuneGlyph {
    segments: &[
        [0.0, 0.0, 0.0, 1.0],
        [0.0, 0.42, 0.38, 0.03],
        [0.0, 0.67, 0.38, 0.28],
    ],
    width: 0.56,
};
/// ᚢ (U+16A2)
const UR: RuneGlyph = RuneGlyph {
    segments: &[
        [0.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.42, 0.45],
        [0.42, 0.45, 0.42, 1.0],
    ],
    width: 0.6,
};
/// ᚦ (U+16A6)
const THURS: RuneGlyph = RuneGlyph {
    segments: &[
        [0.0, 0.0, 0.0, 1.0],
        [0.0, 0.25, 0.35, 0.5],
        [0.35, 0.5, 0.0, 0.75],
    ],
    width: 0.53,
};
/// ᚱ (U+16B1) — the bowl is OPEN: its return stroke stops at x 0.18 and
/// never touches the stave (all sources agree; closing it is the classic
/// implementer's mistake).
const REID: RuneGlyph = RuneGlyph {
    segments: &[
        [0.0, 0.0, 0.0, 1.0],
        [0.0, 0.03, 0.3, 0.27],
        [0.3, 0.27, 0.18, 0.55],
        [0.18, 0.55, 0.33, 0.98],
    ],
    width: 0.51,
};
/// ᚴ (U+16B4)
const KAUN: RuneGlyph = RuneGlyph {
    segments: &[[0.0, 0.0, 0.0, 1.0], [0.0, 0.46, 0.47, 0.0]],
    width: 0.65,
};
/// ᚾ (U+16BE) — one continuous diagonal CROSSING the stave, upper-left
/// to lower-right. Direction is load-bearing: reversed, it silently
/// becomes ᛅ (test-pinned mirror pair).
const NAUD: RuneGlyph = RuneGlyph {
    segments: &[[0.0, 0.0, 0.0, 1.0], [-0.22, 0.38, 0.22, 0.62]],
    width: 0.62,
};
/// ᛁ (U+16C1)
const ISS: RuneGlyph = RuneGlyph {
    segments: &[[0.0, 0.0, 0.0, 1.0]],
    width: 0.26,
};
/// ᛅ (U+16C5) — the exact x-mirror of ᚾ: upper-right to lower-left.
const AR: RuneGlyph = RuneGlyph {
    segments: &[[0.0, 0.0, 0.0, 1.0], [0.22, 0.38, -0.22, 0.62]],
    width: 0.62,
};
/// ᛋ (U+16CB) — a stave-less three-segment bolt: this glyph is why the
/// renderer takes plain polylines instead of a stave-plus-branches model.
const SOL: RuneGlyph = RuneGlyph {
    segments: &[
        [0.0, 0.0, 0.0, 0.62],
        [0.0, 0.62, 0.4, 0.38],
        [0.4, 0.38, 0.4, 1.0],
    ],
    width: 0.58,
};
/// ᛏ (U+16CF)
const TYR: RuneGlyph = RuneGlyph {
    segments: &[
        [0.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, -0.28, 0.22],
        [0.0, 0.0, 0.28, 0.22],
    ],
    width: 0.74,
};
/// ᛒ (U+16D2) — two stacked wedges that DO close onto the stave (unlike
/// ᚱ's open bowl); the mid-stave closure at 0.5 separates the pockets.
const BJARKAN: RuneGlyph = RuneGlyph {
    segments: &[
        [0.0, 0.0, 0.0, 1.0],
        [0.0, 0.03, 0.33, 0.25],
        [0.33, 0.25, 0.0, 0.5],
        [0.0, 0.5, 0.33, 0.75],
        [0.33, 0.75, 0.0, 0.97],
    ],
    width: 0.51,
};
/// ᛚ (U+16DA)
const LOGR: RuneGlyph = RuneGlyph {
    segments: &[[0.0, 0.0, 0.0, 1.0], [0.0, 0.03, 0.33, 0.3]],
    width: 0.51,
};
/// ᛘ (U+16D8) — the trident: arms RISE from the stave at y 0.33 to tips
/// at the top. Flipped vertically this is ᛦ, a different real rune that
/// does not look broken — the arms-rise test is the guard.
const MADR: RuneGlyph = RuneGlyph {
    segments: &[
        [0.0, 0.0, 0.0, 1.0],
        [0.0, 0.33, -0.3, 0.0],
        [0.0, 0.33, 0.3, 0.0],
    ],
    width: 0.78,
};

/// Look up a rune's skeleton. `None` for anything outside the thirteen
/// staves the verses carve (spaces are the caller's word gaps).
pub(crate) fn rune_glyph(c: char) -> Option<&'static RuneGlyph> {
    match c {
        'ᚠ' => Some(&FE),
        'ᚢ' => Some(&UR),
        'ᚦ' => Some(&THURS),
        'ᚱ' => Some(&REID),
        'ᚴ' => Some(&KAUN),
        'ᚾ' => Some(&NAUD),
        'ᛁ' => Some(&ISS),
        'ᛅ' => Some(&AR),
        'ᛋ' => Some(&SOL),
        'ᛏ' => Some(&TYR),
        'ᛒ' => Some(&BJARKAN),
        'ᛚ' => Some(&LOGR),
        'ᛘ' => Some(&MADR),
        _ => None,
    }
}

/// Blank advance between words, in stave-heights.
pub(crate) const RUNE_WORD_SPACE: f32 = 0.40;

/// Side bearing folded into every glyph's advance (0.09 per side).
const RUNE_SIDE_BEARING: f32 = 0.09;

/// A glyph's pen-to-stave offset: the side bearing, widened by however
/// far the glyph's ink reaches LEFT of its stave — this is what keeps
/// the two-sided runes (ᚾ ᛅ ᛏ ᛘ) from overhanging their neighbours.
pub(crate) fn left_bearing(glyph: &RuneGlyph) -> f32 {
    let min_x = glyph
        .segments
        .iter()
        .map(|s| s[0].min(s[2]))
        .fold(0.0f32, f32::min);
    RUNE_SIDE_BEARING - min_x
}

/// Advance width of a whole verse line, in stave-heights. Unknown
/// characters advance nothing (and the verse test forbids them).
pub(crate) fn verse_advance(line: &str) -> f32 {
    line.chars()
        .map(|c| {
            if c == ' ' {
                RUNE_WORD_SPACE
            } else {
                rune_glyph(c).map_or(0.0, |g| g.width)
            }
        })
        .sum()
}

/// The four verses of the moon's dream, in the old tongue. The carver
/// left no translation — what they say is between the moon and whoever
/// cares to read staves.
pub(crate) const DREAM_VERSES: [&str; 4] = [
    "ᛏᚢᛅᛋ ᛒᚱᛁᛚᛁᚴ ᛅᛏ ᚦᛁ ᛋᛚᛁᚦᛁ ᛏᚢᚠᛋ",
    "ᛏᛁᛏ ᚴᛅᛁᚱ ᛅᛏ ᚴᛁᛒᛚ ᛁᚾ ᚦᛁ ᚢᛅᛒ",
    "ᛅᛚ ᛘᛁᛘᛋᛁ ᚢᛁᚱ ᚦᛁ ᛒᚢᚱᚢᚴᚢᚠᛋ",
    "ᛅᛏ ᚦᛁ ᛘᚢᛘ ᚱᛅᚦᛋ ᛅᚢᛏᚴᚱᛅᛒ",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything the thirteen skeletons cover, for sweep tests.
    const ALL: [(char, &RuneGlyph); 13] = [
        ('ᚠ', &FE),
        ('ᚢ', &UR),
        ('ᚦ', &THURS),
        ('ᚱ', &REID),
        ('ᚴ', &KAUN),
        ('ᚾ', &NAUD),
        ('ᛁ', &ISS),
        ('ᛅ', &AR),
        ('ᛋ', &SOL),
        ('ᛏ', &TYR),
        ('ᛒ', &BJARKAN),
        ('ᛚ', &LOGR),
        ('ᛘ', &MADR),
    ];

    /// Every character the verses carve must have a skeleton — a rune
    /// with no glyph would silently render as a word-gap-less nothing.
    #[test]
    fn every_verse_character_has_a_glyph() {
        for verse in DREAM_VERSES {
            for c in verse.chars() {
                assert!(
                    c == ' ' || rune_glyph(c).is_some(),
                    "verse character {c:?} (U+{:04X}) has no skeleton",
                    c as u32
                );
            }
        }
    }

    /// The verses are pinned char-for-char. The set-equality test below
    /// can't catch a transposition, a mid-word swap, or a reordering —
    /// and staves have no human proofreader, so a corrupting edit would
    /// otherwise ship invisibly. A deliberate change must touch both
    /// sites and show loudly in the diff.
    #[test]
    fn verses_are_pinned_char_for_char() {
        const PINNED: [&str; 4] = [
            "ᛏᚢᛅᛋ ᛒᚱᛁᛚᛁᚴ ᛅᛏ ᚦᛁ ᛋᛚᛁᚦᛁ ᛏᚢᚠᛋ",
            "ᛏᛁᛏ ᚴᛅᛁᚱ ᛅᛏ ᚴᛁᛒᛚ ᛁᚾ ᚦᛁ ᚢᛅᛒ",
            "ᛅᛚ ᛘᛁᛘᛋᛁ ᚢᛁᚱ ᚦᛁ ᛒᚢᚱᚢᚴᚢᚠᛋ",
            "ᛅᛏ ᚦᛁ ᛘᚢᛘ ᚱᛅᚦᛋ ᛅᚢᛏᚴᚱᛅᛒ",
        ];
        assert_eq!(DREAM_VERSES, PINNED);
    }

    /// The verses use exactly the thirteen runes the module ships — no
    /// dead glyph, no missing one.
    #[test]
    fn verses_use_exactly_the_shipped_runes() {
        let used: std::collections::BTreeSet<char> = DREAM_VERSES
            .iter()
            .flat_map(|v| v.chars())
            .filter(|c| *c != ' ')
            .collect();
        let shipped: std::collections::BTreeSet<char> = ALL.iter().map(|(c, _)| *c).collect();
        assert_eq!(used, shipped);
    }

    /// Ink stays inside the stave box: y within [0, 1], x within the
    /// two-sided reach. A stray coordinate here draws outside the line's
    /// layout envelope and collides with neighbours.
    #[test]
    fn glyph_ink_stays_inside_the_stave_box() {
        for (c, g) in ALL {
            for s in g.segments {
                for x in [s[0], s[2]] {
                    assert!((-0.5..=0.9).contains(&x), "{c}: x {x} out of range");
                }
                for y in [s[1], s[3]] {
                    assert!((0.0..=1.0).contains(&y), "{c}: y {y} out of range");
                }
            }
        }
    }

    /// Every advance covers its ink plus the side bearings — a heavy
    /// stroke at a too-narrow advance closes up between glyphs.
    #[test]
    fn glyph_advances_cover_their_ink() {
        for (c, g) in ALL {
            let min_x = g
                .segments
                .iter()
                .map(|s| s[0].min(s[2]))
                .fold(0.0f32, f32::min);
            let max_x = g
                .segments
                .iter()
                .map(|s| s[0].max(s[2]))
                .fold(0.0f32, f32::max);
            assert!(
                g.width >= (max_x - min_x) + 2.0 * RUNE_SIDE_BEARING - 0.011,
                "{c}: advance {} too narrow for ink {}..{}",
                g.width,
                min_x,
                max_x
            );
        }
    }

    /// ᚾ and ᛅ are exact x-mirrors and NOT equal — the highest-risk
    /// confusion pair in the row. A flipped diagonal doesn't look broken;
    /// it silently spells a different rune.
    #[test]
    fn naud_and_ar_are_mirrors_not_twins() {
        let mirror = |s: &[f32; 4]| [-s[0], s[1], -s[2], s[3]];
        assert_eq!(NAUD.segments.len(), AR.segments.len());
        for (n, a) in NAUD.segments.iter().zip(AR.segments) {
            assert_eq!(mirror(n), *a, "ᛅ must be ᚾ mirrored in x");
        }
        assert_ne!(
            NAUD.segments[1], AR.segments[1],
            "the diagonals must differ"
        );
    }

    /// ᛘ's arms RISE (tips above the fork). Flipped vertically the same
    /// strokes are ᛦ — a different real rune, invisible as a bug.
    #[test]
    fn madr_arms_rise_to_the_top() {
        for arm in &MADR.segments[1..] {
            assert!(
                arm[3] < arm[1],
                "ᛘ arm must rise: tip y {} vs fork y {}",
                arm[3],
                arm[1]
            );
            assert!((arm[3] - 0.0).abs() < 1e-6, "ᛘ arm tips reach the top");
        }
    }

    /// ᚱ's bowl stays open: no non-stave segment endpoint returns to the
    /// stave line between the bowl's top and the leg's departure.
    #[test]
    fn reid_bowl_never_recloses_on_the_stave() {
        for s in &REID.segments[1..] {
            for (x, y) in [(s[0], s[1]), (s[2], s[3])] {
                assert!(
                    x > 0.05 || y < 0.05,
                    "ᚱ branch endpoint ({x}, {y}) touches the stave mid-run"
                );
            }
        }
    }

    /// Layout sanity: the left bearing exactly cancels the deepest
    /// negative reach, so translated ink never starts left of the pen.
    #[test]
    fn left_bearing_covers_negative_reach() {
        for (c, g) in ALL {
            let lb = left_bearing(g);
            for s in g.segments {
                assert!(lb + s[0].min(s[2]) >= RUNE_SIDE_BEARING - 1e-6, "{c}");
            }
        }
    }
}
