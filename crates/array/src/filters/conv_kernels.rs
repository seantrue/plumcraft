//! yapocis's named 1-D convolution kernels (`yapocis.lib.convolve_kernels` driven by
//! `interfaces/convolves.yml`): the generators in float64 as numpy computes them (sums with
//! npexact's pairwise sum), stored as float32.

use npexact::reduce::{sum_f32, sum_f64};

/// convolves.yml: (kernel type, sizes), in the file's order (windows, smoothing, derivatives,
/// highpass, bandpass).
const TYPES: &[(&str, &[usize])] = &[
    ("hamming", &[5, 9, 13, 17, 25]),
    ("hann", &[5, 9, 13, 17, 25]),
    ("blackman", &[7, 11, 15, 21]),
    ("bartlett", &[5, 9, 13, 17]),
    ("box", &[3, 5, 7, 9, 11, 15, 21]),
    ("triangle", &[3, 5, 7, 9, 11]),
    ("binomial", &[3, 5, 7, 9]),
    ("deriv1", &[3, 5, 7]),
    ("deriv2", &[3, 5, 7]),
    ("sobel", &[3]),
    ("prewitt", &[3]),
    ("sharpen", &[3, 5]),
    ("unsharp", &[3, 5, 7]),
    ("dog", &[5, 7, 9, 11]),
];

/// convolves.yml's custom kernels: (name, weights, normalise by the sum of |weights|).
const CUSTOM: &[(&str, &[f32], bool)] = &[
    ("edge3", &[-1.0, 0.0, 1.0], false),
    ("edge5", &[-1.0, -0.5, 0.0, 0.5, 1.0], false),
    ("savgol5", &[-3.0, 12.0, 17.0, 12.0, -3.0], true),
    ("savgol7", &[-2.0, 3.0, 6.0, 7.0, 6.0, 3.0, -2.0], true),
];

/// Every kernel name, in the research software's order (`convolution_names`).
pub fn names() -> Vec<String> {
    let mut v: Vec<String> = TYPES.iter().flat_map(|(t, sizes)| sizes.iter().map(move |s| format!("{t}{s}"))).collect();
    v.extend(CUSTOM.iter().map(|(n, _, _)| (*n).to_owned()));
    v
}

fn normalised(w: Vec<f64>) -> Vec<f64> {
    let s = sum_f64(&w);
    w.iter().map(|v| v / s).collect()
}

/// A window of `f(n, size - 1)`, normalised to sum to 1.
fn cosine_window(size: usize, f: impl Fn(f64, f64) -> f64) -> Vec<f64> {
    let m = (size - 1) as f64;
    normalised((0..size).map(|n| f(n as f64, m)).collect())
}

const PI: f64 = std::f64::consts::PI;

fn bartlett(size: usize) -> Vec<f64> {
    let half = (size - 1) as f64 / 2.0;
    normalised((0..size).map(|n| 1.0 - ((n as f64 - half) / half).abs()).collect())
}

fn deriv1(size: usize) -> Vec<f64> {
    match size {
        5 => [1.0, -8.0, 0.0, 8.0, -1.0].iter().map(|v| v / 12.0).collect(),
        7 => [-1.0, 9.0, -45.0, 0.0, 45.0, -9.0, 1.0].iter().map(|v| v / 60.0).collect(),
        _ => vec![-0.5, 0.0, 0.5],
    }
}

/// `generate_kernel(type, size)`, float64.
fn generate(kind: &str, size: usize) -> Option<Vec<f64>> {
    Some(match kind {
        // as numpy evaluates `2 * np.pi * n / (size - 1)`, left to right
        "hamming" => cosine_window(size, |n, m| 0.54 - 0.46 * (2.0 * PI * n / m).cos()),
        "hann" => cosine_window(size, |n, m| 0.5 * (1.0 - (2.0 * PI * n / m).cos())),
        "blackman" => cosine_window(size, |n, m| 0.42 - 0.5 * (2.0 * PI * n / m).cos() + 0.08 * (4.0 * PI * n / m).cos()),
        "bartlett" => bartlett(size),
        "box" => vec![1.0 / size as f64; size],
        "triangle" => normalised(bartlett(size)),
        "binomial" => normalised(match size {
            3 => vec![1.0, 2.0, 1.0],
            5 => vec![1.0, 4.0, 6.0, 4.0, 1.0],
            7 => vec![1.0, 6.0, 15.0, 20.0, 15.0, 6.0, 1.0],
            _ => vec![1.0, 8.0, 28.0, 56.0, 70.0, 56.0, 28.0, 8.0, 1.0],
        }),
        "deriv1" => deriv1(size),
        "deriv2" => match size {
            5 => [-1.0, 16.0, -30.0, 16.0, -1.0].iter().map(|v| v / 12.0).collect(),
            7 => [2.0, -27.0, 270.0, -490.0, 270.0, -27.0, 2.0].iter().map(|v| v / 180.0).collect(),
            _ => vec![1.0, -2.0, 1.0],
        },
        "sobel" | "prewitt" => {
            if size == 3 {
                vec![-1.0, 0.0, 1.0]
            } else {
                deriv1(size)
            }
        }
        "sharpen" => {
            if size == 5 {
                vec![-1.0, -1.0, 5.0, -1.0, -1.0]
            } else {
                vec![-1.0, 3.0, -1.0]
            }
        }
        "unsharp" => {
            let mut k = vec![-(1.0 / size as f64); size];
            if let Some(c) = k.get_mut(size / 2) {
                *c += 2.0;
            }
            k
        }
        "dog" => {
            let n = match size {
                7 => 7,
                9 => 9,
                11 => 11,
                _ => 5,
            };
            let centre = (n - 1) as f64;
            (0..n)
                .map(|i| {
                    let v = if i == n / 2 { centre } else { -1.0 };
                    v / centre
                })
                .collect()
        }
        _ => return None,
    })
}

/// A named kernel's float32 weights.
pub fn kernel(name: &str) -> Option<Vec<f32>> {
    if let Some((_, w, norm)) = CUSTOM.iter().find(|(n, _, _)| *n == name) {
        if !norm {
            return Some(w.to_vec());
        }
        let abs: Vec<f32> = w.iter().map(|v| v.abs()).collect();
        let s = sum_f32(&abs);
        return Some(w.iter().map(|v| v / s).collect());
    }
    for (kind, sizes) in TYPES {
        if let Some(size) = name.strip_prefix(kind).and_then(|s| s.parse::<usize>().ok())
            && sizes.contains(&size)
        {
            return generate(kind, size).map(|w| w.iter().map(|v| *v as f32).collect());
        }
    }
    None
}
