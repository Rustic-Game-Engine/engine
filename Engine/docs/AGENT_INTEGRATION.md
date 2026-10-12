# AI agents and editor app data

Rustic stores its project agent instructions, Codex plugin, and Claude skill in the
editor's application data instead of creating `.agents`, `.claude`, `.mcp.json`,
`AGENTS.md`, or `CLAUDE.md` in your game directory. Gameplay source, scenes, and the
generated programming workspace still belong to the game project.

## Connect an agent

1. Install Rustic and open your game project in the Rustic editor once. Opening
   the project generates its agent information and registers the
   `rustic-workspace` MCP server in your user Codex and Claude configuration.
2. Reload the agent's MCP connections if it was already running. Starting
   from the game directory (or a directory below it) automatically discovers
   the containing `project.engine` file. The backend also starts from other
   directories: the MCP handshake and tool list do not require a game project.
   To bind a game from the Codex desktop app or an engine checkout, call
   `workspace_info` with `{"project_root":"C:/Games/MyGame"}`. Use an absolute
   directory path; selection is retained for this MCP session. Alternatively,
   configure the installed `rustic-agent-backend.exe` with arguments
   `["--project", "C:/Games/MyGame"]` and reload the connection.
3. Invoke the `rustic-workspace` skill, or `/rustic-workspace` in Claude, and ask
   it to inspect your project. Call `workspace_info` first; its `project_root`
   identifies the game and `agent_directory` identifies its stored information.
4. Read instructions through `read_agent_file` before editing game content.
   Use `scene_summary` with your scene's relative path before changing a scene.


The stdio MCP server runs on the same computer as the agent client and the game
files. A local Rustic editor registration is not available to a Codex cloud
chat. Use a local Codex chat with the registered MCP connection to control that
game. Opening a game locally does not expose the backend to a remote executor,
and a missing `@rustic-workspace` entry in a cloud chat does not establish that
the local server crashed.

Only small discovery skills, a Claude command, and client registration settings
remain in the user profile so agent applications can find the integration. The
full skills and plugin live in Rustic app data. No gameplay script attachment or
Play session is needed to use these authoring tools.

## Read and edit agent information

The following are MCP tool names and copyable argument objects:

```json
{}
```

Pass that object to `workspace_info`, then to `list_agent_files` to list this
project's stored instructions and integration files.

Read project instructions with `read_agent_file`:

```json
{"path":"AGENTS.md"}
```

Replace instructions with `write_agent_file` (read the existing content first and
include any instructions you want to retain):

```json
{"path":"AGENTS.md","content":"# My game\nKeep gameplay scripts in scripts/. Inspect scenes before editing them.\n"}
```

Expect a `written` path in the response. Read it again to confirm the saved text.
Instruction and skill edits survive opening the editor again.

All three tools default to `scope: "project"`. To inspect shared user integrations,
pass `{"scope":"user"}` to `list_agent_files`. For example, read the stored Claude
skill with:

```json
{"scope":"user","path":".claude/skills/rustic-workspace/SKILL.md"}
```

`write_agent_file` accepts the same scope together with `path` and `content`.
Shared skill and instruction edits persist; Rustic refreshes client registration
and the shared plugin MCP command when the editor starts to follow the installed
backend executable.

## Storage and migration

On Windows the location is normally
`%LOCALAPPDATA%/RusticEngine/RusticGameEngine/data/editor/agents`.
`projects/<hash>` separates projects using the canonical project path; `user`
holds shared integrations. Trust `workspace_info.agent_directory` for the actual
project location rather than calculating a hash yourself.

Opening an older project transfers Rustic's plugin and Claude skill, including
edited files, into app data. Generated `AGENTS.md` and `CLAUDE.md` move as well.
Rustic removes only its own registration from shared project MCP and marketplace
files. Other plugins and user-owned instruction files stay in place. If both
locations contain different versions of a file, migration keeps the old file in
the project as well as the app-data version; compare them before removing either.
Empty integration directories are removed after migration.

## Limits and troubleshooting

- These tools operate locally for the current OS user. App-data instructions are
  not included when sharing or checking in a game directory. Moving the project
  changes its storage key; copy important instructions through the tools if needed.
- Reads and writes accept UTF-8 text up to 2 MiB. Paths are relative to the selected
  agent directory and use `/` separators. Absolute paths (including Windows drive
  paths on Linux), backslashes, colons, `..`, `.git`, and symlinks that escape it
  are rejected. For example, use `notes/behavior.md`, not `C:/notes/behavior.md`
  or `notes\behavior.md`. Project and shared user scopes do not expose other
  projects' storage. A path error means you should call `list_agent_files`, then
  use a relative path within the selected scope.
- If the tools are missing, reopen the editor and reload your agent's MCP server.
  Confirm the installed backend executable exists. You can rerun
  `rustic-agent-backend.exe --install-user-integrations` to register it again.
- “No Rustic project is selected” is a tool error, not a failed MCP startup.
  Call `workspace_info` with `{"project_root":"C:/Games/MyGame"}` using an
  absolute path to a readable game directory containing `project.engine`.
  Later file and scene tools use that selected game. An invalid selection
  reports a tool error and keeps the previous selection; it does not close the
  server. You can also set `--project` in the MCP registration and reload it.
- An older backend may exit with “The supplied folder is not a readable Rustic
  project” before connecting when Codex starts outside a game. Point that
  registration at the game with `--project`, or install the updated backend
  and reload the connection. Opening the editor alone does not replace an
  already running MCP process.
- If a file is missing, call `list_agent_files` and check the scope and relative
  path. Ordinary `read_file` and `write_file` address game content; agent metadata
  uses `read_agent_file` and `write_agent_file`.
- If migration or saving fails, check permissions on the project and Rustic app
  data and the editor's startup error output. Existing files are only removed
  after their contents have been stored; unrelated agent directories stay intact.
