//! End to end, as a developer meets it (`docs/research/17-engine-generator.md`, step 11): generate
//! an engine with every type, sync it, run its own checks, hold it to conformance on both
//! bindings, run its demo through PlumCraft, then follow its README to add a node. Needs uv, as
//! the Python conformance cases do.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ALL_TYPES: &str = "text,table,array,svg,image,audio,json,list,any";

fn text(o: &Output) -> String {
    format!("{}\n--- stderr\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn plumcraft(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plumcraft")).args(args).output().expect("plumcraft runs")
}

fn uv() -> String {
    std::env::var("PLUMCRAFT_UV").unwrap_or_else(|_| "uv".into())
}

fn in_project(dest: &Path, program: &str, args: &[&str]) -> Output {
    let mut cmd = script(program);
    let o = cmd.args(args).current_dir(dest).output().unwrap_or_else(|e| panic!("{program}: {e}"));
    assert!(o.status.success(), "{program} {args:?}\n{}", text(&o));
    o
}

/// A command for `program`. The generated scripts are bash; Windows runs them with Git for
/// Windows' bash (`$PLUMCRAFT_BASH`, else its usual place), as the generated README says.
fn script(program: &str) -> Command {
    if cfg!(windows) && program.starts_with("scripts/") {
        let bash = std::env::var("PLUMCRAFT_BASH").unwrap_or_else(|_| r"C:\Program Files\Git\bin\bash.exe".into());
        let mut c = Command::new(bash);
        c.arg(program);
        c
    } else {
        Command::new(program)
    }
}

fn generate(dest: &Path, types: &str) {
    let python: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python").canonicalize().expect("python/ is here");
    let new = Command::new(uv())
        .args(["run", "--project"])
        .arg(&python)
        .args(["plumcraft-engine", "new"])
        .arg(dest)
        .args(["--name", "Atlas", "--types", types])
        .output()
        .expect("uv runs");
    assert!(new.status.success(), "{}", text(&new));
}

#[test]
fn a_generated_engine_with_every_type_works_end_to_end() {
    let dest = plumcraft_scratch::dir("generated-end-to-end").join("atlas-engine");
    generate(&dest, ALL_TYPES);
    in_project(&dest, &uv(), &["sync", "--quiet"]);
    assert!(dest.join("uv.lock").is_file(), "uv.lock is committed with the project");
    let check = in_project(&dest, "scripts/check", &[]);
    assert!(text(&check).contains(" passed"), "{}", text(&check));
    let m = dest.join("plumcraft-engine.toml").display().to_string();
    let conform = plumcraft(&["conform", "--engine-manifest", &m]);
    assert!(conform.status.success() && text(&conform).contains("18 of 18 cases hold"), "{}", text(&conform));
    for how in [&["run", "--engine-manifest", &m, "--watch", "green.svg"][..], &["run", "--engine-manifest", &m, "--http", "--watch", "quiet.audio"][..]] {
        let run = plumcraft(how);
        let out = text(&run);
        assert!(run.status.success(), "{out}");
        let demo: serde_json::Value = serde_json::from_slice(&std::fs::read(dest.join("examples/demo.json")).unwrap()).unwrap();
        let nodes = demo["nodes"].as_object().unwrap().len();
        let done = out.lines().filter(|l| l.split_whitespace().nth(1) == Some("done")).count();
        assert_eq!(done, nodes, "every node of the demo done, in the summary table: {out}");
    }
}

/// The generated README's "Adding a node", followed to the letter.
#[test]
fn the_readme_on_adding_a_node_is_true() {
    let dest = plumcraft_scratch::dir("generated-readme").join("atlas-engine");
    generate(&dest, "text");
    in_project(&dest, &uv(), &["sync", "--quiet"]);
    let readme = std::fs::read_to_string(dest.join("README.md")).unwrap();
    let start = readme.find("```python\n").expect("the README shows a node") + "```python\n".len();
    let example = &readme[start..start + readme[start..].find("```").unwrap()];
    let source = dest.join("src/atlas_engine/__init__.py");
    let code = std::fs::read_to_string(&source).unwrap();
    let indented: String = example.lines().map(|l| if l.is_empty() { "\n".to_owned() } else { format!("    {l}\n") }).collect();
    std::fs::write(&source, format!("{code}\n{indented}")).unwrap();
    // The catalogue changed, so the golden says so until it is rewritten, as the README says.
    let stale = Command::new(uv()).args(["run", "pytest", "-q"]).current_dir(&dest).output().unwrap();
    assert!(!stale.status.success() && text(&stale).contains("test_the_catalogue_matches_the_golden"), "{}", text(&stale));
    let updated = Command::new(uv()).args(["run", "pytest", "-q"]).env("UPDATE_GOLDEN", "1").current_dir(&dest).output().unwrap();
    assert!(updated.status.success(), "{}", text(&updated));
    in_project(&dest, "scripts/check", &[]);
    let nodes = in_project(&dest, "scripts/nodes", &[]);
    assert!(text(&nodes).contains("atlas.reverse"), "{}", text(&nodes));
}
