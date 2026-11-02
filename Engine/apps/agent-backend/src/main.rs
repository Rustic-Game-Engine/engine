mod editor_tools;

use engine_project::Project;
use serde_json::{Value, json};
use std::fs;
use std::io::{self, BufRead as _, Write as _};
use std::path::{Component, Path, PathBuf};

const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let first = arguments.next();
    if first.as_deref() == Some(std::ffi::OsStr::new("--install-user-integrations")) {
        if let Err(error) = engine_scripting::install_user_agent_integrations() {
            eprintln!("Could not install Rustic agent integrations: {error}");
            std::process::exit(1);
        }
        return;
    }
    let project_root = match (first, arguments.next()) {
        (Some(flag), Some(root)) if flag == "--project" => PathBuf::from(root),
        (None, None) => std::env::current_dir().unwrap_or_default(),
        _ => {
            eprintln!("Usage: rustic-agent-backend [--project <project-folder>]");
            std::process::exit(2);
        }
    };
    // Global MCP registrations have no --project argument, and desktop clients
    // may launch us outside a game. Project discovery must not gate the handshake.
    let mut session = WorkspaceSession::new(project_root);
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(response) = handle(&mut session, &request) {
            let _ = writeln!(stdout, "{response}");
            let _ = stdout.flush();
        }
    }
}

struct WorkspaceSession {
    start: PathBuf,
    root: Option<PathBuf>,
}

impl WorkspaceSession {
    fn new(start: PathBuf) -> Self {
        Self {
            root: find_project_root(&start),
            start,
        }
    }

    fn project_for_tool(&mut self, name: &str, args: &Value) -> Result<&Path, String> {
        if name == "workspace_info" && args.get("project_root").is_some() {
            let supplied = Path::new(required_arg(args, "project_root")?);
            if !supplied.is_absolute() {
                return Err("project_root must be an absolute directory path".into());
            }
            if !supplied.is_dir() {
                return Err("project_root must name a readable game directory".into());
            }
            // Keep an existing selection if a replacement is invalid.
            let root = find_project_root(supplied).ok_or_else(|| {
                format!(
                    "No readable Rustic project was found at {}. Select a game directory containing project.engine.",
                    supplied.display()
                )
            })?;
            self.root = Some(root);
        }
        if self.root.is_none() {
            self.root = find_project_root(&self.start);
        }
        self.root.as_deref().ok_or_else(|| {
            "No Rustic project is selected. Call workspace_info with project_root set to an absolute game directory containing project.engine, or configure the MCP server with --project <game-directory>.".into()
        })
    }
}

fn find_project_root(start: &Path) -> Option<PathBuf> {
    let canonical = start.canonicalize().ok()?;
    canonical
        .ancestors()
        .find(|path| Project::open(path).is_ok())
        .map(Path::to_path_buf)
}

fn handle(session: &mut WorkspaceSession, request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "rustic-workspace", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "Call workspace_info first. If no game is selected, supply its absolute directory in project_root. Read project instructions with read_agent_file and read the bundled authoring_guide before editing game content. Prefer custom models and persist complete scenes before playtesting. Read scene_explorer_guide for hierarchy organization and folder/object creation, duplication and deletion. Use editor tools for properties, scenes, script scopes, model previews and playtest input."
        }),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or(Value::Null);
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match session
                .project_for_tool(name, &args)
                .and_then(|root| call_tool(root, name, &args))
            {
                Ok(mut value) => {
                    let image = value.as_object_mut().and_then(|v| v.remove("image_base64"));
                    let mut content = vec![json!({"type":"text","text":value.to_string()})];
                    if let Some(image) = image.and_then(|v| v.as_str().map(str::to_owned)) {
                        content.push(json!({"type":"image","mimeType":"image/png","data":image}));
                    }
                    json!({"content":content})
                }
                Err(error) => {
                    json!({"isError": true, "content": [{"type": "text", "text": error}]})
                }
            }
        }
        "ping" => json!({}),
        _ => {
            return Some(
                json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32601,"message":"method not found"}}),
            );
        }
    };
    Some(json!({"jsonrpc":"2.0", "id":id, "result":result}))
}

fn tools() -> Value {
    let mut tools = json!([
        {"name":"workspace_info","description":"Return the canonical Rustic project root, project metadata, and repository guidance. Supply project_root to select a game when the MCP client starts outside a project; selection persists for this connection.","inputSchema":{"type":"object","properties":{"project_root":{"type":"string","description":"Absolute game directory containing project.engine, or a directory below it."}}}},
        {"name":"list_agent_files","description":"List agent information in Rustic editor app data. Scope defaults to project; user selects shared integrations.","inputSchema":{"type":"object","properties":{"scope":{"type":"string","enum":["project","user"]}}}},
        {"name":"read_agent_file","description":"Read a UTF-8 agent file relative to the app-data agent directory (maximum 2 MiB). Scope defaults to project.","inputSchema":{"type":"object","required":["path"],"properties":{"path":{"type":"string"},"scope":{"type":"string","enum":["project","user"]}}}},
        {"name":"write_agent_file","description":"Atomically edit app-data agent information; edits persist across editor restarts. Scope defaults to project.","inputSchema":{"type":"object","required":["path","content"],"properties":{"path":{"type":"string"},"content":{"type":"string"},"scope":{"type":"string","enum":["project","user"]}}}},
        {"name":"list_files","description":"List project files below an optional relative directory.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}}}},
        {"name":"read_file","description":"Read a UTF-8 project file (maximum 2 MiB).","inputSchema":{"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}},
        {"name":"write_file","description":"Write a UTF-8 file atomically inside the project. Parent directories are created.","inputSchema":{"type":"object","required":["path","content"],"properties":{"path":{"type":"string"},"content":{"type":"string"}}}},
        {"name":"scene_summary","description":"Inspect a Rustic scene as structured entities, transforms, and component kinds.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}}}},
        {"name":"docs_index","description":"List relevant Markdown documentation available in this project and its engine repository.","inputSchema":{"type":"object","properties":{}}}
    ]);
    tools.as_array_mut().expect("tools array").push(json!({"name":"authoring_guide","description":"Read the bundled, current Rustic game design, modeling, scene, scope and playtest guidance. Works with existing customized integrations.","inputSchema":{"type":"object","properties":{}}}));
    tools.as_array_mut().expect("tools array").push(json!({"name":"scene_explorer_guide","description":"Read the current rustic-scene-explorer skill: organize hierarchy folders, add objects, duplicate/delete subtrees, reparent safely, undo and save. Available with preserved custom instructions.","inputSchema":{"type":"object","properties":{}}}));
    tools
        .as_array_mut()
        .expect("tools array")
        .extend(editor_tools::tools());
    tools
}

fn call_tool(root: &Path, name: &str, args: &Value) -> Result<Value, String> {
    if editor_tools::contains(name) {
        return engine_scripting::call_editor(root, name, args);
    }
    match name {
        "scene_explorer_guide" => {
            Ok(json!({"content":engine_scripting::AGENT_SCENE_EXPLORER_SKILL}))
        }
        "authoring_guide" => Ok(json!({"content":engine_scripting::AGENT_AUTHORING_GUIDE})),
        "workspace_info" => {
            let project = Project::open(root).map_err(|error| error.to_string())?;
            let agent_root = ensure_agent_directory(root)?;
            Ok(
                json!({"project_root":root,"name":project.metadata().name,"project_file":root.join("project.engine"),"agent_directory":agent_root,"scene_directory":project.directory(engine_project::VirtualDirectory::Scenes).map_err(|e| e.to_string())?,"start_scene":project.start_scene_path().map_err(|e| e.to_string())?,"authoring_guide":"Call authoring_guide before authoring; open this project in the editor for live editing tools.","instructions":["Keep final game content in this project; run Python/Blender asset generators and temporary tools outside the game directory. Import only finished assets.","Do not edit the engine source repository unless the user explicitly requests engine changes.","Use read_agent_file to read AGENTS.md or CLAUDE.md before editing; use list_agent_files and write_agent_file to inspect or edit agent information in editor app data."]}),
            )
        }
        "list_agent_files" | "read_agent_file" | "write_agent_file" => {
            let agent_root = match arg(args, "scope").unwrap_or("project") {
                "project" => ensure_agent_directory(root)?,
                "user" => {
                    let directory = engine_scripting::agent_data_directory()
                        .map_err(|error| error.to_string())?
                        .join("user");
                    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
                    directory
                        .canonicalize()
                        .map_err(|error| error.to_string())?
                }
                _ => return Err("scope must be project or user".into()),
            };
            let mapped = match name {
                "list_agent_files" => "list_files",
                "read_agent_file" => "read_file",
                _ => "write_file",
            };
            if mapped == "write_file" {
                let path = safe_path(&agent_root, required_arg(args, "path")?, true)?;
                let content = required_arg(args, "content")?;
                if content.len() as u64 > MAX_READ_BYTES {
                    return Err("content exceeds the 2 MiB write limit".into());
                }
                engine_scripting::write_agent_information(&path, content)
                    .map_err(|error| error.to_string())?;
                Ok(json!({"written":relative(&agent_root, &path)}))
            } else if mapped == "list_files" {
                call_tool(&agent_root, mapped, &json!({}))
            } else {
                call_tool(&agent_root, mapped, args)
            }
        }
        "list_files" => {
            let base = safe_path(root, arg(args, "path").unwrap_or("."), true)?;
            let mut files = Vec::new();
            collect_files(root, &base, &mut files, 0)?;
            Ok(json!({"files":files}))
        }
        "read_file" => {
            let path = safe_path(root, required_arg(args, "path")?, false)?;
            let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
            if metadata.len() > MAX_READ_BYTES {
                return Err("file exceeds the 2 MiB read limit".into());
            }
            Ok(
                json!({"path":relative(root,&path),"content":fs::read_to_string(path).map_err(|error| error.to_string())?}),
            )
        }
        "write_file" => {
            let path = safe_path(root, required_arg(args, "path")?, true)?;
            let content = required_arg(args, "content")?;
            if content.len() as u64 > MAX_READ_BYTES {
                return Err("content exceeds the 2 MiB write limit".into());
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let temporary = path.with_extension("rustic-agent.tmp");
            fs::write(&temporary, content).map_err(|error| error.to_string())?;
            fs::rename(&temporary, &path).map_err(|error| error.to_string())?;
            Ok(json!({"written":relative(root,&path)}))
        }
        "scene_summary" => {
            let project = Project::open(root).map_err(|e| e.to_string())?;
            let start = project.start_scene_path().map_err(|e| e.to_string())?;
            scene_summary(root, arg(args, "path").unwrap_or(&relative(root, &start)))
        }
        "docs_index" => {
            let mut docs = Vec::new();
            collect_markdown(root, root, &mut docs, 0)?;
            if let Some(engine) = find_engine_root(root) {
                collect_markdown(&engine, &engine.join("docs"), &mut docs, 0)?;
            }
            docs.sort();
            docs.dedup();
            Ok(json!({"documents":docs}))
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}

fn ensure_agent_directory(root: &Path) -> Result<PathBuf, String> {
    let directory =
        engine_scripting::project_agent_directory(root).map_err(|error| error.to_string())?;
    if !directory.is_dir() {
        let project = Project::open(root).map_err(|error| error.to_string())?;
        engine_scripting::generate_programming_workspace(root, &project.metadata().name)
            .map_err(|error| error.to_string())?;
    }
    directory.canonicalize().map_err(|error| error.to_string())
}

fn scene_summary(root: &Path, value: &str) -> Result<Value, String> {
    let path = safe_path(root, value, false)?;
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let load = engine_world::load_scene(&bytes).map_err(|error| error.to_string())?;
    let entities: Vec<_> = load.document.entities.iter().map(|entity| json!({
        "id":entity.id.to_string(), "name":entity.name, "folder":entity.folder, "parent":entity.parent.map(|id| id.to_string()),
        "translation":entity.local_transform.translation.to_array(), "rotation":entity.local_transform.rotation.to_array(), "scale":entity.local_transform.scale.to_array(),
        "components":{"camera":entity.camera.is_some(),"light":entity.light.is_some(),"mesh":entity.mesh.is_some(),"material":entity.material.is_some(),"primitive":entity.primitive.as_ref().map(|p| format!("{p:?}")),"scripts":entity.scripts.len()}
    })).collect();
    Ok(
        json!({"path":relative(root,&path),"name":load.document.name,"schema_version":load.schema_version,"read_only":load.read_only,"entities":entities,"diagnostics":load.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>()}),
    )
}

fn safe_path(root: &Path, relative_path: &str, allow_missing: bool) -> Result<PathBuf, String> {
    // Unix would otherwise treat Windows drive paths as local filenames.
    if relative_path.contains('\\') || relative_path.contains(':') {
        return Err("path must use relative slash-separated components".into());
    }
    let relative_path = Path::new(relative_path);
    if relative_path.is_absolute()
        || relative_path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err("path must be relative and remain inside the project".into());
    }
    if relative_path
        .components()
        .any(|part| part.as_os_str() == ".git")
    {
        return Err("the .git directory is not exposed".into());
    }
    let candidate = root.join(relative_path);
    if allow_missing {
        let existing = candidate
            .ancestors()
            .find(|path| path.exists())
            .ok_or("path has no existing ancestor")?;
        if !existing
            .canonicalize()
            .map_err(|error| error.to_string())?
            .starts_with(root)
        {
            return Err("path escapes the project".into());
        }
    } else if !candidate
        .canonicalize()
        .map_err(|error| error.to_string())?
        .starts_with(root)
    {
        return Err("path escapes the project".into());
    }
    Ok(candidate)
}

fn collect_files(
    root: &Path,
    path: &Path,
    output: &mut Vec<String>,
    depth: u8,
) -> Result<(), String> {
    if depth > 8 || output.len() >= 2_000 {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if matches!(
            entry.file_name().to_str(),
            Some(".git" | "cache" | "temp" | "builds")
        ) {
            continue;
        }
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            collect_files(root, &entry.path(), output, depth + 1)?;
        } else {
            output.push(relative(root, &entry.path()));
        }
    }
    output.sort();
    Ok(())
}

fn collect_markdown(
    display_root: &Path,
    path: &Path,
    output: &mut Vec<String>,
    depth: u8,
) -> Result<(), String> {
    if !path.is_dir() || depth > 5 {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            collect_markdown(display_root, &entry.path(), output, depth + 1)?;
        } else if entry
            .path()
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            output.push(
                entry
                    .path()
                    .strip_prefix(display_root)
                    .unwrap_or(&entry.path())
                    .display()
                    .to_string(),
            );
        }
    }
    Ok(())
}

fn find_engine_root(project: &Path) -> Option<PathBuf> {
    project
        .ancestors()
        .find(|path| {
            path.join("Cargo.toml").is_file() && path.join("docs/ARCHITECTURE.md").is_file()
        })
        .map(Path::to_path_buf)
}
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
fn arg<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str)
}
fn required_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    arg(args, name).ok_or_else(|| format!("missing string argument: {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_and_tool_discovery_do_not_require_a_project() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = WorkspaceSession::new(temp.path().to_path_buf());
        for method in ["initialize", "tools/list", "ping"] {
            let response = handle(
                &mut session,
                &json!({"jsonrpc":"2.0","id":1,"method":method}),
            )
            .unwrap();
            assert!(response.get("result").is_some(), "{response}");
        }
        assert!(session.root.is_none());
        let response = handle(
            &mut session,
            &json!({"id":2,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"project.engine"}}}),
        )
        .unwrap();
        assert_eq!(response["result"]["isError"], true);
        assert!(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("workspace_info")
        );
        assert!(handle(&mut session, &json!({"method":"notifications/initialized"})).is_none());
    }

    #[test]
    fn project_selection_persists_and_invalid_selection_keeps_the_previous_game() {
        let temp = tempfile::tempdir().unwrap();
        let project = Project::create(
            temp.path().join("game"),
            "Selected game",
            engine_project::ProjectTemplate::Blank,
        )
        .unwrap();
        let root = project.root().canonicalize().unwrap();
        let mut session = WorkspaceSession::new(temp.path().to_path_buf());
        assert_eq!(
            session
                .project_for_tool("workspace_info", &json!({"project_root": root}))
                .unwrap(),
            root
        );
        assert_eq!(
            session.project_for_tool("read_file", &json!({})).unwrap(),
            root
        );
        for supplied in [
            json!(temp.path()),
            json!(temp.path().join("missing")),
            json!(root.join("project.engine")),
            json!("relative/game"),
            json!(null),
            json!(42),
        ] {
            assert!(
                session
                    .project_for_tool("workspace_info", &json!({"project_root": supplied}))
                    .is_err()
            );
            assert_eq!(session.root.as_ref(), Some(&root));
        }
        let response = handle(
            &mut session,
            &json!({"id":3,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"project.engine"}}}),
        )
        .unwrap();
        assert_eq!(response["result"]["content"][0]["type"], "text");
        assert!(response["result"].get("isError").is_none());
    }

    #[test]
    fn discovery_from_a_game_subdirectory_still_selects_that_game() {
        let temp = tempfile::tempdir().unwrap();
        let project = Project::create(
            temp.path().join("game"),
            "Discovered game",
            engine_project::ProjectTemplate::Blank,
        )
        .unwrap();
        let session = WorkspaceSession::new(project.root().join("scripts"));
        assert_eq!(session.root, Some(project.root().canonicalize().unwrap()));
    }

    #[test]
    fn agent_tools_read_write_list_and_reject_escapes() {
        let temp = tempfile::tempdir().unwrap();
        let project = Project::create(
            temp.path().join("game"),
            "Agent test",
            engine_project::ProjectTemplate::Blank,
        )
        .unwrap();
        let root = project.root().canonicalize().unwrap();
        let info = call_tool(&root, "workspace_info", &json!({})).unwrap();
        assert!(info["agent_directory"].as_str().is_some());
        call_tool(
            &root,
            "write_agent_file",
            &json!({"path":"notes/behavior.md","content":"first"}),
        )
        .unwrap();
        call_tool(
            &root,
            "write_agent_file",
            &json!({"path":"notes/behavior.md","content":"replaced"}),
        )
        .unwrap();
        let read = call_tool(
            &root,
            "read_agent_file",
            &json!({"path":"notes/behavior.md"}),
        )
        .unwrap();
        assert_eq!(read["content"], "replaced");
        let listed = call_tool(&root, "list_agent_files", &json!({})).unwrap();
        assert!(
            listed["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value.as_str().unwrap().ends_with("behavior.md"))
        );
        assert!(!root.join("notes/behavior.md").exists());
        for path in [
            "../other/AGENTS.md",
            "C:/outside.md",
            "C:\\outside.md",
            "..\\other\\AGENTS.md",
            "//server/share/outside.md",
            ".git/config",
        ] {
            assert!(
                call_tool(
                    &root,
                    "write_agent_file",
                    &json!({"path":path,"content":"bad"})
                )
                .is_err(),
                "accepted {path}"
            );
        }
        assert!(call_tool(&root, "list_agent_files", &json!({"scope":"other"})).is_err());
    }
    #[test]
    fn rejects_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        assert!(safe_path(temp.path(), "../secret", true).is_err());
    }
}
