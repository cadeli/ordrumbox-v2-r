//! Short texts the session's modules put in menus and toasts.

/// `beat 2.3`: a cursor position.
pub fn beat_label(beat: i32, step: i32) -> String {
    format!("beat {}.{}", beat + 1, step + 1)
}

/// `3 notes`.
pub fn notes_label(count: usize) -> String {
    format!("{count} note{}", if count == 1 { "" } else { "s" })
}

/// `copied 3 notes — tracks 1-2, beats 1-2`: what a copy took, and where from.
pub fn copied_message(count: usize, zone: &str) -> String {
    format!("copied {} — {zone}", notes_label(count))
}

/// `Deleted 3 notes — tracks 1-2, beats 1-2`: what a delete cleared, and where.
pub fn deleted_message(count: usize, zone: &str) -> String {
    format!("Deleted {} — {zone}", notes_label(count))
}

/// `pasted 3 notes`, or what was left out and why (`left_out`: `past the
/// pattern`).
pub fn pasted_message(placed: usize, dropped: usize, left_out: &str) -> String {
    match dropped {
        0 => format!("pasted {}", notes_label(placed)),
        dropped => format!("pasted {}, {dropped} {left_out} left out", notes_label(placed)),
    }
}

/// A style's name in the menus: a learned one says from how many patterns.
pub fn style_label(style: &ordrumbox_core::Style) -> String {
    match style.learned {
        Some((patterns, measures)) => format!("{} (learned: {patterns} patterns, {measures} measures)", style.name),
        None => style.name.clone(),
    }
}
