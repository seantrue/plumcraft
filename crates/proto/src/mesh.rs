//! Triangle meshes from STL files (`model/stl`), binary or ASCII, for the mesh viewer (#38).
//!
//! Adapted from SolidCraft's `read_stl` (`crates/io/src/lib.rs` in
//! <https://github.com/tangency-labs/solidcraft>, MIT OR Apache-2.0): the file is binary when
//! its size is exactly what its triangle count says (so a binary header that starts with
//! `solid`, as some CAD exporters write, still reads as binary), else ASCII when it starts with
//! `solid` and has facets. A binary count of 0, or one larger than the file, takes the
//! triangles that are there. ASCII is read as tokens, so CRLF and odd layouts don't matter.
//!
//! Hostile input (spec 18): at most [`MAX_BYTES`] read and [`MAX_TRIANGLES`] kept, nothing
//! reserved from the header's count beyond what the bytes hold, a vertex that isn't finite (or
//! is absurdly far) drops its triangle, and nothing here panics on any bytes.

/// The largest STL read.
pub const MAX_BYTES: usize = 256 << 20;
/// The most triangles kept (a binary file of this many is 250 MB).
pub const MAX_TRIANGLES: usize = 5_000_000;
/// Coordinates at or past this are treated as corrupt.
const FAR: f32 = 1e9;

/// A triangle's three corners.
pub type Triangle = [[f32; 3]; 3];

/// An STL file's triangles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub triangles: Vec<Triangle>,
    /// It was ASCII STL.
    pub ascii: bool,
    /// The name after `solid` (ASCII), or the header's printable text (binary).
    pub name: String,
    /// Triangles dropped for a bad vertex.
    pub dropped: usize,
}

impl Mesh {
    /// The smallest box holding every corner, `(min, max)`; `None` with no triangles.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut corners = self.triangles.iter().flatten();
        let first = *corners.next()?;
        Some(corners.fold((first, first), |(lo, hi), p| (std::array::from_fn(|k| lo[k].min(p[k])), std::array::from_fn(|k| hi[k].max(p[k])))))
    }
}

/// Whether a media type names STL (`model/stl` and the older names for it).
pub fn is_stl_media(media: &str) -> bool {
    let m = media.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    matches!(m.as_str(), "model/stl" | "model/x.stl-binary" | "model/x.stl-ascii" | "application/sla" | "application/vnd.ms-pki.stl")
}

fn sane(v: &[f32; 3]) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() < FAR)
}

/// Read binary or ASCII STL (see the module docs).
pub fn read_stl(bytes: &[u8]) -> Result<Mesh, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("{} bytes is more than the mesh view reads ({} MB)", bytes.len(), MAX_BYTES >> 20));
    }
    let count = bytes.get(80..84).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let exact = count.is_some_and(|n| n.checked_mul(50).and_then(|s| s.checked_add(84)) == Some(bytes.len()));
    let text = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let start = text.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(text.len());
    let text = text.get(start..).unwrap_or_default();
    let mesh = if !exact && text.starts_with(b"solid") && has_facet(text) { read_ascii(text)? } else { read_binary(bytes, count)? };
    if mesh.triangles.is_empty() {
        return Err(if mesh.dropped > 0 { format!("every triangle ({}) has a bad vertex", mesh.dropped) } else { "the STL has no triangles".into() });
    }
    Ok(mesh)
}

fn has_facet(text: &[u8]) -> bool {
    text.windows(5).any(|w| w == b"facet")
}

fn read_binary(bytes: &[u8], count: Option<usize>) -> Result<Mesh, String> {
    let n = count.ok_or_else(|| format!("{} bytes is too short for STL", bytes.len()))?;
    // a wrong count in the header (some writers leave 0): the triangles that are there
    let room = bytes.len().saturating_sub(84) / 50;
    let n = if n == 0 || n > room { room } else { n };
    if n > MAX_TRIANGLES {
        return Err(format!("{n} triangles is more than the mesh view reads ({MAX_TRIANGLES})"));
    }
    let header = bytes.get(..80).unwrap_or_default();
    let name: String = header.iter().take_while(|b| **b != 0).map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { ' ' }).collect();
    let f = |o: usize| bytes.get(o..o + 4).map_or(f32::NAN, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let mut mesh = Mesh { name: name.trim().to_owned(), triangles: Vec::with_capacity(n), ..Mesh::default() };
    for i in 0..n {
        // the facet normal (the first 12 bytes) is ignored: the winding gives the orientation
        let o = 84 + i * 50 + 12;
        let v = |k: usize| [f(o + k * 12), f(o + k * 12 + 4), f(o + k * 12 + 8)];
        let t = [v(0), v(1), v(2)];
        if t.iter().all(sane) {
            mesh.triangles.push(t);
        } else {
            mesh.dropped += 1;
        }
    }
    Ok(mesh)
}

fn read_ascii(text: &[u8]) -> Result<Mesh, String> {
    let first_line = text.split(|b| *b == b'\n').next().unwrap_or_default();
    let name = String::from_utf8_lossy(first_line.get(5..).unwrap_or_default()).trim().to_owned();
    let mut mesh = Mesh { ascii: true, name, ..Mesh::default() };
    let mut tokens = text.split(|b| b.is_ascii_whitespace()).filter(|t| !t.is_empty());
    let mut corners: Vec<[f32; 3]> = Vec::with_capacity(3);
    let mut bad = false;
    while let Some(t) = tokens.next() {
        match t {
            b"vertex" => {
                let mut v = [0.0f32; 3];
                for x in &mut v {
                    match tokens.next().and_then(|t| std::str::from_utf8(t).ok()).and_then(|s| s.parse::<f32>().ok()) {
                        Some(n) => *x = n,
                        None => bad = true,
                    }
                }
                bad |= !sane(&v) || corners.len() >= 3;
                corners.push(v);
            }
            // each facet takes exactly its three corners; a bad one drops the facet
            b"endfacet" => {
                match corners[..] {
                    [a, b, c] if !bad => mesh.triangles.push([a, b, c]),
                    _ => mesh.dropped += 1,
                }
                corners.clear();
                bad = false;
                if mesh.triangles.len() > MAX_TRIANGLES {
                    return Err(format!("more than {MAX_TRIANGLES} triangles, the most the mesh view reads"));
                }
            }
            _ => {}
        }
    }
    Ok(mesh)
}

/// Binary STL of `triangles` (normals zero), for tests and examples.
pub fn write_binary_stl(header: &str, triangles: &[Triangle]) -> Vec<u8> {
    let mut out: Vec<u8> = header.bytes().chain(std::iter::repeat(0)).take(80).collect();
    out.extend((triangles.len() as u32).to_le_bytes());
    for t in triangles {
        out.extend([0u8; 12]);
        for v in t.iter().flatten() {
            out.extend(v.to_le_bytes());
        }
        out.extend([0u8; 2]);
    }
    out
}

/// ASCII STL of `triangles`, for tests and examples.
pub fn write_ascii_stl(name: &str, triangles: &[Triangle]) -> String {
    let mut s = format!("solid {name}\n");
    for t in triangles {
        s.push_str("  facet normal 0 0 0\n    outer loop\n");
        for v in t {
            s.push_str(&format!("      vertex {} {} {}\n", v[0], v[1], v[2]));
        }
        s.push_str("    endloop\n  endfacet\n");
    }
    s.push_str(&format!("endsolid {name}\n"));
    s
}

/// A unit tetrahedron, for tests and examples.
pub fn tetrahedron() -> Vec<Triangle> {
    let (o, x, y, z) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    vec![[o, y, x], [o, x, z], [o, z, y], [x, y, z]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_and_ascii_read_the_same_triangles() {
        let tris = tetrahedron();
        let b = read_stl(&write_binary_stl("tetra", &tris)).unwrap();
        assert_eq!((b.triangles.clone(), b.ascii, b.name.as_str()), (tris.clone(), false, "tetra"));
        let a = read_stl(write_ascii_stl("tetra", &tris).as_bytes()).unwrap();
        assert_eq!((a.triangles, a.ascii, a.name.as_str()), (tris, true, "tetra"));
        assert_eq!(b.bounds(), Some(([0.0; 3], [1.0; 3])));
    }

    #[test]
    fn a_binary_header_that_says_solid_is_still_binary() {
        let bytes = write_binary_stl("solid made by a CAD exporter, facets follow", &tetrahedron());
        let m = read_stl(&bytes).unwrap();
        assert!(!m.ascii);
        assert_eq!(m.triangles.len(), 4);
    }

    #[test]
    fn crlf_tabs_and_a_byte_order_mark_are_fine() {
        let s = format!("\u{feff}{}", write_ascii_stl("x", &tetrahedron()).replace('\n', "\r\n").replace("  ", "\t"));
        assert_eq!(read_stl(s.as_bytes()).unwrap().triangles.len(), 4);
    }

    #[test]
    fn a_wrong_count_takes_the_triangles_there_are() {
        let mut bytes = write_binary_stl("", &tetrahedron());
        for count in [0u32, 3, u32::MAX] {
            bytes[80..84].copy_from_slice(&count.to_le_bytes());
            let want = if count == 3 { 3 } else { 4 };
            assert_eq!(read_stl(&bytes).unwrap().triangles.len(), want, "count {count}");
        }
        // a truncated file: the whole triangles
        bytes.truncate(84 + 50 * 2 + 17);
        bytes[80..84].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(read_stl(&bytes).unwrap().triangles.len(), 2);
    }

    #[test]
    fn bad_vertices_drop_their_triangle() {
        let mut tris = tetrahedron();
        tris[1][2] = [f32::NAN, 0.0, 0.0];
        tris[2][0] = [2e9, 0.0, 0.0];
        let m = read_stl(&write_binary_stl("", &tris)).unwrap();
        assert_eq!((m.triangles.len(), m.dropped), (2, 2));
        let ascii = write_ascii_stl("", &tetrahedron()).replacen("vertex 0 0 0", "vertex 0 zero 0", 1);
        let m = read_stl(ascii.as_bytes()).unwrap();
        assert_eq!((m.triangles.len(), m.dropped), (3, 1));
        let all_bad = vec![[[f32::INFINITY; 3]; 3]; 3];
        assert!(read_stl(&write_binary_stl("", &all_bad)).unwrap_err().contains("bad vertex"));
    }

    #[test]
    fn nothing_to_read_is_an_error() {
        for bytes in [&b""[..], b"solid", b"solid x\nendsolid x\n", &[0u8; 83], &[0u8; 84]] {
            assert!(read_stl(bytes).is_err(), "{bytes:?}");
        }
        assert!(read_stl(&vec![0u8; MAX_BYTES + 1]).is_err());
    }

    #[test]
    fn stl_media_types() {
        assert!(is_stl_media("model/stl") && is_stl_media("Model/STL; x=1") && is_stl_media("application/sla"));
        assert!(!is_stl_media("model/obj") && !is_stl_media(""));
    }
}
