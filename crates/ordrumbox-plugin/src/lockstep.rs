//! Rendering in step with the player thread when the host renders ahead of
//! real time (a bounce, or a host that computes ahead): before each block,
//! the audio thread has the player thread tick and waits for it, so the
//! passes, pattern and kit of that block are there when it renders, however
//! fast the host goes. In real time it never waits.

use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Ahead of real time by this much, the host can wait for the player thread.
const AHEAD_MIN: Duration = Duration::from_millis(250);
/// What a wait leaves of the time the host is ahead.
const AHEAD_MARGIN: Duration = Duration::from_millis(150);
/// The longest wait for one tick: past it, the block renders with what it has.
const MAX_WAIT: Duration = Duration::from_secs(1);

/// How far the host renders ahead of real time, from the audio it asked for
/// and the time it took.
#[derive(Debug)]
pub struct Pace {
    /// The host said it renders offline (`ProcessMode::Offline`).
    offline: bool,
    /// When the blocks counted in `rendered` started, `None` before the first.
    since: Option<Instant>,
    /// The audio asked for since `since`.
    rendered: Duration,
}

impl Pace {
    pub fn new(offline: bool) -> Self {
        Self { offline, since: None, rendered: Duration::ZERO }
    }

    /// A block of `length` asked for at `now`: how long it may wait for the
    /// player thread; `None` in real time, where it must not wait.
    pub fn budget(&mut self, now: Instant, length: Duration) -> Option<Duration> {
        let elapsed = self.since.map(|since| now.saturating_duration_since(since));
        // Behind real time, or a first block: counted from now on.
        let ahead = match elapsed.and_then(|elapsed| self.rendered.checked_sub(elapsed)) {
            Some(ahead) => ahead,
            None => {
                (self.since, self.rendered) = (Some(now), Duration::ZERO);
                Duration::ZERO
            }
        };
        self.rendered += length;
        if self.offline { Some(MAX_WAIT) } else { (ahead >= AHEAD_MIN).then(|| (ahead - AHEAD_MARGIN).min(MAX_WAIT)) }
    }
}

/// The ticks the audio thread asks of the player thread, and those done.
#[derive(Debug, Default)]
struct Count {
    asked: u64,
    done: u64,
    /// Blocks rendered without their tick, since the player thread last looked.
    late: u64,
}

/// The audio thread's handshake with the player thread.
#[derive(Debug)]
pub struct Steps {
    count: Mutex<Count>,
    changed: Condvar,
}

impl Default for Steps {
    fn default() -> Self {
        let steps = Self { count: Mutex::default(), changed: Condvar::new() };
        // Used once here, so that whatever a platform sets up on first use
        // is not set up on the audio thread.
        drop(steps.lock());
        steps.changed.notify_all();
        steps
    }
}

impl Steps {
    fn lock(&self) -> MutexGuard<'_, Count> {
        self.count.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Audio thread: a tick asked of the player thread, waited for
    /// `timeout` at most. Whether it was done.
    pub fn tick(&self, timeout: Duration) -> bool {
        let mut count = self.lock();
        count.asked += 1;
        let asked = count.asked;
        self.changed.notify_all();
        let (mut count, wait) = self
            .changed
            .wait_timeout_while(count, timeout, |count| count.done < asked)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        count.late += u64::from(wait.timed_out());
        !wait.timed_out()
    }

    /// Player thread: waits for a tick asked, `timeout` at most; the
    /// number of the last tick asked, for [`Self::done`].
    pub fn wait(&self, timeout: Duration) -> u64 {
        let count = self.lock();
        let (count, _) = self
            .changed
            .wait_timeout_while(count, timeout, |count| count.done >= count.asked)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        count.asked
    }

    /// Player thread: how many blocks rendered without waiting for their
    /// tick (past [`MAX_WAIT`]) since the last call.
    pub fn take_late(&self) -> u64 {
        std::mem::take(&mut self.lock().late)
    }

    /// Player thread: the ticks asked up to `asked` are done.
    pub fn done(&self, asked: u64) {
        let mut count = self.lock();
        count.done = count.done.max(asked);
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    const BLOCK: Duration = Duration::from_millis(10);

    #[test]
    fn real_time_never_waits_and_a_host_ahead_does() {
        let start = Instant::now();
        let mut pace = Pace::new(false);
        // Blocks asked as they are played.
        for k in 0..100 {
            assert_eq!(pace.budget(start + BLOCK * k, BLOCK), None);
        }
        // A burst: 100 blocks (1 s) in no time.
        let now = start + BLOCK * 100;
        let budgets: Vec<_> = (0..100).map(|_| pace.budget(now, BLOCK)).collect();
        assert_eq!(budgets[24], None, "240 ms ahead");
        assert_eq!(budgets[25], Some(Duration::from_millis(100)), "250 ms ahead, 150 kept");
        assert_eq!(budgets[99], Some(Duration::from_millis(840)));
        // The host stalls past the burst: real time again, counted from then.
        assert_eq!(pace.budget(now + Duration::from_secs(5), BLOCK), None);
        assert_eq!(Pace::new(true).budget(start, BLOCK), Some(MAX_WAIT), "offline");
    }

    #[test]
    fn a_tick_asked_is_waited_for() {
        let steps = Arc::new(Steps::default());
        assert!(!steps.tick(Duration::from_millis(1)), "no player thread");
        assert_eq!(steps.take_late(), 1, "the block rendered without its tick is counted");
        assert_eq!(steps.take_late(), 0, "and told once");
        let player = steps.clone();
        let thread = std::thread::spawn(move || {
            // The tick that timed out, then the two below (some answered together).
            loop {
                let asked = player.wait(Duration::from_secs(5));
                player.done(asked);
                if asked >= 3 {
                    break;
                }
            }
        });
        for _ in 0..2 {
            assert!(steps.tick(Duration::from_secs(5)));
        }
        thread.join().unwrap();
        assert_eq!(steps.take_late(), 0);
    }
}
