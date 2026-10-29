//! `Array.prototype.sort` as V8 runs it (`third_party/v8/builtins/array-sort.tq`,
//! a port of CPython's listsort): TimSort with binary insertion, galloping and
//! the same merge pattern, step for step.
//!
//! For a consistent comparator any stable sort gives the same order. The JS
//! app also sorts with inconsistent ones (the MIDI exporter's note on/off
//! order says "a before b" for two note-offs whichever comes first); then the
//! result is whatever this exact algorithm does, and only it reproduces the
//! JS bytes.

/// Initial threshold of galloping.
const MIN_GALLOP: usize = 7;

struct Sorter<'a, T, F> {
    a: &'a mut [T],
    less: F,
    min_gallop: usize,
    runs: Vec<(usize, usize)>,
}

/// Sort `items` with `compare` returning a JS comparator's number: negative
/// when its first argument goes first (NaN counts as 0).
pub fn sort_by<T: Clone>(items: &mut [T], mut compare: impl FnMut(&T, &T) -> f64) {
    let n = items.len();
    if n < 2 {
        return;
    }
    let less = move |x: &T, y: &T| compare(x, y) < 0.0;
    let mut s = Sorter { a: items, less, min_gallop: MIN_GALLOP, runs: Vec::new() };
    let min_run = min_run_length(n);
    let (mut low, mut remaining) = (0, n);
    while remaining != 0 {
        let mut run = s.count_and_make_run(low, low + remaining);
        if run < min_run {
            let forced = min_run.min(remaining);
            s.binary_insertion_sort(low, low + run, low + forced);
            run = forced;
        }
        s.runs.push((low, run));
        s.merge_collapse();
        low += run;
        remaining -= run;
    }
    s.merge_force_collapse();
}

fn min_run_length(mut n: usize) -> usize {
    let mut r = 0;
    while n >= 64 {
        r |= n & 1;
        n >>= 1;
    }
    n + r
}

impl<T: Clone, F: FnMut(&T, &T) -> bool> Sorter<'_, T, F> {
    fn lt(&mut self, x: &T, y: &T) -> bool {
        (self.less)(x, y)
    }

    /// `CountAndMakeRun`: the run starting at `low` (a strictly descending one reversed).
    fn count_and_make_run(&mut self, low: usize, high: usize) -> usize {
        if low + 1 == high {
            return 1;
        }
        let (second, first) = (self.a[low + 1].clone(), self.a[low].clone());
        let descending = self.lt(&second, &first);
        let mut previous = second;
        let mut length = 2;
        for i in low + 2..high {
            let current = self.a[i].clone();
            let order = self.lt(&current, &previous);
            if if descending { !order } else { order } {
                break;
            }
            previous = current;
            length += 1;
        }
        if descending {
            self.a[low..low + length].reverse();
        }
        length
    }

    /// `BinaryInsertionSort`: `[low, start)` is sorted; insert the rest up to `high`.
    fn binary_insertion_sort(&mut self, low: usize, start: usize, high: usize) {
        let mut start = if low == start { start + 1 } else { start };
        while start < high {
            let pivot = self.a[start].clone();
            let (mut left, mut right) = (low, start);
            while left < right {
                let mid = left + ((right - left) >> 1);
                let x = self.a[mid].clone();
                if self.lt(&pivot, &x) {
                    right = mid;
                } else {
                    left = mid + 1;
                }
            }
            self.a[left..=start].rotate_right(1);
            start += 1;
        }
    }

    fn merge_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            let len = |s: &Self, i: usize| s.runs[i].1;
            let established = |s: &Self, i: usize| i < 2 || len(s, i - 2) > len(s, i - 1) + len(s, i);
            if !established(self, n + 1) || !established(self, n) {
                if len(self, n - 1) < len(self, n + 1) {
                    n -= 1;
                }
                self.merge_at(n);
            } else if len(self, n) <= len(self, n + 1) {
                self.merge_at(n);
            } else {
                break;
            }
        }
    }

    fn merge_force_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if n > 0 && self.runs[n - 1].1 < self.runs[n + 1].1 {
                n -= 1;
            }
            self.merge_at(n);
        }
    }

    fn merge_at(&mut self, i: usize) {
        let (base_a, len_a) = self.runs[i];
        let (base_b, len_b) = self.runs[i + 1];
        self.runs[i].1 = len_a + len_b;
        self.runs.remove(i + 1);

        let key = self.a[base_b].clone();
        let k = self.gallop_right(&key, base_a, len_a, 0, None);
        let (base_a, len_a) = (base_a + k, len_a - k);
        if len_a == 0 {
            return;
        }
        let key = self.a[base_a + len_a - 1].clone();
        let len_b = self.gallop_left(&key, base_b, len_b, len_b - 1, None);
        if len_b == 0 {
            return;
        }
        if len_a <= len_b {
            self.merge_low(base_a, len_a, base_b, len_b);
        } else {
            self.merge_high(base_a, len_a, base_b, len_b);
        }
    }

    fn at(&self, base: usize, i: usize, temp: Option<&[T]>) -> T {
        match temp {
            Some(t) => t[base + i].clone(),
            None => self.a[base + i].clone(),
        }
    }

    /// `GallopLeft`: where `key` goes in the sorted `[base, base + length)`,
    /// before equal elements, searching from `hint` (in `temp` when given).
    fn gallop_left(&mut self, key: &T, base: usize, length: usize, hint: usize, temp: Option<&[T]>) -> usize {
        let (mut last_ofs, mut ofs) = (0usize, 1usize);
        let x = self.at(base, hint, temp);
        if self.lt(&x, key) {
            let max_ofs = length - hint;
            while ofs < max_ofs {
                let x = self.at(base, hint + ofs, temp);
                if !self.lt(&x, key) {
                    break;
                }
                last_ofs = ofs;
                ofs = (ofs << 1) + 1;
            }
            ofs = ofs.min(max_ofs);
            last_ofs += hint;
            ofs += hint;
        } else {
            let max_ofs = hint + 1;
            while ofs < max_ofs {
                let x = self.at(base, hint - ofs, temp);
                if self.lt(&x, key) {
                    break;
                }
                last_ofs = ofs;
                ofs = (ofs << 1) + 1;
            }
            ofs = ofs.min(max_ofs);
            let k = last_ofs;
            // `lastOfs = hint - ofs` may be -1: kept one higher (+1 below).
            last_ofs = hint + 1 - ofs;
            ofs = hint - k;
            return self.gallop_finish_left(key, base, last_ofs, ofs, temp);
        }
        self.gallop_finish_left(key, base, last_ofs + 1, ofs, temp)
    }

    /// The binary search ending `GallopLeft` over `(lo - 1, hi]`, `lo` already incremented.
    fn gallop_finish_left(&mut self, key: &T, base: usize, mut lo: usize, mut hi: usize, temp: Option<&[T]>) -> usize {
        while lo < hi {
            let m = lo + ((hi - lo) >> 1);
            let x = self.at(base, m, temp);
            if self.lt(&x, key) {
                lo = m + 1;
            } else {
                hi = m;
            }
        }
        hi
    }

    /// `GallopRight`: where `key` goes in the sorted `[base, base + length)`,
    /// after equal elements, searching from `hint` (in `temp` when given).
    fn gallop_right(&mut self, key: &T, base: usize, length: usize, hint: usize, temp: Option<&[T]>) -> usize {
        let (mut last_ofs, mut ofs) = (0usize, 1usize);
        let x = self.at(base, hint, temp);
        let (lo, hi) = if self.lt(key, &x) {
            let max_ofs = hint + 1;
            while ofs < max_ofs {
                let x = self.at(base, hint - ofs, temp);
                if !self.lt(key, &x) {
                    break;
                }
                last_ofs = ofs;
                ofs = (ofs << 1) + 1;
            }
            ofs = ofs.min(max_ofs);
            // lastOfs = hint - ofs (maybe -1), ofs = hint - lastOfs; then ++lastOfs.
            (hint + 1 - ofs, hint - last_ofs)
        } else {
            let max_ofs = length - hint;
            while ofs < max_ofs {
                let x = self.at(base, hint + ofs, temp);
                if self.lt(key, &x) {
                    break;
                }
                last_ofs = ofs;
                ofs = (ofs << 1) + 1;
            }
            ofs = ofs.min(max_ofs);
            (last_ofs + hint + 1, ofs + hint)
        };
        let (mut lo, mut hi) = (lo, hi);
        while lo < hi {
            let m = lo + ((hi - lo) >> 1);
            let x = self.at(base, m, temp);
            if self.lt(key, &x) {
                hi = m;
            } else {
                lo = m + 1;
            }
        }
        hi
    }

    /// `MergeLow`: run A (copied aside) merged with run B, from the left.
    fn merge_low(&mut self, base_a: usize, len_a: usize, base_b: usize, len_b: usize) {
        let temp: Vec<T> = self.a[base_a..base_a + len_a].to_vec();
        let (mut len_a, mut len_b) = (len_a, len_b);
        let (mut dest, mut cursor_temp, mut cursor_b) = (base_a, 0usize, base_b);
        self.a[dest] = self.a[cursor_b].clone();
        dest += 1;
        cursor_b += 1;
        len_b -= 1;
        let mut min_gallop = self.min_gallop;

        'outer: {
            if len_b == 0 {
                break 'outer; // Succeed
            }
            if len_a == 1 {
                return self.copy_b_low(&temp, dest, cursor_temp, cursor_b, len_b);
            }
            loop {
                let (mut wins_a, mut wins_b) = (0usize, 0usize);
                loop {
                    let (b, t) = (self.a[cursor_b].clone(), temp[cursor_temp].clone());
                    if self.lt(&b, &t) {
                        self.a[dest] = b;
                        dest += 1;
                        cursor_b += 1;
                        wins_b += 1;
                        len_b -= 1;
                        wins_a = 0;
                        if len_b == 0 {
                            break 'outer;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    } else {
                        self.a[dest] = t;
                        dest += 1;
                        cursor_temp += 1;
                        wins_a += 1;
                        len_a -= 1;
                        wins_b = 0;
                        if len_a == 1 {
                            return self.copy_b_low(&temp, dest, cursor_temp, cursor_b, len_b);
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP || wins_b >= MIN_GALLOP || first {
                    first = false;
                    min_gallop = min_gallop.saturating_sub(1).max(1);
                    self.min_gallop = min_gallop;
                    let key = self.a[cursor_b].clone();
                    wins_a = self.gallop_right(&key, cursor_temp, len_a, 0, Some(&temp));
                    if wins_a > 0 {
                        self.a[dest..dest + wins_a].clone_from_slice(&temp[cursor_temp..cursor_temp + wins_a]);
                        dest += wins_a;
                        cursor_temp += wins_a;
                        len_a -= wins_a;
                        if len_a == 1 {
                            return self.copy_b_low(&temp, dest, cursor_temp, cursor_b, len_b);
                        }
                        if len_a == 0 {
                            break 'outer;
                        }
                    }
                    self.a[dest] = self.a[cursor_b].clone();
                    dest += 1;
                    cursor_b += 1;
                    len_b -= 1;
                    if len_b == 0 {
                        break 'outer;
                    }
                    let key = temp[cursor_temp].clone();
                    wins_b = self.gallop_left(&key, cursor_b, len_b, 0, None);
                    if wins_b > 0 {
                        for k in 0..wins_b {
                            self.a[dest + k] = self.a[cursor_b + k].clone();
                        }
                        dest += wins_b;
                        cursor_b += wins_b;
                        len_b -= wins_b;
                        if len_b == 0 {
                            break 'outer;
                        }
                    }
                    self.a[dest] = temp[cursor_temp].clone();
                    dest += 1;
                    cursor_temp += 1;
                    len_a -= 1;
                    if len_a == 1 {
                        return self.copy_b_low(&temp, dest, cursor_temp, cursor_b, len_b);
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        }
        // Succeed: what is left of A.
        self.a[dest..dest + len_a].clone_from_slice(&temp[cursor_temp..cursor_temp + len_a]);
    }

    /// `MergeLow`'s CopyB: the rest of B, then the last element of A.
    fn copy_b_low(&mut self, temp: &[T], dest: usize, cursor_temp: usize, cursor_b: usize, len_b: usize) {
        for k in 0..len_b {
            self.a[dest + k] = self.a[cursor_b + k].clone();
        }
        self.a[dest + len_b] = temp[cursor_temp].clone();
    }

    /// `MergeHigh`: run A merged with run B (copied aside), from the right.
    fn merge_high(&mut self, base_a: usize, len_a: usize, base_b: usize, len_b: usize) {
        let temp: Vec<T> = self.a[base_b..base_b + len_b].to_vec();
        let (mut len_a, mut len_b) = (len_a, len_b);
        // Cursors as signed: they step below their run's start at the end.
        let mut dest = (base_b + len_b - 1) as isize;
        let mut cursor_temp = len_b as isize - 1;
        let mut cursor_a = (base_a + len_a - 1) as isize;
        let at = |i: isize| i as usize;
        self.a[at(dest)] = self.a[at(cursor_a)].clone();
        dest -= 1;
        cursor_a -= 1;
        len_a -= 1;
        let mut min_gallop = self.min_gallop;

        'outer: {
            if len_a == 0 {
                break 'outer;
            }
            if len_b == 1 {
                return self.copy_a_high(&temp, dest, cursor_temp, cursor_a, len_a);
            }
            loop {
                let (mut wins_a, mut wins_b) = (0usize, 0usize);
                loop {
                    let (t, x) = (temp[at(cursor_temp)].clone(), self.a[at(cursor_a)].clone());
                    if self.lt(&t, &x) {
                        self.a[at(dest)] = x;
                        dest -= 1;
                        cursor_a -= 1;
                        wins_a += 1;
                        len_a -= 1;
                        wins_b = 0;
                        if len_a == 0 {
                            break 'outer;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    } else {
                        self.a[at(dest)] = t;
                        dest -= 1;
                        cursor_temp -= 1;
                        wins_b += 1;
                        len_b -= 1;
                        wins_a = 0;
                        if len_b == 1 {
                            return self.copy_a_high(&temp, dest, cursor_temp, cursor_a, len_a);
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP || wins_b >= MIN_GALLOP || first {
                    first = false;
                    min_gallop = min_gallop.saturating_sub(1).max(1);
                    self.min_gallop = min_gallop;
                    let key = temp[at(cursor_temp)].clone();
                    let k = self.gallop_right(&key, base_a, len_a, len_a - 1, None);
                    wins_a = len_a - k;
                    if wins_a > 0 {
                        dest -= wins_a as isize;
                        cursor_a -= wins_a as isize;
                        for j in (1..=wins_a as isize).rev() {
                            self.a[at(dest + j)] = self.a[at(cursor_a + j)].clone();
                        }
                        len_a -= wins_a;
                        if len_a == 0 {
                            break 'outer;
                        }
                    }
                    self.a[at(dest)] = temp[at(cursor_temp)].clone();
                    dest -= 1;
                    cursor_temp -= 1;
                    len_b -= 1;
                    if len_b == 1 {
                        return self.copy_a_high(&temp, dest, cursor_temp, cursor_a, len_a);
                    }
                    let key = self.a[at(cursor_a)].clone();
                    let k = self.gallop_left(&key, 0, len_b, len_b - 1, Some(&temp));
                    wins_b = len_b - k;
                    if wins_b > 0 {
                        dest -= wins_b as isize;
                        cursor_temp -= wins_b as isize;
                        for j in 0..wins_b {
                            self.a[at(dest + 1) + j] = temp[at(cursor_temp + 1) + j].clone();
                        }
                        len_b -= wins_b;
                        if len_b == 1 {
                            return self.copy_a_high(&temp, dest, cursor_temp, cursor_a, len_a);
                        }
                        if len_b == 0 {
                            break 'outer;
                        }
                    }
                    self.a[at(dest)] = self.a[at(cursor_a)].clone();
                    dest -= 1;
                    cursor_a -= 1;
                    len_a -= 1;
                    if len_a == 0 {
                        break 'outer;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        }
        // Succeed: what is left of B.
        if len_b > 0 {
            let start = at(dest) + 1 - len_b;
            self.a[start..start + len_b].clone_from_slice(&temp[..len_b]);
        }
    }

    /// `MergeHigh`'s CopyA: the rest of A moved right, then the first element of B.
    fn copy_a_high(&mut self, temp: &[T], dest: isize, cursor_temp: isize, cursor_a: isize, len_a: usize) {
        let dest = dest - len_a as isize;
        let cursor_a = cursor_a - len_a as isize;
        for j in (1..=len_a).rev() {
            self.a[(dest + j as isize) as usize] = self.a[(cursor_a + j as isize) as usize].clone();
        }
        self.a[dest as usize] = temp[cursor_temp as usize].clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_and_is_stable() {
        let mut state = 12345u64;
        for n in [0, 1, 2, 3, 10, 63, 64, 65, 200, 1000, 5000] {
            let mut items: Vec<(u32, usize)> = (0..n)
                .map(|i| {
                    state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    (((state >> 33) % 50) as u32, i)
                })
                .collect();
            let mut expected = items.clone();
            expected.sort_by_key(|e| e.0);
            sort_by(&mut items, |a, b| f64::from(a.0) - f64::from(b.0));
            assert_eq!(items, expected, "n = {n}");
        }
    }
}
