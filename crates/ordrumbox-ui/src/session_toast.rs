//! The message line and the toasts that pop up over the window.

use super::Session;
use crate::scale::Scale;
use std::time::Instant;

impl Session {
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
