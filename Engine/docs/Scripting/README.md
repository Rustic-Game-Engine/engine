# Scripting language guides

Rustic supports ten script asset types. Choose the guide that matches the file
extension you create in the Explorer:

| Language | Extensions | Runtime | Guide |
| --- | --- | --- | --- |
| Lua 5.4 | `.lua` | bundled, embedded | [Lua 5.4](scriptingLua.md) |
| Luau | `.luau` | bundled isolated Luau host | [Luau](scriptingLuau.md) |
| JavaScript | `.js`, `.mjs` | bundled, embedded | [JavaScript](scriptingJavaScript.md) |
| Python | `.py` | external Python 3 | [Python](scriptingPython.md) |
| C | `.c` | external C17 compiler | [C](scriptingC.md) |
| C++ | `.cc`, `.cpp`, `.cxx` | external C++20 compiler | [C++](scriptingCpp.md) |
| C# | `.cs` | external .NET SDK | [C#](scriptingCSharp.md) |
| Java | `.java` | external `java` and `javac` | [Java](scriptingJava.md) |
| PHP | `.php` | external PHP CLI; `ui/` only | [PHP](scriptingPHP.md) |
| HTML/CSS | `.html`, `.htm`, `.css` | bundled Web validator and inline-JS sandbox; `ui/` only | [Web](scriptingWeb.md) |

Every gameplay language uses engine-owned built-in functions. Scripts define
callbacks and call the API; no user-written JSON request/response loop is needed.
CSS is styling only; use inline JavaScript in HTML for gameplay API calls.

## Rules shared by every language

Create, move, rename, and attach scripts in the editor. Never edit
`config/scripts.ron`, a `.scene` file, or generated metadata to connect a script.
Rustic assigns an Asset ID when the script is created or imported; scene and object
records retain that ID even if the source file moves.

**Play keyboard status:** The embedded Play viewport currently forwards only held
WASD, arrow, and Shift keys to scripts. Press/release events, named input actions,
other key names, and separate runtime-window keyboard input are not wired yet.
See the [Lua input guide](scriptingLua.md#input) for supported names, setup, a
copyable controller, and troubleshooting. The same held-key limitation applies
to other script languages.

Use **Programming > New Script** to create a **Global Startup Script**, **Scene
Startup Script**, or **Object Component Script**. Use **Programming > Attach Existing
Script**, **+ Add Component** in the Inspector, or drag a script from Explorer onto
the viewport to attach an existing component script to the selected object.

New scenes live at `scene/{scene-name}.scene`. The older `scenes/*.rscene` layout,
legacy `entry_script` settings are ignored, and snake-case callbacks remain readable and are saved in
place. Do not migrate an old project by hand.

Execution is deterministic: global scope before scene scope before component scope;
then ascending execution-order value; then entity ID; then script Asset ID; then
attachment order. Editor-created references currently use execution order `0`; do not
edit serialized data to change it. Only implemented callbacks are invoked. Component instances follow
their object lifetime. Disabling/destroying the owner unregisters them. Global instances begin at game startup and scene instances begin
when the scene loads.

The common data boundary consists of booleans, integers, finite numbers, strings,
2D/3D vectors, and optional stable entity IDs. Languages must communicate through
engine state, Engine Events, or Script API operations—not VM objects or language
globals. A language-global variable is private to that behavior instance. The core
scheduler retains its language-neutral `ScriptEvent` envelope. API 1.1 also exposes
shared gameplay signals through `Events.on`, `once`, `emit`, `connect`, and
`disconnect` in every supported script type; see
[Shared gameplay actions](gameplayActions.md) for signatures and examples.

All source must be UTF-8 and at most 1 MiB. Play uses an immutable project snapshot.
Reload validates before swapping at a simulation boundary; a failed reload leaves the
last good instance running. External callbacks have a three-second deadline and a
1 MiB response limit. See each language guide for its exact callback and API surface.

See [Edit scene objects](sceneObjects.md) to target built-in properties on another
object using `rustic.game.<scene-name>.<hierarchy>` in native language syntax.
