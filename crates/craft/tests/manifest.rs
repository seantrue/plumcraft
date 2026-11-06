//! Manifests: the built-in packs parse, and malformed or hostile ones are errors, never panics.

use plumcraft_craft::manifest::{Kind, Manifest, builtin, choose_among, fits, version_parts};

#[test]
fn the_built_in_packs_parse() {
    let vc = builtin("vectorcraft").unwrap().unwrap();
    assert_eq!(vc.id, "vectorcraft");
    assert_eq!(vc.doc.wire, "vectorcraft.doc");
    assert_eq!(vc.env.get("SOURCE_DATE_EPOCH").map(String::as_str), Some("0"), "saves are reproducible only with the date pinned");
    let curated: Vec<&str> = vc.nodes.iter().map(|n| n.id.as_str()).collect();
    for id in [
        "vc.open",
        "vc.new",
        "vc.trace",
        "vc.colors",
        "vc.recolor",
        "vc.reduce_colors",
        "vc.randomize_colors",
        "vc.simplify",
        "vc.offset_path",
        "vc.outline_stroke",
        "vc.pathfinder",
        "vc.cleanup",
        "vc.move",
        "vc.scale",
        "vc.rotate",
        "vc.fill",
        "vc.stroke",
        "vc.expand",
        "vc.halftone",
        "vc.mosaic",
        "vc.fit_artboard",
        "vc.export_svg",
        "vc.export_png",
        "vc.export_pdf",
    ] {
        assert!(curated.contains(&id), "note 14's curated node {id}");
    }
    assert_eq!(curated.len(), 25, "the 24 of note 14, 2.3, and vc.expand_appearance");
    assert!(vc.nodes.iter().filter(|n| n.kind == Kind::Export).all(|n| n.format.is_some()));
    assert!(!vc.nodes.iter().find(|n| n.id == "vc.export_pdf").unwrap().pure, "PDF carries ModDate: memo scope none (D43, D47)");
    assert_eq!(vc.generated.len(), 1);
    assert!(vc.run_node.is_some());
    let sc = builtin("soundcraft").unwrap().unwrap();
    assert_eq!((sc.tools.run.tool.as_str(), sc.tools.run.id.as_str()), ("execute", "command"));
    assert!(builtin("nosuchcraft").is_none());
}

#[test]
fn bad_manifests_are_errors() {
    let good = plumcraft_craft::manifest::SOUNDCRAFT;
    assert!(Manifest::parse(good).is_ok());
    let cases = [
        good.replace("id = \"soundcraft\"", "id = \"../etc\""),
        good.replace("command = [\"soundcraft-cli\", \"mcp\"]", "command = []"),
        good.replace("[roots]", "surprise = 1\n[roots]"),
        good.replace("stem = \"session\"", "stem = \"../x\""),
        good.replace("id = \"sc.inspect\"", "id = \"sc.new\""),
        good.replace("kind = \"query\"", "kind = \"teleport\""),
        format!("{good}\n[[node]]\nid = \"sc.x\"\ndisplay = \"x\"\nkind = \"export\"\n"),
        format!("{good}\n[[node]]\nid = \"sc.y\"\ndisplay = \"y\"\nkind = \"transform\"\ncommand = \"a\"\nselect = \"all\"\n"),
        format!("{good}\n[[node]]\nid = \"sc.z\"\ndisplay = \"z\"\nkind = \"transform\"\n"),
        String::new(),
        "\u{0}\u{1}garbage = = =".into(),
        "id = 3".into(),
        "[[[[".into(),
    ];
    for (i, c) in cases.iter().enumerate() {
        assert!(Manifest::parse(c).is_err(), "case {i} parsed");
    }
    assert!(Manifest::load(std::path::Path::new("/no/such/manifest.toml")).is_err());
    assert!(Manifest::named("/no/such/manifest.toml").is_err());
}

#[test]
fn a_manifest_file_loads() {
    let dir = plumcraft_scratch::root().join("craft-manifest");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("sc.toml");
    std::fs::write(&f, plumcraft_craft::manifest::SOUNDCRAFT).unwrap();
    assert_eq!(Manifest::named(f.to_str().unwrap()).unwrap().id, "soundcraft");
    let big = dir.join("big.toml");
    std::fs::write(&big, vec![b'#'; 2 << 20]).unwrap();
    assert!(Manifest::load(&big).unwrap_err().contains("over"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// M3: the VectorCraft pack names its desktop app (with an env override) and ships a demo whose
/// every node is a curated one.
#[test]
fn the_vectorcraft_pack_names_its_app_and_a_demo() {
    let vc = builtin("vectorcraft").unwrap().unwrap();
    let gui = vc.gui.clone().expect("a [gui] section");
    assert_eq!(gui.label.as_deref(), Some("VectorCraft"));
    assert_eq!(gui.env.as_deref(), Some("PLUMCRAFT_VECTORCRAFT_APP"));
    assert_eq!(gui.command, ["vectorcraft"]);
    assert!(!gui.program()[0].starts_with('~'), "~ is expanded");
    // a missing app is an error naming the path and the fix, never a panic
    let missing = plumcraft_craft::manifest::GuiSpec { command: vec!["/no/such/vectorcraft".into()], env: None, label: None, hint: "build it".into() };
    let e = missing.executable().unwrap_err();
    assert!(e.contains("/no/such/vectorcraft") && e.contains("build it"), "{e}");
    let on_path = plumcraft_craft::manifest::GuiSpec {
        command: vec!["no-such-app-on-path-xyz".into()],
        env: Some("PLUMCRAFT_NO_SUCH_VAR".into()),
        label: None,
        hint: String::new(),
    };
    assert!(on_path.executable().unwrap_err().contains("PLUMCRAFT_NO_SUCH_VAR"));
    // SoundCraft names its app too since its pack grew (2026-10-28); a pack without [gui] is a None.
    assert_eq!(builtin("soundcraft").unwrap().unwrap().gui.and_then(|g| g.label).as_deref(), Some("SoundCraft"));
    let bad = plumcraft_craft::manifest::VECTORCRAFT.replace("command = [\"vectorcraft\"]", "command = []");
    assert!(Manifest::parse(&bad).unwrap_err().contains("gui.command"));
    let demo: serde_json::Value = serde_json::from_str(plumcraft_craft::manifest::demo("vectorcraft").unwrap()).unwrap();
    let kinds: Vec<&str> = demo["nodes"].as_object().unwrap().values().map(|n| n["kind"].as_str().unwrap()).collect();
    assert!(kinds.iter().all(|k| vc.nodes.iter().any(|n| n.id == *k)), "{kinds:?}");
    assert!(plumcraft_craft::manifest::demo("nosuchcraft").is_none());
}

// ---------- B17 (#56): packs versioned against their app ----------

#[test]
fn versions_read_and_requirements_match() {
    assert_eq!(version_parts("0.20.2"), Some(vec![0, 20, 2]));
    assert_eq!(version_parts("0.21.0-beta.3"), Some(vec![0, 21, 0]));
    assert_eq!(version_parts("v1"), None);
    assert!(fits(">=0.20.2", "0.20.2").unwrap());
    assert!(fits(">=0.20.2", "0.21").unwrap());
    assert!(!fits(">=0.20.2", "0.20.1").unwrap());
    assert!(fits(">=0.20, <0.21", "0.20.9").unwrap());
    assert!(!fits(">=0.20, <0.21", "0.21.0").unwrap());
    assert!(fits("=0.20", "0.20.7").unwrap(), "= takes the components given");
    assert!(!fits("=0.20.1", "0.20").unwrap());
    assert!(!fits(">=0.1", "unknown").unwrap(), "a version that doesn't read meets nothing");
    for bad in ["0.20", "~0.2", ">=", ">=x", &">=1,".repeat(20)] {
        assert!(fits(bad, "1.0").is_err(), "{bad:?}");
    }
}

/// The SoundCraft pack, with another version and range.
fn pack(version: &str, apps: &str) -> String {
    plumcraft_craft::manifest::SOUNDCRAFT
        .replace("pack_version = \"1\"", &format!("pack_version = \"{version}\""))
        .replace("app_versions = \">=0.17.1\"", &format!("app_versions = \"{apps}\""))
}

#[test]
fn the_newest_pack_that_fits_the_app_is_chosen() {
    let mut newest = Manifest::parse(&pack("3", ">=2.0")).unwrap();
    newest.builtin = true;
    let (old, older) = (pack("2", ">=1.5, <2.0"), pack("1", "<1.5"));
    let variants = [("soundcraft", old.as_str()), ("soundcraft", older.as_str()), ("other", "id = \"other\"")];
    let pick = |v: &str| {
        let (m, w) = choose_among(&newest, &variants, v);
        (m.pack_version, w.is_some())
    };
    assert_eq!(pick("2.3.1"), ("3".into(), false));
    assert_eq!(pick("1.7"), ("2".into(), false));
    assert_eq!(pick("1.0"), ("1".into(), false));
    // a version none fits: the newest, with a warning
    let (m, w) = choose_among(&Manifest::parse(&pack("3", ">=2.0, <3")).unwrap(), &[], "4.0");
    assert_eq!(m.pack_version, "3");
    assert!(w.unwrap().contains("SoundCraft 4.0 isn't a version its pack (pack 3) was made for (>=2.0, <3)"));
    // a manifest that isn't built in is never swapped
    let file = Manifest::parse(&pack("3", ">=2.0")).unwrap();
    assert_eq!(choose_among(&file, &variants, "1.7").0.pack_version, "3");
}

#[test]
fn a_bad_pack_version_or_range_is_refused() {
    assert!(Manifest::parse(&pack("one", ">=1")).unwrap_err().contains("pack_version"));
    assert!(Manifest::parse(&pack("1", "about 1")).unwrap_err().contains("app_versions"));
    // without either: version 1, any app
    let m = Manifest::parse(&plumcraft_craft::manifest::SOUNDCRAFT.replace("pack_version = \"1\"\n", "").replace("app_versions = \">=0.17.1\"\n", "")).unwrap();
    assert_eq!((m.pack_version.as_str(), m.app_versions.as_deref()), ("1", None));
}

#[test]
fn every_built_in_pack_says_its_version_and_the_apps_it_fits() {
    for id in plumcraft_craft::manifest::BUILTIN {
        let m = plumcraft_craft::manifest::builtin(id).unwrap().unwrap();
        assert!(m.builtin, "{id}");
        assert!(m.app_versions.is_some(), "{id}: no app_versions");
    }
}
