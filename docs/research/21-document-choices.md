# 21. Choices from the document: pick the thing, not its key

This note is a design. Nothing in it is built.

The question: a port whose value is a key into the document (a SoundCraft insert slot, a track, a layer) shows the key: `0..9`, `a`, `Synth`. The person can't see what the key points at.

The maintainer, 2026-11-04:

- "If it's a key to an invisible map, shouldn't we set a list of descriptors/labels instead of an opaque key?"
- "I don't want to see "Insert B: Compressor" -- I want "Compressor". If there are companion parameters, they should be added: "Compressor: Level <x>""
- "There are default values. Not clear that they need to be changeable, unless the SoundCraft editor itself allows them to be changed."
- "I don't want to see the slot numbers w/o knowing what the refer to. You could add the slot to the end of the text ("Compressor: range 0-10 threshold -20 [a]") for slot a ... Pretty clear that if the actual node and values can change in a later instance, we need to capture the value/parameters for the graph, not just the slot."

## 1. Today

A port's `choices` is a list of values, fixed by the node type in the catalogue. The interface has no labels for choices.

Three related fields exist, and all are static:

- `disabled_choices` maps a value to why it can't run.
- `options` lists the sub-ports a choice brings, as `{choice: [Port…]}`.
- `fills` sets other ports from a choice.

None can depend on the document flowing into the node. A track's inserts are known only after the upstream nodes have run.

SoundCraft's `slot` (`mix.insert`, `mix.insert_params`, `mix.insert_bypass`, `mix.insert_remove`, `mix.insert_move`) is a key of this kind.

- **Its description:** `0..9 | a..j`. Those are two names for the same ten slots; `c` is slot 2.
- **What the auto widgets did:** first they offered the literal text `0..9` and `a..j`, which SoundCraft rejects. Fixed 2026-11-04, they now offer all 20 names, so each slot appears twice.

## 2. What the person sees

**Each choice is what the key points at, as the craft names it, then its settings, then the key last:**

- `Compressor/Limiter: threshold -20 dB, ratio 4 [a]`
- `Chorus: rate 0.8 Hz, depth 50% [b]`
- `(empty) [c]`

The settings are the parameters that differ from the plug-in's defaults. If none differ, the label is just `Compressor/Limiter [a]`.

**The key is never shown alone.** A port whose choices the engine can't describe yet isn't shown as a bare list of keys. Until there's a document to describe, it shows the last thing picked, or nothing.

A choice that has companion parameters brings them as fields under the choice, each holding the document's current value.

- **Editable only where the craft's own editor lets them change.** SoundCraft's plug-in catalogue has no read-only parameters. Every one of its 226 is a control in SoundCraft's plug-in window, and `mix.insert_params` sets any of them. A craft that marks some as fixed shows those read-only.
- **Not sent unless changed.** A field left alone keeps the document's value. A changed one goes out as that command's parameters, here `mix.insert_params` on that slot.

## 2a. What the graph saves: the thing, not the key

The document into the node can change in a later run, for example a different session, or an upstream edit that moves the inserts. Slot `a` may then hold something else. So the saved value records what was picked as well as where:

```json
{"slot": "a", "plugin": "compressor", "label": "Compressor/Limiter", "params": {"threshold": -20.0, "ratio": 4.0}, "changed": ["threshold"]}
```

- **`plugin` and `params` are what the person saw and picked:** the identity and settings at pick time.
- **`changed` lists the fields the person edited.** Only those are written on a run.
- **The memo key covers the whole descriptor,** so an edit or a different pick reruns the node.

**On a run, the engine checks the slot against the descriptor:**

- **Slot `a` still holds a compressor:** it is the one picked. Apply `changed`.
- **Something else is in slot `a`, but exactly one slot on the track holds a compressor:** use that slot, and say so in the node's text ("found in slot c").
- **No compressor, or several and none in `a`:** the node fails, naming both what it was given and what it found ("picked Compressor/Limiter in slot a; slot a now holds Chorus"). It never quietly acts on whatever sits in `a`.

The editor shows the saved label until the engine answers for the new document. If the answer differs, the port says what changed.

## 3. The interface (additive: a capability)

- **A new request, `port.choices`.**
  - **It sends:** `{node, port, inputs}`. `inputs` are the node's input values, as handles, as a run would get them.
  - **It replies:** `{choices: [{value, label, identity, ports?: [Port…], disabled?: why}]}`.
  - **`value`:** the key (`a`).
  - **`identity`:** what the key points at, in the craft's terms (`{plugin: "compressor"}`), so the editor can save the descriptor of 2a.
  - **`ports`:** the choice's companion parameters. A `Port` with `readonly: true` shows a value it can't change. Each port's `default` is the document's current value.
- **An engine offers it per port.** A new `Port.choices_from: "document"` says this port's choices come from `port.choices`. Static `choices` stay as the fallback.
- **The editor asks after the upstream nodes have run.** It caches the reply by the input handles and asks again when they change. A failure is data: the reply's error shows on the port, and the static choices stay.
- **What is saved on the node:** the port's value is the descriptor of 2a, not the bare key. An engine without the capability gets the bare key, as today.
- **The engine resolves it before running:** it maps the descriptor to a key as 2a says, and the changed values to the command that sets them.

As AGENTS rule 5 requires, this needs:

- `docs/interface.md`, in the same commit;
- the schema regenerated;
- the hostile-input sweep extended to the new reply;
- both SDKs.

## 4. The craft adapter, SoundCraft first

The pack declares the map, so no engine code knows SoundCraft:

- **Choices:** `session.inspect`, field `tracks[name=<track>].inserts`, with `value` from `slot`, `identity` from `plugin`, and `label` from the plug-in's name and its non-default `params`. Slots with nothing in them are `(empty)`.
- **Resolving a descriptor:** the same inspect, matching `plugin`.
- **Companion parameters:** `engine.plugins`, field `[id=<plugin>].params`, with `name`, `default`, `min`, `max`, `choices`, `unit` and `taper`. `params` from inspect overrides `default` where the insert has values.
- **Writing:** `mix.insert_params {track, slot, params}`.

The track is itself a key of this kind, with choices from `session.inspect`'s `tracks[].name`. So the same mechanism serves `track` everywhere in the pack, and FilmCraft's clips and tracks, EffectCraft's layers and PhotoCraft's layers.

## 5. Which nodes

`sc.compressor`, `sc.reverb`, `sc.delay` and `sc.insert` put a plug-in in a slot. For them, picking an occupied slot ("Chorus") means replacing it. That is legal, but it isn't the natural fit.

The natural fit is a node that adjusts an existing insert, `sc.insert_params` (to add). Pick "Compressor/Limiter" and its parameters appear. Bypass and remove would take the same port.

## 6. Until this is built

`slot` is hidden on the four insert nodes (`hide = ["slot"]` in `packs/soundcraft.toml`), as it was before the auto widgets. SoundCraft then picks the slot.

A bare key list such as `a`–`j` is what the maintainer ruled out, so it isn't offered as a stopgap. The same holds for any other key into a document that the auto widgets expose. A pack hides it until the port can describe its choices.

## 7. Open

- **Caching across documents:** whether the editor shows the last known labels, marked stale, while upstream nodes are running again, or falls back to the static choices.
- **Graph-level choices:** whether `port.choices` should also answer choices that don't come from a document.
