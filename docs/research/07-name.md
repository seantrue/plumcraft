# 07: Project name for the generic node-flow base

*2026-10-27. Research only. Web search and registry lookups per name.*

## Decision

**PlumCraft**, chosen by the maintainer on 2026-10-27 after this note recommended WireCraft. The candidates below were screened afterwards in conversation, on 2026-10-27, by web search and registry lookups:

| Name | Registries | Verdict |
|---|---|---|
| **PlumCraft** | crates.io, PyPI (`plumcraft`, `pyplumcraft`) and npm free; GitHub user name taken by an inactive personal account (Olivier Lebhard, last updated 2021); `.com` parked; `.dev`, `.app` and `.io` don't resolve | **Chosen**: a pun on plumbing (piping data between nodes, a nod to pipekit) |
| PlumbCraft | crates.io, PyPI, npm free; GitHub user name taken (2026); `.com` is a plumbing business | Usable second; search results are dominated by the hardware brand |
| Plumbium | PyPI held by a dormant 2016 package by Jon Stutters ("Records the inputs and outputs of scripts"); crates.io and npm free; GitHub user name taken by an empty account; `.com` registered | Usable but breaks the `{Function}Craft` convention; not a real word (Latin *plumbum* is lead) |
| PipeCraft | PyPI (a pipeline library), npm (a CI/CD pipeline generator, active) and GitHub org taken; crates.io free; `.dev`, `.app`, `.io` registered | Reject: crowded with pipeline tools; also PipeCraft2 (bioinformatics GUI) and a 2026 iOS pipe-planning app |
| NodeCraft | crates.io (about 92k downloads), PyPI (an AI workflow framework), npm and GitHub org all taken | Reject |

Registry naming follows the crafts: crates `plumcraft-*`, PyPI `plumcraft` (or the py-prefixed form), and the repository under an existing GitHub owner, since the `plumcraft` user name is taken.

## Original recommendation (superseded)

**WireCraft.** Products in the family are named `{Function}Craft`, and the function of this app is wiring nodes together. "Wire" is already the word used in the code and docs ("parameter wires", egui-snarl wires, wire types), so the name describes what the user does, as PhotoCraft and SoundCraft do.

- **Registries:** the `wirecraft` names on crates.io and PyPI, the GitHub org name, and `wirecraft.dev` were all free on 2026-10-27.
- **The main conflict:** an active npm package `wirecraft` (a local API gateway and mock engine, last updated 2026-08-31) and `wirecraft.app` ("Free servers and network tools"). Both are small, unregistered and in the developer-tool space. The name is still usable, but it has neighbours. The research software supporting www.virtimp.com ships no web technology, so we never need the npm name.

**Fallbacks:**

1. **DagCraft** is the cleanest name found: nothing on any registry. Two drawbacks: "DAG" is jargon, and in British and Australian slang "dag" means a dork (or dung-matted wool).
2. **JunctionCraft**: also clean everywhere, but long.

**Next step:** reserve `wirecraft` on crates.io and PyPI, or use the py-prefixed PyPI convention from the FloCraft notes. Also reserve the GitHub org and `wirecraft.dev` soon, since names like these get taken quickly.

## Method

For each candidate, on 2026-10-27:

1. A general web search: products, companies and games, especially Minecraft-style "*craft" mods and servers.
2. Registry lookups by script:
   - crates.io API;
   - PyPI JSON;
   - the npm registry;
   - the GitHub user or org endpoint;
   - GitHub repository search for the leading names.
3. DNS resolution of `.com`, `.dev` and `.app`, plus the homepage title where a domain resolves.

A resolving domain means it is registered, not that it is in use. The scripts are in the session scratchpad, not in the repo.

Names already used in the family were excluded: cadcraft, deckcraft, designcraft, effectcraft, figcraft, filmcraft, gridcraft, lightcraft, mailcraft, pdfcraft, photocraft, solidcraft, soundcraft, vectorcraft, wordcraft and the research software.

## Top 5

| Rank | Name | crates.io | PyPI | npm | GitHub user/org | Domains (.com/.dev/.app) | Web conflicts | Fit |
|---|---|---|---|---|---|---|---|---|
| 1 | WireCraft | free | free | **taken** (API gateway tool, active 2026) | free | .com registered (timed out), .dev free, .app = "free servers and network tools" | Wirecrafters LLC (wire partitions); small repos (wirecraft-team, a Minecraft-over-WireGuard tunnel, a Craft CMS boilerplate) | Best: "wire" is the core gesture of a node editor |
| 2 | DagCraft | free | free | free | free | .com registered (timed out); .dev, .app free | "DDAG Craft" Minecraft modpack (different spelling); 2 empty repos | Precise (DAG) but jargon, plus the slang meaning |
| 3 | JunctionCraft | free | free | free | free | .com registered (blank page); .dev, .app free | None found | Good (where wires meet); long |
| 4 | WeirCraft | free | free | free | free | none resolve | None found | Flow-control metaphor; obscure, and sounds like "weird craft" |
| 5 | LoomCraft | free | free | free | **taken** | all three registered | Textile businesses; jity16/Loomcraft ("AI-native topology engine", adjacent); Loom (Atlassian) sits close by | Weaving threads together; risk of confusion with Loom |

## All candidates screened

| Name | Verdict | Main evidence |
|---|---|---|
| NodeCraft | Reject | Nodecraft Inc. (game server hosting); dRPC "NodeCraft"; crates.io, PyPI, npm and GitHub all taken |
| GraphCraft | Reject | GraphCraft Unity asset (a graph-driven object builder, so the same domain); npm taken; GitHub user taken |
| FlowCraft | Reject | Nextflow pipeline builder on PyPI; a Flutter whiteboard with an MCP server; an OutSystems flow editor; all registries taken |
| FloCraft | Weak | Daracyii/FloCraft on GitHub; flocraft.com is for sale (HugeDomains); "Flo" bends the convention |
| PipeCraft | Reject | PipeCraft2 (a GUI pipeline tool for metabarcoding); iOS PipeCraft app (2026); PyPI and npm taken |
| PatchCraft | Reject | Ultra PRO products; SAP Commerce PatchCraft plugin; PyPI taken |
| RigCraft | Reject | Several "RigCraft" PC-builder apps; all domains registered; "rig" suggests 3D rigging |
| PlumbCraft | Reject | PlumbCraft retail plumbing brand (Waxman, Ace Hardware) and UK and NZ plumbing firms |
| WeaveCraft | Reject | Cornell/Stanford Weavecraft (SIGGRAPH Asia 2020); a Minecraft mod; furniture and saree brands |
| FlumeCraft | Weak | Registries clear, but leans on Apache Flume, a dataflow product |
| SpliceCraft | Reject | Splice's "Craft" feature (music); PyPI taken |
| PlexCraft | Weak | Registries clear, but sits next to Plex Inc.; PlexCraft YouTube channel |
| KnotCraft | Reject | Knot-tying books; GitHub user taken |
| SluiceCraft | Clean, awkward | Nothing anywhere; hard to say |
| TrellisCraft | Weak | Trellis software (a cannabis platform); a garden book |
| ConduitCraft | Weak | GitHub user taken; the Conduit Minecraft mod; long |
| RelayCraft | Weak | npm and GitHub taken; Magnecraft relays nearby |
| TetherCraft | Clean, weak fit | Nothing found; "tether" suggests restraint, not flow |
| SocketCraft | Reject | Socket Craft, a custom software company (socketcraft.com) |
| DuctCraft | Clean, weak fit | Nothing found; HVAC connotation |
| CircuitCraft | Reject | Rust desktop circuit editor (2026); Minecraft mod; itch.io app |
| SignalCraft | Reject | npm and GitHub taken; all domains registered |
| StageCraft | Reject | ILM StageCraft; all registries taken |
| NetCraft | Reject | Netcraft Ltd (internet security company) |
| ChainCraft, StreamCraft, LinkCraft, MeshCraft, FluxCraft | Reject | Taken on registries and/or as Minecraft mods (FluxCraft MQTT mod) |

## Sources

- [Nodecraft (game hosting)](https://nodecraft.com/blog/company-updates/nodecraft-turns-five)
- [dRPC NodeCraft](https://drpc.org/blog/?p=3281)
- [GraphCraft Unity asset](https://marketplace.unity.com/packages/tools/modeling/graphcraft-27813)
- [GraphCraft npm (scoped)](https://socket.dev/npm/package/@tpypan/graphcraft)
- [FlowCraft (Nextflow)](https://flowcraft.readthedocs.io/en/dev/getting_started/overview.html)
- [FlowCraft OutSystems](https://www.outsystems.com/forge/component-overview/24771/flowcraft-o11)
- [FlowCraft MCP whiteboard](https://mcpmarket.com/server/flowcraft)
- [PipeCraft2](https://pipecraft2-manual.readthedocs.io/en/1.0.0/index.html)
- [PatchCraft SAP plugin](https://www.webshopextension.com/en/sap-commerce-patchcraft-plugin)
- [Weavecraft (Cornell)](https://www.cs.cornell.edu/projects/weavecraft)
- [Weave Craft Minecraft mod](https://www.curseforge.com/minecraft/mc-mods/weave-craft)
- [FluxCraft Minecraft mod](https://modrinth.com/mod/fluxcraft)
- [DDAG Craft modpack](https://moddex.gg/modpack/ddagcraft)
- [CircuitCraft Rust editor](https://users.rust-lang.org/t/circuitcraft-a-rust-desktop-circuit-editor-with-ngspice-simulation/143114)
- [CircuitCraft Minecraft mod](https://modrinth.com/mod/circuitcraft)
- [PlumbCraft brand](https://events.gripsintelligence.com/insights/brands/plumbcraft)
- [Splice Craft](https://splice.com/blog/spitfire-audio-originals-craft-splice-instrument/)
- [PlexCraft YouTube](https://www.youtube.com/channel/UCis5qewMsuGOSSgm8JEtcQg)
- Registry endpoints used: `crates.io/api/v1/crates/<name>`, `pypi.org/pypi/<name>/json`, `registry.npmjs.org/<name>`, `api.github.com/users/<name>`, `api.github.com/search/repositories`
