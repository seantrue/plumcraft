//! XML as an outline (docs/BUGS.md B4, #26): elements you can open and close, with their
//! attributes and text, and the source a click away. XML arrives as text, or as bytes whose
//! media type says XML (`application/xml`, `text/xml`, `…+xml`); SVG keeps its own viewer.
//!
//! Hostile input (spec 18): roxmltree parses with DTDs refused (so no entity expansion) and a
//! node limit; the outline stops at a depth and a node budget; nothing here panics on any bytes.

use plumcraft_proto::catalogue::Media;
use plumcraft_proto::display::{Display, Rendition};
use serde_json::{Value, json};

use super::basic::{dim, text_of};
use super::{ViewInput, Viewer};
use crate::canvas::Action;
use crate::theme::Tokens;

/// The most bytes parsed: past this the Text tab shows the value.
pub const MAX_BYTES: usize = 16 << 20;
/// The most nodes roxmltree is allowed to build.
const MAX_PARSE_NODES: u32 = 2_000_000;
/// How deep the outline goes; deeper elements show as "…".
pub const MAX_DEPTH: usize = 256;
/// The most outline nodes kept (and drawn).
pub const MAX_NODES: usize = 20_000;

/// Whether `d` is XML this viewer shows.
pub fn accepts(d: &Display) -> bool {
    if d.media == Media::Vector {
        return false; // SVG: its own viewer
    }
    if let Some(f) = &d.full {
        let m = f.meta.media.to_ascii_lowercase();
        let m = m.split(';').next().unwrap_or_default().trim();
        if m == "application/xml" || m == "text/xml" || (m.ends_with("+xml") && m != "image/svg+xml") {
            return true;
        }
    }
    d.media == Media::Text
        && d.renditions.iter().any(|r| match r {
            Rendition::Excerpt { text, offset, .. } => *offset == 0 && looks_like_xml(text),
            _ => false,
        })
}

/// Whether text starts as an XML document does: a declaration, or an element (after a byte
/// order mark, whitespace and comments), with its tag closed.
pub fn looks_like_xml(s: &str) -> bool {
    let mut s = s.trim_start_matches('\u{feff}').trim_start();
    if s.starts_with("<?xml") {
        return true;
    }
    while let Some(rest) = s.strip_prefix("<!--") {
        let Some(end) = rest.find("-->") else { return false };
        s = rest.get(end + 3..).unwrap_or_default().trim_start();
    }
    let Some(rest) = s.strip_prefix('<') else { return false };
    let starts_name = rest.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == ':');
    starts_name && rest.contains('>')
}

/// One line of the outline.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Element {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<Item>,
    },
    Text(String),
    Comment(String),
    Instruction(String),
    /// Past [`MAX_DEPTH`] or [`MAX_NODES`].
    Elided,
}

/// A parsed document: its prolog's comments and instructions, then its root element.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub items: Vec<Item>,
    /// Elements in the document (not only those kept).
    pub elements: usize,
    /// The outline stopped at [`MAX_NODES`].
    pub truncated: bool,
}

/// Parse `text` as XML into an outline, or say where it isn't well-formed.
pub fn outline(text: &str) -> Result<Outline, String> {
    if text.len() > MAX_BYTES {
        return Err(format!("{} bytes is more than the XML view reads ({} MB): see the Text tab", text.len(), MAX_BYTES >> 20));
    }
    let opt = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: MAX_PARSE_NODES };
    let doc = roxmltree::Document::parse_with_options(text, opt).map_err(|e| format!("not well-formed XML: {e}"))?;
    let elements = doc.descendants().filter(|n| n.is_element()).count();
    let mut budget = MAX_NODES;
    let items = children(doc.root(), 0, &mut budget);
    Ok(Outline { items, elements, truncated: budget == 0 })
}

fn children(node: roxmltree::Node<'_, '_>, depth: usize, budget: &mut usize) -> Vec<Item> {
    let mut out = Vec::new();
    for c in node.children() {
        if *budget == 0 {
            out.push(Item::Elided);
            break;
        }
        let item = if c.is_element() {
            if depth >= MAX_DEPTH {
                out.push(Item::Elided);
                break;
            }
            *budget -= 1;
            let name = qualified(c);
            let attrs = c.attributes().map(|a| (a.name().to_owned(), a.value().to_owned())).collect();
            Item::Element { name, attrs, children: children(c, depth + 1, budget) }
        } else if c.is_text() {
            let t = c.text().unwrap_or_default().trim();
            if t.is_empty() {
                continue;
            }
            *budget -= 1;
            Item::Text(t.to_owned())
        } else if c.is_comment() {
            *budget -= 1;
            Item::Comment(c.text().unwrap_or_default().trim().to_owned())
        } else if c.is_pi() {
            *budget -= 1;
            Item::Instruction(c.pi().map(|p| format!("{} {}", p.target, p.value.unwrap_or_default()).trim().to_owned()).unwrap_or_default())
        } else {
            continue;
        };
        out.push(item);
    }
    out
}

/// An element's name as written: with its prefix when it has one.
fn qualified(n: roxmltree::Node<'_, '_>) -> String {
    let tag = n.tag_name();
    match tag.namespace().and_then(|ns| n.lookup_prefix(ns)) {
        Some(p) if !p.is_empty() => format!("{p}:{}", tag.name()),
        _ => tag.name().to_owned(),
    }
}

/// The XML viewer: the outline, or the source.
#[derive(Default)]
pub struct Xml {
    raw: bool,
    /// The text last parsed (its length and hash) and what it parsed to.
    parsed: Option<(usize, u64, Result<Outline, String>)>,
}

fn key(t: &str) -> (usize, u64) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    (t.len(), h.finish())
}

fn start_tag(name: &str, attrs: &[(String, String)]) -> String {
    let mut s = format!("<{name}");
    for (k, v) in attrs {
        s.push_str(&format!(" {k}=\"{v}\""));
    }
    s.push('>');
    s
}

fn draw(ui: &mut egui::Ui, items: &[Item], path: &str, tokens: &Tokens, colour: egui::Color32) {
    for (i, item) in items.iter().enumerate() {
        let here = format!("{path}/{i}");
        match item {
            Item::Element { name, attrs, children } => {
                let tag = start_tag(name, attrs);
                match children.as_slice() {
                    [] => {
                        ui.label(egui::RichText::new(format!("{}/>", tag.trim_end_matches('>'))).monospace().color(colour));
                    }
                    [Item::Text(t)] if t.len() <= 120 && !t.contains('\n') => {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(egui::RichText::new(&tag).monospace().color(colour));
                            ui.label(egui::RichText::new(t).monospace().color(tokens.text_dim));
                            ui.label(egui::RichText::new(format!("</{name}>")).monospace().color(colour));
                        });
                    }
                    _ => {
                        let head = format!("{tag}  ({} {})", children.len(), if children.len() == 1 { "child" } else { "children" });
                        egui::CollapsingHeader::new(egui::RichText::new(head).monospace().color(colour))
                            .id_salt(("viewer-xml", here.as_str()))
                            .default_open(path.matches('/').count() < 2)
                            .show(ui, |ui| draw(ui, children, &here, tokens, colour));
                    }
                }
            }
            Item::Text(t) => {
                ui.label(egui::RichText::new(t).monospace().color(tokens.text_dim));
            }
            Item::Comment(t) => {
                ui.label(egui::RichText::new(format!("<!-- {t} -->")).monospace().color(tokens.text_faint));
            }
            Item::Instruction(t) => {
                ui.label(egui::RichText::new(format!("<?{t}?>")).monospace().color(tokens.text_faint));
            }
            Item::Elided => {
                ui.label(egui::RichText::new("…").monospace().color(tokens.text_faint));
            }
        }
    }
}

fn source(ui: &mut egui::Ui, t: &str, tokens: &Tokens, colour: egui::Color32) {
    let lines: Vec<&str> = t.lines().collect();
    let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
    let width = lines.len().to_string().len();
    egui::ScrollArea::both().id_salt("viewer-xml-source").auto_shrink([false, false]).show_rows(ui, row_h, lines.len(), |ui, range| {
        for i in range {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{:>width$}", i + 1)).monospace().color(tokens.text_faint));
                ui.add(
                    egui::Label::new(egui::RichText::new(lines.get(i).copied().unwrap_or_default()).monospace().color(colour))
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            });
        }
    });
}

impl Viewer for Xml {
    fn sync(&mut self, _: &egui::Context, _: &ViewInput<'_>) {}

    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action> {
        let Some(t) = text_of(v) else {
            dim(ui, "no XML", tokens);
            return Vec::new();
        };
        let colour = if v.missing { tokens.text_faint } else { tokens.text };
        let (len, hash) = key(&t);
        if !matches!(&self.parsed, Some((l, h, _)) if *l == len && *h == hash) {
            self.parsed = Some((len, hash, outline(&t)));
        }
        let Some((_, _, parsed)) = &self.parsed else { return Vec::new() };
        let raw = self.raw;
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.raw, "Source");
            match parsed {
                Ok(o) => {
                    let more = if o.truncated { format!(", the first {MAX_NODES} shown") } else { String::new() };
                    ui.label(egui::RichText::new(format!("{} elements{more}", o.elements)).small().color(tokens.text_faint));
                }
                Err(e) => {
                    ui.label(egui::RichText::new(e).small().color(tokens.error));
                }
            }
        });
        match (parsed, raw) {
            (Ok(o), false) => {
                egui::ScrollArea::both().id_salt("viewer-xml").auto_shrink([false, false]).show(ui, |ui| draw(ui, &o.items, "", tokens, colour));
            }
            _ => source(ui, &t, tokens, colour),
        }
        Vec::new()
    }

    fn inspect(&self) -> Value {
        match &self.parsed {
            Some((_, _, Ok(o))) => {
                let root = o.items.iter().find_map(|i| match i {
                    Item::Element { name, .. } => Some(name.clone()),
                    _ => None,
                });
                json!({"raw": self.raw, "state": "ok", "elements": o.elements, "root": root, "truncated": o.truncated})
            }
            Some((_, _, Err(e))) => json!({"raw": self.raw, "state": "error", "error": e}),
            None => json!({"raw": self.raw, "state": "none"}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::data::{BlobMeta, DataRef};

    const BOOKS: &str = r#"<?xml version="1.0"?>
<!-- a catalogue -->
<shelf xmlns:d="urn:dc" kind="fiction">
  <book id="b1"><d:title>Dune</d:title><year>1965</year></book>
  <book id="b2"><d:title>Solaris</d:title><empty/></book>
  some loose text
</shelf>"#;

    fn text_display(excerpt: &str) -> Display {
        Display { media: Media::Text, renditions: vec![Rendition::Excerpt { text: excerpt.into(), offset: 0, total_chars: 0, lines: 0 }], ..Default::default() }
    }

    fn blob_display(media: &str, kind: Media) -> Display {
        Display {
            media: kind,
            full: Some(DataRef { meta: BlobMeta { media: media.into(), ..Default::default() }, ..Default::default() }),
            ..Default::default()
        }
    }

    #[test]
    fn an_outline_has_the_elements_attributes_and_text() {
        let o = outline(BOOKS).unwrap();
        assert_eq!(o.elements, 7);
        assert!(!o.truncated);
        assert_eq!(o.items[0], Item::Comment("a catalogue".into()));
        let Item::Element { name, attrs, children } = &o.items[1] else { panic!("{:?}", o.items) };
        assert_eq!((name.as_str(), attrs.as_slice()), ("shelf", &[("kind".to_owned(), "fiction".to_owned())][..]));
        let Item::Element { name, attrs, children: book } = &children[0] else { panic!() };
        assert_eq!((name.as_str(), attrs[0].1.as_str()), ("book", "b1"));
        assert_eq!(book[0], Item::Element { name: "d:title".into(), attrs: vec![], children: vec![Item::Text("Dune".into())] });
        assert_eq!(children[2], Item::Text("some loose text".into()));
    }

    #[test]
    fn xml_text_and_xml_bytes_are_accepted_and_svg_and_prose_are_not() {
        assert!(accepts(&text_display(BOOKS)));
        assert!(accepts(&text_display("\u{feff}  <!-- hi --> <root/>")));
        assert!(accepts(&text_display("<a b='1'>")));
        for not in ["Plum < 3 and 4 > 2", "", "   ", "<", "<1>", "a <b>bold</b> word", "<!-- never closed"] {
            assert!(!accepts(&text_display(not)), "{not:?}");
        }
        assert!(
            !accepts(&Display {
                media: Media::Text,
                renditions: vec![Rendition::Excerpt { text: "<a/>".into(), offset: 10, total_chars: 0, lines: 0 }],
                ..Default::default()
            }),
            "the middle of a value"
        );
        for m in ["application/xml", "text/xml", "application/atom+xml", "Application/XML; charset=utf-8"] {
            assert!(accepts(&blob_display(m, Media::Bytes)), "{m}");
        }
        assert!(!accepts(&blob_display("image/svg+xml", Media::Vector)));
        assert!(!accepts(&blob_display("image/svg+xml", Media::Bytes)));
        assert!(!accepts(&blob_display("application/json", Media::Bytes)));
        // the registry offers it, ahead of plain text
        let ids: Vec<&str> = super::super::kinds_for(&text_display(BOOKS)).iter().map(|k| k.id).collect();
        assert_eq!(&ids[..2], ["xml", "text.plain"], "{ids:?}");
        assert!(super::super::viewable(&blob_display("application/xml", Media::Bytes)));
    }

    #[test]
    fn hostile_xml_is_an_error_or_bounded_never_a_panic() {
        // every truncation and every byte flipped
        let b = BOOKS.as_bytes();
        for n in 0..=b.len() {
            let _ = outline(&String::from_utf8_lossy(&b[..n]));
        }
        for i in 0..b.len() {
            let mut c = b.to_vec();
            c[i] ^= 0xa5;
            let _ = outline(&String::from_utf8_lossy(&c));
        }
        // entity expansion: DTDs are refused
        let laughs = r#"<?xml version="1.0"?><!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;&lol;&lol;">]><lolz>&lol2;</lolz>"#;
        assert!(outline(laughs).unwrap_err().contains("not well-formed"));
        // deep nesting stops at MAX_DEPTH, wide documents at MAX_NODES
        let deep = format!("{}{}", "<a>".repeat(MAX_DEPTH + 50), "</a>".repeat(MAX_DEPTH + 50));
        let o = outline(&deep).unwrap();
        let (mut depth, mut items) = (0, &o.items);
        while let Some(Item::Element { children, .. }) = items.first() {
            depth += 1;
            items = children;
        }
        assert_eq!((depth, items.first()), (MAX_DEPTH, Some(&Item::Elided)));
        let wide = format!("<r>{}</r>", "<i/>".repeat(MAX_NODES + 10));
        let o = outline(&wide).unwrap();
        assert!(o.truncated && o.elements == MAX_NODES + 11);
        // too large to read
        assert!(outline(&" ".repeat(MAX_BYTES + 1)).unwrap_err().contains("Text tab"));
    }
}
