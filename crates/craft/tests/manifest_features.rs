//! The manifest fields added for the other crafts: export nodes with their own action, optional
//! roots, `selection.set_key`, `tools.list_resource`, and explicit document ids.

use plumcraft_craft::manifest::Manifest;

const BASE: &str = r#"
id = "toy"
name = "Toy"
command = ["toy-cli", "mcp"]

[tools]
list = "list_commands"
run = { tool = "execute", id = "command", params = "params" }

[actions]
open = { tool = "open", path = "path" }
save = { tool = "save", path = "path" }

[doc]
wire = "toy.doc"
ext = "toy"
media = "application/x-toy"
app = "toy"
"#;

#[test]
fn a_pack_without_roots_parses_unconfined() {
    let m = Manifest::parse(BASE).unwrap();
    assert!(m.roots.is_none());
    assert!(m.tools.list_resource.is_none() && m.doc.id_param.is_none() && m.actions.open.returns.is_none());
}

#[test]
fn export_nodes_need_a_format_or_their_own_command_or_action() {
    let node = |body: &str| format!("{BASE}\n[[node]]\nid = \"t.png\"\ndisplay = \"PNG\"\nkind = \"export\"\next = \"png\"\n{body}\n");
    // neither: refused
    assert!(Manifest::parse(&node("")).unwrap_err().contains("own command or action"));
    // a format (the shared export action), a command, or an action: each is fine
    for body in ["format = \"png\"", "command = \"frame.save\"", "action = { tool = \"screenshot\", path = \"path\" }"] {
        let m = Manifest::parse(&node(body)).unwrap_or_else(|e| panic!("{body}: {e}"));
        assert_eq!(m.nodes.len(), 1);
    }
    // an action with neither a tool nor a command is refused
    assert!(Manifest::parse(&node("action = { path = \"path\" }")).unwrap_err().contains("neither a tool nor a command"));
    // ext is still required and plain
    let no_ext = format!("{BASE}\n[[node]]\nid = \"t.x\"\ndisplay = \"X\"\nkind = \"export\"\ncommand = \"x\"\n");
    assert!(Manifest::parse(&no_ext).is_err());
}

#[test]
fn selection_keys_resources_and_document_ids_parse() {
    let text = BASE
        .replace("list = \"list_commands\"", "list = \"list_commands\"\nlist_resource = \"toy://commands\"")
        .replace("open = { tool = \"open\", path = \"path\" }", "open = { tool = \"open\", path = \"path\", returns = \"doc\" }")
        .replace("app = \"toy\"", "app = \"toy\"\nid_param = \"doc\"")
        + "\n[selection]\nall = \"select.all\"\nnone = \"select.none\"\nset = \"select.set\"\nset_key = \"handles\"\n";
    let m = Manifest::parse(&text).unwrap();
    assert_eq!(m.tools.list_resource.as_deref(), Some("toy://commands"));
    assert_eq!(m.actions.open.returns.as_deref(), Some("doc"));
    assert_eq!(m.doc.id_param.as_deref(), Some("doc"));
    assert_eq!(m.selection.as_ref().and_then(|s| s.set_key.as_deref()), Some("handles"));
    // unknown fields are still refused
    assert!(Manifest::parse(&format!("{text}\nsurprise = 1\n")).is_err() || Manifest::parse(&text.replace("set_key", "set_keys")).is_err());
}
