//! A wire's slice (spec 7.6 rule 3): Python's `start:stop:step` (any part left out), or a single
//! index. A range keeps a list a list; an index takes one item. Parsed identically by the editor
//! and by every engine, held to the shared vectors in `fixtures/slices.json` (made from Python's
//! own slicing by `claude-debug/slices/make_slice_vectors.py`).
//!
//! Lifted from the research software's `crates/graph/src/ports.rs` and `python/.../executor.py` (2026-10-31,
//! The research software `8480c03`), and hostile input: parsing is total, and every count is bounded by the
//! list's length.

/// A wire's slice, parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slice {
    Index(i64),
    Range { start: Option<i64>, stop: Option<i64>, step: Option<i64> },
}

/// How many lengths [`parse`] tries before calling a slice empty whatever the list.
const EMPTY_PROBE: usize = 64;

impl Slice {
    /// An index makes a list a single item; a range keeps it a list.
    pub fn is_index(&self) -> bool {
        matches!(self, Slice::Index(_))
    }

    /// `(first, end, step)` as Python's `slice.indices`, for a range over `n` items.
    fn bounds(start: Option<i64>, stop: Option<i64>, step: Option<i64>, n: i128) -> (i128, i128, i128) {
        let step = i128::from(step.unwrap_or(1));
        let (lower, upper) = if step > 0 { (0, n) } else { (-1, n - 1) };
        let clamp = |v: Option<i64>, default: i128| match v.map(i128::from) {
            None => default,
            Some(v) if v < 0 => (v + n).max(lower),
            Some(v) => v.min(upper),
        };
        (clamp(start, if step < 0 { upper } else { lower }), clamp(stop, if step < 0 { lower } else { upper }), step)
    }

    /// The number of items this slice picks from a list of `n`; for an index, 1 when it is in
    /// range, else `None`. A zero step, which [`parse`] refuses, picks `None`.
    pub fn len_of(&self, n: usize) -> Option<usize> {
        let n = i128::try_from(n).ok()?;
        match *self {
            Slice::Index(i) => {
                let i = i128::from(i);
                (-n <= i && i < n).then_some(1)
            }
            Slice::Range { step: Some(0), .. } => None,
            Slice::Range { start, stop, step } => {
                let (a, b, step) = Self::bounds(start, stop, step, n);
                let count = if step > 0 {
                    if a < b { (b - a - 1) / step + 1 } else { 0 }
                } else if b < a {
                    (a - b - 1) / (-step) + 1
                } else {
                    0
                };
                usize::try_from(count).ok()
            }
        }
    }

    /// The items picked from a list of `n`, in order. An index out of range and an empty result
    /// are errors, in words for the node that wanted them.
    pub fn picks(&self, n: usize) -> Result<Vec<usize>, String> {
        let wide = i128::try_from(n).map_err(|_| "the list is too long".to_owned())?;
        match *self {
            Slice::Index(i) => {
                let at = if i < 0 { i128::from(i) + wide } else { i128::from(i) };
                match usize::try_from(at) {
                    Ok(at) if at < n => Ok(vec![at]),
                    _ => Err(format!("index {i} is out of range for {n} items")),
                }
            }
            Slice::Range { start, stop, step } => {
                let count = self.len_of(n).unwrap_or(0);
                if count == 0 {
                    return Err(format!("slice {self} of {n} items is empty"));
                }
                let (a, _, step) = Self::bounds(start, stop, step, wide);
                Ok((0..count).filter_map(|k| i128::try_from(k).ok().and_then(|k| usize::try_from(a + k * step).ok())).collect())
            }
        }
    }
}

impl std::fmt::Display for Slice {
    /// Canonical text: `-1`, `1:-1:2`, `:-1`, `::2`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let part = |v: Option<i64>| v.map_or_else(String::new, |v| v.to_string());
        match *self {
            Slice::Index(i) => write!(f, "{i}"),
            Slice::Range { start, stop, step: None } => write!(f, "{}:{}", part(start), part(stop)),
            Slice::Range { start, stop, step } => write!(f, "{}:{}:{}", part(start), part(stop), part(step)),
        }
    }
}

/// Parse a slice. Refuses anything but the grammar, a zero step, a number too large for 64 bits,
/// and a slice that is empty whatever the list's length (`3:1`, `:0`, `-1:-3`), which would
/// otherwise only be found when it runs. One whose emptiness depends on the length (`5:` of
/// three items) is left to [`Slice::picks`].
pub fn parse(spec: &str) -> Result<Slice, String> {
    let usage = || format!("slice {spec:?}: use start:stop:step or an index, e.g. 1:-1:2 or -1");
    if spec.trim().is_empty() {
        return Err(format!("slice {spec:?} is empty"));
    }
    let parts: Vec<&str> = spec.split(':').map(str::trim).collect();
    if parts.len() > 3 {
        return Err(usage());
    }
    let mut nums = Vec::with_capacity(3);
    for p in &parts {
        if p.is_empty() {
            nums.push(None);
            continue;
        }
        let digits = p.strip_prefix('-').unwrap_or(p);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(usage());
        }
        nums.push(Some(p.parse::<i64>().map_err(|_| format!("slice {spec:?}: {p} is too large"))?));
    }
    let slice = match nums.as_slice() {
        [Some(i)] => return Ok(Slice::Index(*i)),
        [start, stop] => Slice::Range { start: *start, stop: *stop, step: None },
        [start, stop, step] => {
            if *step == Some(0) {
                return Err(format!("slice {spec:?}: the step can't be 0"));
            }
            Slice::Range { start: *start, stop: *stop, step: *step }
        }
        _ => return Err(usage()),
    };
    if (1..=EMPTY_PROBE).all(|n| slice.len_of(n) == Some(0)) {
        return Err(format!("slice {spec:?} is always empty: it picks nothing from a list of any length"));
    }
    Ok(slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn the_shared_vectors_hold() {
        let v: Value = serde_json::from_str(include_str!("../fixtures/slices.json")).unwrap();
        for case in v["parse"].as_array().unwrap() {
            let spec = case["spec"].as_str().unwrap();
            let got = parse(spec).ok().map(|s| s.to_string());
            assert_eq!(got.as_deref(), case["canonical"].as_str(), "{spec:?}");
        }
        for case in v["pick"].as_array().unwrap() {
            let spec = case["spec"].as_str().unwrap();
            let n = case["n"].as_u64().unwrap() as usize;
            let want: Option<Vec<usize>> = serde_json::from_value(case["items"].clone()).unwrap();
            assert_eq!(parse(spec).unwrap().picks(n).ok(), want, "{spec:?} of {n}");
        }
    }

    #[test]
    fn extremes_are_bounded_and_never_panic() {
        for spec in ["-9223372036854775808", "9223372036854775807", "::-9223372036854775808", "-9223372036854775808:9223372036854775807:9223372036854775807"] {
            let s = parse(spec).unwrap();
            for n in [0, 1, 5] {
                let _ = s.picks(n);
                let _ = s.len_of(n);
            }
        }
        assert!(parse("99999999999999999999").unwrap_err().contains("too large"));
        assert_eq!(Slice::Range { start: None, stop: None, step: Some(0) }.len_of(3), None);
    }
}
