//! Every menu the editor draws, opened as a person opens it, written out as text: the menu
//! bar's menus and their submenus, and each context menu (a node, a selected node, a collapsed
//! group, a group's header, a wire, an input, an output, a frame, a net's tag, the canvas), in
//! several states (empty, a selection, a run in progress, a prompt open, an engine compiled in,
//! a process engine, engines picked automatically).
//!
//! What is written is what is on screen: each widget of the open menus as the accessibility tree
//! has it (role, label, enabled, checked), and each text painted in a menu with its colour (a
//! disabled item's is dimmer), shortcut texts and check marks included. N1 slice 8 drew the menus
//! from the core's menu model ([`plumcraft_editor_core::menu`]) and compared the dumps before and
//! after: `PLUMCRAFT_MENU_DUMP=<folder>` writes them there.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{add, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use plumcraft_engine::host::Host;
use plumcraft_graph::{NodeId, PortRef};
use plumcraft_host::{Connection, InProc};
use plumcraft_session::Session;
use plumcraft_session::engine_cmds::{EngineChoice, EngineTarget};
use plumcraft_ui::EditorApp;
use plumcraft_ui::link::Link;
use serde_json::json;

type H = Harness<'static, EditorApp>;

/// The rects of the menus (and other popups) open now.
fn popups(h: &H) -> Vec<egui::Rect> {
    h.ctx.memory(|m| m.layer_ids().filter(|l| l.order == egui::Order::Foreground && m.areas().is_visible(l)).filter_map(|l| m.area_rect(l.id)).collect())
}

fn inside(rects: &[egui::Rect], r: egui::Rect) -> bool {
    rects.iter().any(|p| p.expand(1.0).contains_rect(r))
}

fn texts(shape: &egui::Shape, out: &mut Vec<(egui::Rect, String, egui::Color32)>) {
    match shape {
        egui::Shape::Text(t) => {
            let color = t
                .override_text_color
                .or_else(|| t.galley.job.sections.first().map(|s| s.format.color))
                .filter(|c| *c != egui::Color32::PLACEHOLDER)
                .unwrap_or(t.fallback_color);
            out.push((t.visual_bounding_rect(), t.galley.job.text.clone(), color));
        }
        egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
        _ => {}
    }
}

/// What the open menus show, and the menu bar.
fn snapshot(h: &H) -> String {
    let rects = popups(h);
    let mut lines = Vec::new();
    lines.push("# widgets".to_owned());
    let mut widgets = Vec::new();
    for n in h.root().children_recursive() {
        let a = n.accesskit_node();
        let Some(b) = a.bounding_box() else { continue };
        let r = egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32));
        let bar = r.max.y <= 30.0;
        if !(bar || inside(&rects, r)) {
            continue;
        }
        let label = a.label().unwrap_or_default();
        if label.is_empty() && a.value().is_none() {
            continue;
        }
        widgets.push((
            (r.min.y.round() as i64, r.min.x.round() as i64),
            format!(
                "{:?} {:?}{}{}{}{}",
                a.role(),
                label,
                a.value().map(|v| format!(" value={v:?}")).unwrap_or_default(),
                if a.is_disabled() { " disabled" } else { "" },
                a.toggled().map(|t| format!(" toggled={t:?}")).unwrap_or_default(),
                a.is_selected().map(|s| format!(" selected={s}")).unwrap_or_default(),
            ),
        ));
    }
    // by place, top to bottom (the tree's order of the canvas's own widgets varies)
    widgets.sort();
    lines.extend(widgets.into_iter().map(|(_, l)| l));
    lines.push("# painted".to_owned());
    let mut painted = Vec::new();
    for c in &h.output().shapes {
        texts(&c.shape, &mut painted);
    }
    let mut shown: Vec<((i64, i64), String)> = painted
        .into_iter()
        .filter(|(r, text, _)| !text.trim().is_empty() && (r.max.y <= 30.0 || inside(&rects, *r)))
        .map(|(r, text, color)| ((r.min.y.round() as i64, r.min.x.round() as i64), format!("{text:?} {:?} at {:.0},{:.0}", color.to_array(), r.min.x, r.min.y)))
        .collect();
    shown.sort();
    lines.extend(shown.into_iter().map(|(_, l)| l));
    lines.join("\n") + "\n"
}

struct Dump {
    dir: Option<std::path::PathBuf>,
    count: usize,
}

impl Dump {
    fn new() -> Dump {
        let dir = std::env::var_os("PLUMCRAFT_MENU_DUMP").map(std::path::PathBuf::from);
        if let Some(d) = &dir {
            std::fs::create_dir_all(d).unwrap();
        }
        Dump { dir, count: 0 }
    }

    fn write(&mut self, name: &str, h: &H) {
        let text = snapshot(h);
        assert!(text.lines().count() > 2, "{name}: nothing on screen");
        self.count += 1;
        if let Some(d) = &self.dir {
            let file: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
            std::fs::write(d.join(format!("{file}.txt")), text).unwrap();
        }
    }
}

fn close(h: &mut H) {
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    // a click on the status bar closes what Escape left open
    h.hover_at(egui::pos2(700.0, 795.0));
    h.run_steps(2);
}

/// The menu bar's titles, in order (File, Edit, View, Node, Run, then those commands name).
fn titles() -> Vec<String> {
    let mut menus: Vec<String> = ["File", "Edit", "View", "Node", "Run"].iter().map(|s| (*s).to_owned()).collect();
    for spec in plumcraft_session::command_specs() {
        if let Some(m) = spec.menu.first()
            && !menus.iter().any(|x| x == m)
        {
            menus.push((*m).to_owned());
        }
    }
    menus
}

/// The submenus' buttons inside the menus open now ("Name ⏵"), top first.
fn submenus(h: &H) -> Vec<(String, egui::Pos2)> {
    let rects = popups(h);
    let mut out: Vec<(String, egui::Pos2)> = h
        .root()
        .children_recursive()
        .filter_map(|n| {
            let label = n.accesskit_node().label()?;
            let r = n.rect();
            (label.ends_with('⏵') && inside(&rects, r) && !n.accesskit_node().is_disabled()).then(|| (label, r.center()))
        })
        .collect();
    out.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
    out
}

/// Every menu of the menu bar, and each submenu in it (two levels down).
fn bar(d: &mut Dump, h: &mut H, state: &str) {
    for title in titles() {
        let Some(at) = h.query_all_by_label(&title).map(|n| n.rect()).find(|r| r.max.y <= 30.0).map(|r| r.center()) else { continue };
        h.hover_at(at);
        h.run_steps(1);
        common::click_at(h, at);
        h.run_steps(3);
        d.write(&format!("{state}.bar.{title}"), h);
        for (sub, at) in submenus(h) {
            h.hover_at(at);
            h.run_steps(4);
            d.write(&format!("{state}.bar.{title}.{sub}"), h);
            let inner: Vec<(String, egui::Pos2)> = submenus(h).into_iter().filter(|(s, _)| s != &sub).collect();
            if let Some((sub2, at2)) = inner.into_iter().find(|(_, p)| p.x > at.x + 40.0) {
                h.hover_at(egui::pos2(at2.x, at.y));
                h.run_steps(2);
                h.hover_at(at2);
                h.run_steps(4);
                d.write(&format!("{state}.bar.{title}.{sub}.{sub2}"), h);
                h.hover_at(at);
                h.run_steps(3);
            }
        }
        close(h);
    }
}

fn secondary(h: &mut H, at: egui::Pos2) {
    h.hover_at(at);
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(4);
}

/// Right-click node `id`'s header.
fn node_menu(h: &mut H, id: &str) {
    let r = h.state().canvas.node_rects_by_id().into_iter().find(|(n, _)| n.0 == id).unwrap().1;
    let at = h.state().canvas.to_global * (r.min + egui::vec2(40.0, 12.0));
    secondary(h, at);
}

/// Right-click the leftmost widget labelled `label` on the canvas (a caption).
fn caption_menu(h: &mut H, label: &str) {
    let local = h.query_all_by_label(label).map(|n| n.rect()).min_by(|a, b| a.left().total_cmp(&b.left())).expect("the caption").center();
    let at = h.state().canvas.to_global * local;
    secondary(h, at);
}

/// And every submenu in the menu open now.
fn with_subs(d: &mut Dump, h: &mut H, name: &str) {
    d.write(name, h);
    for (sub, at) in submenus(h) {
        h.hover_at(at);
        h.run_steps(4);
        d.write(&format!("{name}.{sub}"), h);
    }
}

fn choice(engine: &str, label: &str, mode: &str, mode_label: &str) -> EngineChoice {
    EngineChoice { engine: engine.into(), mode: mode.into(), engine_label: label.into(), mode_label: mode_label.into(), warning: None }
}

fn choices() -> Vec<EngineChoice> {
    vec![
        choice("text", "Text", "process", "Rust · separate process"),
        choice("text", "Text", "inproc", "Rust · in-process"),
        choice("audio", "Audio", "http", "Python · HTTP"),
    ]
}

/// literal → upper, a replace (an input to name), a slow node, a frame.
fn graph() -> (Session, [String; 4]) {
    let mut s = session();
    let a = add(&mut s, "text.literal", 0.0, 0.0);
    let u = add(&mut s, "text.upper", 420.0, 0.0);
    let r = add(&mut s, "text.replace", 0.0, 360.0);
    let w = add(&mut s, "text.slow", 420.0, 360.0);
    s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": u, "port": "text"}})).unwrap();
    s.execute("frame.add", &json!({"title": "inputs", "pos": [-40, 600], "size": [300, 200]})).unwrap();
    (s, [a, u, r, w])
}

fn linked(mut s: Session, mode: &str, auto: bool) -> EditorApp {
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    let conn: Arc<dyn Connection> = Arc::new(InProc(host));
    s.engines.choices = choices();
    s.engines.auto = auto;
    s.engines.current = Some(EngineTarget::Choice { engine: "text".into(), mode: mode.into() });
    EditorApp::new(s).with_link(Link::start(conn, "in-process", None).unwrap())
}

#[test]
fn every_menu_opens_and_says_what_it_offers() {
    let mut d = Dump::new();

    // nothing: no engine, an empty graph
    let mut h = harness(EditorApp::new(session()));
    bar(&mut d, &mut h, "empty");
    // the canvas's own right-click: the palette
    secondary(&mut h, egui::pos2(700.0, 400.0));
    d.write("empty.canvas", &h);
    close(&mut h);

    // engines to choose from, none connected
    let mut s = session();
    s.engines.choices = choices();
    let mut h = harness(EditorApp::new(s));
    bar(&mut d, &mut h, "choices");

    // a graph, two nodes selected
    let (s, [a, u, _, w]) = graph();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().canvas.picked.set(vec![NodeId::from(a.as_str()), NodeId::from(u.as_str())]);
    h.run_steps(2);
    bar(&mut d, &mut h, "selection");
    node_menu(&mut h, &a);
    with_subs(&mut d, &mut h, "selection.node-selected");
    close(&mut h);
    node_menu(&mut h, &w);
    with_subs(&mut d, &mut h, "selection.node-other");
    close(&mut h);
    // an input that can be named, then the output's label
    caption_menu(&mut h, "find");
    d.write("selection.input", &h);
    close(&mut h);
    caption_menu(&mut h, "seconds");
    d.write("selection.input-slow", &h);
    close(&mut h);
    // a wire
    let style = h.ctx.global_style();
    let catalog = h.state().session.catalog().clone();
    let mid = h.state().canvas.wire_midpoints(&style, &catalog)[0].0;
    secondary(&mut h, mid);
    d.write("selection.wire", &h);
    close(&mut h);
    // the frame's title
    let at = h.get_by_label("frame inputs").rect().center();
    secondary(&mut h, at);
    with_subs(&mut d, &mut h, "selection.frame");
    close(&mut h);

    // a named input, and a net's tag
    let (mut s, [_, u, r, _]) = graph();
    s.execute("graph.port.name", &json!({"port": {"node": r, "port": "find"}, "name": "needle"})).unwrap();
    s.execute("wire.label", &json!({"to": {"node": u, "port": "text"}, "name": "text"})).unwrap();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(2);
    caption_menu(&mut h, "find");
    d.write("named.input", &h);
    close(&mut h);
    let use_tag = plumcraft_ui::net_view::Tag::Use(PortRef::new(u.as_str(), "text"));
    if let Some(tag) = h.state().canvas.nets.tag_rect(&use_tag) {
        secondary(&mut h, tag.center());
        d.write("named.net", &h);
        close(&mut h);
    }

    // a group: its header, then collapsed (a proxy node)
    let (s, [_, _, r, w]) = graph();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().run("group.create", &json!({"nodes": [r, w]}));
    h.run_steps(4);
    if let Some(at) = h.query_by_label("group group 1").map(|n| n.rect().center()) {
        secondary(&mut h, at);
        with_subs(&mut d, &mut h, "group.header");
        close(&mut h);
    }
    node_menu(&mut h, &w);
    with_subs(&mut d, &mut h, "group.member");
    close(&mut h);
    bar(&mut d, &mut h, "group");
    h.state_mut().run("group.collapse", &json!({"group": "g1", "collapsed": true}));
    h.run_steps(4);
    let proxy = h.state().canvas.node_rects_by_id().into_iter().find(|(n, _)| !h.state().session.graph().nodes.contains_key(n)).map(|(n, _)| n.0);
    if let Some(p) = proxy {
        node_menu(&mut h, &p);
        with_subs(&mut d, &mut h, "group.proxy");
        close(&mut h);
    }

    // a run in progress
    let (s, _) = graph();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().session.run.running = true;
    h.run_steps(2);
    bar(&mut d, &mut h, "running");

    // a prompt open: New over unsaved changes
    let (s, _) = graph();
    let mut h = harness(EditorApp::new(s));
    h.state_mut().invoke("graph.new");
    h.run_steps(3);
    d.write("prompt.open", &h);
    bar(&mut d, &mut h, "prompt");

    // an engine compiled in, then one in a process, then picking automatically
    let (s, [a, ..]) = graph();
    let mut h = harness(linked(s, "inproc", false));
    h.run_steps(4);
    bar(&mut d, &mut h, "inproc");
    node_menu(&mut h, &a);
    with_subs(&mut d, &mut h, "inproc.node");
    close(&mut h);
    let (s, _) = graph();
    let mut h = harness(linked(s, "process", false));
    h.run_steps(4);
    bar(&mut d, &mut h, "process");
    let (s, _) = graph();
    let mut h = harness(linked(s, "inproc", true));
    h.run_steps(4);
    bar(&mut d, &mut h, "auto");

    assert!(d.count > 40, "only {} menus", d.count);
}

/// The Engine menu on a device that can't start programs: the process hostings are listed,
/// disabled (the reason is their tooltip), and what can start is as it was.
#[test]
fn the_engine_menu_shows_what_this_device_cant_start_disabled() {
    use plumcraft_editor_core::engines::{Capabilities, choices_for};
    let caps = Capabilities { subprocess: false, ..Capabilities::DESKTOP };
    let mut s = session();
    s.engines.choices = choices_for(&caps);
    let mut app = EditorApp::new(s);
    app.doc.caps = caps;
    let mut h = harness(app);
    common::click(&mut h, "Engine");
    h.run_steps(2);
    let text = h.get_by_label("Text ⏵").rect().center();
    h.hover_at(text);
    h.run_steps(4);
    let process = h.get_by_label("Rust · process (mode 2)");
    assert!(process.accesskit_node().is_disabled(), "the process hosting is disabled");
    assert!(!h.get_by_label("Rust · in-process (mode 1)").accesskit_node().is_disabled());
}
