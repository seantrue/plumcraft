# How a run is scheduled

This page covers:

- which nodes a run computes;
- in what order;
- how a node with a valid result is skipped.

The rules are in `docs/interface.md`: section 8.1 for runs and targets, section 7.6 for bindings, and section 14 for checkpoint reuse. This page says how the engines carry them out.

The scheduler is `run` in `crates/engine/src/exec.rs`, which every engine built on the Rust SDK uses. The Python SDK's `python/src/plumcraft_engine/exec.py` does the same, in the same order. The test `rust_and_python_engines_agree` compares their replies and status streams.

The editor doesn't schedule. Each run it sends the engine the whole graph, which nodes it wants, and how to use the cache (`RunRequest`). The engine decides the rest.

## 1. Which nodes run

`needed()` works out the set:

1. **Start from what was asked for:** the request's `targets`, or with no targets the graph's sinks (nodes with no wire leaving them), plus every watched node.
2. **Add every ancestor,** walking back along the wires.
3. **Leave out the rest.** A node outside the set isn't touched, not even looked up in the cache.

So a click on a node's title (`run.start {targets: [node]}`) runs that node and what feeds it, and nothing downstream.

**Bindings** come first (`RunRequest.inputs`, section 7.6 rule 10). A binding gives an input port its value directly, for example:

- a document saved in a craft app;
- a value from another engine in a multi-engine run.

The wire into that port is dropped, so whatever fed it is neither needed nor waited for.

## 2. In what order

`topo_order()` is Kahn's algorithm: it repeatedly takes a node whose inputs are all done, ties broken by node id, so the order is the same on every run and in both SDKs. A cycle fails the run before anything is computed.

Nodes then run one at a time in that order. Before each one:

- **Cancelled:** if the run was cancelled, the node is `Cancelled`.
- **An upstream node didn't finish:** the node is `Skipped`, with "upstream X did not finish". So an error stops what depends on it, and the graph's other branches still run.

## 3. How a valid result is recognised

Each node gets a **plan key** (section 14.2, `memo::plan_key`). It is a hash of:

- the node's kind and version;
- the engine's **salt**: its id, version and a digest of its code, so new engine code reuses nothing;
- the node's resolved **parameters**: widget values with defaults applied and numbers coerced to the port's type, so `2` and `2.0` key alike;
- for each wired input, **the upstream node's plan key**, port and slice.

The last item is what lets the engine skip work without looking at data. A key contains its parents' keys, which contain theirs. So it names the whole computation above the node, like a Merkle tree, and it is worked out from the graph alone, before anything runs.

The engine then looks the key up (section 14.3):

1. **In memory:** the engine's in-process memo, used only if the files its outputs point to are still there.
2. **In the store:** for a node whose memo scope is `machine`, results persisted on disk (section 14.5), which survive restarts.
3. **By content:** when the plan key misses, a **content key** made from the input bytes themselves. The same node on the same data hits, however that data was made: another route through the graph, or another engine.

A hit sends the stored outputs on with the state `Cached`, and counts the time saved in the run's statistics. A miss computes the node and stores its outputs under its keys.

## 4. What may use the cache

- **The request's `memo`:**
  - `use` reads and writes it.
  - `refresh` recomputes the targets but still reads it above them.
  - `off` computes everything.
- **The node definition's `memo.scope`** (section 14.4):
  - `none` is for a node whose output can differ on the same inputs, such as a craft query or a clock. It is never reused, and it passes a per-run identity downstream (`memo:none:<run>:<generation>:<node>`) so nothing below it is reused either.
  - `machine` is reused and persisted.
- **The workflow node's `cache`** (Node › Cache): `never` stops persistence, and `always` asks for it.
- **A volatile node:** a node that reads the outside world, such as a file loader, adds what it read to its key (`Node::volatile`, for example a file's size and modification time). A changed file then misses.

## 5. Several engines

With more than one engine connected, the coordinator (`crates/coordinator`, `docs/research/15-multi-engine.md`):

1. splits the graph into one fragment per engine;
2. runs the fragments in dependency order;
3. passes values between them as bindings.

Each engine runs the scheduler above on its fragment, with its own cache. A bound input is identified by its content (section 14.2), because another engine's plan key means nothing here. So a downstream engine still hits when the upstream engine recomputes the same bytes.

## 6. How many edits at once

Nothing tracks what changed. Every run works out every needed node's key afresh from the graph as it now stands, so:

- **Any number of edits between runs, anywhere,** are all taken into account. Every node whose own parameters changed, or that has a changed node above it, gets a new key and is computed. Everything else keeps its key and comes back `Cached`.
- **Edits while a run is going:**
  - With auto-run, the editor sends a new request after an edit, and **the latest request wins** (`crates/session/src/run.rs`).
  - The run in flight is cancelled, and its late records are ignored by generation.
  - The new run reuses whatever the cancelled one finished, since those results are stored under the same keys.
- **Changes outside the graph** are seen in these ways:
  - new engine code changes the salt;
  - a changed file changes a volatile node's key;
  - a document edited in a craft app is bound in as a changed input;
  - a `none`-scope node always recomputes.

  A change none of these sees isn't noticed. That's a node that reads the outside world without declaring it volatile or `none`. Such a node is a bug in that node.

The editor's **stale** marks are only a display, worked out the same way: a node is stale when its own values changed, or something above it did. They don't decide what runs. The keys do.
