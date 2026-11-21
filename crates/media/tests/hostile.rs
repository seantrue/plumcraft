//! Every value an engine sends is hostile input (spec 18, golden rule 1): the media crate's
//! decoders and its service answer junk with an error, never a panic, a hang or an allocation
//! the size a header claims. This joins the hostile-input sweep (`crates/proto/tests/hostile.rs`)
//! for what the editor decodes.

use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "spectrogram")]
use plumcraft_media::audio::spectrogram;
use plumcraft_media::audio::{Clip, Peaks, decode_inline};
use plumcraft_media::decode::{decode_numbers, decode_picture, numbers, picture_size, rendition};
use plumcraft_media::raster::{Rgba, shrink};
use plumcraft_media::{Config, Fetch, MediaService, Want};
use plumcraft_proto::data::{BlobMeta, DataRef, Form, Locator};
use plumcraft_store::Blob;

fn blobs() -> Vec<Blob> {
    let shapes: Vec<Vec<u64>> = vec![
        vec![],
        vec![0],
        vec![0, 0, 4],
        vec![1, 1, 4],
        vec![2, 3, 4],
        vec![u64::MAX, u64::MAX, 4],
        vec![u64::MAX, 2],
        vec![1 << 40, 1 << 40, 4],
        vec![4096, 4096, 4],
        vec![1, u64::MAX, 2, 2],
        vec![u64::MAX, 1, 1, 2],
        vec![1, 1, 1 << 20, 2],
        vec![3],
    ];
    let dtypes = ["|u1", "<f4", "<f8", "<i2", "|b1", "", "garbage", "<c16"];
    let payloads: Vec<Vec<u8>> =
        vec![vec![], vec![0; 3], vec![255; 24], vec![7; 4096], b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\xff\xff\xff\xff\xff\xff\xff\xff".to_vec()];
    let mut out = Vec::new();
    for s in &shapes {
        for d in dtypes {
            for p in &payloads {
                out.push(Blob { dtype: d.into(), shape: s.clone(), bytes: p.clone().into() });
            }
        }
    }
    out
}

fn files() -> Vec<Vec<u8>> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend_from_slice(&0x7fff_ffffu32.to_be_bytes());
    png.extend_from_slice(&0x7fff_ffffu32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut zero = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    zero.extend_from_slice(&[0; 8]);
    vec![
        Vec::new(),
        b"\x89PNG".to_vec(),
        png,
        zero,
        b"\xff\xd8\xff\xe0junk".to_vec(),
        b"II*\x00\x08\x00\x00\x00\xff\xff".to_vec(),
        b"GIF89a\xff\xff\xff\xff".to_vec(),
        b"RIFF\x00\x00\x00\x00WEBPVP8 ".to_vec(),
        b"<svg xmlns='http://www.w3.org/2000/svg' width='1e30' height='1e30'/>".to_vec(),
        b"<svg xmlns='http://www.w3.org/2000/svg' width='0' height='0'/>".to_vec(),
        b"<svg".to_vec(),
        vec![0xff; 70_000],
    ]
}

#[test]
fn hostile_blobs_never_panic_a_decoder() {
    for b in blobs() {
        let _ = rendition(&b);
        let _ = Peaks::from_blob(&b);
        let _ = numbers(&b);
        for rate in [0, 1, 8000, u32::MAX] {
            let _ = Clip::from_blob(&b, rate);
            // N4 slice 5: a sound read where it is, for its waveform and spectrogram
            if let Ok(s) = plumcraft_media::audio::Samples::of(&b, rate) {
                let _ = s.peaks(usize::MAX);
                let _ = s.peaks(0);
                #[cfg(feature = "spectrogram")]
                let _ = plumcraft_media::audio::spectrogram_of(&s, 1024, 256, 64);
            }
        }
        let _ = plumcraft_media::table::Table::from_bytes(&b.bytes);
    }
    // a torn picture shrinks to nothing worse than itself
    let _ = shrink(Rgba { w: u32::MAX, h: u32::MAX, pixels: vec![1; 16] }, 2);
    let _ = shrink(Rgba { w: 0, h: 0, pixels: Vec::new() }, 0);
}

/// The pictures' decoders refuse junk, and nothing they make is the size a header claims.
///
/// The picture decoders are timed: each file takes well under a millisecond alone, so 5 s is
/// reached only by believing a header (a 2³¹-pixel-wide PNG). SVGs are drawn at the size the
/// caller asks, clamped (`svg::render_size`), not at the size they claim; that clamp is what bounds
/// the work, so it is checked on the picture made, not timed (B33). At the clamp, 4096 × 4096, an
/// empty drawing takes about 0.23 s in release and 4 s unoptimised, past 5 s on a loaded machine.
#[test]
fn hostile_files_never_panic_a_decoder() {
    for f in files() {
        let t = Instant::now();
        let _ = picture_size(&f);
        assert!(decode_picture(&f, 64).is_err(), "{:?}", String::from_utf8_lossy(&f[..f.len().min(16)]));
        let _ = decode_numbers(&f);
        assert!(plumcraft_media::table::Table::from_bytes(&f).is_err(), "not a table");
        assert!(t.elapsed() < Duration::from_secs(5), "a header's claim was believed");
        #[cfg(feature = "svg")]
        for long in [1e9, f32::NAN] {
            use plumcraft_media::decode::MAX_TEXTURE_SIDE;
            use plumcraft_media::svg::{MAX_RENDER_PIXELS, rasterise};
            if let Ok(img) = rasterise(&f, long) {
                let (w, h) = (u64::from(img.w), u64::from(img.h));
                assert!(img.w <= MAX_TEXTURE_SIDE && img.h <= MAX_TEXTURE_SIDE && w * h <= MAX_RENDER_PIXELS, "an SVG's claim was believed: {w}×{h}");
                assert_eq!(img.pixels.len() as u64, w * h * 4);
            }
        }
    }
    #[cfg(feature = "spectrogram")]
    for v in [vec![], vec![f32::NAN; 5000], vec![f32::INFINITY; 2000], vec![1e38; 4096]] {
        let s = spectrogram(&v, 0, 0, 0, 0);
        let _ = s.rgba(&|t| [(t * 255.0) as u8, 0, 0, 255]);
    }
}

/// N4 slice 5: tables and the keys shells borrow by are hostile input too.
#[test]
fn hostile_tables_and_keys_are_refused() {
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let cases: Vec<Vec<u8>> = vec![
        deep.into_bytes(),
        br#"{"columns": [{"name": 1, "values": []}]}"#.to_vec(),
        br#"{"columns": [{"values": [1e999]}]}"#.to_vec(),
        br#"[[], [1], "x"]"#.to_vec(),
        format!("{{\"columns\": [{}]}}", vec![r#"{"values": []}"#; 5000].join(",")).into_bytes(),
        b"{\"columns\": [{\"values\": [\"\xff\xfe\"]}]}".to_vec(),
    ];
    for c in &cases {
        let _ = plumcraft_media::table::Table::from_bytes(c);
    }
    assert!(plumcraft_media::table::Table::from_bytes(&cases[4]).is_err(), "too many columns");
    for k in [
        "",
        "derive:",
        "derive:x:y",
        "derive:sorted:h@",
        "derive:sorted:h@9x",
        "derive:sorted:h@99999999a",
        "derive:waveform:h@0",
        "derive:image:@5",
        "derive:table:",
    ] {
        assert!(plumcraft_media::parse_derived(k).is_none_or(|(h, _)| !h.is_empty()), "{k}");
    }
    for w in [Want::Sorted(3, true), Want::Waveform(800), Want::Spectrogram, Want::Table, Want::Json, Want::Picture(512)] {
        assert_eq!(plumcraft_media::parse_derived(&plumcraft_media::derived_key("md5:raw:ab", w)), Some(("md5:raw:ab".to_owned(), w)));
    }
}

#[test]
fn hostile_references_are_answered_by_the_service() {
    let m = MediaService::new(Config { workers: 2, ..Config::default() });
    let junk = blobs();
    let n = junk.len();
    let junk = Arc::new(junk);
    let j = Arc::clone(&junk);
    m.set_resolver(Some(Arc::new(move |r: &DataRef| {
        let i = r.meta.size as usize % n;
        j.get(i).cloned()
    })));
    let forms = [
        Form::Array { dtype: "|u1".into(), shape: vec![u64::MAX, 3, 4], space: None, names: Vec::new(), table: false },
        Form::Audio { dtype: "<f4".into(), channels: u16::MAX, frames: u64::MAX, sample_rate: 0 },
        Form::Image { width: u32::MAX, height: 0 },
        Form::default(),
    ];
    let wants = [
        Want::Bytes,
        Want::Rendition,
        Want::Peaks,
        Want::Picture(0),
        Want::Picture(u32::MAX),
        Want::Svg(u32::MAX),
        Want::Numbers,
        // N4 slice 5's viewers
        Want::Waveform(0),
        Want::Waveform(u32::MAX),
        Want::Spectrogram,
        Want::Table,
        Want::Sorted(u16::MAX, true),
        Want::Sorted(0, false),
        Want::Json,
    ];
    let mut refs = Vec::new();
    for (i, form) in forms.iter().enumerate() {
        for k in 0..n {
            let meta = BlobMeta { size: k as u64, form: form.clone(), ..BlobMeta::default() };
            let at = match k % 3 {
                0 => vec![],
                1 => vec![Locator::Inline { b64: "!!not base64".into() }],
                _ => vec![Locator::Inline { b64: "AAAAAAAAAAAAAAAAAAAAAA==".into() }],
            };
            refs.push(DataRef { handle: Some(plumcraft_proto::Handle::raw(format!("{i}/{k}").as_bytes())), meta, at });
        }
    }
    let t = Instant::now();
    for r in &refs {
        for w in wants {
            let _ = m.get(r, w);
        }
    }
    assert!(t.elapsed() < Duration::from_secs(5), "asking never waits");
    assert!(m.wait_idle(Duration::from_secs(60)), "{:?}", m.stats());
    for r in &refs {
        for w in wants {
            match m.get(r, w) {
                Fetch::Pending => {} // refused while the queue was full: asked again later
                Fetch::Ready(_) | Fetch::Missing | Fetch::Failed(_) => {}
            }
        }
    }
    assert!(m.wait_idle(Duration::from_secs(60)));
    let _ = decode_inline(&refs[1]);
}
