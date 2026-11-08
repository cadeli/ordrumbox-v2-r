//! The app's colours: a ground (the window, panels a step lighter, lines,
//! text) taken from the colour scheme in use, and six neon accents shared
//! by every scheme. Each accent has one meaning, the same in every view: the
//! roles below ([`ACCENT`], [`FOCUS`], [`LIVE`], [`VALUE`], [`CHANCE`],
//! [`GENERATED`], [`ALERT`]) are the only way to them, besides the tracks'
//! own colours (a track's stripe and its notes in the piano roll). Labels
//! are muted, not coloured. Flat colours only: no glow, no gradient.

use std::cell::Cell;

use crate::canvas::Rgb;

// ─── Schemes ─────────────────────────────────────────────────────────────────

/// The ground and text colours of a colour scheme; the accents are shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scheme {
    /// Its name, as the UI shows it.
    pub name: &'static str,
    /// The window.
    pub bg: Rgb,
    /// Panels (the track editor, the menu bar, cards), and a step lighter.
    pub panel: Rgb,
    pub panel_2: Rgb,
    /// An empty step, and the first step of a beat.
    pub cell: Rgb,
    pub cell_beat: Rgb,
    /// Lines, borders, a control's track.
    pub line: Rgb,
    /// Behind plots (waveforms, spectrum, scope).
    pub screen: Rgb,
    /// The frame around a panel over the window.
    pub shade: Rgb,
    /// The row of the selection (the track, the list item).
    pub selected_row: Rgb,
    /// A highlighted menu item or list row.
    pub highlight: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub disabled: Rgb,
}

/// The colour schemes, numbered from 1 as in `settings.json`
/// (`colorScheme`): indigo, amber, deep sea (the web version's three), then
/// crimson and toxic, saturated grounds of this version only.
pub const SCHEMES: [Scheme; 5] = [
    Scheme {
        name: "Indigo",
        bg: 0x0014_163A,
        panel: 0x001B_1E4B,
        panel_2: 0x0022_265C,
        cell: 0x0026_2B66,
        cell_beat: 0x0031_3878,
        line: 0x003A_3F86,
        screen: 0x000B_0C24,
        shade: 0x000B_0C24,
        selected_row: 0x003A_1F55,
        highlight: 0x0033_3A8A,
        text: 0x00E8_E9FF,
        muted: 0x008A_8FC7,
        disabled: 0x0055_5A99,
    },
    // Amber on warm brown, a retro terminal.
    Scheme {
        name: "Amber",
        bg: 0x001C_1206,
        panel: 0x0026_190A,
        panel_2: 0x0030_200C,
        cell: 0x0038_260E,
        cell_beat: 0x0047_3112,
        line: 0x0066_441A,
        screen: 0x0014_0D03,
        shade: 0x0012_0B03,
        selected_row: 0x004A_2414,
        highlight: 0x005C_3E14,
        text: 0x00FF_EDC8,
        muted: 0x00C0_9660,
        disabled: 0x0070_5434,
    },
    // Electric blue on deep navy.
    Scheme {
        name: "Deep sea",
        bg: 0x0006_0F24,
        panel: 0x000B_1836,
        panel_2: 0x0010_2045,
        cell: 0x0014_2852,
        cell_beat: 0x001A_3366,
        line: 0x0026_4A94,
        screen: 0x0003_0A18,
        shade: 0x0003_070F,
        selected_row: 0x0028_1E58,
        highlight: 0x0020_3C84,
        text: 0x00DB_E9FF,
        muted: 0x0080_A0D8,
        disabled: 0x0040_5A8E,
    },
    // Blood red: a saturated crimson ground, rose text.
    Scheme {
        name: "Crimson",
        bg: 0x0024_0008,
        panel: 0x0036_040F,
        panel_2: 0x0046_0816,
        cell: 0x0052_0C1C,
        cell_beat: 0x0068_1226,
        line: 0x008C_1E36,
        screen: 0x0014_0004,
        shade: 0x000E_0003,
        selected_row: 0x0060_1048,
        highlight: 0x0078_1A30,
        text: 0x00FF_E0E6,
        muted: 0x00E0_8A9C,
        disabled: 0x0080_3A4C,
    },
    // Acid green on black, a radioactive terminal.
    Scheme {
        name: "Toxic",
        bg: 0x0002_0F02,
        panel: 0x0006_1C06,
        panel_2: 0x000A_280A,
        cell: 0x000E_330E,
        cell_beat: 0x0014_4614,
        line: 0x0020_6A1C,
        screen: 0x0000_0800,
        shade: 0x0000_0500,
        selected_row: 0x0010_3C3C,
        highlight: 0x001A_5218,
        text: 0x00D8_FFC8,
        muted: 0x007C_C46A,
        disabled: 0x0036_6A2E,
    },
];

thread_local! {
    /// The index in [`SCHEMES`] of the scheme in use. The UI draws on one
    /// thread; each test thread has its own.
    static CURRENT: Cell<usize> = const { Cell::new(0) };
}

/// The scheme in use.
pub fn scheme() -> &'static Scheme {
    &SCHEMES[CURRENT.with(Cell::get)]
}

/// The number (from 1) of the scheme in use.
pub fn scheme_number() -> usize {
    CURRENT.with(Cell::get) + 1
}

/// Use scheme `number` (from 1); false, and nothing changed, when there is
/// no such scheme.
pub fn set_scheme(number: usize) -> bool {
    let valid = (1..=SCHEMES.len()).contains(&number);
    if valid {
        CURRENT.with(|current| current.set(number - 1));
    }
    valid
}

/// The number of the scheme after `number`, round again.
pub fn next_scheme(number: usize) -> usize {
    number % SCHEMES.len() + 1
}

// ─── Ground and text, from the scheme in use ─────────────────────────────────

pub fn bg() -> Rgb {
    scheme().bg
}
pub fn panel() -> Rgb {
    scheme().panel
}
pub fn panel_2() -> Rgb {
    scheme().panel_2
}
pub fn cell() -> Rgb {
    scheme().cell
}
pub fn cell_beat() -> Rgb {
    scheme().cell_beat
}
pub fn line() -> Rgb {
    scheme().line
}
pub fn screen() -> Rgb {
    scheme().screen
}
pub fn shade() -> Rgb {
    scheme().shade
}
pub fn selected_row() -> Rgb {
    scheme().selected_row
}
pub fn highlight() -> Rgb {
    scheme().highlight
}
pub fn text() -> Rgb {
    scheme().text
}
pub fn muted() -> Rgb {
    scheme().muted
}
pub fn disabled() -> Rgb {
    scheme().disabled
}

// ─── Shared ──────────────────────────────────────────────────────────────────

pub const WHITE: Rgb = 0x00FF_FFFF;

// ─── Accents ─────────────────────────────────────────────────────────────────

const PINK: Rgb = 0x00FF_4FA3;
const CYAN: Rgb = 0x0038_D9F5;
const LIME: Rgb = 0x00B8_F23A;
const YELLOW: Rgb = 0x00FF_D23F;
const VIOLET: Rgb = 0x009B_7BFF;
const ORANGE: Rgb = 0x00FF_8A3D;
const RED: Rgb = 0x00FF_5C6C;

// ─── Roles ───────────────────────────────────────────────────────────────────

/// The app itself: the logo, titles, the header's edge, the selected item
/// of a list, slider handles, where a dragged item lands.
pub const ACCENT: Rgb = PINK;
/// Keyboard focus: the grid cursor, the focused control, a text caret.
pub const FOCUS: Rgb = CYAN;
/// What sounds: the transport playing, a switch that is on, the playhead,
/// the beat heard, the pattern next in line, the scope, a plain note.
pub const LIVE: Rgb = LIME;
/// What the user set: values and the arcs of their knobs, the active tab,
/// a solo, loop marks, a fixed trig condition.
pub const VALUE: Rgb = YELLOW;
/// What varies by itself: a note's probability or cycle, a random trig
/// condition, an LFO, a track in auto mode.
pub const CHANCE: Rgb = VIOLET;
/// The hits a note makes by itself: retriggers, euclidean pulses.
pub const GENERATED: Rgb = ORANGE;
/// Problems and mutes (a mute from a plugin host is dimmed).
pub const ALERT: Rgb = RED;

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
    mix(cell(), color, 0.35 + 0.65 * velocity.clamp(0.0, 1.0))
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
        assert_ne!(note(PINK, 0.0), cell());
    }

    #[test]
    fn schemes_cycle_and_change_the_ground_only() {
        assert_eq!(scheme_number(), 1);
        assert_eq!(next_scheme(1), 2);
        assert_eq!(next_scheme(SCHEMES.len()), 1);
        let first = bg();
        assert!(set_scheme(2));
        assert_eq!(scheme_number(), 2);
        assert_ne!(bg(), first);
        assert!(!set_scheme(0));
        assert!(!set_scheme(SCHEMES.len() + 1));
        assert_eq!(scheme_number(), 2);
        assert!(set_scheme(1));
        assert_eq!(bg(), first);
    }
}
