//! A hosted website runs the native language adapters; the browser transports input,
//! pixels and audio. Each WebSocket owns a disposable snapshot and game session.
use engine_play::{game_session::GameSession, pointer_input::PointerEvent};
use image::ImageEncoder as _;
use renderer_wgpu::{BackendRequest, SceneViewportRenderer};
use serde::Deserialize;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::{Message, WebSocket, accept_hdr_with_config, protocol::WebSocketConfig};

const MAX_CONNECTIONS: usize = 8;
const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const MAX_INPUT: usize = 64 * 1024;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Input {
    keys: Vec<String>,
    pointers: Vec<PointerEvent>,
    reset: bool,
}

pub(super) fn run(package: &Path, address: &str) -> Result<(), String> {
    let data = package.join("data");
    engine_play::PlaySnapshot::open(&data).map_err(|e| e.to_string())?;
    check_toolchains(&data)?;
    let listener =
        TcpListener::bind(address).map_err(|e| format!("Could not bind website: {e}"))?;
    eprintln!(
        "Website listening on http://{}",
        listener.local_addr().map_err(|e| e.to_string())?
    );
    let connections = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let mut stream = stream.map_err(|e| e.to_string())?;
        if connections.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
            connections.fetch_sub(1, Ordering::AcqRel);
            let _ = respond(
                &mut stream,
                "503 Service Unavailable",
                "text/plain",
                b"All game sessions are busy. Try again later.",
            );
            continue;
        }
        let permit = ConnectionPermit(Arc::clone(&connections));
        let package = package.to_owned();
        std::thread::Builder::new()
            .name("website-player".into())
            .spawn(move || {
                let _permit = permit;
                if let Err(error) = connection(stream, &package) {
                    eprintln!("Website connection ended: {error}");
                }
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

struct ConnectionPermit(Arc<AtomicUsize>);
impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn check_toolchains(data: &Path) -> Result<(), String> {
    let path = data.join("config/scripts.ron");
    if path.is_file() {
        let loaded =
            engine_scripting::load_manifest(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let mut missing = std::collections::BTreeSet::new();
        for script in loaded.manifest.scripts {
            let availability = engine_scripting::probe_language_toolchain(script.language);
            if !availability.available {
                missing.insert(format!(
                    "{}: {}",
                    script.language.display_name(),
                    availability.detail
                ));
            }
        }
        if !missing.is_empty() {
            return Err(format!(
                "Install the game's language toolchains on the website host:\n{}",
                missing.into_iter().collect::<Vec<_>>().join("\n")
            ));
        }
    }
    Ok(())
}

// Preserve the already bounded HTTP header when handing the socket to tungstenite.
struct Connection {
    header: std::io::Cursor<Vec<u8>>,
    stream: TcpStream,
}
impl Read for Connection {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.header.position() < self.header.get_ref().len() as u64 {
            self.header.read(buffer)
        } else {
            self.stream.read(buffer)
        }
    }
}
impl Write for Connection {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.stream.write(buffer)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}
fn read_header(stream: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut header = Vec::new();
    while header.len() < 8192 {
        let mut byte = [0];
        stream.read_exact(&mut byte).map_err(|e| e.to_string())?;
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            return Ok(header);
        }
    }
    Err("HTTP header exceeds 8 KiB".into())
}
fn respond(
    stream: &mut impl Write,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n", body.len()).map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())
}
fn connection(mut stream: TcpStream, package: &Path) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    let header = read_header(&mut stream)?;
    let request = std::str::from_utf8(&header).map_err(|e| e.to_string())?;
    let Some(path) = request
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("GET "))
        .and_then(|line| line.strip_suffix(" HTTP/1.1"))
    else {
        return respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            b"GET required",
        );
    };
    match path {
        "/" | "/index.html" => {
            return respond(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                include_bytes!("website/player.html"),
            );
        }
        "/player.js" => {
            return respond(
                &mut stream,
                "200 OK",
                "text/javascript; charset=utf-8",
                include_bytes!("website/player.js"),
            );
        }
        "/game-info.json" => {
            let info = std::fs::read(package.join("game-info.json"))
                .unwrap_or_else(|_| b"{\"name\":\"Rustic Game\"}".to_vec());
            return respond(&mut stream, "200 OK", "application/json", &info);
        }
        "/play" => {}
        _ => return respond(&mut stream, "404 Not Found", "text/plain", b"Not found"),
    }
    let connection = Connection {
        header: std::io::Cursor::new(header),
        stream,
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_INPUT))
        .max_frame_size(Some(MAX_INPUT));
    let mut socket = accept_hdr_with_config(connection, validate_origin, Some(config))
        .map_err(|e| e.to_string())?;
    let result = play(&mut socket, &package.join("data"));
    if let Err(error) = &result {
        let _ = socket.send(Message::Text(
            serde_json::json!({"error": error}).to_string().into(),
        ));
    }
    let _ = socket.close(None);
    result
}
#[allow(
    clippy::result_large_err,
    reason = "tungstenite requires an HTTP error response in its handshake callback"
)]
fn validate_origin(request: &Request, response: Response) -> Result<Response, ErrorResponse> {
    let host = request.headers().get("host").and_then(|h| h.to_str().ok());
    let origin = request
        .headers()
        .get("origin")
        .and_then(|h| h.to_str().ok());
    if host.zip(origin).is_some_and(|(host, origin)| {
        origin == format!("http://{host}") || origin == format!("https://{host}")
    }) {
        Ok(response)
    } else {
        Err(tungstenite::http::Response::builder()
            .status(403)
            .body(Some("Same-origin game connections required".into()))
            .expect("constant response"))
    }
}

fn copy_snapshot(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let target = destination.join(entry.file_name());
        if kind.is_symlink() {
            return Err("Snapshot contains a symbolic link".into());
        }
        if kind.is_dir() {
            copy_snapshot(&entry.path(), &target)?;
        } else if kind.is_file() {
            // Fresh files inherit writable temporary-directory permissions even
            // when the installed package is read-only.
            let mut source_file = std::fs::File::open(entry.path()).map_err(|e| e.to_string())?;
            let mut target_file = std::fs::File::create(target).map_err(|e| e.to_string())?;
            std::io::copy(&mut source_file, &mut target_file).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn parse_input(text: &str) -> Result<Input, String> {
    if text.len() > MAX_INPUT {
        return Err("Input exceeds 64 KiB".into());
    }
    let input: Input = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if input.keys.len() > 128
        || input.keys.iter().any(|key| key.len() > 64)
        || input.pointers.len() > 1024
    {
        return Err("Too many input events".into());
    }
    Ok(input)
}
fn play(socket: &mut WebSocket<Connection>, data: &Path) -> Result<(), String> {
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let root: PathBuf = temporary.path().join("game");
    copy_snapshot(data, &root)?;
    let mut game = GameSession::load_streamed(&root)?;
    let models = engine_assets::load_model_library(&root)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(id, (_, model))| (id, model))
        .collect();
    let mut renderer =
        SceneViewportRenderer::new(BackendRequest::Auto).map_err(|e| e.to_string())?;
    renderer.set_environment_root(&root);
    let ui = super::game_ui::GameUi::load(&root)?;
    let mut previous = Instant::now();
    loop {
        let message = socket.read().map_err(|e| e.to_string())?;
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => return Ok(()),
            Message::Ping(_) | Message::Pong(_) => {
                socket.flush().map_err(|e| e.to_string())?;
                continue;
            }
            _ => return Err("Game input must be JSON text".into()),
        };
        let input = parse_input(text.as_str())?;
        // The host owns elapsed time. A client cannot accelerate the simulation.
        let elapsed = previous.elapsed();
        let interval = Duration::from_secs_f64(1.0 / 30.0);
        if let Some(wait) = interval.checked_sub(elapsed) {
            std::thread::sleep(wait);
        }
        let now = Instant::now();
        game.step(
            now.duration_since(previous).as_secs_f64(),
            input.keys,
            input.pointers,
            input.reset,
        )?;
        previous = now;
        let scene = game.render_world(|world| {
            renderer_wgpu::game_scene_with_models(world, 16.0 / 9.0, &models)
        })?;
        let mut frame = renderer
            .render(WIDTH, HEIGHT, &scene)
            .map_err(|e| e.to_string())?
            .ok_or("Game renderer did not produce a frame")?;
        ui.composite_rgba(WIDTH, HEIGHT, &mut frame.rgba8)?;
        let audio = game.take_stream_audio();
        let mut pixels = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(
            &mut pixels,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::NoFilter,
        )
        .write_image(&frame.rgba8, WIDTH, HEIGHT, image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
        let mut packet = Vec::with_capacity(16 + pixels.len() + audio.len() * 4);
        packet.extend_from_slice(&WIDTH.to_le_bytes());
        packet.extend_from_slice(&HEIGHT.to_le_bytes());
        packet.extend_from_slice(
            &u32::try_from(pixels.len())
                .map_err(|e| e.to_string())?
                .to_le_bytes(),
        );
        packet.extend_from_slice(
            &u32::try_from(audio.len())
                .map_err(|e| e.to_string())?
                .to_le_bytes(),
        );
        packet.extend_from_slice(&pixels);
        for sample in audio {
            packet.extend_from_slice(&sample.to_le_bytes());
        }
        socket
            .send(Message::Binary(packet.into()))
            .map_err(|e| e.to_string())?;
        for (level, message) in game.drain_logs() {
            eprintln!("{level}: {message}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_origin_connections_are_rejected() {
        for origin in ["http://evil.example", "null", "https://game.example.evil"] {
            let request = Request::builder()
                .uri("/play")
                .header("host", "game.example")
                .header("origin", origin)
                .body(())
                .unwrap();
            assert!(validate_origin(&request, Response::new(())).is_err());
        }
        for origin in ["https://game.example", "http://game.example"] {
            let request = Request::builder()
                .uri("/play")
                .header("host", "game.example")
                .header("origin", origin)
                .body(())
                .unwrap();
            assert!(validate_origin(&request, Response::new(())).is_ok());
        }
        let request = Request::builder()
            .header("host", "game.example")
            .body(())
            .unwrap();
        assert!(validate_origin(&request, Response::new(())).is_err());
    }

    #[test]
    fn input_rejects_oversized_and_unexpected_commands() {
        assert!(
            parse_input(r#"{"keys":["KeyD"],"reset":true}"#)
                .unwrap()
                .reset
        );
        assert!(parse_input(r#"{"seconds":10000}"#).is_err());
        assert!(parse_input(&serde_json::json!({"keys": vec!["KeyD"; 129]}).to_string()).is_err());
        assert!(parse_input(&serde_json::json!({"keys": ["x".repeat(65)]}).to_string()).is_err());
        assert!(parse_input(&" ".repeat(MAX_INPUT + 1)).is_err());
        assert!(read_header(&mut std::io::Cursor::new(vec![b'x'; 8192])).is_err());
    }

    #[test]
    fn connection_cannot_fetch_game_sources() {
        let package = tempfile::tempdir().unwrap();
        std::fs::create_dir(package.path().join("data")).unwrap();
        std::fs::write(package.path().join("data/secret.py"), b"SECRET").unwrap();
        for path in [
            "/data/secret.py",
            "/../data/secret.py",
            "/%2e%2e/data/secret.py",
            "/player.js",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (server, _) = listener.accept().unwrap();
            write!(client, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            connection(server, package.path()).unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            assert!(!response.contains("SECRET"));
            assert!(response.starts_with(if path == "/player.js" {
                "HTTP/1.1 200"
            } else {
                "HTTP/1.1 404"
            }));
        }
    }

    #[test]
    fn player_file_writes_are_isolated() {
        let source = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("save.txt"), b"initial").unwrap();
        copy_snapshot(source.path(), first.path()).unwrap();
        copy_snapshot(source.path(), second.path()).unwrap();
        std::fs::write(first.path().join("save.txt"), b"changed").unwrap();
        assert_eq!(
            std::fs::read(second.path().join("save.txt")).unwrap(),
            b"initial"
        );
        assert_eq!(
            std::fs::read(source.path().join("save.txt")).unwrap(),
            b"initial"
        );
    }
}

#[cfg(test)]
mod smoke_test {
    use super::*;
    use engine_play::{PlayMode, SnapshotBuilder, SnapshotInput};
    use engine_world::{
        Camera, EntitySnapshot, LocalTransform, PartAttributes, Primitive, SceneDocument,
        ScriptComponent,
    };
    use tungstenite::client::IntoClientRequest as _;

    #[test]
    #[ignore = "requires native graphics and the Python toolchain"]
    fn native_website_streams_rendered_frames_and_executes_python_input() {
        let package = tempfile::tempdir().unwrap();
        let authoring = package.path().join("authoring");
        std::fs::create_dir_all(authoring.join("scripts")).unwrap();
        let source = b"from rustic import rustic,run\ndef fixed_update(dt):\n    if rustic.key('KeyD')['held']:\n        x,y,z=rustic.get_translation()\n        rustic.set_translation(x+dt*5,y,z)\nrun(globals())\n";
        std::fs::write(authoring.join("scripts/move.py"), source).unwrap();
        let script =
            engine_scripting::register_script_asset(&authoring, "scripts/move.py").unwrap();
        let mut scene = SceneDocument::new("Hosted smoke");
        scene.entities = vec![
            EntitySnapshot {
                camera: Some(Camera {
                    active: true,
                    ..Camera::default()
                }),
                local_transform: LocalTransform {
                    translation: glam::Vec3::new(0.0, 0.0, -5.0),
                    ..LocalTransform::IDENTITY
                },
                ..EntitySnapshot::default()
            },
            EntitySnapshot {
                primitive: Some(Primitive::Cube { size: 2.0 }),
                scripts: vec![ScriptComponent::new(script)],
                part_attributes: PartAttributes {
                    color: [1.0, 0.0, 0.0, 1.0],
                    anchored: true,
                    ..PartAttributes::default()
                },
                ..EntitySnapshot::default()
            },
        ];
        let snapshot = SnapshotBuilder::new(package.path().join("stage"))
            .stage(
                PlayMode::Standalone,
                SnapshotInput::new("scene/main.scene", scene.to_bytes().unwrap()),
                &[
                    SnapshotInput::new("scripts/move.py", source.as_slice()),
                    SnapshotInput::new(
                        "config/scripts.ron",
                        std::fs::read(authoring.join("config/scripts.ron")).unwrap(),
                    ),
                ],
            )
            .unwrap();
        copy_snapshot(snapshot.root(), &package.path().join("data")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let path = package.path().to_owned();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            connection(stream, &path)
        });
        let stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let mut request = format!("ws://{address}/play")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", format!("http://{address}").parse().unwrap());
        let (mut client, _) = tungstenite::client(request, stream).unwrap();
        client.send(Message::Text("{}".into())).unwrap();
        let initial = client.read().unwrap();
        let Message::Binary(initial) = initial else {
            panic!("Expected frame, got {initial:?}")
        };
        let length = u32::from_le_bytes(initial[8..12].try_into().unwrap()) as usize;
        let image = image::load_from_memory(&initial[16..16 + length]).unwrap();
        assert_eq!((image.width(), image.height()), (WIDTH, HEIGHT));
        let mut changed = false;
        for _ in 0..5 {
            client
                .send(Message::Text(r#"{"keys":["KeyD"]}"#.into()))
                .unwrap();
            let Message::Binary(frame) = client.read().unwrap() else {
                panic!("Expected frame")
            };
            let length = u32::from_le_bytes(frame[8..12].try_into().unwrap()) as usize;
            let moved = image::load_from_memory(&frame[16..16 + length]).unwrap();
            changed |= image.as_bytes() != moved.as_bytes();
        }
        assert!(
            changed,
            "Python keyboard input must change the rendered game"
        );
        client.close(None).unwrap();
        assert!(server.join().unwrap().is_ok());
    }
}
