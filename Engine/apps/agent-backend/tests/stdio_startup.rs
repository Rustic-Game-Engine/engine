use engine_project::{Project, ProjectTemplate};
use serde_json::{Value, json};
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

fn exchange(directory: &Path, args: &[&str], requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rustic-agent-backend"))
        .args(args)
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for request in requests {
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "backend exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str(line).expect("stdout must contain only JSON-RPC responses")
        })
        .collect()
}

#[test]
fn server_starts_outside_a_game_and_responds_to_initialize_and_tools_list() {
    let temp = tempfile::tempdir().unwrap();
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"startup-regression","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"workspace_info","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"ping"}),
    ];
    // Both the globally registered launch and a stale explicit --project path
    // must keep the connection alive, with project selection errors at tool time.
    for args in [vec![], vec!["--project", "missing-game"]] {
        let responses = exchange(temp.path(), &args, &requests);
        assert_eq!(responses.len(), 4);
        assert_eq!(
            responses[0]["result"]["serverInfo"]["name"],
            "rustic-workspace"
        );
        assert!(
            responses[1]["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "workspace_info"
                    && tool["inputSchema"]["properties"]["project_root"]["type"] == "string")
        );
        assert_eq!(responses[2]["result"]["isError"], true);
        assert_eq!(responses[3]["result"], json!({}));
    }
}

#[test]
fn project_can_be_selected_after_connecting_and_file_tools_use_it() {
    let temp = tempfile::tempdir().unwrap();
    let project =
        Project::create(temp.path().join("game"), "MCP game", ProjectTemplate::Blank).unwrap();
    let root = project.root().canonicalize().unwrap();
    let responses = exchange(
        temp.path(),
        &[],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"workspace_info","arguments":{"project_root":root}}}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"project.engine"}}}),
        ],
    );
    assert_eq!(responses.len(), 3);
    let workspace: Value = serde_json::from_str(
        responses[1]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(workspace["project_root"], json!(root));
    assert_eq!(workspace["name"], "MCP game");
    assert!(responses[2]["result"].get("isError").is_none());
}
