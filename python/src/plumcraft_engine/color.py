"""Colour spaces on channel planes, with RGB (0..1 floats) as the hub: a numpy port of the Rust
SDK's `crates/engine/src/color.rs`, which ports impressionist's `virtual_impressionist/colorspaces.py`
and everything it calls. Same spaces, same conventions, same float32 arithmetic step by step, held
to the same fixture (`crates/engine/tests/fixtures/color.json`, impressionist's own output). A colour
space is optional everywhere: an array without one is plain numbers.

Spaces (see the Rust module for the full notes):

- `RGB`: red, green, blue, 0..1.
- `HSI`: yapocis's `hsi` kernel (the GLSL "IQ" RGB->HSV) as impressionist calls it: the forward
  kernel reads the pixel as (r, b, g); hue a fraction of a turn; s and i clamped to 0..1.
- `IHSL`: yapocis_external's `ihsl` kernel (Hanbury): hue in turns from the acos formula,
  saturation max - min, BT.709 luminance; a grey gets hue 0.5; the inverse's four-digit matrix.
- `HSV`, `HLS`: OpenCV's float conventions, hue in degrees 0..360.
- `Lab`: OpenCV's forward 33^3 fixed-point table with trilinear interpolation (L quantised to
  100/16384, a and b to 1/64), input clipped to 0..1; the inverse OpenCV's float formula with its
  1024-knot sRGB spline.
- `Luv`: OpenCV's float paths: gamma splines, cube-root spline, D65.
- `YCrCb`: OpenCV's float coefficients and 0.5 delta.
- `XYZ`, `xyY`: yapocis colormath's `srgb_d65` matrices with sRGB decode/encode; black (any
  exact zero sum X+Y+Z) has no chromaticity: x and y are 0 there and Y is kept, as in the kernel
  (yapocis3 #66).
"""

from __future__ import annotations

import functools
import math

import numpy as np

SPACES = ("RGB", "HSI", "IHSL", "HSV", "HLS", "Lab", "Luv", "YCrCb", "XYZ", "xyY")

F = np.float32


def _q(a: float, b: float) -> np.float32:
    """`a / b` divided in float32, as a Rust f32 constant expression is."""
    return F(a) / F(b)


EPS = F(np.finfo(np.float32).eps)


def to_rgb(space: str, planes: list[np.ndarray]) -> list[np.ndarray] | None:
    """Three planes in `space` -> RGB planes (not clipped); None for a space this doesn't know or
    planes that aren't three of one shape."""
    p = _three(planes)
    if p is None:
        return None
    if space == "RGB":
        return list(p)
    f = {
        "HSI": _hsi_to_rgb,
        "IHSL": _ihsl_to_rgb,
        "HSV": _hsv_to_rgb,
        "HLS": _hls_to_rgb,
        "Lab": _lab_to_rgb,
        "Luv": _luv_to_rgb,
        "YCrCb": _ycrcb_to_rgb,
        "XYZ": _xyz_to_rgb,
        "xyY": lambda a, b, c: _xyz_to_rgb(*_xyy_to_xyz(a, b, c)),
    }.get(space)
    return None if f is None else _run(f, p)


def from_rgb(space: str, planes: list[np.ndarray]) -> list[np.ndarray] | None:
    """RGB planes -> the three planes of `space`; None as for `to_rgb`."""
    p = _three(planes)
    if p is None:
        return None
    if space == "RGB":
        return list(p)
    f = {
        "HSI": _rgb_to_hsi,
        "IHSL": _rgb_to_ihsl,
        "HSV": _rgb_to_hsv,
        "HLS": _rgb_to_hls,
        "Lab": _rgb_to_lab,
        "Luv": _rgb_to_luv,
        "YCrCb": _rgb_to_ycrcb,
        "XYZ": _rgb_to_xyz,
        "xyY": lambda a, b, c: _xyz_to_xyy(*_rgb_to_xyz(a, b, c)),
    }.get(space)
    return None if f is None else _run(f, p)


def _three(planes: list) -> list[np.ndarray] | None:
    if not isinstance(planes, list | tuple) or len(planes) != 3:
        return None
    p = [np.asarray(x, dtype=np.float32) for x in planes]
    return p if p[0].shape == p[1].shape == p[2].shape else None


def _run(f, p: list[np.ndarray]) -> list[np.ndarray]:
    shape = p[0].shape
    with np.errstate(all="ignore"):
        out = f(*(x.ravel() for x in p))
    return [np.asarray(o, dtype=np.float32).reshape(shape) for o in out]


def _fmax(a, b):
    """Rust/OpenCL `max`: NaN loses."""
    return np.fmax(a, b)


def _fmin(a, b):
    return np.fmin(a, b)


def _clamp(x, lo, hi):
    """OpenCL `clamp`: fmin(fmax(x, lo), hi), so NaN becomes lo."""
    return np.minimum(np.fmax(x, F(lo)), F(hi)).astype(np.float32)


# ---------- HSI: yapocis hsi kernel via rgb2hsi3 / hsi2rgb3 ----------

HSI_EPS = F(1.0e-10)
_FRACT_MAX = np.frombuffer(np.uint32(0x3F7FFFFF).tobytes(), dtype=np.float32)[0]


def _rgb_to_hsi(r, g, b):
    c0, c1, c2 = r, b, g  # rgb2hsi3 reads (r, b, g)
    swap = c1 < c2
    p0 = np.where(swap, c2, c1)
    p1 = np.where(swap, c1, c2)
    p2 = np.where(swap, F(-1.0), F(0.0))
    p3 = np.where(swap, _q(2, 3), _q(-1, 3))
    lt = c0 < p0
    q0 = np.where(lt, p0, c0)
    q1 = p1
    q2 = np.where(lt, p3, p2)
    q3 = np.where(lt, c0, p0)
    d = q0 - _fmin(q3, q1)
    h = q2 + (q3 - q1) / (F(6.0) * d + HSI_EPS)
    h = np.where(h < 0, -h, h)
    s = d / (q0 + HSI_EPS)
    top = F(1.0) - HSI_EPS
    return _clamp(h, 0.0, top), _clamp(s, 0.0, top), _clamp(q0, 0.0, top)


def _fract(x):
    return np.minimum(x - np.floor(x), _FRACT_MAX)


def _hsi_to_rgb(h, s, i):
    out = []
    for k in (F(1.0), _q(1, 3), _q(2, 3)):
        p = np.abs(_fract(h + k) * F(6.0) - F(3.0))
        t = _clamp(p - F(1.0), 0.0, 1.0)
        out.append(i * (F(1.0) + (t - F(1.0)) * s))
    return out


# ---------- IHSL: yapocis_external ihsl kernel ----------


def _acospi(x):
    return (np.arccos(x.astype(np.float64)) / math.pi).astype(np.float32)


def _sinpi(x):
    return np.sin(np.asarray(x, dtype=np.float64) * math.pi).astype(np.float32)


def _cospi(x):
    return np.cos(np.asarray(x, dtype=np.float64) * math.pi).astype(np.float32)


def _rgb_to_ihsl(r, g, b):
    mx = _fmax(_fmax(r, g), b)
    mn = _fmin(_fmin(r, g), b)
    y = (F(0.2126) * r) + (F(0.7152) * g) + (F(0.0722) * b)
    s = mx - mn
    num = r - (F(0.5) * g) - (F(0.5) * b)
    den = np.sqrt(((r * r) + (g * g) + (b * b)) - ((r * g) + (r * b) + (b * g)))
    hp = _acospi(_clamp(num / den, -1.0, 1.0))
    h = np.where(b > g, F(1.0) - F(0.5) * hp, F(0.5) * hp)
    bad = ~np.isfinite(h)
    return np.where(bad, F(0.0), h), np.where(bad, F(0.0), s), y


def _ihsl_to_rgb(h, s, y):
    finite = np.isfinite(h)
    hh = np.where(finite, h, F(0.0))
    hstar = hh - np.floor(hh * F(6.0)) / F(6.0)
    chroma = (F(1.7320508) * s) / (F(2.0) * _sinpi(_q(2, 3) - F(2.0) * hstar))
    c1 = np.where(finite, chroma * _cospi(F(2.0) * hh), F(0.0))
    c2 = np.where(finite, -chroma * _sinpi(F(2.0) * hh), F(0.0))
    r = y + (F(0.7875) * c1) + (F(0.3714) * c2)
    g = y + (F(-0.2125) * c1) + (F(-0.2059) * c2)
    b = y + (F(-0.2125) * c1) + (F(0.9488) * c2)
    grey = s <= EPS
    return np.where(grey, y, r), np.where(grey, y, g), np.where(grey, y, b)


# ---------- HSV, HLS, YCrCb: OpenCV float paths ----------


def _rgb_to_hsv(r, g, b):
    v = _fmax(_fmax(r, g), b)
    vmin = _fmin(_fmin(r, g), b)
    diff = v - vmin
    s = diff / (np.abs(v) + EPS)
    d = F(60.0) / (diff + EPS)
    h = np.where(v == r, (g - b) * d, np.where(v == g, (b - r) * d + F(120.0), (r - g) * d + F(240.0)))
    h = np.where(h < 0, h + F(360.0), h)
    return h, s, v


# OpenCV's sector table: which of tab[0..4] is b, g, r in each hue sector.
_SECTORS = np.array([[1, 3, 0], [1, 0, 2], [3, 0, 1], [0, 2, 1], [0, 1, 3], [2, 1, 0]])


def _sector_rgb(tab: np.ndarray, sector: np.ndarray):
    idx = _SECTORS[sector]  # (n, 3): indices of b, g, r
    pick = lambda k: np.take_along_axis(tab, idx[:, k : k + 1], axis=1)[:, 0]  # noqa: E731
    return pick(2), pick(1), pick(0)


def _sector(h6):
    """h in sector units: (sector index, fraction), as OpenCV's `fmod`/`floor` and its guard."""
    h = np.fmod(h6, F(6.0))
    h = np.where(h < 0, h + F(6.0), h)
    sector = np.floor(h)
    h = h - sector
    bad = ~((sector >= 0) & (sector < 6))
    sector = np.where(bad, F(0.0), sector)
    h = np.where(bad, F(0.0), h)
    return np.nan_to_num(sector).astype(np.int64), h


def _hsv_to_rgb(h, s, v):
    sector, f = _sector(h * _q(6, 360))
    tab = np.stack([v, v * (F(1.0) - s), v * (F(1.0) - s * f), v * (F(1.0) - s * (F(1.0) - f))], axis=1)
    r, g, b = _sector_rgb(tab, sector)
    grey = s == 0
    return np.where(grey, v, r), np.where(grey, v, g), np.where(grey, v, b)


def _rgb_to_hls(r, g, b):
    vmax = _fmax(_fmax(r, g), b)
    vmin = _fmin(_fmin(r, g), b)
    diff = vmax - vmin
    l = (vmax + vmin) * F(0.5)  # noqa: E741
    colour = diff > EPS
    s = np.where(l < F(0.5), diff / (vmax + vmin), diff / (F(2.0) - vmax - vmin))
    d = F(60.0) / diff
    h = np.where(vmax == r, (g - b) * d, np.where(vmax == g, (b - r) * d + F(120.0), (r - g) * d + F(240.0)))
    h = np.where(h < 0, h + F(360.0), h)
    return np.where(colour, h, F(0.0)), l, np.where(colour, s, F(0.0))


def _hls_to_rgb(h, l, s):  # noqa: E741
    p2 = np.where(l <= F(0.5), l * (F(1.0) + s), l + s - l * s)
    p1 = F(2.0) * l - p2
    h6 = h * _q(6, 360)
    h6 = np.where(np.isfinite(h6), h6, F(0.0))
    sector, f = _sector(h6)
    tab = np.stack([p2, p1, p1 + (p2 - p1) * (F(1.0) - f), p1 + (p2 - p1) * f], axis=1)
    r, g, b = _sector_rgb(tab, sector)
    grey = s == 0
    return np.where(grey, l, r), np.where(grey, l, g), np.where(grey, l, b)


def _rgb_to_ycrcb(r, g, b):
    y = r * F(0.299) + g * F(0.587) + b * F(0.114)
    return y, (r - y) * F(0.713) + F(0.5), (b - y) * F(0.564) + F(0.5)


def _ycrcb_to_rgb(y, cr, cb):
    b = y + (cb - F(0.5)) * F(1.773)
    g = y + (cb - F(0.5)) * F(-0.344) + (cr - F(0.5)) * F(-0.714)
    r = y + (cr - F(0.5)) * F(1.403)
    return r, g, b


# ---------- Lab, Luv: OpenCV color_lab.cpp float paths, D65 ----------

SRGB2XYZ_D65 = (0.412453, 0.357580, 0.180423, 0.212671, 0.715160, 0.072169, 0.019334, 0.119193, 0.950227)
XYZ2SRGB_D65 = (3.240479, -1.53715, -0.498535, -0.969256, 1.875991, 0.041556, 0.055648, -0.204043, 1.057311)
D65 = (0.950456, 1.0, 1.088754)
GAMMA_TAB_SIZE = 1024
LAB_CBRT_TAB_SIZE = 1024
LAB_LUT_DIM = 33
LAB_BASE = F(16384.0)
LTHRESH = _q(216, 24389)
LSCALE = _q(841, 108)
LBIAS = _q(16, 116)
CBRT_TAB_SCALE = F(LAB_CBRT_TAB_SIZE * 2) / F(3.0)


def _apply_gamma(x: np.ndarray) -> np.ndarray:
    """OpenCV `applyGamma` (sRGB decode) in double, rounded to float."""
    x = np.asarray(x, dtype=np.float64)
    v = np.where(x <= 809.0 / 20000.0, x / (323.0 / 25.0), ((x + 11.0 / 200.0) / (1.0 + 11.0 / 200.0)) ** 2.4)
    return v.astype(np.float32)


def _apply_inv_gamma(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, dtype=np.float64)
    with np.errstate(invalid="ignore"):
        v = np.where(x <= 7827.0 / 2_500_000.0, x * (323.0 / 25.0), x ** (1.0 / 2.4) * (1.0 + 11.0 / 200.0) - 11.0 / 200.0)
    return v.astype(np.float32)


def _spline_build(f: np.ndarray, n: int) -> np.ndarray:
    """OpenCV `splineBuild` in float32, step by step as the Rust port."""
    f = [F(v) for v in f]
    tab = [F(0.0)] * (n * 4)
    for i in range(1, n):
        t = (f[i + 1] - f[i] * F(2.0) + f[i - 1]) * F(3.0)
        l = F(1.0) / (F(4.0) - tab[(i - 1) * 4])  # noqa: E741
        m = (t - tab[(i - 1) * 4 + 1]) * l
        tab[i * 4] = l
        tab[i * 4 + 1] = m
    cn = F(0.0)
    for i in range(n - 1, -1, -1):
        c = tab[i * 4 + 1] - tab[i * 4] * cn
        b = f[i + 1] - f[i] - (cn + c * F(2.0)) / F(3.0)
        d = (cn - c) / F(3.0)
        tab[i * 4 : i * 4 + 4] = [f[i], b, c, d]
        cn = c
    return np.array(tab, dtype=np.float32)


def _spline(x: np.ndarray, tab: np.ndarray, n: int) -> np.ndarray:
    """OpenCV `splineInterpolate`: x truncated toward zero (NaN reads as 0), clamped to 0..n-1."""
    t = np.trunc(np.nan_to_num(x, nan=0.0, posinf=float(n), neginf=0.0).astype(np.float64))
    ix = np.clip(t, 0, n - 1).astype(np.int64)
    x = x - ix.astype(np.float32)
    k = ix * 4
    a, b, c, d = tab[k], tab[k + 1], tab[k + 2], tab[k + 3]
    return ((d * x + c) * x + b) * x + a


@functools.cache
def _tables() -> dict:
    scale = F(GAMMA_TAB_SIZE)
    i = np.arange(GAMMA_TAB_SIZE + 1, dtype=np.float32) / scale
    gamma = _spline_build(_apply_gamma(i), GAMMA_TAB_SIZE)
    inv_gamma = _spline_build(_apply_inv_gamma(i), GAMMA_TAB_SIZE)
    x = (F(1.0) / CBRT_TAB_SCALE) * np.arange(LAB_CBRT_TAB_SIZE + 1, dtype=np.float32)
    lin = (x.astype(np.float64) * float(LSCALE) + float(LBIAS)).astype(np.float32)
    c = np.where(x < LTHRESH, lin, np.cbrt(x.astype(np.float64)).astype(np.float32))
    return {"gamma": gamma, "inv_gamma": inv_gamma, "cbrt": _spline_build(c, LAB_CBRT_TAB_SIZE), "lab_lut": _lab_lut()}


def _lab_lut() -> np.ndarray:
    """OpenCV `initLabTabs`' RGB2Labprev grid: (L, a, b) int16 per grid point, indexed
    (p + q*33 + r*33*33) with p, q, r the R, G, B steps."""
    s = [F(SRGB2XYZ_D65[k] / D65[k // 3]) for k in range(9)]

    def f(v):
        lin = (v.astype(np.float64) * float(LSCALE) + float(LBIAS)).astype(np.float32)
        return np.where(v > LTHRESH, np.cbrt(v.astype(np.float64)).astype(np.float32), lin)

    dim = LAB_LUT_DIM
    steps = _apply_gamma(np.arange(dim, dtype=np.float32) / F(dim - 1))
    r_, q_, p_ = np.meshgrid(np.arange(dim), np.arange(dim), np.arange(dim), indexing="ij")
    rr, gg, bb = steps[p_], steps[q_], steps[r_]
    x = rr * s[0] + gg * s[1] + bb * s[2]
    y = rr * s[3] + gg * s[4] + bb * s[5]
    z = rr * s[6] + gg * s[7] + bb * s[8]
    fx, fy, fz = f(x), f(y), f(z)
    l = np.where(y > LTHRESH, F(116.0) * fy - F(16.0), y * _q(24389, 27))  # noqa: E741
    a = F(500.0) * (fx - fy)
    b = F(200.0) * (fy - fz)
    v = np.stack([LAB_BASE * l / F(100.0), LAB_BASE * (a + F(128.0)) / F(256.0), LAB_BASE * (b + F(128.0)) / F(256.0)], axis=-1)
    return np.rint(v).astype(np.int16).reshape(-1)  # index ((r*dim + q)*dim + p)*3 + k


def _clip01(v):
    """OpenCV's clip (std::min(std::max(v, 0), 1)), which keeps NaN."""
    return np.where(np.isnan(v), v, np.clip(v, F(0.0), F(1.0)))


def _rgb_to_lab(r, g, b):
    lut = _tables()["lab_lut"].astype(np.int64)
    c = [np.nan_to_num(np.rint(_clip01(v) * LAB_BASE), nan=0.0).astype(np.int64) for v in (r, g, b)]
    cell = [v >> 9 for v in c]
    frac = [(v >> 5) & 15 for v in c]
    dim = LAB_LUT_DIM
    strides = (1, dim, dim * dim)
    acc = [np.zeros_like(c[0]) for _ in range(3)]
    for corner in range(8):
        d = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1)
        w = np.ones_like(c[0])
        idx = np.zeros_like(c[0])
        for k in range(3):
            w = w * (frac[k] if d[k] else 16 - frac[k])
            idx = idx + np.minimum(cell[k] + d[k], dim - 1) * strides[k]
        for k in range(3):
            acc[k] = acc[k] + lut[idx * 3 + k] * w
    il, ia, ib = [((v + (1 << 11)) >> 12).astype(np.float32) for v in acc]
    inv = F(1.0) / LAB_BASE
    return (il * inv) * F(100.0), (ia * inv) * F(256.0) - F(128.0), (ib * inv) * F(256.0) - F(128.0)


def _encode_spline(v):
    return _spline(_clip01(v) * F(GAMMA_TAB_SIZE), _tables()["inv_gamma"], GAMMA_TAB_SIZE)


def _decode_spline(v):
    return _spline(_clip01(v) * F(GAMMA_TAB_SIZE), _tables()["gamma"], GAMMA_TAB_SIZE)


def _lab_to_rgb(li, ai, bi):
    c = [F(XYZ2SRGB_D65[k] * D65[k % 3]) for k in range(9)]
    small = li <= F(8.0)
    ys = li / F(903.3)
    fys = F(7.787) * ys + _q(16, 116)
    fyb = (li + F(16.0)) / F(116.0)
    y = np.where(small, ys, fyb * fyb * fyb)
    fy = np.where(small, fys, fyb)
    fthresh = _q(6, 29)

    def cube(f):
        return np.where(f <= fthresh, (f - _q(16, 116)) / F(7.787), f * f * f)

    x = cube(ai / F(500.0) + fy)
    z = cube(fy - bi / F(200.0))
    r = c[0] * x + c[1] * y + c[2] * z
    g = c[3] * x + c[4] * y + c[5] * z
    b = c[6] * x + c[7] * y + c[8] * z
    return _encode_spline(r), _encode_spline(g), _encode_spline(b)


def _luv_white() -> tuple[np.float32, np.float32]:
    d = 1.0 / (D65[0] + D65[1] * 15.0 + D65[2] * 3.0)
    return F(d * 52.0 * D65[0]), F(d * 117.0 * D65[1])


def _rgb_to_luv(r, g, b):
    c = [F(v) for v in SRGB2XYZ_D65]
    un, vn = _luv_white()
    r, g, b = _decode_spline(r), _decode_spline(g), _decode_spline(b)
    x = r * c[0] + g * c[1] + b * c[2]
    y = r * c[3] + g * c[4] + b * c[5]
    z = r * c[6] + g * c[7] + b * c[8]
    l = _spline(y * CBRT_TAB_SCALE, _tables()["cbrt"], LAB_CBRT_TAB_SIZE)  # noqa: E741
    l = F(116.0) * l - F(16.0)  # noqa: E741
    d = F(52.0) / _fmax(x + F(15.0) * y + F(3.0) * z, EPS)
    u = l * (x * d - un)
    v = l * (F(2.25) * y * d - vn)
    return l, u, v


def _luv_to_rgb(l, u, v):  # noqa: E741
    c = [F(x) for x in XYZ2SRGB_D65]
    un, vn = _luv_white()
    yb = (l + F(16.0)) * (F(1.0) / F(116.0))
    y = np.where(l >= F(8.0), yb * yb * yb, l * (F(1.0) / F(903.3)))
    up = F(3.0) * (u + l * un)
    vp = F(0.25) / (v + l * vn)
    vp = np.where(np.isnan(vp), vp, np.clip(vp, F(-0.25), F(0.25)))
    x = y * F(3.0) * up * vp
    z = y * ((F(156.0) * l - up) * vp - F(5.0))
    r = x * c[0] + y * c[1] + z * c[2]
    g = x * c[3] + y * c[4] + z * c[5]
    b = x * c[6] + y * c[7] + z * c[8]
    return _encode_spline(r), _encode_spline(g), _encode_spline(b)


# ---------- XYZ, xyY: yapocis colormath srgb_d65 with sRGB decode/encode ----------

RGB2XYZ = [F(v) for v in (0.4124564, 0.3575761, 0.1804375, 0.2126729, 0.7151522, 0.072175, 0.0193339, 0.119192, 0.9503041)]
XYZ2RGB = [F(v) for v in (3.2404542, -1.5371385, -0.4985314, -0.969266, 1.8760108, 0.041556, 0.0556434, -0.2040259, 1.0572252)]


def _clip0(v):
    """numpy clip(v, 0, None): NaN stays NaN."""
    return np.where(v < 0, F(0.0), v)


def _srgb_decode(v):
    v = _clip0(v)
    return np.where(v <= F(0.04045), v / F(12.92), np.power((v + F(0.055)) / F(1.055), F(2.4)))


def _srgb_encode(v):
    v = _clip0(v)
    return np.where(v <= F(0.0031308), v * F(12.92), F(1.055) * np.power(v, F(1.0 / 2.4)) - F(0.055))


def _matrix(m, a, b, c):
    return a * m[0] + b * m[1] + c * m[2], a * m[3] + b * m[4] + c * m[5], a * m[6] + b * m[7] + c * m[8]


def _rgb_to_xyz(r, g, b):
    return _matrix(RGB2XYZ, _srgb_decode(r), _srgb_decode(g), _srgb_decode(b))


def _xyz_to_rgb(x, y, z):
    return tuple(_srgb_encode(v) for v in _matrix(XYZ2RGB, x, y, z))


def _xyz_to_xyy(x, y, z):
    s = x + y + z
    zero = s == 0
    safe = np.where(zero, F(1.0), s)
    return np.where(zero, F(0.0), x / safe), np.where(zero, F(0.0), y / safe), y


def _xyy_to_xyz(x, y, big_y):
    zero = y == 0
    safe = np.where(zero, F(1.0), y)
    return np.where(zero, F(0.0), x * big_y / safe), np.where(zero, F(0.0), big_y), np.where(zero, F(0.0), ((F(1.0) - x - y) * big_y) / safe)
