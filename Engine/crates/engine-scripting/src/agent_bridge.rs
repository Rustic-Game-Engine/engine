//! Local, authenticated editor RPC. Only the writable editor publishes an endpoint.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

const LIMIT: u64 = 4 * 1024 * 1024;

pub struct AgentRequest {
    pub deadline: std::time::Instant,
    pub tool: String,
    pub arguments: Value,
    pub reply: mpsc::SyncSender<Result<Value, String>>,
}

pub struct AgentBridge {
    receiver: mpsc::Receiver<AgentRequest>,
    stopped: Arc<AtomicBool>,
    endpoint: PathBuf,
    token: String,
}

impl AgentBridge {
    /// Publish a loopback endpoint in project-specific editor app data.
    /// # Errors
    /// Returns local I/O, app-data or thread creation errors.
    pub fn start(project: &Path) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let token = crate::ScriptId::new().to_string();
        let endpoint = crate::project_agent_directory(project)
            .map_err(|e| e.to_string())?
            .join("editor-endpoint.json");
        std::fs::create_dir_all(endpoint.parent().ok_or("missing endpoint parent")?)
            .map_err(|e| e.to_string())?;
        let descriptor = json!({"address":listener.local_addr().map_err(|e| e.to_string())?.to_string(),"token":token});
        let mut file =
            tempfile::NamedTempFile::new_in(endpoint.parent().ok_or("missing endpoint parent")?)
                .map_err(|e| e.to_string())?;
        file.write_all(descriptor.to_string().as_bytes())
            .map_err(|e| e.to_string())?;
        let (sender, receiver) = mpsc::sync_channel(4);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let secret = token.clone();
        std::thread::Builder::new()
            .name("agent-editor-bridge".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(20));
                        continue;
                    };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                    let result = (|| -> Result<Value, String> {
                        let mut line = String::new();
                        BufReader::new((&stream).take(LIMIT + 1))
                            .read_line(&mut line)
                            .map_err(|e| e.to_string())?;
                        if line.len() as u64 > LIMIT {
                            return Err("request exceeds 4 MiB".into());
                        }
                        let request: Value =
                            serde_json::from_str(&line).map_err(|e| e.to_string())?;
                        if request["token"].as_str() != Some(secret.as_str()) {
                            return Err("invalid editor token".into());
                        }
                        let (reply, response) = mpsc::sync_channel(1);
                        sender
                            .try_send(AgentRequest {
                                deadline: std::time::Instant::now() + Duration::from_secs(15),
                                tool: request["tool"].as_str().ok_or("missing tool")?.into(),
                                arguments: request["arguments"].clone(),
                                reply,
                            })
                            .map_err(|e| e.to_string())?;
                        response
                            .recv_timeout(Duration::from_secs(15))
                            .map_err(|_| {
                                "Editor did not respond; check its dialogs and retry".to_string()
                            })?
                    })();
                    let response = match result {
                        Ok(value) => json!({"result":value}),
                        Err(error) => json!({"error":error}),
                    };
                    let _ = writeln!(stream, "{response}");
                }
            })
            .map_err(|e| e.to_string())?;
        if let Err(error) = file.persist(&endpoint) {
            stopped.store(true, Ordering::Relaxed);
            return Err(error.to_string());
        }
        Ok(Self {
            receiver,
            stopped,
            endpoint,
            token,
        })
    }

    pub fn try_recv(&self) -> Option<AgentRequest> {
        self.receiver.try_recv().ok()
    }
}

impl Drop for AgentBridge {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if std::fs::read(&self.endpoint)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .is_some_and(|v| v["token"].as_str() == Some(self.token.as_str()))
        {
            let _ = std::fs::remove_file(&self.endpoint);
        }
    }
}

/// Send one tool request to the running writable editor of this project.
/// # Errors
/// Returns an actionable error when no editor is open, authentication fails, or the tool fails.
pub fn call_editor(project: &Path, tool: &str, arguments: &Value) -> Result<Value, String> {
    let endpoint = crate::project_agent_directory(project)
        .map_err(|e| e.to_string())?
        .join("editor-endpoint.json");
    let bytes = std::fs::read(endpoint)
        .map_err(|_| "Open this project in the Rustic editor to use editor tools".to_string())?;
    let descriptor: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let address: std::net::SocketAddr = descriptor["address"]
        .as_str()
        .ok_or("missing editor address")?
        .parse()
        .map_err(|_| "invalid editor address")?;
    if !address.ip().is_loopback() {
        return Err("editor address must be loopback".into());
    }
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|_| "Editor is unavailable; reopen this project in Rustic".to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let request =
        json!({"token":descriptor["token"],"tool":tool,"arguments":arguments}).to_string();
    if request.len() as u64 > LIMIT {
        return Err("request exceeds 4 MiB".into());
    }
    writeln!(stream, "{request}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream.take(32 * 1024 * 1024))
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let response: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    if let Some(error) = response["error"].as_str() {
        Err(error.into())
    } else {
        Ok(response["result"].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_rpc_is_project_scoped_and_requires_token() {
        let project = tempfile::tempdir().unwrap();
        let bridge = AgentBridge::start(project.path()).unwrap();
        let descriptor: Value =
            serde_json::from_slice(&std::fs::read(&bridge.endpoint).unwrap()).unwrap();
        let mut unauthorized = TcpStream::connect(descriptor["address"].as_str().unwrap()).unwrap();
        writeln!(
            unauthorized,
            "{}",
            json!({"token":"wrong","tool":"scene_save"})
        )
        .unwrap();
        let mut response = String::new();
        BufReader::new(unauthorized)
            .read_line(&mut response)
            .unwrap();
        assert!(response.contains("invalid editor token"));
        assert!(bridge.try_recv().is_none());
        let root = project.path().to_path_buf();
        let client =
            std::thread::spawn(move || call_editor(&root, "editor_status", &json!({"test":true})));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(request) = bridge.try_recv() {
                assert_eq!(request.tool, "editor_status");
                assert_eq!(request.arguments["test"], true);
                request
                    .reply
                    .send(Ok(json!({"active":"main.scene"})))
                    .unwrap();
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(client.join().unwrap().unwrap()["active"], "main.scene");
        let other = tempfile::tempdir().unwrap();
        assert!(
            call_editor(other.path(), "scene_save", &json!({}))
                .unwrap_err()
                .contains("Open this project")
        );
        let endpoint = bridge.endpoint.clone();
        drop(bridge);
        assert!(!endpoint.exists());
    }
}
