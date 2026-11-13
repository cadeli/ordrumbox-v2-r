//! The message line and the toasts that pop up over the window.

use super::Session;
use crate::panel::Rect;
use crate::scale::Scale;
use crate::view;
use std::time::{Duration, Instant};

/// The lower edge of the toast in a window `height` high.
pub(super) fn toast_bottom(height: i32, scale: Scale) -> i32 {
    height - 2 * crate::frame::margin(scale)
}

/// How long a new message is shown as a toast.
pub(super) const FLASH: Duration = Duration::from_secs(1);

/// How many toasts are on screen at most; a new one beyond pushes out the oldest.
pub(super) const TOAST_LIMIT: usize = 5;

/// The words of a toast for `text` shown `repeat_count` times in a row: the
/// count follows them from the second time on.
#[must_use]
pub(super) fn toast_text(text: &str, repeat_count: u32) -> String {
    if repeat_count > 1 { format!("{text} (×{repeat_count})") } else { text.to_owned() }
}

/// A message shown as a toast, until a time.
#[derive(Debug, Clone)]
pub(super) struct Toast {
    pub(super) text: String,
    pub(super) until: Instant,
}

/// The message last shown on the status line, how many times in a row, and
/// the toasts still on screen, the oldest first.
#[derive(Debug, Clone, Default)]
pub(super) struct Flash {
    pub(super) text: String,
    repeat_count: u32,
    pub(super) toasts: Vec<Toast>,
    /// No toast is shown (View > Toasts unchecked); messages still go on the status line.
    pub(super) hidden: bool,
}

impl Flash {
    /// `text` is shown at `now`: it stands out for [`FLASH`] in a toast of
    /// its own, under the ones still there, or not at all when empty. The same
    /// words again get a toast of their own too, which counts the repeats.
    pub(super) fn shown(&mut self, text: &str, now: Instant) {
        self.repeat_count = if text == self.text { self.repeat_count.saturating_add(1) } else { 1 };
        self.text = text.to_owned();
        if text.is_empty() || self.hidden {
            return;
        }
        if self.toasts.len() == TOAST_LIMIT {
            self.toasts.remove(0);
        }
        let text = toast_text(text, self.repeat_count);
        self.toasts.push(Toast { text, until: now + FLASH });
    }

    /// Each toast with its rectangle in a window `width` × `height`: the
    /// newest at the bottom, the older ones stacked above it.
    pub(super) fn rects(&self, width: i32, height: i32, scale: Scale) -> Vec<(Rect, &Toast)> {
        let mut bottom = toast_bottom(height, scale);
        let mut rects: Vec<(Rect, &Toast)> = self
            .toasts
            .iter()
            .rev()
            .map(|toast| {
                let rect = view::toast_rect(width, &toast.text, bottom, scale);
                bottom = rect.y - view::toast_gap(scale);
                (rect, toast)
            })
            .collect();
        rects.reverse();
        rects
    }
}

impl Session {
    /// View > Toasts: show or hide the messages that pop up over the window.
    pub fn toggle_toasts(&mut self) {
        self.flash.hidden = !self.flash.hidden;
        if self.flash.hidden {
            self.flash.toasts.clear();
        }
        self.message = if self.flash.hidden { "toasts off".into() } else { "toasts on".into() };
    }

    /// Before an input: the message shown, taken away so that what the
    /// input says, even the same words again, is known as new
    /// ([`Self::input_done`]).
    pub(crate) fn input_starts(&mut self) -> String {
        std::mem::take(&mut self.message)
    }

    /// After an input: a message it gave stands out; without one, `before`
    /// (from [`Self::input_starts`]) stays as it was.
    pub(crate) fn input_done(&mut self, before: String) {
        if self.message.is_empty() {
            self.message = before;
        } else {
            self.flash.shown(&self.message, Instant::now());
        }
    }

    /// A message that changed outside an input (an MCP call, a drag, an
    /// update check) stands out too.
    pub(crate) fn note_message(&mut self) {
        if self.message != self.flash.text {
            self.flash.shown(&self.message, Instant::now());
        }
    }

    /// When the next toast goes; `None` when there is none.
    pub(crate) fn flash_until(&self) -> Option<Instant> {
        self.flash.toasts.iter().map(|toast| toast.until).min()
    }

    /// The toasts on screen, the oldest first.
    #[cfg(test)]
    pub(crate) fn toast_texts(&self) -> Vec<&str> {
        self.flash.toasts.iter().map(|toast| toast.text.as_str()).collect()
    }

    /// A press at `(x, y)` on a toast hides it at once; whether there was
    /// one there.
    pub(crate) fn toast_click(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let (width, height, scale) = size;
        let hit = self.flash.rects(width, height, scale).iter().position(|(rect, _)| rect.contains(x, y));
        if let Some(index) = hit {
            self.flash.toasts.remove(index);
        }
        hit.is_some()
    }

    /// The toasts whose time is over go; whether one did.
    pub(crate) fn end_flash(&mut self, now: Instant) -> bool {
        let count = self.flash.toasts.len();
        self.flash.toasts.retain(|toast| toast.until > now);
        self.flash.toasts.len() != count
    }
}
