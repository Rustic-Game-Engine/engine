# Gameplay API smoke-test entry scripts

These entries exercise the current engine's Script API 1.1 in all ten supported
languages. They are component scripts, not standalone project files. Use the
latest engine and let it generate the matching SDK; do not copy SDK files from
an older project or implement the JSON transport yourself.

## Run in the editor

1. Create or open a project in the latest Rustic editor and add a Part to a scene.
2. Select the Part and use **Programming > New Script > Object Component Script**,
   choosing the language to test. Replace the generated source with the matching
   entry here. Use `RusticBehavior.java` for Java. Keep the editor-assigned Asset ID
   and manifest entry rather than renaming files or editing metadata by hand.
3. Confirm the script is attached to the Part in the Inspector. If necessary,
   attach it with **+ Add Component** or drag the script from Explorer.
   Test one entry at a time so multiple scripts do not move the same Part.
4. Press **Play** and expect `Behavior started` once in the Console. Focus the
   embedded game viewport and hold **W**: the Part moves along positive Z at one
   unit per scaled fixed second. Release W to stop; Stop Play restores the scene.

The editor stores PHP and Web entries under `ui/`; other languages use `scripts/`.
PHP here is a gameplay component requiring PHP CLI, not a standalone PHP UI page.
HTML runs its inline JavaScript through the gameplay Web bridge. Copy `entry.css`
next to the HTML entry to resolve its optional stylesheet. CSS alone has no
callable gameplay API and is not a movement test.

Lua 5.4, JavaScript and the HTML bridge are bundled. Luau requires the engine's
Luau host; Python requires Python 3; C/C++ require C17/C++20 compilers; C# requires
.NET SDK 10+; Java requires both Java and javac; PHP requires PHP CLI. Configure
missing toolchains on PATH and check the Console's availability diagnostic.
Rustic supplies `rustic.py`, `rustic.h`, `rustic.hpp`, `Rustic.cs`, `Rustic.java`
and `rustic.php` as appropriate. Regenerate the programming workspace when
upgrading an existing project. Do not install an unrelated Python `rustic` package.

Lua, JavaScript and HTML also inspect timing, input, scene lookup, attributes and
an optional `smoke_value` property. No custom action map or property declaration
is required. These entries preserve the initial position and do not clone their
scripted owner, so Play does not recursively create more behavior instances.
If movement is absent, check the attachment, Console, viewport keyboard focus,
and whether Play or the scaled clock is paused.

## Automated compatibility check

From `Engine/`, run:

```sh
cargo test -p engine-play shipped_api_smoke_entries_move_only_while_w_is_held -- --nocapture
```

This loads these exact files through the runtime adapters against a live scene.
It checks startup, idle input, two held-key fixed ticks, key release, update,
stop and destroy callbacks, one startup log, preserved X/Y and no extra objects.
Unavailable external adapters are explicitly skipped. To require every toolchain:

```sh
RUSTIC_REQUIRE_ALL_API_ADAPTERS=1 cargo test -p engine-play shipped_api_smoke_entries_move_only_while_w_is_held -- --nocapture
```
