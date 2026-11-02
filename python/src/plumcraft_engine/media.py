"""Media types and file extensions, both ways, as `crates/proto/src/media.rs`: the name a value's
file gets when it is written or opened elsewhere (`application/pdf` -> `pdf`), and the media type
a file read by name has."""

from __future__ import annotations

# (media type, extension). The first row for a media type gives its extension; the first row for
# an extension gives its media type.
TABLE: tuple[tuple[str, str], ...] = (
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
)

# Rust's `str::trim` (Unicode White_Space).
_WS = "\t\n\x0b\x0c\r \x85\xa0                　"


def _ascii_lower(s: str) -> str:
    """Rust's `to_ascii_lowercase`: only A-Z change."""
    return "".join(chr(ord(c) + 32) if "A" <= c <= "Z" else c for c in s)


def ext_for(media_type: str) -> str:
    """The file extension for a media type (parameters and case ignored); `bin` when unknown."""
    m = _ascii_lower(media_type.split(";", 1)[0].strip(_WS))
    return next((e for t, e in TABLE if t == m), "bin")


def media_for_ext(ext: str) -> str:
    """The media type of a file by its extension (case ignored); `application/octet-stream` when
    unknown."""
    e = _ascii_lower(ext.lstrip("."))
    return next((t for t, x in TABLE if x == e), "application/octet-stream")
