//! E3 in the window: group the selection (Cmd+G), collapse it from its frame's header, a
//! promoted widget on the collapsed group, a wire dragged to the collapsed group's pin, expand
//! it from its chevron, enter and leave it, and ungroup (Cmd+Shift+G), each undoable.

mod common;

use common::{add, click_in_node, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::{GroupId, NodeId, PortRef};
use plumcraft_ui::EditorApp;
use serde_json::json;

const CMD: egui::Modifiers = egui::Modifiers::COMMAND;
const CMD_SHIFT: egui::Modifiers = egui::Modifiers { shift: true, ..egui::Modifiers::COMMAND };

fn press(h: &mut Harness<'static, EditorApp>, mods: egui::Modifiers, key: egui::Key) {
    h.key_press_modifiers(mods, key);
    h.run_steps(4);
}

/// story -> slow -> upper, and a lone literal to wire in later.
fn three() -> (EditorApp, [String; 4]) {
    let mut s = session();
    let story = add(&mut s, "text.literal", 0.0, 0.0);
    let slow = add(&mut s, "text.slow", 320.0, 0.0);
    let up = add(&mut s, "text.upper", 640.0, 0.0);
    let spare = add(&mut s, "text.literal", 0.0, 300.0);
    s.execute("wire.connect", &json!({"from": {"node": story, "port": "text"}, "to": {"node": slow, "port": "text"}})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": slow, "port": "text"}, "to": {"node": up, "port": "text"}})).unwrap();
    (EditorApp::new(s), [story, slow, up, spare])
}

fn g1(h: &Harness<'static, EditorApp>) -> Option<plumcraft_graph::Group> {
    h.state().session.graph().group(&GroupId::from("g1")).cloned()
}

#[test]
fn group_collapse_promote_wire_expand_and_ungroup() {
    let (app, [_story, slow, up, spare]) = three();
    let mut h = harness(app);
    h.state_mut().canvas.picked.set(vec![NodeId::from(slow.as_str()), NodeId::from(up.as_str())]);
    press(&mut h, CMD, egui::Key::G);
    let g = g1(&h).expect("Cmd+G grouped the selection");
    assert_eq!(g.nodes, [NodeId::from(slow.as_str()), NodeId::from(up.as_str())].into());
    assert_eq!(g.name, "group 1");
    assert_eq!(h.state().session.undo_label(), Some("Group"));
    h.run_steps(4);
    // the open group's frame has a header; its chevron collapses it
    h.get_by_label("group group 1");
    let at = h.get_by_label("Collapse group").rect().center();
    common::click_at(&mut h, at);
    assert!(g1(&h).unwrap().collapsed, "collapsed from the header");
    h.run_steps(4);
    // one proxy node: its title, the member input wired from outside, the output wired nowhere
    h.get_by_label("Group: group 1");
    let proxies = h.state().canvas.snarl.nodes_ids_data().filter(|(_, n)| n.value.proxy.is_some()).count();
    assert_eq!(proxies, 1);
    // promote slow's seconds: the collapsed group shows its widget
    h.state_mut().run("group.promote", &json!({"group": "g1", "port": {"node": slow, "port": "seconds"}, "label": "delay"}));
    h.run_steps(4);
    h.get_by_label("delay");
    let inputs: Vec<String> = h
        .state()
        .canvas
        .snarl
        .nodes_ids_data()
        .find_map(|(_, n)| n.value.proxy.as_ref().map(|_| n.value.inputs.iter().map(|r| r.label.clone()).collect()))
        .unwrap();
    assert_eq!(inputs[0], "delay");
    // a wire dragged from the spare literal to the group's "Slow › text" pin connects slow.text
    let catalog = h.state().session.catalog().clone();
    let from = h.state().canvas.output_pin_on_screen(&catalog, &PortRef::new(spare.as_str(), "text")).expect("the spare's pin");
    let to = h.state().canvas.input_pin_on_screen(&PortRef::new(slow.as_str(), "text")).expect("the proxy's pin for slow.text");
    h.drag_at(from);
    h.run_steps(2);
    h.hover_at(to);
    h.run_steps(2);
    h.drop_at(to);
    h.run_steps(4);
    let w = h.state().session.graph().wire_into(&PortRef::new(slow.as_str(), "text")).cloned().unwrap();
    assert_eq!(w.from, PortRef::new(spare.as_str(), "text"), "wired through the collapsed group");
    // enter it (the pencil), then leave from the breadcrumb
    click_in_node(&mut h, "Enter group");
    h.run_steps(4);
    assert_eq!(h.state().canvas.focus, Some(GroupId::from("g1")));
    assert_eq!(h.state().canvas.snarl.nodes_ids_data().count(), 2, "only its members");
    let at = h.get_by_label("Leave group").rect().center();
    common::click_at(&mut h, at);
    assert_eq!(h.state().canvas.focus, None);
    h.run_steps(4);
    // expand it from its chevron
    click_in_node(&mut h, "Expand group");
    assert!(!g1(&h).unwrap().collapsed);
    h.run_steps(4);
    // ungroup: the selected members' group
    h.state_mut().canvas.picked.set(vec![NodeId::from(up.as_str())]);
    press(&mut h, CMD_SHIFT, egui::Key::G);
    assert!(g1(&h).is_none(), "Cmd+Shift+G ungrouped it");
    assert_eq!(h.state().session.undo_label(), Some("Ungroup"));
    // Cmd+Z undoes the ungroup
    press(&mut h, CMD, egui::Key::Z);
    assert!(g1(&h).is_some(), "undo brought the group back");
}

#[test]
fn a_selected_collapsed_group_is_ungrouped_and_deleted_with_its_members() {
    let (app, [_story, slow, up, _]) = three();
    let mut h = harness(app);
    h.state_mut().run("group.create", &json!({"nodes": [slow, up], "name": "think", "collapsed": true}));
    h.run_steps(4);
    // a click on the proxy selects the group
    let r = h.get_by_label("Group: think").rect();
    common::click_canvas(&mut h, r);
    assert_eq!(h.state().canvas.picked.groups(), [GroupId::from("g1")]);
    assert_eq!(h.state().group_to_ungroup(), Some(GroupId::from("g1")));
    // Delete takes its members (and the emptied group)
    press(&mut h, egui::Modifiers::NONE, egui::Key::Delete);
    assert!(h.state().session.graph().groups.is_empty());
    assert_eq!(h.state().session.graph().nodes.len(), 2);
    press(&mut h, CMD, egui::Key::Z);
    assert!(g1(&h).is_some());
}

#[test]
fn ui_inspect_and_focus_report_groups() {
    let (app, [_, slow, up, _]) = three();
    let mut h = harness(app);
    h.state_mut().run("group.create", &json!({"nodes": [slow, up], "name": "think", "collapsed": true}));
    h.run_steps(4);
    let ctx = h.ctx.clone();
    let r = h.state_mut().control_now(&ctx, "ui.inspect", &json!({}));
    let groups = &r["result"]["groups"];
    assert_eq!(groups["list"][0]["name"], "think");
    assert_eq!(groups["proxies"][0]["group"], "g1");
    assert!(groups["proxies"][0]["inputs"].as_array().unwrap().iter().any(|i| i["port"] == "text" && i["node"] == slow.as_str()));
    let r = h.state_mut().control_now(&ctx, "ui.set", &json!({"focus": "g1"}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(h.state().canvas.focus, Some(GroupId::from("g1")));
    assert_eq!(h.state_mut().control_now(&ctx, "ui.set", &json!({"focus": "g9"}))["ok"], false);
    assert_eq!(h.state_mut().control_now(&ctx, "ui.set", &json!({"focus": 3}))["ok"], false);
    h.state_mut().control_now(&ctx, "ui.set", &json!({"focus": null}));
    assert_eq!(h.state().canvas.focus, None);
}
