# Custom installer API

Windows game exports include `scripts/install/server.py`, `installer.json`, and
`index.html` alongside the `game` payload. This is an additional installer path;
the existing Inno Setup wizard remains available. The host requires Python 3.10+
on the end user's computer. It is not a standalone executable or an uninstaller.

## Design and launch

Export a Windows game from the editor Build window. To customize future exports,
put `installer.json` and/or `index.html` in your **project's** `scripts/install/`
directory and export again. Only these two files override the bundled defaults.
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
| POST | `/scripts/install/cancel` | Request cancellation between files |

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

Poll status while installing. Progress measures completed files, rather than
bytes; `currentTask` names the file being copied. Status is `ready`, `installing`,
`completed`, `cancelled`, or `failed`. Configuration locks after start. HTTP 400
means invalid configuration/state; 401 means a missing/wrong token.

## Limits and troubleshooting

Installation copies files into the selected folder with the current user's
permissions. It does not execute payload commands, install runtimes, create
shortcuts, register uninstallers, or elevate privileges. Existing files are
never overwritten; choose a fresh folder if installation reports a collision.
Cancellation/failure can leave partial files, with no rollback or resume. Logs
and state are session-only; save API responses in your UI if persistence is needed.
Symlink payloads and destination paths escaping the install folder are rejected.
Use a trusted payload and a destination under your control.

If the host cannot start, verify Python's version, the JSON configuration, and
that `../../game` exists relative to `installer.json`. If Connect fails, use the
URL and token from the current process, rather than a previous session. A failed
copy's `error` field reports the filesystem failure; check folder permissions and
existing files. Custom native UI frameworks must be shipped by the UI author.
