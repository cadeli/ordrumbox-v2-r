//! Which lines of the matrix the editor lists: those in use, then one free
//! line to start a new route on.

use ordrumbox_core::json::Object;

use super::{MATRIX_SIZE, text};

/// Whether line `n` (1 to [`MATRIX_SIZE`]) of the matrix of `draft` is in
/// use: it has a source or a target.
#[must_use]
pub fn is_matrix_line_used(draft: &Object, n: usize) -> bool {
    let source = text(draft, &format!("matrix.source{n}")).unwrap_or("off");
    let target = text(draft, &format!("matrix.target{n}")).unwrap_or("NOT");
    source != "off" || target != "NOT"
}

/// The lines of the matrix of `draft` the editor lists, in order (1 to
/// [`MATRIX_SIZE`]): every line in use, then the first free one, if any.
#[must_use]
pub fn matrix_lines_listed(draft: &Object) -> Vec<usize> {
    let mut lines: Vec<usize> = (1..=MATRIX_SIZE).filter(|&n| is_matrix_line_used(draft, n)).collect();
    if let Some(free) = (1..=MATRIX_SIZE).find(|&n| !is_matrix_line_used(draft, n)) {
        lines.push(free);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth_model::{defaults, set};
    use ordrumbox_core::json::Value;

    #[test]
    fn the_lines_in_use_then_the_first_free_one() {
        let mut draft = defaults();
        assert_eq!(matrix_lines_listed(&draft), [1]);
        set(&mut draft, "matrix.source3", Value::from("lfo1"));
        set(&mut draft, "matrix.target5", Value::from("filter.freq"));
        assert_eq!(matrix_lines_listed(&draft), [3, 5, 1]);
        for n in 1..=MATRIX_SIZE {
            set(&mut draft, &format!("matrix.source{n}"), Value::from("env"));
        }
        assert_eq!(matrix_lines_listed(&draft), (1..=MATRIX_SIZE).collect::<Vec<_>>(), "no free line left");
    }
}
