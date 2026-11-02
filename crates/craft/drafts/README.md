# Draft craft packs

Packs that are written but blocked, kept here so they are tracked. `build.rs` reads only `../packs/`, so nothing here is a built-in engine or an editor entry. Each draft's header and its notes in `claude-debug/craft-pack-<id>.md` say what blocks it.

| Draft | Blocked on |
| --- | --- |
| `lightcraft.toml` | LightCraft has no single-file document (a catalog). Needs a "stateless doc" kind in the adapter (the value is the source image, transforms carry develop settings, one export renders); a decision for the maintainer. |
| `mailcraft.toml` | MailCraft refuses agent saves of the mailbox by design. Needs a MailCraft change (agent `file.saveAs` of a copy into `--files` under `--allow mailbox`) and an adapter way to pass `--files` once. Tests use `--sample` only. |

To promote one: move it to `../packs/<id>.toml`, and its test then runs against the built-in pack.
