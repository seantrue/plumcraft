//! Viewers (docs/research/16-viewers.md): a preview and a full view for every base type, chosen
//! from one registry, so adding a type is one [`Viewer`] and one line in [`registry`].
//!
//! - [`Viewer`]: takes a value ([`ViewInput`]), draws a small `preview` (a wire's or a node's
//!   hover) and a `full` view with its controls (the Watched panel, a pop-out window).
//! - [`ViewerKind`]: a viewer's id, label, which values it accepts, and how to make a fresh one
//!   (each window has its own state: zoom, slice, scroll).
//! - [`registry`]: every kind, in order of preference; [`kinds_for`] the ones a value offers, the
//!   first its default. The last, [`hex`], accepts everything.
//! - [`Pane`]: one port's value as the editor follows it (a pop-out, a wire's preview): the
//!   latest display, its full bytes read once, its textures, and a viewer per kind offered.
//!
//! The Watched panel keeps its own image, SVG, document, array and audio viewers
//! (`EditorApp::media`, `EditorApp::audio`, which the control channel drives) and shows the
//! other kinds offered as tabs beside them.

use std::sync::Arc;

pub mod basic;
pub mod grid;
pub mod mesh;
pub mod pdf;
pub mod plot;
pub mod wrap;
pub mod xml;
pub mod yaml;

use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::DataRef;
use plumcraft_proto::display::{Display, Rendition};
use plumcraft_session::run::WatchRole;
use plumcraft_session::viewer_cmds::Tools;
use plumcraft_store::Blob;
use serde_json::{Value, json};

use crate::canvas::Action;
use crate::run_view::Thumbs;
use crate::theme::Tokens;
use crate::watched::Watched;

/// What a viewer is given: the value and what the editor already made of it.
pub struct ViewInput<'a> {
    pub display: &'a Display,
    /// The full value's bytes, when read (never for a document).
    pub full: Option<&'a Arc<Blob>>,
    /// The full value was gone (reaped): the viewer greys what it shows.
    pub missing: bool,
    /// The first `rgba8` rendition's texture (the engine's picture of the value).
    pub rendition: Option<&'a egui::TextureHandle>,
    /// An array's view pictures (renditions with role `view`), captioned.
    pub pictures: &'a [(String, egui::TextureHandle)],
    /// A list's items' thumbnails, by item.
    pub sheet: &'a [Option<egui::TextureHandle>],
    /// The views the value's wire type offers (arrays: auto, heat, …).
    pub offered_views: &'a [String],
    /// Its probe and any zoom asked for (`viewer.probe`, `viewer.zoom`).
    pub tools: Tools,
}

/// A viewer of one value (see the module docs).
pub trait Viewer {
    /// Take the value. Cheap when nothing changed; heavy work goes to a worker thread.
    fn sync(&mut self, ctx: &egui::Context, v: &ViewInput<'_>);
    /// Small and without controls, within `max`.
    fn preview(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
        preview_default(ui, v, max, tokens);
    }
    /// Everything, with its controls. Returns the commands its controls ask for.
    fn full(&mut self, ui: &mut egui::Ui, v: &ViewInput<'_>, tokens: &Tokens) -> Vec<Action>;
    /// Its state, for `ui.inspect`.
    fn inspect(&self) -> Value {
        Value::Null
    }
}

/// The preview most viewers give: the engine's picture when there is one, else the summary.
pub fn preview_default(ui: &mut egui::Ui, v: &ViewInput<'_>, max: egui::Vec2, tokens: &Tokens) {
    if let Some(t) = v.rendition {
        fitted_image(ui, t, max);
    }
    ui.label(egui::RichText::new(&v.display.summary).small().color(tokens.text_dim));
}

/// A texture shrunk to fit `max`, never enlarged past twice its size.
pub fn fitted_image(ui: &mut egui::Ui, t: &egui::TextureHandle, max: egui::Vec2) {
    let size = t.size_vec2();
    let scale = (max.x / size.x.max(1.0)).min(max.y / size.y.max(1.0)).min(2.0);
    ui.add(egui::Image::new((t.id(), size * scale)));
}

/// A kind of viewer.
pub struct ViewerKind {
    /// `array.grid`: what `view.viewer` and `view.popout` name.
    pub id: &'static str,
    /// The tab's label.
    pub label: &'static str,
    /// Whether it shows this value.
    pub accepts: fn(&Display) -> bool,
    /// A fresh one.
    pub make: fn() -> Box<dyn Viewer>,
    /// The Watched panel draws this kind with its own viewer (`EditorApp::media`, `audio`).
    pub panel_builtin: bool,
}

fn media_is(d: &Display, m: &[Media]) -> bool {
    m.contains(&d.media)
}

fn is_pdf(d: &Display) -> bool {
    d.full.as_ref().is_some_and(|f| f.meta.media == "application/pdf")
}

static KINDS: std::sync::OnceLock<Vec<ViewerKind>> = std::sync::OnceLock::new();
/// What an application adds, until the registry is built.
static ADDED: std::sync::Mutex<Vec<ViewerKind>> = std::sync::Mutex::new(Vec::new());

/// Add an application's viewers, ahead of PlumCraft's, so they are preferred for what they
/// accept. Before anything reads the registry.
pub fn register(kinds: Vec<ViewerKind>) -> Result<(), String> {
    if KINDS.get().is_some() {
        return Err("viewers are registered at startup".into());
    }
    ADDED.lock().map_err(|_| "the viewer registry is poisoned".to_owned())?.extend(kinds);
    Ok(())
}

/// Every kind of viewer, in order of preference: the first a value accepts is its default.
pub fn registry() -> &'static [ViewerKind] {
    KINDS.get_or_init(|| {
        let mut kinds = ADDED.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
        kinds.extend(builtin());
        kinds
    })
}

/// PlumCraft's viewers.
fn builtin() -> Vec<ViewerKind> {
    vec![
        ViewerKind {
            id: "picture",
            label: "Picture",
            accepts: |d| media_is(d, &[Media::Image]),
            make: || Box::new(wrap::Picture::default()),
            panel_builtin: true,
        },
        ViewerKind { id: "svg", label: "SVG", accepts: |d| media_is(d, &[Media::Vector]), make: || Box::new(wrap::Svg::default()), panel_builtin: true },
        ViewerKind { id: "pdf.pages", label: "Pages", accepts: is_pdf, make: || Box::new(pdf::Pages::default()), panel_builtin: false },
        // an STL file is a mesh to turn and zoom; its bytes stay a tab away (#38)
        ViewerKind { id: "mesh", label: "Mesh", accepts: mesh::accepts, make: || Box::new(mesh::MeshView::default()), panel_builtin: false },
        ViewerKind { id: "doc", label: "Document", accepts: |d| media_is(d, &[Media::Doc]), make: || Box::new(wrap::Doc::default()), panel_builtin: true },
        // an (N, k) table of positions plots first; its numbers stay a tab away
        ViewerKind { id: "array.plot", label: "Plot", accepts: plot::accepts, make: || Box::new(plot::TablePlot::default()), panel_builtin: false },
        ViewerKind {
            id: "array.pictures",
            label: "Pictures",
            accepts: |d| media_is(d, &[Media::Ndarray]),
            make: || Box::new(wrap::ArrayPictures::default()),
            panel_builtin: true,
        },
        ViewerKind {
            id: "array.grid",
            label: "Values",
            accepts: |d| media_is(d, &[Media::Ndarray]),
            make: || Box::new(grid::ArrayGrid::default()),
            panel_builtin: false,
        },
        ViewerKind { id: "audio", label: "Audio", accepts: |d| media_is(d, &[Media::Audio]), make: || Box::new(wrap::Audio::default()), panel_builtin: true },
        ViewerKind {
            id: "table.grid",
            label: "Table",
            // a table, or rows (a list of lists: Table to Rows', a spreadsheet's Read range)
            accepts: |d| media_is(d, &[Media::Table]) || d.value.as_ref().is_some_and(basic::is_rows),
            make: || Box::new(basic::Table::default()),
            panel_builtin: false,
        },
        // XML text reads as an outline first; its text stays a tab away (B4)
        ViewerKind { id: "xml", label: "XML", accepts: xml::accepts, make: || Box::new(xml::Xml::default()), panel_builtin: false },
        ViewerKind {
            id: "text.plain",
            label: "Text",
            accepts: |d| media_is(d, &[Media::Text]),
            make: || Box::new(basic::Text::default()),
            panel_builtin: true,
        },
        ViewerKind { id: "scalar", label: "Value", accepts: |d| media_is(d, &[Media::Scalar]), make: || Box::new(basic::Scalar), panel_builtin: true },
        // JSON reads as YAML first (the maintainer: "format as Yaml for display")
        ViewerKind { id: "yaml", label: "YAML", accepts: |d| d.media == Media::Tree, make: || Box::new(basic::Yaml::default()), panel_builtin: false },
        ViewerKind {
            id: "tree",
            label: "Tree",
            accepts: |d| d.media == Media::Tree || d.media == Media::Table || (d.value.as_ref().is_some_and(|v| v.is_object() || v.is_array())),
            make: || Box::new(basic::Tree::default()),
            panel_builtin: false,
        },
        ViewerKind { id: "items", label: "Items", accepts: |d| d.items > 1 && !d.sheet.is_empty(), make: || Box::new(basic::Items), panel_builtin: false },
        ViewerKind { id: "hex", label: "Bytes", accepts: |d| d.full.is_some() && d.media != Media::Doc, make: || Box::new(basic::Hex), panel_builtin: false },
    ]
}

/// The kinds that show `d`, its default first.
pub fn kinds_for(d: &Display) -> Vec<&'static ViewerKind> {
    registry().iter().filter(|k| (k.accepts)(d)).collect()
}

/// Whether something other than the hex dump shows `d`. Every value of every engine must be
/// (spec 10.1): `crates/ui/tests/no_hex.rs` holds PlumCraft's engines to it, and a pack's tests
/// call this for its own types.
pub fn viewable(d: &Display) -> bool {
    kinds_for(d).iter().any(|k| k.id != "hex")
}

/// The kind `id`.
pub fn kind(id: &str) -> Option<&'static ViewerKind> {
    registry().iter().find(|k| k.id == id)
}

/// Add `popout`'s id to the commands that act on a watch entry, so a pop-out's controls act on
/// its own value, not the panel's.
pub fn aim(asks: Vec<Action>, role: WatchRole) -> Vec<Action> {
    let WatchRole::Popout(id) = role else { return asks };
    asks.into_iter()
        .map(|(cmd, mut p)| {
            if matches!(cmd, "run.view" | "run.item" | "view.viewer" | "viewer.probe" | "viewer.zoom" | "viewer.sheet")
                && let Some(o) = p.as_object_mut()
            {
                o.insert("popout".into(), json!(id));
            }
            (cmd, p)
        })
        .collect()
}

/// One port's value as the editor follows it: what a pop-out window (or a wire's preview) shows.
#[derive(Default)]
pub struct Pane {
    /// Its latest display (kept while a new run computes, so it doesn't blink).
    display: Option<Display>,
    read: Watched,
    rendition: Option<egui::TextureHandle>,
    pictures: Vec<(String, egui::TextureHandle)>,
    sheet: Vec<Option<egui::TextureHandle>>,
    offered_views: Vec<String>,
    viewers: Vec<(&'static str, Box<dyn Viewer>)>,
    /// The viewer shown; `None`: the value's default.
    pub chosen: Option<String>,
    /// What the session last gave its viewers.
    tools: Tools,
}

impl Pane {
    /// Take the newest display for the port (`None` while a run hasn't sent one: the last
    /// stays). True when the value's bytes are gone and a rerun should publish them again.
    /// `full` reads the full value, `resolve` renditions; `tools` is this pane's probe and zoom.
    #[allow(clippy::too_many_arguments)]
    pub fn sync(
        &mut self,
        ctx: &egui::Context,
        display: Option<&Display>,
        offered_views: Vec<String>,
        thumbs: &mut Thumbs,
        resolve: &dyn Fn(&DataRef) -> Option<Blob>,
        full: &dyn Fn(&DataRef) -> Option<Arc<Blob>>,
        tools: Tools,
    ) -> bool {
        self.tools = tools;
        let Some(d) = display.or(self.display.as_ref()).cloned() else { return false };
        let rerun = self.read.update(Some(&d), full);
        let rgba = |r: &Rendition| match r {
            Rendition::Rgba8 { data, role, .. } if role.is_empty() || role == "thumb" => Some(data.clone()),
            _ => None,
        };
        self.rendition = d.renditions.iter().find_map(rgba).and_then(|data| thumbs.get(ctx, &data, resolve).ok());
        self.pictures = d
            .renditions
            .iter()
            .filter_map(|r| match r {
                Rendition::Rgba8 { data, role, caption, .. } if role == "view" => thumbs.picture(ctx, data, resolve).ok().map(|t| (caption.clone(), t)),
                _ => None,
            })
            .collect();
        if self.rendition.is_none() {
            self.rendition = self.pictures.first().map(|(_, t)| t.clone());
        }
        self.sheet = d
            .sheet
            .iter()
            .map(|c| match &c.thumb {
                Some(Rendition::Rgba8 { data, .. }) => thumbs.get(ctx, data, resolve).ok(),
                _ => None,
            })
            .collect();
        self.offered_views = offered_views;
        let kinds = kinds_for(&d);
        self.viewers.retain(|(id, _)| kinds.iter().any(|k| k.id == *id));
        for k in kinds {
            if !self.viewers.iter().any(|(id, _)| *id == k.id) {
                self.viewers.push((k.id, (k.make)()));
            }
        }
        self.display = Some(d);
        let (Some(d), Some(i)) = (self.display.as_ref(), self.current_index()) else { return rerun };
        let input = ViewInput {
            display: d,
            full: self.read.full.as_ref(),
            missing: self.read.missing,
            rendition: self.rendition.as_ref(),
            pictures: &self.pictures,
            sheet: &self.sheet,
            offered_views: &self.offered_views,
            tools: self.tools,
        };
        if let Some((_, v)) = self.viewers.get_mut(i) {
            v.sync(ctx, &input);
        }
        rerun
    }

    /// The display shown.
    pub fn display(&self) -> Option<&Display> {
        self.display.as_ref()
    }

    fn current_index(&self) -> Option<usize> {
        let d = self.display.as_ref()?;
        let kinds = kinds_for(d);
        let want = self.chosen.as_deref().filter(|c| kinds.iter().any(|k| k.id == *c)).or_else(|| kinds.first().map(|k| k.id))?;
        self.viewers.iter().position(|(id, _)| *id == want)
    }

    /// The id of the viewer shown.
    pub fn current(&self) -> Option<&'static str> {
        self.current_index().and_then(|i| self.viewers.get(i)).map(|(id, _)| *id)
    }

    /// The full view: the viewers offered as tabs, then the one chosen. `role` aims its
    /// controls' commands.
    pub fn full_ui(&mut self, ui: &mut egui::Ui, role: WatchRole, tokens: &Tokens) -> Vec<Action> {
        let mut asks = Vec::new();
        let Some(d) = self.display.clone() else {
            ui.label(egui::RichText::new("waiting for the value…").color(tokens.text_dim));
            return asks;
        };
        if let Some(e) = &d.error {
            ui.colored_label(tokens.error, &e.message);
            return asks;
        }
        ui.label(egui::RichText::new(&d.summary).color(tokens.text_dim));
        let current = self.current();
        let kinds = kinds_for(&d);
        if kinds.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for k in &kinds {
                    if ui.selectable_label(current == Some(k.id), k.label).on_hover_text(format!("view.viewer {{viewer: {:?}}}", k.id)).clicked() {
                        asks.push(("view.viewer", json!({"viewer": k.id})));
                    }
                }
            });
        }
        ui.separator();
        asks.extend(self.content_ui(ui, tokens));
        aim(asks, role)
    }

    /// Only the chosen viewer's full view (the Watched panel draws its own header and tabs).
    pub fn content_ui(&mut self, ui: &mut egui::Ui, tokens: &Tokens) -> Vec<Action> {
        let (Some(d), Some(i)) = (self.display.clone(), self.current_index()) else { return Vec::new() };
        let input = ViewInput {
            display: &d,
            full: self.read.full.as_ref(),
            missing: self.read.missing,
            rendition: self.rendition.as_ref(),
            pictures: &self.pictures,
            sheet: &self.sheet,
            offered_views: &self.offered_views,
            tools: self.tools,
        };
        self.viewers.get_mut(i).map(|(_, v)| v.full(ui, &input, tokens)).unwrap_or_default()
    }

    /// The preview, within `max`.
    pub fn preview_ui(&mut self, ui: &mut egui::Ui, max: egui::Vec2, tokens: &Tokens) {
        let Some(d) = self.display.clone() else { return };
        let Some(i) = self.current_index() else { return };
        let input = ViewInput {
            display: &d,
            full: self.read.full.as_ref(),
            missing: self.read.missing,
            rendition: self.rendition.as_ref(),
            pictures: &self.pictures,
            sheet: &self.sheet,
            offered_views: &self.offered_views,
            tools: self.tools,
        };
        if let Some((_, v)) = self.viewers.get_mut(i) {
            v.preview(ui, &input, max, tokens);
        }
    }

    /// Its state, for `ui.inspect`.
    pub fn inspect(&self) -> Value {
        let d = self.display.as_ref();
        json!({
            "summary": d.map(|d| d.summary.clone()),
            "viewer": self.current(),
            "viewers": d.map(|d| kinds_for(d).iter().map(|k| k.id).collect::<Vec<_>>()).unwrap_or_default(),
            "state": self.current_index().and_then(|i| self.viewers.get(i)).map(|(_, v)| v.inspect()),
            "full": self.read.full.is_some(),
            "missing": self.read.missing,
        })
    }
}
