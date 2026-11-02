//! Random Points and Mask Points: numpy's draws (`np.random.default_rng(seed)`) through npexact,
//! so the points are the ones the research software makes.

use npexact::random::Generator;
use plumcraft_engine::{Cx, Inputs, Outputs};
use plumcraft_proto::error::NodeError;

use super::{as_channel, check_table, seed, size_of, table};
use crate::op::{config, f, img, n, one, s};

/// `np.column_stack([rng.uniform(0, w - 1, count), rng.uniform(0, h - 1, count)])` with
/// `rng = np.random.default_rng(seed)`, then `extra` appended to every row; float32, row-major.
pub fn random_points_of(h: usize, w: usize, count: usize, seed: u128, extra: &[f64]) -> Vec<f32> {
    let mut rng = Generator::default_rng(&[seed]);
    let xs = rng.uniform(0.0, w as f64 - 1.0, count);
    let ys = rng.uniform(0.0, h as f64 - 1.0, count);
    let mut out = Vec::with_capacity(count * (2 + extra.len()));
    for (x, y) in xs.into_iter().zip(ys) {
        out.push(x as f32);
        out.push(y as f32);
        out.extend(extra.iter().map(|v| *v as f32));
    }
    out
}

/// The numbers in `text`, separated by spaces or commas (Python's `float` of each).
fn numbers(text: &str) -> Result<Vec<f64>, String> {
    text.replace(',', " ").split_whitespace().map(|t| t.parse::<f64>().map_err(|_| format!("extra: {t:?} is not a number"))).collect()
}

pub(super) fn random_points(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let (h, w) = size_of(&img(i, "like")?);
    let count = n(i, "count", 200).clamp(0, 10_000_000) as usize;
    let extra = numbers(&s(i, "extra", "")).map_err(config)?;
    check_table(count, 2 + extra.len())?;
    let names = ["x".to_owned(), "y".to_owned()].into_iter().chain((0..extra.len()).map(|k| format!("c{}", k + 3))).collect();
    Ok(one("points", table(random_points_of(h, w, count, seed(i), &extra), names)))
}

/// The (x, y) of the pixels above 0.5, in scan order, each kept when the next of
/// `np.random.default_rng(seed).random(len)` is below `fraction`; float32, row-major.
pub fn mask_points_of(mask: &[f32], w: usize, fraction: f64, seed: u128) -> Vec<f32> {
    let on: Vec<usize> = mask.iter().enumerate().filter(|(_, v)| **v > 0.5).map(|(k, _)| k).collect();
    let draws = Generator::default_rng(&[seed]).random_f64(on.len());
    let w = w.max(1);
    on.into_iter().zip(draws).filter(|(_, d)| *d < fraction).flat_map(|(k, _)| [(k % w) as f32, (k / w) as f32]).collect()
}

pub(super) fn mask_points(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    let mask = img(i, "mask")?;
    let p = as_channel(&mask, "mask")?;
    let fraction = f(i, "fraction", 1.0);
    let names = vec!["x".to_owned(), "y".to_owned()];
    Ok(one("points", table(mask_points_of(&p, mask.w, fraction, seed(i)), names)))
}
