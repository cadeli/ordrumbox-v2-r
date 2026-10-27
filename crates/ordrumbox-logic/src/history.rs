//! Undo/redo (`src/logic/history_manager.js`): a bounded list of steps, each
//! holding the patterns as they were before and after it.
//!
//! The JS steps are closures that redo or undo one change in place; here a
//! step keeps copies of what changed (one pattern, or the pattern list), which
//! needs no per-command inverse and cannot drift from the state.

use ordrumbox_core::Pattern;

/// Steps kept (`new HistoryManager(50)`).
pub const MAX_STEPS: usize = 50;
/// Two edits with the same coalescing key this close together (ms) are one
/// undo step (`COALESCE_WINDOW_MS`): a whole slider drag, a held key.
pub const COALESCE_WINDOW_MS: f64 = 400.0;

/// What one step changed.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Pattern `index`, before and after.
    Pattern { index: usize, before: Box<Pattern>, after: Box<Pattern> },
    /// The whole pattern list (patterns added or removed).
    Patterns { before: Vec<Pattern>, after: Vec<Pattern> },
}

impl Change {
    fn apply(&self, patterns: &mut Vec<Pattern>, undo: bool) {
        match self {
            Self::Pattern { index, before, after } => {
                if let Some(slot) = patterns.get_mut(*index) {
                    *slot = if undo { before } else { after }.as_ref().clone();
                }
            }
            Self::Patterns { before, after } => *patterns = if undo { before } else { after }.clone(),
        }
    }

    /// Same target (same pattern, or both the whole list).
    fn same_target(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Pattern { index: a, .. }, Self::Pattern { index: b, .. }) => a == b,
            (Self::Patterns { .. }, Self::Patterns { .. }) => true,
            _ => false,
        }
    }

    /// `self` followed by `next` (same target): the first before, the last after.
    fn merge(&mut self, next: Self) {
        match (self, next) {
            (Self::Pattern { after, .. }, Self::Pattern { after: next_after, .. }) => *after = next_after,
            (Self::Patterns { after, .. }, Self::Patterns { after: next_after, .. }) => *after = next_after,
            _ => {}
        }
    }
}

/// One undo step.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// What the step did, for the user ("Add note on KICK in \"rock\"").
    pub desc: String,
    change: Change,
    coalesce_key: Option<String>,
    /// Time of the last edit merged into this step, ms.
    at: f64,
}

#[derive(Debug, Clone, Default)]
pub struct History {
    past: Vec<Step>,
    future: Vec<Step>,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.past.len()
    }

    pub fn redo_len(&self) -> usize {
        self.future.len()
    }

    /// What undo would revert.
    pub fn next_undo(&self) -> Option<&str> {
        self.past.last().map(|s| s.desc.as_str())
    }

    /// What redo would apply again.
    pub fn next_redo(&self) -> Option<&str> {
        self.future.last().map(|s| s.desc.as_str())
    }

    /// Record a change made at `now` (ms). With the coalescing key of the last
    /// step, on the same target, within [`COALESCE_WINDOW_MS`], it extends
    /// that step instead (its description becomes the newest one). Recording
    /// drops what could be redone.
    pub fn record(&mut self, desc: String, change: Change, coalesce_key: Option<String>, now: f64) {
        if let (Some(key), Some(last)) = (&coalesce_key, self.past.last_mut())
            && last.coalesce_key.as_ref() == Some(key)
            && last.change.same_target(&change)
            && now - last.at < COALESCE_WINDOW_MS
        {
            last.change.merge(change);
            last.desc = desc;
            last.at = now;
            self.future.clear();
            return;
        }
        self.past.push(Step { desc, change, coalesce_key, at: now });
        if self.past.len() > MAX_STEPS {
            self.past.remove(0);
        }
        self.future.clear();
    }

    /// Revert the last step on `patterns`; its description.
    pub fn undo(&mut self, patterns: &mut Vec<Pattern>) -> Option<String> {
        let step = self.past.pop()?;
        step.change.apply(patterns, true);
        let desc = step.desc.clone();
        self.future.push(step);
        Some(desc)
    }

    /// Apply the last undone step again on `patterns`; its description.
    pub fn redo(&mut self, patterns: &mut Vec<Pattern>) -> Option<String> {
        let mut step = self.future.pop()?;
        step.change.apply(patterns, false);
        let desc = step.desc.clone();
        step.coalesce_key = None; // a redone step never absorbs the next edit
        self.past.push(step);
        Some(desc)
    }

    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(bpm: f64) -> Pattern {
        Pattern { bpm, ..Pattern::default() }
    }

    fn change(before: f64, after: f64) -> Change {
        Change::Pattern { index: 0, before: Box::new(pattern(before)), after: Box::new(pattern(after)) }
    }

    #[test]
    fn undo_redo_and_limit() {
        let mut history = History::default();
        let mut patterns = vec![pattern(1.0)];
        for i in 1..=60 {
            history.record(format!("bpm {i}"), change(f64::from(i), f64::from(i + 1)), None, f64::from(i) * 1000.0);
        }
        assert_eq!(history.undo_len(), MAX_STEPS);
        assert_eq!(history.undo(&mut patterns).as_deref(), Some("bpm 60"));
        assert_eq!(patterns[0].bpm, 60.0);
        assert_eq!(history.redo(&mut patterns).as_deref(), Some("bpm 60"));
        assert_eq!(patterns[0].bpm, 61.0);
        history.undo(&mut patterns);
        history.record("new".into(), change(60.0, 3.0), None, 1e9);
        assert!(!history.can_redo());
    }

    #[test]
    fn coalescing_keeps_the_first_before() {
        let mut history = History::default();
        let key = Some("track:0:velocity".to_owned());
        history.record("a".into(), change(1.0, 2.0), key.clone(), 0.0);
        history.record("b".into(), change(2.0, 3.0), key.clone(), 300.0);
        history.record("c".into(), change(3.0, 4.0), key.clone(), 600.0); // 300 ms after the last
        history.record("d".into(), change(4.0, 5.0), key, 1100.0); // too late: a new step
        assert_eq!(history.undo_len(), 2);
        let mut patterns = vec![pattern(5.0)];
        history.undo(&mut patterns);
        assert_eq!(patterns[0].bpm, 4.0);
        assert_eq!(history.undo(&mut patterns).as_deref(), Some("c"));
        assert_eq!(patterns[0].bpm, 1.0);
    }
}
