//! The engine manifest (spec 16.6): how PlumCraft finds and starts an engine that isn't compiled
//! in, as a TOML file. It is local configuration, not a message, but it comes from outside, so it
//! is parsed as hostile input (spec 18): bounded, never panicking, unknown keys ignored (pipekit's
//! among them), and an empty command, an unknown mode or a bad id is an error naming the field.
//! Resolving it (paths, the program) is the host's: `plumcraft_host::manifest`.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The largest manifest read.
pub const MAX_BYTES: usize = 64 << 10;
/// The longest engine id. An id is what prefixes node kinds, so it is short and plain.
pub const MAX_ID: usize = 32;

/// How an engine is hosted (spec 16.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Inproc,
    #[default]
    Process,
    Http,
    Pipekit,
}

/// What the generic conformance suite needs from an engine.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct Conformance {
    /// A node that takes `seconds`, for the cancel and off-the-run-path cases.
    pub slow: Option<String>,
}

/// An engine manifest, parsed and checked; paths are as written.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct EngineManifest {
    pub id: String,
    pub name: String,
    pub mode: Mode,
    /// The program and its arguments (`process`, or `http` when PlumCraft starts it).
    pub command: Vec<String>,
    /// Where it runs. Relative to the manifest's real directory.
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub restart: Option<String>,
    pub max_restarts: Option<u32>,
    /// An engine already running (`http`).
    pub url: Option<String>,
    pub token_file: Option<String>,
    pub hint: Option<String>,
    /// A workflow to open when this engine is chosen with none. Relative as `cwd` is.
    pub demo: Option<String>,
    pub conformance: Conformance,
}

/// Whether `id` can name an engine: a lower-case letter, then lower-case letters, digits and `_`.
pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= MAX_ID && chars.next().is_some_and(|c| c.is_ascii_lowercase()) && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl EngineManifest {
    /// Parse and check a manifest.
    pub fn parse(text: &str) -> Result<EngineManifest, String> {
        if text.len() > MAX_BYTES {
            return Err(format!("a manifest is at most {} KiB", MAX_BYTES >> 10));
        }
        let m: EngineManifest = toml::from_str(text).map_err(|e| e.message().to_owned())?;
        if !valid_id(&m.id) {
            let id: String = m.id.chars().take(40).collect();
            return Err(format!("id {id:?}: lower-case letters, digits and _, starting with a letter, at most {MAX_ID}"));
        }
        let started = m.command.iter().any(|c| !c.is_empty());
        if m.command.first().is_some_and(String::is_empty) {
            return Err("command: its first word is empty".into());
        }
        match m.mode {
            Mode::Process if !started => Err("mode process needs a command".into()),
            Mode::Http if !started && m.url.as_deref().is_none_or(str::is_empty) => Err("mode http needs a command or a url".into()),
            _ => Ok(m),
        }
    }

    /// The name to show: `name`, else the id.
    pub fn title(&self) -> &str {
        if self.name.is_empty() { &self.id } else { &self.name }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: &str = r#"
id = "weather"
name = "Weather"
mode = "process"
command = ["uv", "run", "--quiet", "--project", ".", "python", "-m", "weather_engine"]
cwd = "."
hint = "uv sync, in this project"
demo = "examples/demo.json"
restart = "on-crash"
max_restarts = 5

[env]
YAPOCIS_PROJECT = "~/workspace/yapocis_external"

[conformance]
slow = "weather.slow"
"#;

    #[test]
    fn a_generated_manifest_reads() {
        let m = EngineManifest::parse(GENERATED).unwrap();
        assert_eq!((m.id.as_str(), m.title(), m.mode), ("weather", "Weather", Mode::Process));
        assert_eq!(m.command[0], "uv");
        assert_eq!(m.conformance.slow.as_deref(), Some("weather.slow"));
        assert_eq!(m.demo.as_deref(), Some("examples/demo.json"));
        assert_eq!(m.env["YAPOCIS_PROJECT"], "~/workspace/yapocis_external");
    }

    #[test]
    fn unknown_keys_are_ignored_and_mode_defaults() {
        let m = EngineManifest::parse("id = \"a\"\ncommand = [\"x\"]\nbroker = \"amqp://localhost\"\nsystem = \"vimp\"\n[later]\nz = 1\n").unwrap();
        assert_eq!(m.mode, Mode::Process);
        assert_eq!(m.title(), "a");
        assert_eq!(EngineManifest::parse("id = \"a\"\nmode = \"http\"\nurl = \"http://127.0.0.1:1\"\n").unwrap().mode, Mode::Http);
        assert_eq!(EngineManifest::parse("id = \"a\"\nmode = \"pipekit\"\n").unwrap().mode, Mode::Pipekit);
    }

    #[test]
    fn what_cant_work_is_an_error_naming_it() {
        let err = |t: &str| EngineManifest::parse(t).unwrap_err();
        assert!(err("name = \"no id\"").contains("id \"\""));
        for bad in ["Bad", "9a", "a-b", "a.b", "", "é"] {
            assert!(err(&format!("id = \"{bad}\"\ncommand = [\"x\"]")).contains("lower-case"), "{bad}");
        }
        assert!(err(&format!("id = \"{}\"\ncommand = [\"x\"]", "a".repeat(33))).contains("lower-case"));
        assert!(err("id = \"a\"").contains("needs a command"));
        assert!(err("id = \"a\"\ncommand = []").contains("needs a command"));
        assert!(err("id = \"a\"\ncommand = [\"\", \"x\"]").contains("first word"));
        assert!(err("id = \"a\"\nmode = \"http\"").contains("command or a url"));
        assert!(EngineManifest::parse("id = \"a\"\nmode = \"carrier-pigeon\"\ncommand = [\"x\"]").is_err());
        assert!(EngineManifest::parse("id = \"a\"\ncommand = \"not a list\"").is_err());
        assert!(err(&"#".repeat(MAX_BYTES + 1)).contains("at most"));
    }
}
