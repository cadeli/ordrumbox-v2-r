//! Where the header's controls sit: from the window's margin, a strip of
//! metal with a row of cards in the note editor's hardware look — the transport, the
//! pattern, its editing, its generation, the kit and the views. A card holds
//! two rows of buttons and lists, then its knobs or spins; it keeps its place
//! and its size whatever it shows (the song mode, no pattern).

use crate::canvas::text_width;
use crate::frame::{HEADER_CONTROLS_HEIGHT, content_top, margin};
use crate::panel::hardware::CARD_HEAD;
use crate::panel::{Rect, ViewButton};
use crate::scale::Scale;

/// The header's macro knobs: their pattern key and label, short as BPM's
/// (their help gives the whole name).
pub const MACROS: [(&str, &str); 3] = [
    (ordrumbox_core::style::ENERGY, "NRG"),
    (ordrumbox_core::style::COMPLEXITY, "CPLX"),
    (ordrumbox_core::style::VARIATION, "VAR"),
];

/// The pattern card's title while the pattern shown waits for the one
/// playing to end its loop.
pub const WAITING_TITLE: &str = "NEXT";
/// The pattern card's title in song mode, where it holds the song list.
pub const SONG_TITLE: &str = "SONG";

/// The mode switch's labels: what the window edits and plays.
pub const MODES: [&str; 2] = ["PATT", "SONG"];

/// The header's cards, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderCard {
    Transport,
    Pattern,
    Edit,
    Generate,
    Kit,
    View,
}

impl HeaderCard {
    pub const ALL: [Self; 6] = [Self::Transport, Self::Pattern, Self::Edit, Self::Generate, Self::Kit, Self::View];

    /// The title in its band.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Transport => "PLAY",
            Self::Pattern => "PATTERN",
            Self::Edit => "EDIT",
            Self::Generate => "GENERATE",
            Self::Kit => "KIT",
            Self::View => "VIEW",
        }
    }
}

/// The space between the metal's edge and the cards (at scale 1).
const METAL_PAD: i32 = 4;
/// The space inside a card around its controls, between two cards, between
/// two columns of a card and two controls of a row (at scale 1).
const CARD_PAD: i32 = 6;
const CARD_GAP: i32 = 6;
const COLUMN_GAP: i32 = 6;
const CONTROL_GAP: i32 = 4;
/// The space under a card's title band (at scale 1).
const HEAD_GAP: i32 = 4;
/// A row of buttons or lists: its height, and where the two rows start
/// under the title band (at scale 1).
const ROW_H: i32 = 20;
const ROW_TOPS: [i32; 2] = [2, 2 + ROW_H + CONTROL_GAP];
/// A knob's height, with its label and value under it, and its narrowest
/// column (at scale 1).
const KNOB_H: i32 = 48;
const KNOB_W: i32 = 46;

/// Where the header's controls are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderControls {
    /// The metal behind the cards.
    pub metal: Rect,
    /// The cards, as [`HeaderCard::ALL`] lists them.
    pub cards: [Rect; 6],
    /// The mode switch: the pattern, the song.
    pub mode: [Rect; 2],
    pub play: Rect,
    /// REC beside PLAY: in song mode, the mixer writes the song's automation.
    pub record: Rect,
    pub bpm: Rect,
    pub pattern: Rect,
    pub beats: Rect,
    /// New, duplicate, and the form of the name, description and tags.
    pub pattern_buttons: [Rect; 3],
    /// Quantize, humanize, clear, compact.
    pub edit_buttons: [Rect; 4],
    /// The generation: the style list, ♫, AUTO; drums, bass, chords; the
    /// scale and the key; the energy, complexity and variation macros.
    pub style: Rect,
    pub generate: Rect,
    pub auto: Rect,
    pub parts: [Rect; 3],
    pub scale: Rect,
    pub key: Rect,
    pub macros: [Rect; 3],
    pub kit: Rect,
    /// A random kit, every track on a synth sound, every track back on
    /// auto-assign, a random sample on every track.
    pub kit_buttons: [Rect; 4],
    /// The view buttons, as [`ViewButton::ALL`] lists them (the song's slot
    /// is empty: the mode switch is its button).
    pub views: [Rect; ViewButton::ALL.len()],
}

/// Lays out the cards left to right, a column at a time.
struct Cards {
    s: Scale,
    x: i32,
    top: i32,
    /// The left edge of the card being laid out.
    card_x: i32,
    cards: Vec<Rect>,
}

impl Cards {
    fn open(&mut self) {
        self.card_x = self.x;
        self.x += CARD_PAD * self.s;
    }

    fn close(&mut self) {
        // The last column's gap is the card's padding.
        self.x += (CARD_PAD - COLUMN_GAP) * self.s;
        self.cards.push(Rect::new(self.card_x, self.top, self.x - self.card_x, card_height(self.s)));
        self.x += CARD_GAP * self.s;
    }

    /// Where a card's controls start: under its title band.
    fn content_top(&self) -> i32 {
        self.top + (CARD_HEAD + HEAD_GAP) * self.s
    }

    /// A row of controls `widths` wide (at scale 1) at the column's left,
    /// on row `line` (0 or 1).
    fn row<const N: usize>(&self, line: usize, widths: [i32; N]) -> [Rect; N] {
        let s = self.s;
        let y = self.content_top() + ROW_TOPS[line.min(1)] * s;
        let mut x = self.x;
        widths.map(|w| {
            let rect = Rect::new(x, y, w * s, ROW_H * s);
            x += (w + CONTROL_GAP) * s;
            rect
        })
    }

    /// A column of two rows of controls; the column is as wide as the wider.
    fn rows<const A: usize, const B: usize>(&mut self, top: [i32; A], bottom: [i32; B]) -> ([Rect; A], [Rect; B]) {
        let (top, bottom) = (self.row(0, top), self.row(1, bottom));
        let right = top.iter().chain(&bottom).map(|r| r.x + r.w).max().unwrap_or(self.x);
        self.x = right + COLUMN_GAP * self.s;
        (top, bottom)
    }

    /// A knob's column, as wide as its label.
    fn knob(&mut self, label: &str) -> Rect {
        let s = self.s;
        let w = (KNOB_W * s).max(text_width(label, s) + 4 * s);
        let rect = Rect::new(self.x, self.content_top(), w, KNOB_H * s);
        self.x += w + COLUMN_GAP * s;
        rect
    }
}

/// A card's height: its title band, its controls, its padding.
fn card_height(scale: Scale) -> i32 {
    (HEADER_CONTROLS_HEIGHT - 2 * METAL_PAD) * scale
}

/// The header's controls at `scale`.
#[must_use]
pub fn header_controls(scale: Scale) -> HeaderControls {
    let s = scale;
    let top = content_top(s);
    let metal_x = margin(s);
    let x = metal_x + METAL_PAD * s;
    let mut c = Cards { s, x, top: top + METAL_PAD * s, card_x: x, cards: Vec::new() };

    c.open();
    // PLAY twice as wide as REC, to stand out; both as wide as the mode switch.
    let (mode, [play, record]) = c.rows([36, 36], [48, 24]);
    let bpm = c.knob("BPM");
    c.close();

    c.open();
    // INFO under the beats.
    let ([pattern, beats], pattern_buttons) = c.rows([104, 68], [50, 50, 68]);
    c.close();

    c.open();
    let ([quantize, humanize], [clear, compact]) = c.rows([48, 48], [48, 48]);
    c.close();

    c.open();
    let ([style, generate, auto], parts) = c.rows([84, 20, 36], [46, 46, 48]);
    let ([scale_spin], [key]) = c.rows([86], [86]);
    let macros = MACROS.map(|(_, label)| c.knob(label));
    c.close();

    c.open();
    // The kit list over the sound assignment of Shift+G, H, J and K.
    let ([kit], kit_buttons) = c.rows([170], [30, 44, 48, 36]);
    c.close();

    c.open();
    let ([grid, roll, synth], [mixer, kit_view]) = c.rows([36, 36, 40], [36, 36]);
    c.close();

    let cards: [Rect; 6] = std::array::from_fn(|i| c.cards.get(i).copied().unwrap_or_default());
    let right = cards[5].x + cards[5].w + METAL_PAD * s;
    let metal = Rect::new(metal_x, top, right - metal_x, HEADER_CONTROLS_HEIGHT * s);
    // The song's slot, closed: the mode switch is its button.
    let song = Rect::new(roll.x + roll.w, roll.y, 0, 0);
    HeaderControls {
        metal,
        cards,
        mode,
        play,
        record,
        bpm,
        pattern,
        beats,
        pattern_buttons,
        edit_buttons: [quantize, humanize, clear, compact],
        style,
        generate,
        auto,
        parts,
        scale: scale_spin,
        key,
        macros,
        kit,
        kit_buttons,
        views: [grid, roll, song, synth, mixer, kit_view],
    }
}

impl HeaderControls {
    /// The song list of the song mode, in the place of the pattern
    /// list and the beats.
    #[must_use]
    pub fn song(&self) -> Rect {
        Rect::new(self.pattern.x, self.pattern.y, self.beats.x + self.beats.w - self.pattern.x, self.pattern.h)
    }

    /// The card `card`.
    #[must_use]
    pub fn card(&self, card: HeaderCard) -> Rect {
        self.cards[card as usize]
    }

    /// The window's width the header needs: up to the metal's right edge and
    /// the margin.
    #[must_use]
    pub fn width(&self, scale: Scale) -> i32 {
        self.metal.x + self.metal.w + margin(scale)
    }
}
