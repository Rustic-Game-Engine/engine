# Custom installer API

Windows game exports include `scripts/install/server.py`, `installer.json`, and
`index.html` alongside the `game` payload. This is an additional installer path;
the existing Inno Setup wizard remains available. The host requires Python 3.10+
on the end user's computer. It is not a standalone executable or an uninstaller.

## Design and launch

Export a Windows game from the editor Build window. To customize future exports,
put `installer.json` and/or `index.html` in your **project's** `scripts/install/`
directory and export again. These two files override the bundled defaults. The project's `scripts/install/hooks/` directory is also copied recursively into the export.
Installer files are not gameplay behaviors and need no scene attachment.

A complete configuration example:

```json
{
  "title": "My Game",
  "payload": "../../game",
  "location": "~/My Game",
  "allowLocationChange": true,
  "window": { "width": 800, "height": 600 },
  "screens": [
    { "kind": "welcome", "title": "Welcome", "text": "Install My Game" },
    { "kind": "location", "title": "Choose a folder" },
    { "kind": "options", "title": "Select components" },
    { "kind": "install", "title": "Ready" }
  ],
  "options": [
    { "id": "extras", "label": "Install bonus content", "default": false, "pathPrefix": "extras" }
  ]
}
```

`payload` is relative to the exported `scripts/install` directory. Options are
boolean; an unchecked option excludes its `pathPrefix` directory or file from
the payload. Options without a prefix are selections exposed to your UI only.
All other payload files are installed. Supply an empty `options` array if none
are needed. Screen titles/text, order, and content are editable. The example UI
uses width and height for page layout; browsers control their own window size.
Native UI clients can use these values to size their windows.

From the export directory, run:

```sh
python scripts/install/server.py
```

The host prints one JSON line containing a loopback `url` and a random `token`.
Open that URL, paste the token into the example UI, and click Connect. The port
is chosen automatically; `--port 8765` selects a fixed port. Keep the host running
until installation finishes. Ctrl+C closes it. Never publish or log the token.

## HTTP contract

All API requests require `Authorization: Bearer TOKEN`. Responses are JSON.
The host binds only to `127.0.0.1`. Browser clients should be served from its `/`
page; cross-origin browser access is intentionally unavailable. Replace
`index.html` with bundled HTML/CSS/JavaScript (inline assets), or start the host
as a child process from a C#, C++, Python, or other native UI and parse its first
stdout line. No particular UI toolkit is required.

| Method | Route | Result |
| --- | --- | --- |
| GET | `/scripts/install/config` | Screen design, window size, option definitions and defaults |
| GET | `/scripts/install/status` | `status`, `percentage`, `currentTask`, `location`, `options`, `logs`, `error` |
| GET | `/scripts/install/logs` | Most recent 1,000 file/error messages |
| POST | `/scripts/install/configure` | Set `location` and/or boolean `options` before starting |
| POST | `/scripts/install/start` | Start background installation once |
| POST | `/scripts/install/cancel` | Request cancellation between files or stop the active task |

For example, from C# with an `HttpClient` whose BaseAddress is the printed URL:

```csharp
client.DefaultRequestHeaders.Authorization =
    new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", token);
var status = await client.GetStringAsync("scripts/install/status");
var body = new System.Net.Http.StringContent(
    "{\"options\":{\"extras\":true}}", System.Text.Encoding.UTF8, "application/json");
var configured = await client.PostAsync("scripts/install/configure", body);
configured.EnsureSuccessStatusCode();
var started = await client.PostAsync("scripts/install/start", null);
started.EnsureSuccessStatusCode();
```

Poll status while installing. Progress measures completed files and tasks, rather than
bytes; `currentTask` names the file being copied or the running task ID. Status is `ready`, `installing`,
`completed`, `cancelled`, or `failed`. Configuration locks after start. HTTP 400
means invalid configuration/state; 401 means a missing/wrong token.

## Limits and troubleshooting

Installation copies files into the selected folder with the current user's
permissions. File copying does not install runtimes, create shortcuts, register uninstallers,
or elevate privileges. Author-defined tasks can perform additional work with
the current user's permissions. Existing files are
never overwritten; choose a fresh folder if installation reports a collision.
Cancellation/failure can leave partial files or task side effects, with no rollback or resume. Logs
and state are session-only; save API responses in your UI if persistence is needed.
Symlink payloads and destination paths escaping the install folder are rejected.
Use a trusted payload and a destination under your control.

If the host cannot start, verify Python's version, the JSON configuration, and
that `../../game` exists relative to `installer.json`. If Connect fails, use the
URL and token from the current process, rather than a previous session. A failed
copy's `error` field reports the filesystem failure; check folder permissions and
existing files. Custom native UI frameworks must be shipped by the UI author.

## Script installer logic in other languages

Use `tasks` in `installer.json` for installer logic, independently of your UI.
Tasks execute in array order within each phase: `beforeCopy`, then file copying,
then `afterCopy` (the default). Each task has an `id`, a `command` argument array,
an optional `timeoutSeconds` (default 300, maximum 86400), and an optional `option`
ID that gates execution. Arguments run directly without shell expansion.
Commands come from the packaged configuration; HTTP clients cannot submit commands.

```json
{
  "tasks": [
    {
      "id": "prepare",
      "phase": "beforeCopy",
      "command": ["python", "hooks/prepare.py"],
      "timeoutSeconds": 60
    },
    {
      "id": "configure-extras",
      "phase": "afterCopy",
      "option": "extras",
      "command": ["dotnet", "hooks/ConfigureExtras.dll"]
    }
  ]
}
```

Add this property to the complete configuration above. Put task sources and
binaries under your project's `scripts/install/hooks/` before exporting. Commands
run with the exported `scripts/install/` as their working directory. The first
stdin line is a JSON object containing `location`, `options`, `bundle`, `payload`,
and `task`. The task may read this to decide what to install or configure.

Python example (`hooks/prepare.py`):

```python
import json
import sys
from pathlib import Path

context = json.loads(sys.stdin.readline())
print(json.dumps({"log": "Preparing destination", "percentage": 10}), flush=True)
Path(context["location"]).mkdir(parents=True, exist_ok=True)
print(json.dumps({"log": "Preparation complete", "percentage": 100}), flush=True)
```

C# example (compile and publish as `hooks/ConfigureExtras.dll`, including its
runtime configuration and dependencies):

```csharp
using System;
using System.IO;
using System.Text.Json;

using var context = JsonDocument.Parse(Console.ReadLine()!);
var location = context.RootElement.GetProperty("location").GetString()!;
File.WriteAllText(Path.Combine(location, "extras-configured.txt"), "enabled");
Console.WriteLine(JsonSerializer.Serialize(new { log = "Extras configured", percentage = 100 }));
```

Plain stdout/stderr text also becomes a log message. JSON stdout lines can contain
`log` and a task-local `percentage` from 0 to 100; the host combines task progress
with copying progress. Each file and task has equal weight. Exit code zero succeeds;
a nonzero code or timeout fails installation and stops later tasks. Missing
executables also fail with an error in status. Task output is read in chunks of
up to 8 KiB and retained in the bounded session log.

Language choices use ordinary executables:

| Language | Task command example | Requirement |
| --- | --- | --- |
| Python | `["python", "hooks/setup.py"]` | Python runtime |
| JavaScript | `["node", "hooks/setup.mjs"]` | Node.js runtime |
| Lua | `["lua", "hooks/setup.lua"]` | Standalone Lua CLI |
| Luau | `["luau", "hooks/setup.luau"]` | Standalone Luau CLI and any needed JSON/I/O adapter |
| C# | `["dotnet", "hooks/Setup.dll"]` | .NET runtime, or publish a standalone executable |
| Java | `["java", "-jar", "hooks/setup.jar"]` | Java runtime |
| PHP | `["php", "hooks/setup.php"]` | PHP CLI |
| C / C++ | `["hooks/setup.exe"]` | Precompiled executable and its dependencies |

These are installer processes, separate from engine scripting. Bundled gameplay
Lua, Luau, and JavaScript hosts do not supply general installer runtimes or the
gameplay API here. Compile C/C++/C#/Java code before distributing, or explicitly
supply a build tool command if the installer machine has its SDK. Other languages
with a CLI can follow the same protocol. HTML/CSS continue to provide presentation;
JavaScript or a task process supplies logic.

Cancellation kills the direct active task process and prevents subsequent work;
child processes spawned by the task are its responsibility. Tasks run as the
current user and can modify files, so distribute trusted task code. Shutting down
the host while a task is active does not guarantee cleanup of that process.
