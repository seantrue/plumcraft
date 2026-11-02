//! An array as nodes compute on it: `f32` values in C order, (H, W, C) with C ≥ 1, plus the
//! core type's colour space (optional), channel names and table flag. Any numeric dtype comes
//! in as `f32` ([`plumcraft_engine::arrays::values`]); results go out as `<f4`, or as `<i4`
//! when they are integer labels.

use plumcraft_engine::Array;

/// An array being computed on.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Img {
    pub h: usize,
    pub w: usize,
    pub c: usize,
    /// (H, W, C), C order.
    pub data: Vec<f32>,
    pub space: Option<String>,
    pub names: Vec<String>,
    /// An (N, k) table: `h` rows of `c` columns (`w` is 1).
    pub table: bool,
    /// Integer labels: written out as `<i4`.
    pub labels: bool,
}

/// The largest array a node makes: 16384 a side, 256 M values.
pub const MAX_SIDE: usize = 16_384;
pub const MAX_VALUES: usize = 256 << 20;

impl Img {
    /// Read an array: 2-D (one channel), 3-D (a stack), or a table (N, k). Integer arrays
    /// stay labels.
    pub fn from_array(a: &Array) -> Result<Img, String> {
        let dims: Vec<usize> = a.shape.iter().map(|d| usize::try_from(*d).map_err(|_| "too large".to_owned())).collect::<Result<_, _>>()?;
        let (h, w, c) = match (a.table, dims.as_slice()) {
            (true, [n, k]) => (*n, 1, *k),
            (_, [h, w]) => (*h, *w, 1),
            (_, [h, w, c]) => (*h, *w, *c),
            (_, [n]) => (1, *n, 1),
            _ => return Err(format!("expected (H, W), (H, W, C) or a table, got shape {:?}", a.shape)),
        };
        let data = plumcraft_engine::arrays::values(a).ok_or_else(|| format!("unsupported dtype {:?}, or the bytes don't fill the shape", a.dtype))?;
        let labels = plumcraft_engine::arrays::is_integer(&a.dtype);
        Ok(Img { h, w, c, data, space: a.space.clone(), names: a.names.clone(), table: a.table, labels })
    }

    /// An `<f4` array (or `<i4` labels) with this array's metadata.
    pub fn to_array(&self) -> Array {
        let shape: Vec<u64> = if self.table {
            vec![self.h as u64, self.c as u64]
        } else if self.c == 1 {
            vec![self.h as u64, self.w as u64]
        } else {
            vec![self.h as u64, self.w as u64, self.c as u64]
        };
        let a = if self.labels {
            Array::new("<i4", shape, self.data.iter().flat_map(|v| (*v as i32).to_le_bytes()).collect::<Vec<u8>>())
        } else {
            Array::from_f32(shape, &self.data)
        };
        Array { space: self.space.clone(), names: self.names.clone(), table: self.table, ..a }
    }

    /// A new (H, W) or (H, W, C) array of `value`.
    pub fn filled(h: usize, w: usize, c: usize, value: f32) -> Img {
        Img { h, w, c, data: vec![value; h * w * c], ..Default::default() }
    }

    /// One plane (H, W) from its values.
    pub fn plane_of(h: usize, w: usize, data: Vec<f32>) -> Img {
        Img { h, w, c: 1, data, ..Default::default() }
    }

    /// Channel `k` as a plane.
    pub fn plane(&self, k: usize) -> Vec<f32> {
        if self.c == 1 {
            return self.data.clone();
        }
        self.data.iter().skip(k).step_by(self.c).copied().collect()
    }

    pub fn planes(&self) -> Vec<Vec<f32>> {
        (0..self.c).map(|k| self.plane(k)).collect()
    }

    /// Channels as one stack, without a space or names.
    pub fn from_planes(h: usize, w: usize, planes: &[Vec<f32>]) -> Img {
        let c = planes.len().max(1);
        let mut data = vec![0.0; h * w * c];
        for (k, p) in planes.iter().enumerate() {
            for (i, v) in p.iter().take(h * w).enumerate() {
                if let Some(d) = data.get_mut(i * c + k) {
                    *d = *v;
                }
            }
        }
        Img { h, w, c, data, ..Default::default() }
    }

    /// The name of channel `k`: its own, or `channel <k+1>`.
    pub fn name(&self, k: usize) -> String {
        self.names.get(k).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("channel {}", k + 1))
    }

    /// "array Lab 812×808×3" (as summaries say).
    pub fn describe(&self) -> String {
        let what = if self.table {
            "table"
        } else if self.labels {
            "labels"
        } else {
            "array"
        };
        let space = self.space.as_deref().map(|s| format!(" {s}")).unwrap_or_default();
        let shape = if self.table {
            format!("{}×{}", self.h, self.c)
        } else if self.c == 1 {
            format!("{}×{}", self.h, self.w)
        } else {
            format!("{}×{}×{}", self.h, self.w, self.c)
        };
        format!("{what}{space} {shape}")
    }

    /// Apply `f` to each channel (H × W values, its height and width), keeping the space and
    /// names: what the research software's per-channel nodes do with a stack.
    pub fn map_planes(&self, mut f: impl FnMut(&[f32], usize, usize) -> Result<Vec<f32>, String>) -> Result<Img, String> {
        let planes = self.planes().iter().map(|p| f(p, self.h, self.w)).collect::<Result<Vec<_>, _>>()?;
        if planes.iter().any(|p| p.len() != self.h * self.w) {
            return Err("a channel changed size".into());
        }
        Ok(Img { space: self.space.clone(), names: self.names.clone(), ..Img::from_planes(self.h, self.w, &planes) })
    }

    /// Check a size a node is about to make.
    pub fn check_size(h: usize, w: usize, c: usize) -> Result<(), String> {
        if h == 0 || w == 0 || c == 0 {
            return Err(format!("an empty array ({h}×{w}×{c})"));
        }
        if h > MAX_SIDE || w > MAX_SIDE || h.saturating_mul(w).saturating_mul(c) > MAX_VALUES {
            return Err(format!("{h}×{w}×{c} is too large (at most {MAX_SIDE} a side, {MAX_VALUES} values)"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrays_come_in_as_f32_and_go_out_with_their_metadata() {
        let a =
            Array::new("|u1", vec![2, 2, 3], (0u8..12).collect::<Vec<u8>>()).with_space(Some("RGB".into()), vec!["red".into(), "green".into(), "blue".into()]);
        let i = Img::from_array(&a).unwrap();
        assert_eq!((i.h, i.w, i.c, i.labels), (2, 2, 3, true));
        assert_eq!(i.plane(1), vec![1.0, 4.0, 7.0, 10.0]);
        let back = Img { labels: false, ..i.clone() }.to_array();
        assert_eq!((back.dtype.as_str(), back.shape.as_slice(), back.space.as_deref()), ("<f4", &[2u64, 2, 3][..], Some("RGB")));
        let labels = i.to_array();
        assert_eq!(labels.dtype, "<i4");
        let stack = Img::from_planes(1, 2, &[vec![1.0, 2.0], vec![3.0, 4.0]]);
        assert_eq!(stack.data, vec![1.0, 3.0, 2.0, 4.0]);
        assert!(Img::from_array(&Array::new("<f4", vec![1, 1, 1, 1], vec![0u8; 4])).is_err());
        assert!(Img::check_size(20_000, 1, 1).is_err());
    }
}
