# AI agents and editor app data

Rustic stores its project agent instructions, Codex plugin, and Claude skill in the
editor's application data instead of creating `.agents`, `.claude`, `.mcp.json`,
`AGENTS.md`, or `CLAUDE.md` in your game directory. Gameplay source, scenes, and the
generated programming workspace still belong to the game project.

## Connect an agent

1. Install Rustic and open your game project in the Rustic editor once. Opening
   the project generates its agent information and registers the
   `rustic-workspace` MCP server in your user Codex and Claude configuration.
2. Start your agent from the game directory (or a directory below it), and reload
   its MCP connections if it was already running. The backend discovers the
   containing `project.engine` file. You can also explicitly configure its
   command as the installed `rustic-agent-backend.exe` and arguments as
   `["--project", "C:/Games/MyGame"]`.
3. Invoke the `rustic-workspace` skill, or `/rustic-workspace` in Claude, and ask
   it to inspect your project. Call `workspace_info` first; its `project_root`
   identifies the game and `agent_directory` identifies its stored information.
4. Read instructions through `read_agent_file` before editing game content.
   Use `scene_summary` with your scene's relative path before changing a scene.

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
- “The supplied folder is not a readable Rustic project” means the agent started
  outside a game project. Set `--project` to the directory containing `project.engine`.
- If a file is missing, call `list_agent_files` and check the scope and relative
  path. Ordinary `read_file` and `write_file` address game content; agent metadata
  uses `read_agent_file` and `write_agent_file`.
- If migration or saving fails, check permissions on the project and Rustic app
  data and the editor's startup error output. Existing files are only removed
  after their contents have been stored; unrelated agent directories stay intact.
