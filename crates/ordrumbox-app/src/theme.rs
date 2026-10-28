//! The app's colours: a deep indigo ground, panels a step lighter, and five
//! neon accents with one role each — pink for titles, the selection and the
//! slider handles; cyan for labels, sliders and knobs; lime for play, what
//! is on and the playhead; yellow for values and generation; violet and
//! orange, with the others, for the tracks (each track has its colour,
//! which its notes take). Flat colours only: no glow, no gradient.

use crate::canvas::Rgb;

// ─── Ground ──────────────────────────────────────────────────────────────────

/// The window.
pub const BG: Rgb = 0x0014_163A;
/// Panels (the track editor, the menu bar, cards), and a step lighter.
pub const PANEL: Rgb = 0x001B_1E4B;
pub const PANEL_2: Rgb = 0x0022_265C;
/// An empty step, and the first step of a beat.
pub const CELL: Rgb = 0x0026_2B66;
pub const CELL_BEAT: Rgb = 0x0031_3878;
/// Lines, borders, a control's track.
pub const LINE: Rgb = 0x003A_3F86;
/// Behind plots (waveforms, spectrum, scope).
pub const SCREEN: Rgb = 0x000B_0C24;
/// The frame around a panel over the window.
pub const SHADE: Rgb = 0x000B_0C24;
/// The row of the selection (the track, the list item).
pub const SELECTED_ROW: Rgb = 0x003A_1F55;
/// A highlighted menu item or list row.
pub const HIGHLIGHT: Rgb = 0x0033_3A8A;

// ─── Text ────────────────────────────────────────────────────────────────────

pub const TEXT: Rgb = 0x00E8_E9FF;
pub const MUTED: Rgb = 0x008A_8FC7;
pub const DISABLED: Rgb = 0x0055_5A99;
pub const WHITE: Rgb = 0x00FF_FFFF;

// ─── Accents ─────────────────────────────────────────────────────────────────

pub const PINK: Rgb = 0x00FF_4FA3;
pub const CYAN: Rgb = 0x0038_D9F5;
pub const LIME: Rgb = 0x00B8_F23A;
pub const YELLOW: Rgb = 0x00FF_D23F;
pub const VIOLET: Rgb = 0x009B_7BFF;
pub const ORANGE: Rgb = 0x00FF_8A3D;
/// Problems, mute.
pub const RED: Rgb = 0x00FF_5C6C;

/// The colour of track `index` (its stripe, its notes): pink, cyan,
/// yellow, violet, orange, lime, round again.
pub fn track(index: usize) -> Rgb {
    const TRACKS: [Rgb; 6] = [PINK, CYAN, YELLOW, VIOLET, ORANGE, LIME];
    TRACKS[index % TRACKS.len()]
}

/// `from` blended towards `to` by `amount` (0–1), per channel.
pub fn mix(from: Rgb, to: Rgb, amount: f64) -> Rgb {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |shift: u32| {
        let a = f64::from((from >> shift) & 0xFF);
        let b = f64::from((to >> shift) & 0xFF);
        ((a + (b - a) * amount).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// A note of colour `color` at `velocity` (0–1): from a quarter of the
/// way to full, over an empty step.
pub fn note(color: Rgb, velocity: f64) -> Rgb {
    mix(CELL, color, 0.35 + 0.65 * velocity.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_colours_cycle_and_notes_follow_velocity() {
        assert_eq!(track(0), PINK);
        assert_eq!(track(6), PINK);
        assert_ne!(track(1), track(2));
        assert_eq!(note(PINK, 1.0), PINK);
        assert_eq!(mix(0x000000, 0xFF8040, 0.5), 0x804020);
        assert_ne!(note(PINK, 0.0), CELL);
    }
}
