//! Short texts the session's modules put in menus and toasts.

/// `beat 2.3`: a cursor position.
pub fn beat_label(beat: i32, step: i32) -> String {
    format!("beat {}.{}", beat + 1, step + 1)
}

/// `3 notes`.
pub fn notes_label(count: usize) -> String {
    format!("{count} note{}", if count == 1 { "" } else { "s" })
}

/// A style's name in the menus: a learned one says from how many patterns.
pub fn style_label(style: &ordrumbox_core::Style) -> String {
    match style.learned {
        Some((patterns, measures)) => format!("{} (learned: {patterns} patterns, {measures} measures)", style.name),
        None => style.name.clone(),
    }
}
