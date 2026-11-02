//! Media types and file extensions, both ways: the name a value's file gets when it is written
//! or opened elsewhere (`application/pdf` → `pdf`), and the media type a file read by name has.

/// (media type, extension). The first row for a media type gives its extension; the first row
/// for an extension gives its media type.
const TABLE: &[(&str, &str)] = &[
    ("application/pdf", "pdf"),
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/jpeg", "jpeg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
    ("image/tiff", "tiff"),
    ("image/tiff", "tif"),
    ("image/bmp", "bmp"),
    ("image/svg+xml", "svg"),
    ("image/vnd.adobe.photoshop", "psd"),
    ("image/vnd.dwg", "dwg"),
    ("image/vnd.dxf", "dxf"),
    ("video/mp4", "mp4"),
    ("video/quicktime", "mov"),
    ("video/webm", "webm"),
    ("audio/wav", "wav"),
    ("audio/x-wav", "wav"),
    ("audio/wave", "wav"),
    ("audio/aiff", "aiff"),
    ("audio/x-aiff", "aiff"),
    ("audio/aiff", "aif"),
    ("audio/flac", "flac"),
    ("audio/mpeg", "mp3"),
    ("audio/midi", "mid"),
    ("model/stl", "stl"),
    ("model/3mf", "3mf"),
    ("model/obj", "obj"),
    ("model/step", "step"),
    ("model/gltf-binary", "glb"),
    ("text/csv", "csv"),
    ("text/html", "html"),
    ("text/markdown", "md"),
    ("text/plain", "txt"),
    ("application/json", "json"),
    ("application/zip", "zip"),
    ("application/x-npy", "npy"),
    ("application/vnd.recordare.musicxml+xml", "musicxml"),
    ("application/vnd.oasis.opendocument.text", "odt"),
    ("application/vnd.openxmlformats-officedocument.wordprocessingml.document", "docx"),
    ("application/vnd.openxmlformats-officedocument.presentationml.presentation", "pptx"),
    ("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", "xlsx"),
];

/// The file extension for a media type (parameters and case ignored); `bin` when unknown.
pub fn ext_for(media_type: &str) -> &'static str {
    let m = media_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    TABLE.iter().find(|(t, _)| *t == m).map_or("bin", |(_, e)| e)
}

/// The media type of a file by its extension (case ignored); `application/octet-stream` when
/// unknown.
pub fn media_for_ext(ext: &str) -> &'static str {
    let e = ext.trim_start_matches('.').to_ascii_lowercase();
    TABLE.iter().find(|(_, x)| *x == e).map_or("application/octet-stream", |(t, _)| t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_ways() {
        assert_eq!(ext_for("Video/MP4; codecs=avc1"), "mp4");
        assert_eq!(ext_for("x/unknown"), "bin");
        assert_eq!(media_for_ext("JPEG"), "image/jpeg");
        assert_eq!(media_for_ext(".svg"), "image/svg+xml");
        assert_eq!(media_for_ext("weird"), "application/octet-stream");
        for (t, e) in TABLE {
            assert_eq!(ext_for(media_for_ext(e)), ext_for(t), "{t} {e}");
        }
    }
}
