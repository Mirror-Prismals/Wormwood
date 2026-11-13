# Mirror native packages

Mirror is the base DAW and 3D environment. Optional game content is delivered as
independently built native packages. Prismals lives in the sibling Prismals
project. The base Cargo manifest depends only on this directory's API crate.

A package directory contains `mod.json`, a platform dynamic library, optional
`Assets`, and any declared System contracts. The manifest declares schema/API
versions, a stable lowercase ID, supported OS/architecture libraries,
dependencies, capabilities, and exported hooks. Library and contract paths must
stay inside the package. Missing dependencies, cycles, duplicate IDs, unknown
exports, and incompatible API versions are rejected.

Search paths are the asset root's `Mods` directory and `MIRROR_MOD_PATH` (or
repeated `--mod-dir` arguments). Selection is `Mods/enabled.json`, overridden by
`MIRROR_MODS` or repeated `--mod`. The default selection is empty. A package's
presence does not enable it. Selection changes require an application restart.

`--list-mods`/`--check-mods` report the package inventory; `--inspect-world`
constructs the selected world without graphics/audio. A macOS app can include
an independently built package using `BuildSupport/macOS/build_app.py
--mod-package PATH`; the packaged default still leaves it disabled.

Additional Systems can be registered by the manifest's `systems` array:

```json
{"contract":"Systems/ExampleSystem.json","functions":{"Describe":{"input":"Unit","output":"GeometryData"}}}
```

The contract uses the same System/layer/functions/calls/init_steps/entry_steps
format as the engine. The library must export each qualified hook. Extensions
cannot replace a built-in System; call-graph checks and the layer-five Audicle
creation rule still apply. Extension startup is appended in dependency/package
order. Supported extension payload types are Unit, GeometryData, and ViewRequest
(input only). Mutable callbacks are serialized and cannot recursively call the
same mod. Host callbacks keep normal ownership, payload, and declared-call checks.

The C ABI is in `API/src/lib.rs`. Return buffers must be released by the allocator
that produced them. Native libraries remain loaded while worker query pointers
exist. Mod callbacks never run on audio processing threads. Scalar query hooks
must be immutable and thread-safe. Native code is trusted code, not sandboxed.
This development API does not yet promise binary compatibility across SDK edits.

Some legacy game-facing System declarations and small scene adapters remain in
Mirror so existing host call routes and behavior IDs survive the split. The
Prismals builders and character state machines live in its dynamic library.
Further conversion of these adapters can happen without combining codebases.

In the macOS launcher, **Mods…** lists packaged mods and packages placed in
`~/Library/Application Support/Mirror/Mods`. Each starts unchecked. Select the
packages before launching a session; the **Open Mods folder** button opens the
user installation directory. The launcher applies this selection to both the
3D environment and DAW-only sessions. It is session selection, not hot reload.
