use engine_project::Project;
use serde_json::{Value, json};
use std::fs;
use std::io::{self, BufRead as _, Write as _};
use std::path::{Component, Path, PathBuf};

const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let project_root = match (arguments.next(), arguments.next()) {
        (Some(flag), Some(root)) if flag == "--project" => PathBuf::from(root),
        _ => {
            eprintln!("Usage: rustic-agent-backend --project <project-folder>");
            std::process::exit(2);
        }
    };
    let root = match project_root.canonicalize() {
        Ok(root) if Project::open(&root).is_ok() => root,
        _ => {
            eprintln!("The supplied folder is not a readable Rustic project");
            std::process::exit(2);
        }
    };
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(response) = handle(&root, &request) {
            let _ = writeln!(stdout, "{response}");
            let _ = stdout.flush();
        }
    }
}

fn handle(root: &Path, request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "rustic-workspace", "version": env!("CARGO_PKG_VERSION")}
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
            match call_tool(root, name, &args) {
                Ok(value) => json!({"content": [{"type": "text", "text": value.to_string()}]}),
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
    json!([
        {"name":"workspace_info","description":"Return the canonical Rustic project root, project metadata, and repository guidance.","inputSchema":{"type":"object","properties":{}}},
        {"name":"list_files","description":"List project files below an optional relative directory.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}}}},
        {"name":"read_file","description":"Read a UTF-8 project file (maximum 2 MiB).","inputSchema":{"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}},
        {"name":"write_file","description":"Write a UTF-8 file atomically inside the project. Parent directories are created.","inputSchema":{"type":"object","required":["path","content"],"properties":{"path":{"type":"string"},"content":{"type":"string"}}}},
        {"name":"scene_summary","description":"Inspect a Rustic scene as structured entities, transforms, and component kinds.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}}}},
        {"name":"docs_index","description":"List relevant Markdown documentation available in this project and its engine repository.","inputSchema":{"type":"object","properties":{}}}
    ])
}

fn call_tool(root: &Path, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "workspace_info" => {
            let project = Project::open(root).map_err(|error| error.to_string())?;
            Ok(
                json!({"project_root":root,"name":project.metadata().name,"project_file":root.join("project.engine"),"instructions":["Make game-content changes inside this project root.","Do not edit the engine source repository unless the user explicitly requests engine changes.","Read AGENTS.md or CLAUDE.md before editing."]}),
            )
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
        "scene_summary" => scene_summary(root, arg(args, "path").unwrap_or("scenes/main.scene")),
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

fn scene_summary(root: &Path, value: &str) -> Result<Value, String> {
    let path = safe_path(root, value, false)?;
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let load = engine_world::load_scene(&bytes).map_err(|error| error.to_string())?;
    let entities: Vec<_> = load.document.entities.iter().map(|entity| json!({
        "id":entity.id.to_string(), "name":entity.name, "parent":entity.parent.map(|id| id.to_string()),
        "translation":entity.local_transform.translation.to_array(), "rotation":entity.local_transform.rotation.to_array(), "scale":entity.local_transform.scale.to_array(),
        "components":{"camera":entity.camera.is_some(),"light":entity.light.is_some(),"mesh":entity.mesh.is_some(),"material":entity.material.is_some(),"primitive":entity.primitive.as_ref().map(|p| format!("{p:?}")),"scripts":entity.scripts.len()}
    })).collect();
    Ok(
        json!({"path":relative(root,&path),"name":load.document.name,"schema_version":load.schema_version,"read_only":load.read_only,"entities":entities,"diagnostics":load.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>()}),
    )
}

fn safe_path(root: &Path, relative_path: &str, allow_missing: bool) -> Result<PathBuf, String> {
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
        .display()
        .to_string()
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
    fn rejects_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        assert!(safe_path(temp.path(), "../secret", true).is_err());
    }
}
