mod explorer_selection;
mod play_session;
mod scene_graph;
mod script_links;
mod transform_widget;
mod ui_theme;
mod viewport_keyboard;

use eframe::egui;
use egui_tiles::{Behavior, TileId, Tree, UiResponse};
use engine_assets::{
    AssetMeta, DerivedArtifact, ImportRequest, ImporterRegistry, ModelArtifact, load_meta,
    save_meta, sidecar_path,
};
use engine_core::logging::{AsyncLogger, Language, LogConfig, LogStream, Severity};
use engine_core::{
    ApplicationIdentity, ApplicationRole, AssetId, run_headless_smoke_from_arguments,
};
use engine_desktop::icons::Icon;
use engine_editor::{
    AuthoringDocument, ConsoleEntry, EditorCamera, EditorTab, EditorWorkspace, GizmoOperation,
    GizmoSettings, GizmoSpace, PickMesh, ProjectAccess, ProjectLock, RuntimeConsole, WorldRay,
    grid_lines, pick_meshes,
};
use engine_platform::{AtomicFileService, ConfigStore};
use engine_play::{ControlRequest, PlayMode, RuntimeChangeSet, RuntimeState};
use engine_project::{Project, VirtualDirectory};
use engine_scripting::{
    CodeEditor, EditorConfiguration, EngineValue, GameSettings, ProjectOpenBehavior,
    ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
    ScriptReference, ScriptScope, build_editor_launch, discover_code_editor,
    generate_programming_workspace, install_user_agent_integrations, load_manifest,
    open_in_external_editor, probe_language_toolchain, register_script_asset, save_manifest_atomic,
};
use engine_world::ScriptComponent;
use engine_world::{EntityId, LocalTransform, Primitive, RenderWorldBuffer};
use explorer_selection::{ExplorerSelection, selection_roots};
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use num_traits::ToPrimitive as _;
use play_session::EditorPlaySession;
use renderer_wgpu::{
    BackendRequest, RenderedFrame, SceneViewportRenderer, ViewportGuide, ViewportMesh,
    ViewportScene, ViewportVertex,
};
use std::collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let identity = ApplicationIdentity::new("rustic-editor", ApplicationRole::Editor);
    match run_headless_smoke_from_arguments(&identity, &arguments) {
        Ok(true) => return ExitCode::SUCCESS,
        Ok(false) => {}
        Err(error) => {
            eprintln!("rustic-editor smoke failed: {error}");
            return ExitCode::FAILURE;
        }
    }
    let Some(project_root) = project_argument(&arguments) else {
        eprintln!("Usage: rustic-editor --project <project-folder>");
        return ExitCode::FAILURE;
    };
    let project = match Project::open(&project_root) {
        Ok(project) => project,
        Err(error) => {
            eprintln!("Could not open project {}: {error}", project_root.display());
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = generate_programming_workspace(&project_root, &project.metadata().name) {
        eprintln!("Could not refresh programming and AI-agent integration: {error}");
    }
    if let Err(error) = install_user_agent_integrations() {
        eprintln!("Could not install user AI-agent skill/plugin: {error}");
    }
    if let Err(error) = engine_platform::start_desktop_updater() {
        eprintln!("Could not start automatic updates: {error}");
    }
    let title = format!("{} — Rustic Editor", project.metadata().name);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_decorations(!cfg!(target_os = "windows"))
            .with_inner_size([1_440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(app_icon()),
        ..Default::default()
    };
    match eframe::run_native(
        "org.rusticengine.editor",
        options,
        Box::new(move |context| {
            EditorApp::new(project, &context.egui_ctx)
                .map(|app| Box::new(app) as Box<dyn eframe::App>)
                .map_err(Into::into)
        }),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rustic-editor failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn app_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../../../assets/app-icon.png"))
        .expect("embedded app icon must be a valid image")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent editor panel and interaction flags are not mutually exclusive states"
)]
struct EditorApp {
    updates: engine_desktop::DesktopUpdates,
    update_close_requested: bool,
    document: AuthoringDocument,
    project_lock: Option<engine_editor::ProjectLock>,
    dock: Tree<EditorTab>,
    workspace: EditorWorkspace,
    workspace_path: PathBuf,
    workspace_store: ConfigStore,
    logger: Option<AsyncLogger>,
    console: RuntimeConsole,
    active_play_mode: PlayMode,
    play: Option<EditorPlaySession>,
    pending_runtime_changes: Option<RuntimeChangeSet>,
    project_tree: Option<ProjectDirectory>,
    project_scan: Option<Receiver<ProjectDirectory>>,
    last_project_scan: Instant,
    last_scene_scan: Instant,
    scene_disk_snapshot: Option<Vec<u8>>,
    external_scene_change_pending: bool,
    project_panel_mode: ProjectPanelMode,
    project_filter: String,
    preview: Option<egui::TextureHandle>,
    viewport_request: SyncSender<ViewportRenderRequest>,
    preview_result: Receiver<Result<Option<RenderedFrame>, String>>,
    preview_status: String,
    preview_dirty: bool,
    camera: EditorCamera,
    viewport_extent: [u32; 2],
    gizmo: GizmoSettings,
    gizmo_drag: Option<GizmoDrag>,
    gizmo_widget: transform_widget::TransformWidget,
    scene_graph: scene_graph::SceneGraph,
    render_world: RenderWorldBuffer,
    model_assets: BTreeMap<AssetId, ModelArtifact>,
    detach_inspector: bool,
    game_settings: GameSettings,
    game_name_text: String,
    autosave_enabled: bool,
    last_autosave: Option<Instant>,
    show_game_settings: bool,
    missing_dependencies: Vec<ScriptLanguage>,
    show_dependency_prompt: bool,
    scene_rename: Option<SceneRename>,
}

impl EditorApp {
    #[allow(
        clippy::too_many_lines,
        reason = "initialize related editor services and state in their dependency order"
    )]
    fn new(project: Project, context: &egui::Context) -> Result<Self, String> {
        egui_extras::install_image_loaders(context);
        ui_theme::apply(context);
        let access = ProjectLock::acquire(project.root()).map_err(|error| error.to_string())?;
        let (project_lock, read_only) = match access {
            ProjectAccess::Writable(lock) => (Some(lock), false),
            ProjectAccess::ReadOnly { .. } => (None, true),
        };
        let config = project
            .directory(VirtualDirectory::Config)
            .map_err(|error| error.to_string())?;
        let workspace_path = config.join("editor-workspace.ron");
        let workspace_store = ConfigStore::new(AtomicFileService::new());
        let template = project.descriptor().template;
        let workspace_existed = workspace_path.is_file();
        let mut workspace = EditorWorkspace::load(&workspace_store, &workspace_path)
            .unwrap_or_else(|_| EditorWorkspace::for_template(template));
        workspace.consolidate_project_explorer();
        if workspace_existed {
            workspace.migrate_legacy_default_for_template(template);
        } else {
            workspace = EditorWorkspace::for_template(template);
        }
        workspace.recover_monitors(1, [1_440.0, 900.0]);
        context.set_zoom_factor(workspace.ui_scale);
        let document =
            AuthoringDocument::open(project, read_only).map_err(|error| error.to_string())?;
        let model_assets = load_project_model_assets(document.project().root());
        let dock = default_dock(&workspace.tabs);
        let logger = document
            .project()
            .directory(VirtualDirectory::Logs)
            .ok()
            .and_then(|directory| {
                AsyncLogger::start(LogConfig {
                    directory,
                    stream: LogStream::Editor,
                    ..LogConfig::default()
                })
                .ok()
            });
        let project_scan = Some(spawn_project_scan(document.project().root().to_path_buf()));
        let scene_disk_snapshot = std::fs::read(document.scene_path()).ok();
        let (viewport_request, preview_result) =
            spawn_viewport_renderer(document.project().root().to_path_buf());
        let game_settings = GameSettings::load(document.project().root()).unwrap_or_default();
        if !read_only
            && !document
                .project()
                .root()
                .join(engine_scripting::GAME_SETTINGS_FILE)
                .is_file()
        {
            game_settings.save(document.project().root())?;
        }
        let game_name_text = document.project().metadata().name.clone();
        let autosave_enabled = game_settings.autosave;
        let missing_dependencies = missing_project_dependencies(document.project().root());
        let show_dependency_prompt = !missing_dependencies.is_empty();
        let mut app = Self {
            updates: engine_desktop::DesktopUpdates::default(),
            update_close_requested: false,
            document,
            project_lock,
            dock,
            workspace,
            workspace_path,
            workspace_store,
            logger,
            console: RuntimeConsole::default(),
            active_play_mode: PlayMode::Play,
            play: None,
            pending_runtime_changes: None,
            project_tree: None,
            project_scan,
            last_project_scan: Instant::now(),
            last_scene_scan: Instant::now(),
            scene_disk_snapshot,
            external_scene_change_pending: false,
            project_panel_mode: ProjectPanelMode::Scene,
            project_filter: String::new(),
            preview: None,
            viewport_request,
            preview_result,
            preview_status: "Preparing loaded-scene render…".to_owned(),
            preview_dirty: false,
            camera: EditorCamera::default(),
            viewport_extent: [640, 480],
            gizmo: GizmoSettings::default(),
            gizmo_drag: None,
            gizmo_widget: transform_widget::TransformWidget::default(),
            scene_graph: scene_graph::SceneGraph::default(),
            render_world: RenderWorldBuffer::new(),
            model_assets,
            detach_inspector: false,
            game_settings,
            game_name_text,
            autosave_enabled,
            last_autosave: Some(Instant::now()),
            show_game_settings: false,
            missing_dependencies,
            show_dependency_prompt,
            scene_rename: None,
        };
        app.log(
            if read_only {
                Severity::Warning
            } else {
                Severity::Info
            },
            "editor",
            if read_only {
                "A live project lock was found; this editor is read-only"
            } else {
                "Project opened with an exclusive authoring lock"
            },
        );
        app.refresh_preview();
        Ok(app)
    }

    fn log(&mut self, severity: Severity, subsystem: &str, message: impl Into<String>) {
        let message = message.into();
        if let Some(logger) = &self.logger {
            let _ = logger.log(severity, subsystem, None, message.clone());
        }
        self.console.push(ConsoleEntry {
            timestamp_unix_millis: unix_millis(),
            severity,
            subsystem: subsystem.to_owned(),
            language: None,
            process: "editor".to_owned(),
            process_id: std::process::id(),
            message,
            source: None,
            stack_frames: Vec::new(),
            duplicate_count: 1,
        });
    }

    fn refresh_preview(&mut self) {
        if self.play.is_some() {
            return;
        }
        self.preview_dirty = true;
        let scene = viewport_scene(
            &self.document,
            self.camera,
            self.viewport_extent,
            &mut self.render_world,
            &self.model_assets,
        );
        let request = ViewportRenderRequest {
            width: self.viewport_extent[0],
            height: self.viewport_extent[1],
            scene,
        };
        match self.viewport_request.try_send(request) {
            Ok(()) => {
                self.preview_dirty = false;
                "Rendering active scene…".clone_into(&mut self.preview_status);
            }
            Err(mpsc::TrySendError::Full(_)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => {
                "Viewport renderer worker stopped".clone_into(&mut self.preview_status);
            }
        }
    }

    fn poll_background(&mut self, context: &egui::Context) {
        if let Some(receiver) = &self.project_scan
            && let Ok(project_tree) = receiver.try_recv()
        {
            self.project_tree = Some(project_tree);
            self.project_scan = None;
            self.last_project_scan = Instant::now();
        }
        if self.project_scan.is_none() && self.last_project_scan.elapsed() >= Duration::from_secs(1)
        {
            self.project_scan = Some(spawn_project_scan(
                self.document.project().root().to_path_buf(),
            ));
        }
        if self.last_scene_scan.elapsed() >= Duration::from_secs(1) {
            self.last_scene_scan = Instant::now();
            self.reload_scene_if_changed();
        }
        // Keep polling even when the UI is otherwise idle so changes saved by an
        // external editor become visible without requiring mouse movement.
        context.request_repaint_after(Duration::from_secs(1));
        let preview = self.preview_result.try_recv().ok();
        if let Some(result) = preview.filter(|_| self.play.is_none()) {
            match result {
                Ok(Some(frame)) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.width as usize, frame.height as usize],
                        &frame.rgba8,
                    );
                    self.preview = Some(context.load_texture(
                        "loaded-scene-preview",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                    self.preview_status = format!(
                        "{} via {:?} ({})",
                        frame.adapter.adapter_name,
                        frame.adapter.backend,
                        frame.digest_hex()
                    );
                    self.log(Severity::Info, "renderer", self.preview_status.clone());
                }
                Ok(None) => {
                    "Viewport suspended while minimized".clone_into(&mut self.preview_status);
                }
                Err(error) => {
                    self.preview_status = format!("Renderer unavailable: {error}");
                    self.log(Severity::Error, "renderer", self.preview_status.clone());
                }
            }
            if self.preview_dirty {
                self.refresh_preview();
            }
        }
        if self.preview_status == "Rendering active scene…" {
            context.request_repaint_after(std::time::Duration::from_millis(16));
        }
        if let Some(play) = &mut self.play {
            context.request_repaint_after(Duration::from_millis(33));
            if let Ok(Some(frame)) = play.take_latest_frame() {
                let mut rgba = frame.pixels;
                for pixel in rgba.as_chunks_mut::<4>().0 {
                    pixel.swap(0, 2);
                }
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [
                        usize::try_from(frame.width).unwrap_or_default(),
                        usize::try_from(frame.height).unwrap_or_default(),
                    ],
                    &rgba,
                );
                self.preview = Some(context.load_texture(
                    "rustic-runtime-frame",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
                self.preview_status = format!("Runtime frame {}", frame.sequence);
            }
            let _ = play.refresh_entity(self.document.selected());
            let events: Vec<_> = play.drain_console().collect();
            for event in events {
                self.console.push(ConsoleEntry::from_runtime(event));
            }
            if play.has_exited().unwrap_or(true) {
                self.log(
                    Severity::Error,
                    "runtime",
                    "Runtime exited; authoring document is intact",
                );
                self.play = None;
            }
        }
    }

    fn reload_scene_if_changed(&mut self) {
        let path = self.document.scene_path().to_path_buf();
        let Ok(saved) = std::fs::read(&path) else {
            return;
        };
        if self.scene_disk_snapshot.as_ref() == Some(&saved) {
            self.external_scene_change_pending = false;
            return;
        }
        if self.document.is_modified() {
            if !self.external_scene_change_pending {
                self.log(
                    Severity::Warning,
                    "scene.external",
                    "The scene changed outside Rustic; save or undo local edits before it can be reloaded",
                );
                self.external_scene_change_pending = true;
            }
            return;
        }

        let selected = self.document.selected();
        let project = self.document.project().clone();
        let read_only = self.project_lock.is_none();
        let Ok(scenes) = project.directory(VirtualDirectory::Scenes) else {
            return;
        };
        let Ok(relative_path) = path.strip_prefix(&scenes) else {
            return;
        };
        match AuthoringDocument::open_scene(project, relative_path, read_only) {
            Ok(mut document) => {
                document.select(selected);
                self.document = document;
                self.scene_disk_snapshot = Some(saved);
                self.external_scene_change_pending = false;
                self.refresh_preview();
                self.log(
                    Severity::Info,
                    "scene.external",
                    "Reloaded scene changes made outside Rustic",
                );
            }
            Err(error) => {
                if !self.external_scene_change_pending {
                    self.log(Severity::Error, "scene.external", error.to_string());
                    self.external_scene_change_pending = true;
                }
            }
        }
    }

    fn start_play(&mut self, mode: PlayMode) {
        let missing = missing_project_dependencies(self.document.project().root());
        if !missing.is_empty() {
            self.missing_dependencies = missing;
            self.show_dependency_prompt = true;
            return;
        }
        match EditorPlaySession::start(&self.document, mode) {
            Ok(play) => {
                self.play = Some(play);
                self.log(
                    Severity::Info,
                    "runtime",
                    format!("Started {}", mode.as_str()),
                );
            }
            Err(error) => self.log(Severity::Error, "runtime", error),
        }
    }

    fn change_scene(&mut self, relative_path: &Path, context: &egui::Context) {
        if self.play.is_some() || self.pending_runtime_changes.is_some() {
            return;
        }
        match self.document.switch_scene(relative_path) {
            Ok(()) => {
                self.scene_disk_snapshot = std::fs::read(self.document.scene_path()).ok();
                self.external_scene_change_pending = false;
                self.last_scene_scan = Instant::now();
                self.last_autosave = Some(Instant::now());
                self.gizmo_drag = None;
                context.data_mut(|data| {
                    data.remove::<SceneClipboard>(egui::Id::new("scene-clipboard"));
                });
                self.preview = None;
                self.refresh_preview();
                self.log(
                    Severity::Info,
                    "scene",
                    format!("Loaded scene {}", relative_path.display()),
                );
            }
            Err(error) => self.log(
                Severity::Error,
                "scene",
                format!("Could not change scene: {error}"),
            ),
        }
    }

    fn set_start_scene(&mut self, path: &Path) {
        if self.play.is_some() || self.pending_runtime_changes.is_some() {
            return;
        }
        let was_modified = self.document.is_modified();
        let scene_existed = self.document.scene_path().is_file();
        match self.document.set_start_scene(path) {
            Ok(()) => {
                if (was_modified && !self.document.is_modified())
                    || (!scene_existed && self.document.scene_path().is_file())
                {
                    self.scene_disk_snapshot = std::fs::read(self.document.scene_path()).ok();
                    self.external_scene_change_pending = false;
                }
                self.log(
                    Severity::Info,
                    "scene",
                    format!("Start scene set to {}", path.display()),
                );
            }
            Err(error) => self.log(Severity::Error, "scene", error.to_string()),
        }
    }

    fn scene_rename_dialog(&mut self, context: &egui::Context) {
        let Some(mut rename) = self.scene_rename.take() else {
            return;
        };
        let can_edit = !self.document.is_read_only()
            && self.play.is_none()
            && self.pending_runtime_changes.is_none();
        let mut cancel = false;
        let mut submit = false;
        egui::Window::new("Rename scene")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(context, |ui| {
                ui.label(format!("Scene: {}", rename.source.display()));
                ui.add_enabled_ui(can_edit, |ui| {
                    let input = ui
                        .add(egui::TextEdit::singleline(&mut rename.name).hint_text("Scene name"));
                    if rename.focus {
                        input.request_focus();
                        rename.focus = false;
                    }
                    submit = (input.has_focus() || input.lost_focus())
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if let Some(error) = &rename.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                    submit |= ui.button("Rename").clicked();
                });
                cancel = ui.button("Cancel").clicked()
                    || ui.input(|input| input.key_pressed(egui::Key::Escape));
            });
        if cancel {
            return;
        }
        if submit && can_edit {
            let previous_path = self.document.scene_path().to_path_buf();
            let was_modified = self.document.is_modified();
            match self.document.rename_scene(&rename.source, &rename.name) {
                Ok(path) => {
                    if self.document.scene_path() != previous_path
                        || (was_modified && !self.document.is_modified())
                    {
                        self.scene_disk_snapshot = std::fs::read(self.document.scene_path()).ok();
                        self.external_scene_change_pending = false;
                        self.last_scene_scan = Instant::now();
                    }
                    self.project_scan = Some(spawn_project_scan(
                        self.document.project().root().to_path_buf(),
                    ));
                    self.log(
                        Severity::Info,
                        "scene",
                        format!("Renamed scene to {}", path.display()),
                    );
                    return;
                }
                Err(error) => rename.error = Some(error.to_string()),
            }
        }
        self.scene_rename = Some(rename);
    }

    fn apply_deferred_project_action(
        &mut self,
        action: Option<ProjectFileAction>,
        context: &egui::Context,
    ) {
        match action {
            Some(ProjectFileAction::ChangeScene(path)) => self.change_scene(&path, context),
            Some(ProjectFileAction::SetStartScene(path)) => self.set_start_scene(&path),
            Some(ProjectFileAction::RenameScene(path)) => {
                if !self.document.is_read_only()
                    && self.play.is_none()
                    && self.pending_runtime_changes.is_none()
                {
                    self.scene_rename = Some(SceneRename {
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        source: path,
                        error: None,
                        focus: true,
                    });
                }
            }
            Some(ProjectFileAction::Attach(path, scope)) => {
                self.attach_project_script(&path, scope);
            }
            Some(ProjectFileAction::Refresh) => {
                let project_root = self.document.project().root().to_path_buf();
                if let Ok(settings) = GameSettings::load(&project_root) {
                    self.game_settings = settings;
                }
                self.scene_disk_snapshot = std::fs::read(self.document.scene_path()).ok();
                self.model_assets = load_project_model_assets(&project_root);
                self.project_scan = Some(spawn_project_scan(project_root));
                self.last_project_scan = Instant::now();
                self.refresh_preview();
            }
            _ => {}
        }
    }

    fn control_play(&mut self, request: ControlRequest) {
        let result = self
            .play
            .as_mut()
            .ok_or_else(|| "no active runtime".to_owned())
            .and_then(|play| play.control(request));
        match result {
            Ok(ack) => self.log(
                Severity::Debug,
                "runtime",
                format!(
                    "{:?}: state {:?}, fixed tick {}",
                    ack.request, ack.state, ack.fixed_tick
                ),
            ),
            Err(error) => self.log(Severity::Error, "runtime", error),
        }
    }

    fn stop_play(&mut self) {
        if let Some(play) = self.play.take() {
            match play.stop() {
                Ok(changes) => {
                    self.pending_runtime_changes = changes;
                    self.log(
                        Severity::Info,
                        "runtime",
                        "Runtime stopped and snapshot removed; review runtime changes explicitly",
                    );
                }
                Err(error) => self.log(Severity::Error, "runtime", error),
            }
        }
        self.refresh_preview();
    }

    fn runtime_changes_dialog(&mut self, context: &egui::Context) {
        let Some(changes) = self.pending_runtime_changes.clone() else {
            return;
        };
        let mut decision = None;
        egui::Window::new("Review runtime changes")
            .collapsible(false)
            .resizable(true)
            .show(context, |ui| {
                ui.label(format!(
                    "Runtime proposed {} authoring change(s). Nothing is applied automatically.",
                    changes.changes.len()
                ));
                ui.monospace(format!("Base scene: {}", changes.base_scene_sha256));
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for change in &changes.changes {
                            ui.group(|ui| {
                                ui.strong(format!(
                                    "{} · {} · {}",
                                    change.target.entity_id,
                                    change.target.component_schema,
                                    change.target.property
                                ));
                                ui.monospace(format!(
                                    "before: {:?}\nafter:  {:?}",
                                    change.before, change.after
                                ));
                            });
                        }
                        if changes.changes.is_empty() {
                            ui.label("No authoring properties changed during this play session.");
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Discard").clicked() {
                        decision = Some(false);
                    }
                    if ui
                        .add_enabled(
                            !self.document.is_read_only(),
                            egui::Button::new("Apply as one undoable edit"),
                        )
                        .clicked()
                    {
                        decision = Some(true);
                    }
                });
            });
        match decision {
            Some(true) => {
                self.pending_runtime_changes = None;
                match self.document.apply_runtime_changes(&changes) {
                    Ok(count) => {
                        self.log(
                            Severity::Info,
                            "runtime.apply",
                            format!("Applied {count} runtime change group(s)"),
                        );
                        self.refresh_preview();
                    }
                    Err(error) => self.log(Severity::Error, "runtime.apply", error.to_string()),
                }
            }
            Some(false) => {
                self.pending_runtime_changes = None;
                self.log(Severity::Info, "runtime.apply", "Runtime changes discarded");
            }
            None => {}
        }
    }

    fn save(&mut self) {
        match self.document.save() {
            Ok(()) => {
                self.scene_disk_snapshot = std::fs::read(self.document.scene_path()).ok();
                self.log(Severity::Info, "scene", "Scene saved transactionally");
            }
            Err(error) => self.log(Severity::Error, "scene", error.to_string()),
        }
    }

    fn update_close_dialog(&mut self, ui: &mut egui::Ui) {
        if !self.update_close_requested {
            return;
        }
        if self.play.is_some() {
            self.stop_play();
        }
        if self.pending_runtime_changes.is_some() {
            return;
        }
        let mut ready = !self.document.is_modified();
        let mut cancel = false;
        if !ready {
            egui::Window::new("Save before updating?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui, |ui| {
                    ui.label(
                        "This scene has unsaved changes. Save it before installing the update.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Save and update").clicked() {
                            self.save();
                            ready = !self.document.is_modified();
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
        }
        if cancel {
            self.update_close_requested = false;
            self.updates.cancel_install();
        } else if ready {
            if self
                .updates
                .request_install(Some(self.document.project().root()))
            {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.update_close_requested = false;
                if let Some(error) = self.updates.error().map(str::to_owned) {
                    self.log(Severity::Error, "update", error);
                }
            }
        }
    }

    fn autosave_if_needed(&mut self, scene_changed: bool) {
        if !self.game_settings.autosave
            || self.document.is_read_only()
            || !self.document.is_modified()
        {
            return;
        }
        let interval_elapsed = self
            .last_autosave
            .is_none_or(|instant| instant.elapsed() >= Duration::from_secs(30));
        if !scene_changed && !interval_elapsed {
            return;
        }

        self.last_autosave = Some(Instant::now());
        match self.document.save() {
            Ok(()) => {
                self.scene_disk_snapshot = std::fs::read(self.document.scene_path()).ok();
                self.log(Severity::Info, "scene.autosave", "Scene autosaved");
            }
            Err(error) => self.log(Severity::Error, "scene.autosave", error.to_string()),
        }
    }

    fn editor_configuration() -> EditorConfiguration {
        if discover_code_editor(CodeEditor::VisualStudioCode).is_some() {
            EditorConfiguration::default()
        } else {
            EditorConfiguration {
                preferred: CodeEditor::SystemDefault,
                argument_template: vec!["{file}".into()],
                project_open_behavior: ProjectOpenBehavior::ProjectFolder,
                ..EditorConfiguration::default()
            }
        }
    }

    fn open_project_in_code_editor(&mut self) {
        let root = self.document.project().root().to_path_buf();
        let name = self.document.project().metadata().name.clone();
        let result = generate_programming_workspace(&root, &name)
            .map_err(|error| error.to_string())
            .and_then(|workspace| {
                build_editor_launch(
                    &Self::editor_configuration(),
                    &root,
                    Some(&workspace.workspace_file),
                    None,
                    None,
                    None,
                )
                .map_err(|error| error.to_string())
            })
            .and_then(|launch| {
                open_in_external_editor(&launch)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(()) => self.log(
                Severity::Info,
                "programming.editor",
                "Opened project in external code editor",
            ),
            Err(error) => self.log(Severity::Error, "programming.editor", error),
        }
    }

    fn open_script_path(&mut self, relative: &Path, line: u32, column: u32) {
        let root = self.document.project().root();
        let name = self.document.project().metadata().name.clone();
        let workspace = root.join(format!("{name}.code-workspace"));
        let path = root.join(relative);
        let result = build_editor_launch(
            &Self::editor_configuration(),
            root,
            Some(&workspace),
            Some(&path),
            Some(line.max(1)),
            Some(column.max(1)),
        )
        .map_err(|error| error.to_string())
        .and_then(|launch| {
            open_in_external_editor(&launch)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
        match result {
            Ok(()) => self.log(
                Severity::Info,
                "programming.editor",
                format!("Opened {}:{line}:{column}", relative.display()),
            ),
            Err(error) => self.log(Severity::Error, "programming.editor", error),
        }
    }

    fn create_gameplay_script(&mut self, language: ScriptLanguage, scope: ScriptScope) {
        if self.document.is_read_only() {
            self.log(
                Severity::Error,
                "programming",
                "Cannot create a script in a read-only project",
            );
            return;
        }
        let root = self.document.project().root().to_path_buf();
        let manifest_path = root.join("config/scripts.ron");
        let mut manifest = if manifest_path.is_file() {
            match std::fs::read(&manifest_path)
                .map_err(|e| e.to_string())
                .and_then(|b| {
                    load_manifest(&b)
                        .map(|v| v.manifest)
                        .map_err(|e| e.to_string())
                }) {
                Ok(value) => value,
                Err(error) => {
                    self.log(Severity::Error, "programming", error);
                    return;
                }
            }
        } else {
            ScriptManifest::default()
        };
        let id = ScriptId::new();
        let (extension, source) = gameplay_script_template(language);
        let directory = if matches!(language, ScriptLanguage::Php | ScriptLanguage::Web) {
            "ui"
        } else {
            "scripts"
        };
        let relative = PathBuf::from(format!(
            "{directory}/behavior_{}.{}",
            unix_millis(),
            extension
        ));
        if let Err(error) = AtomicFileService::new().write(&root.join(&relative), source) {
            self.log(Severity::Error, "programming", error.to_string());
            return;
        }
        manifest.scripts.push(ScriptManifestEntry {
            id,
            language,
            relative_path: relative.clone(),
            api_version: ScriptApiVersion::CURRENT,
            public_properties: Vec::new(),
        });
        if let Err(error) = save_manifest_atomic(&manifest_path, &manifest) {
            self.log(Severity::Error, "programming", error.to_string());
            return;
        }
        let attachment = match scope {
            ScriptScope::Global => {
                let mut settings = self.game_settings.clone();
                settings.global_scripts.push(ScriptReference::new(id));
                settings.save(&root).map(|()| self.game_settings = settings)
            }
            ScriptScope::Scene => {
                let mut scripts = self.document.scene_startup_scripts().to_vec();
                scripts.push(ScriptReference::new(id));
                self.document
                    .set_scene_startup_scripts(scripts)
                    .map_err(|error| error.to_string())
            }
            ScriptScope::Component => self
                .document
                .selected()
                .ok_or_else(|| "Select an object before creating a component script".to_owned())
                .and_then(|entity| {
                    let mut scripts = self
                        .document
                        .world()
                        .snapshot(entity)
                        .map_err(|error| error.to_string())?
                        .scripts;
                    scripts.push(ScriptComponent::new(id));
                    self.document
                        .set_scripts(entity, scripts)
                        .map_err(|error| error.to_string())
                }),
        };
        if let Err(error) = attachment {
            self.log(Severity::Error, "programming", error);
            return;
        }
        self.log(
            Severity::Info,
            "programming",
            format!("Created {} with stable ID {id}", relative.display()),
        );
        self.open_script_path(&relative, 1, 1);
    }

    fn attach_script_asset(&mut self, id: ScriptId, scope: ScriptScope) -> Result<(), String> {
        match scope {
            ScriptScope::Global => {
                if self
                    .game_settings
                    .global_scripts
                    .iter()
                    .any(|script| script.asset_id == id)
                {
                    return Err("script is already attached globally".into());
                }
                let mut settings = self.game_settings.clone();
                settings.global_scripts.push(ScriptReference::new(id));
                settings.save(self.document.project().root())?;
                self.game_settings = settings;
                Ok(())
            }
            ScriptScope::Scene => {
                let mut scripts = self.document.scene_startup_scripts().to_vec();
                if scripts.iter().any(|script| script.asset_id == id) {
                    return Err("script is already attached to this scene".into());
                }
                scripts.push(ScriptReference::new(id));
                self.document
                    .set_scene_startup_scripts(scripts)
                    .map_err(|error| error.to_string())
            }
            ScriptScope::Component => {
                let entity = self.document.selected().ok_or_else(|| {
                    "Select an object before attaching a component script".to_owned()
                })?;
                let relative = self.document.project().root().join("config/scripts.ron");
                let manifest =
                    load_manifest(&std::fs::read(relative).map_err(|error| error.to_string())?)
                        .map_err(|error| error.to_string())?
                        .manifest;
                let path = manifest
                    .scripts
                    .iter()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| format!("unknown script asset {id}"))?
                    .relative_path
                    .clone();
                attach_script_to_entity(&mut self.document, entity, &path)
            }
        }
    }

    fn attach_project_script(&mut self, relative_path: &Path, scope: ScriptScope) {
        let result = register_script_asset(self.document.project().root(), relative_path)
            .map_err(|error| error.to_string())
            .and_then(|id| self.attach_script_asset(id, scope));
        match result {
            Ok(()) => self.log(
                Severity::Info,
                "programming",
                format!(
                    "Attached {} as {}",
                    relative_path.display(),
                    script_scope_label(scope)
                ),
            ),
            Err(error) => self.log(Severity::Error, "programming", error),
        }
    }

    fn reload_scripts(&mut self) {
        let root = self.document.project().root().to_path_buf();
        let result = self
            .play
            .as_mut()
            .ok_or_else(|| "Reload Scripts requires an active Play session".to_owned())
            .and_then(|play| play.reload_scripts(&root));
        match result {
            Ok(outcomes) => {
                for outcome in outcomes {
                    self.log(Severity::Info, "programming.reload", outcome);
                }
            }
            Err(error) => self.log(Severity::Error, "programming.reload", error),
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the compact native menu bar keeps its closely related command wiring together"
    )]
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Game Settings…").clicked() {
                    self.show_game_settings = true;
                    ui.close();
                }
                if ui
                    .add_enabled(
                        !self.document.is_read_only(),
                        egui::Button::new("Save scene"),
                    )
                    .clicked()
                {
                    self.save();
                    ui.close();
                }
                if ui.button("Refresh renderer").clicked() {
                    self.refresh_preview();
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui
                    .add_enabled(!self.document.is_read_only(), egui::Button::new("Undo"))
                    .clicked()
                    && let Err(error) = self.document.undo()
                {
                    self.log(Severity::Error, "undo", error.to_string());
                } else if ui.input(|input| input.pointer.any_released()) {
                    self.refresh_preview();
                }
                if ui
                    .add_enabled(!self.document.is_read_only(), egui::Button::new("Redo"))
                    .clicked()
                    && let Err(error) = self.document.redo()
                {
                    self.log(Severity::Error, "undo", error.to_string());
                } else if ui.input(|input| input.pointer.any_released()) {
                    self.refresh_preview();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Reset dock layout").clicked() {
                    self.dock = default_dock(&self.workspace.tabs);
                }
                ui.separator();
                egui::widgets::global_theme_preference_buttons(ui);
                ui.separator();
                ui.label("Panels");
                for panel in [
                    EditorTab::Viewport3d,
                    EditorTab::Viewport2d,
                    EditorTab::SceneGraph,
                ] {
                    if !self.workspace.tabs.contains(&panel)
                        && ui.button(format!("Open {}", panel.title())).clicked()
                    {
                        self.workspace.tabs.insert(0, panel);
                        self.dock = default_dock(&self.workspace.tabs);
                        ui.close();
                    }
                }
                ui.checkbox(&mut self.detach_inspector, "Inspector on native viewport");
            });
            ui.menu_button("Programming", |ui| {
                for (label, scope) in [
                    ("Global Startup Script", ScriptScope::Global),
                    ("Scene Startup Script", ScriptScope::Scene),
                    ("Object Component Script", ScriptScope::Component),
                ] {
                    ui.menu_button(label, |ui| {
                        for language in [
                            ScriptLanguage::Lua54,
                            ScriptLanguage::JavaScript,
                            ScriptLanguage::Python,
                            ScriptLanguage::CSharp,
                            ScriptLanguage::C,
                            ScriptLanguage::Cpp,
                            ScriptLanguage::Java,
                            ScriptLanguage::Php,
                            ScriptLanguage::Web,
                        ] {
                            if ui
                                .add_enabled(
                                    !self.document.is_read_only(),
                                    egui::Button::new(language.display_name()),
                                )
                                .clicked()
                            {
                                self.create_gameplay_script(language, scope);
                                ui.close();
                            }
                        }
                    });
                }
                ui.separator();
                let manifest_path = self.document.project().root().join("config/scripts.ron");
                let entries = std::fs::read(manifest_path)
                    .ok()
                    .and_then(|bytes| load_manifest(&bytes).ok())
                    .map_or_else(Vec::new, |load| load.manifest.scripts);
                ui.menu_button("Attach Existing Script", |ui| {
                    for (label, scope) in [
                        ("Global Startup", ScriptScope::Global),
                        ("Scene Startup", ScriptScope::Scene),
                        ("Object Component", ScriptScope::Component),
                    ] {
                        ui.menu_button(label, |ui| {
                            if entries.is_empty() {
                                ui.weak("No script assets in this project");
                            }
                            for entry in &entries {
                                if ui
                                    .button(entry.relative_path.display().to_string())
                                    .clicked()
                                {
                                    if let Err(error) = self.attach_script_asset(entry.id, scope) {
                                        self.log(Severity::Error, "programming", error);
                                    }
                                    ui.close();
                                }
                            }
                        });
                    }
                });
                if ui.button("Open Project in Code Editor").clicked() {
                    self.open_project_in_code_editor();
                    ui.close();
                }
                if ui
                    .add_enabled(self.play.is_some(), egui::Button::new("Reload Scripts"))
                    .clicked()
                {
                    self.reload_scripts();
                    ui.close();
                }
                if ui
                    .add_enabled(self.play.is_some(), egui::Button::new("Restart Play"))
                    .clicked()
                {
                    let mode = self.active_play_mode;
                    self.stop_play();
                    self.start_play(mode);
                    ui.close();
                }
                ui.separator();
                ui.label("Multi-language · API 1.0 · external editor only");
            });
            ui.separator();
            for (label, operation) in [
                ("Move", GizmoOperation::Translate),
                ("Rotate", GizmoOperation::Rotate),
                ("Scale", GizmoOperation::Scale),
            ] {
                if transform_mode_button(ui, self.gizmo.operation, operation, label).clicked() {
                    self.gizmo.operation = operation;
                }
            }
            ui.separator();
            ui.label(
                egui::RichText::new(&self.document.project().metadata().name)
                    .strong()
                    .color(ui.visuals().text_color()),
            );
            if self.document.is_modified() {
                ui.add(Icon::Circle.image(ui.visuals().warn_fg_color, 12.0));
                ui.colored_label(ui.visuals().warn_fg_color, "Unsaved");
            }
            if self.document.is_read_only() {
                ui.colored_label(ui.visuals().warn_fg_color, "READ ONLY");
            }
            ui.separator();
            if self.play.is_none() {
                if ui.add(Icon::Play.button("Play")).clicked() {
                    self.start_play(self.active_play_mode);
                }
                egui::ComboBox::from_id_salt("play-mode")
                    .selected_text(self.active_play_mode.as_str())
                    .show_ui(ui, |ui| {
                        for (label, mode) in [
                            ("Play", PlayMode::Play),
                            ("New Window", PlayMode::NewWindow),
                            ("Standalone", PlayMode::Standalone),
                        ] {
                            ui.selectable_value(&mut self.active_play_mode, mode, label);
                        }
                    });
            } else {
                if ui.add(Icon::Square.button("Stop")).clicked() {
                    self.stop_play();
                }
                let paused = self
                    .play
                    .as_ref()
                    .is_some_and(|play| play.state() == RuntimeState::Paused);
                if ui
                    .add(if paused {
                        Icon::Play.button("Resume")
                    } else {
                        Icon::Pause.button("Pause")
                    })
                    .clicked()
                {
                    self.control_play(if paused {
                        ControlRequest::Resume
                    } else {
                        ControlRequest::Pause
                    });
                }
                if ui
                    .add_enabled(paused, Icon::StepForward.button("Frame"))
                    .clicked()
                {
                    self.control_play(ControlRequest::FrameAdvance);
                }
                if let Some(play) = &self.play {
                    ui.small(format!(
                        "{} · tick {}",
                        play.mode().as_str(),
                        play.fixed_tick()
                    ));
                }
            }
        });
    }

    fn game_settings_dialog(&mut self, context: &egui::Context) {
        if !self.show_game_settings {
            return;
        }
        let mut open = self.show_game_settings;
        egui::Window::new("Game Settings")
            .open(&mut open)
            .resizable(false)
            .show(context, |ui| {
                ui.label("Game name");
                ui.add_enabled(
                    !self.document.is_read_only(),
                    egui::TextEdit::singleline(&mut self.game_name_text)
                        .desired_width(420.0)
                        .hint_text("My Game"),
                );
                ui.small("Shown in the editor title and stored in the project descriptor.");
                ui.add_space(8.0);
                ui.add_enabled(
                    !self.document.is_read_only(),
                    egui::Checkbox::new(&mut self.autosave_enabled, "Autosave scene"),
                );
                ui.small(
                    "Saves after an attribute changes and flushes pending changes every 30 seconds.",
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.document.is_read_only(), egui::Button::new("Save"))
                        .clicked()
                    {
                        let game_name = self.game_name_text.trim().to_owned();
                        let candidate = GameSettings {
                            global_scripts: self.game_settings.global_scripts.clone(),
                            autosave: self.autosave_enabled,
                        };
                        let root = self.document.project().root().to_path_buf();
                        let mut project = self.document.project().clone();
                        project.metadata_mut().name = game_name;
                        let result = candidate
                            .validate()
                            .and_then(|()| {
                                project.descriptor().validate().map_err(|e| e.to_string())
                            })
                            .and_then(|()| candidate.save(&root))
                            .and_then(|()| {
                                project.save().map_err(|e| e.to_string())?;
                                *self.document.project_mut().map_err(|e| e.to_string())? = project;
                                Ok(())
                            });
                        match result {
                            Ok(()) => {
                                let autosave_was_enabled = self.game_settings.autosave;
                                self.game_settings = candidate;
                                if !autosave_was_enabled && self.game_settings.autosave {
                                    self.last_autosave = None;
                                }
                                self.show_game_settings = false;
                                context.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                                    "{} — Rustic Editor",
                                    self.document.project().metadata().name
                                )));
                                self.log(
                                    Severity::Info,
                                    "game.settings",
                                    "Saved game name and autosave settings",
                                );
                            }
                            Err(error) => self.log(Severity::Error, "game.settings", error),
                        }
                    }
                });
            });
        self.show_game_settings &= open;
    }

    fn dependency_prompt(&mut self, context: &egui::Context) {
        if !self.show_dependency_prompt {
            return;
        }
        let names = self
            .missing_dependencies
            .iter()
            .map(|language| language.display_name())
            .collect::<Vec<_>>()
            .join(", ");
        egui::Window::new("Missing gameplay dependencies")
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
                ui.label(format!(
                    "This project uses {names}, but the required toolchains are not installed."
                ));
                ui.label("Would you like to install them now?");
                ui.horizontal(|ui| {
                    if ui.button("Install dependencies").clicked() {
                        match launch_toolchain_manager(&self.missing_dependencies) {
                            Ok(()) => self.log(
                                Severity::Info,
                                "programming.dependencies",
                                "Opened the dependency installer",
                            ),
                            Err(error) => {
                                self.log(Severity::Error, "programming.dependencies", error);
                            }
                        }
                        self.show_dependency_prompt = false;
                    }
                    if ui.button("Not now").clicked() {
                        self.show_dependency_prompt = false;
                    }
                });
            });
    }
}

impl Drop for EditorApp {
    fn drop(&mut self) {
        if let Some(play) = self.play.take() {
            let _ = play.stop();
        }
        self.workspace.ui_scale = self.workspace.ui_scale.clamp(0.75, 2.5);
        let _ = self
            .workspace
            .save(&self.workspace_store, &self.workspace_path);
        if let Some(logger) = &self.logger {
            let _ = logger.shutdown();
        }
    }
}

fn transform_mode_button(
    ui: &mut egui::Ui,
    current: GizmoOperation,
    operation: GizmoOperation,
    label: &str,
) -> egui::Response {
    let icon = match operation {
        GizmoOperation::Translate => Icon::Move3d,
        GizmoOperation::Rotate => Icon::Rotate3d,
        GizmoOperation::Scale => Icon::Scaling,
    };
    ui.add(icon.button(label).selected(current == operation))
        .on_hover_text(format!("{label} tool"))
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let scene_was_modified = self.document.is_modified();
        // Finish even if release happens outside the viewport or another tab is active.
        if !ui.input(|input| input.pointer.primary_down())
            && let Some(drag) = self.gizmo_drag.take()
            && let Err(error) =
                self.document
                    .commit_transform_drag(drag.entity, drag.before, drag.after)
        {
            self.console
                .push(simple_console(Severity::Error, "gizmo", error.to_string()));
        }
        self.poll_background(ui.ctx());
        let title = format!(
            "{} — Rustic Editor",
            self.document.project().metadata().name
        );
        if self.updates.show(ui, &title) {
            self.update_close_requested = true;
        }
        if self.updates.take_cancellation() {
            self.update_close_requested = false;
        }
        egui::Panel::top("editor-toolbar").show(ui, |ui| self.top_bar(ui));
        if self.detach_inspector {
            let selection = self
                .document
                .selected()
                .map_or_else(|| "No selection".to_owned(), |id| format!("Selected {id}"));
            ui.ctx().show_viewport_deferred(
                egui::ViewportId::from_hash_of("rustic-detached-inspector"),
                egui::ViewportBuilder::default()
                    .with_title("Rustic Inspector")
                    .with_inner_size([420.0, 640.0]),
                move |ui, _class| {
                    ui.heading("Inspector");
                    ui.label(&selection);
                    ui.label("The live editing inspector remains docked in the primary window.");
                },
            );
        }
        let project_root = self.document.project().root().to_path_buf();
        let mut project_file_action = None;
        let mut viewer = EditorViewer {
            document: &mut self.document,
            project_root: &project_root,
            console: &mut self.console,
            project_tree: self.project_tree.as_ref(),
            project_panel_mode: &mut self.project_panel_mode,
            project_filter: &mut self.project_filter,
            preview: self.preview.as_ref(),
            preview_status: &self.preview_status,
            request_preview: false,
            playing: self.play.is_some(),
            can_switch_scene: self.play.is_none() && self.pending_runtime_changes.is_none(),
            play: self.play.as_mut(),
            play_keys: BTreeSet::new(),
            camera: &mut self.camera,
            viewport_extent: &mut self.viewport_extent,
            gizmo: &mut self.gizmo,
            gizmo_drag: &mut self.gizmo_drag,
            gizmo_widget: &mut self.gizmo_widget,
            scene_graph: &mut self.scene_graph,
            model_assets: &mut self.model_assets,
            project_file_action: &mut project_file_action,
        };
        self.dock.ui(&mut viewer, ui);
        viewer.flush_play_input();
        let request_preview = viewer.request_preview;
        drop(viewer);
        self.apply_deferred_project_action(project_file_action, ui.ctx());
        if request_preview {
            self.refresh_preview();
        }
        self.runtime_changes_dialog(ui.ctx());
        self.scene_rename_dialog(ui.ctx());
        self.game_settings_dialog(ui.ctx());
        self.dependency_prompt(ui.ctx());
        self.update_close_dialog(ui);
        let scene_changed = !scene_was_modified && self.document.is_modified();
        self.autosave_if_needed(scene_changed);
    }
}

struct EditorViewer<'a> {
    document: &'a mut AuthoringDocument,
    project_root: &'a Path,
    console: &'a mut RuntimeConsole,
    project_tree: Option<&'a ProjectDirectory>,
    project_panel_mode: &'a mut ProjectPanelMode,
    project_filter: &'a mut String,
    preview: Option<&'a egui::TextureHandle>,
    preview_status: &'a str,
    request_preview: bool,
    playing: bool,
    can_switch_scene: bool,
    play: Option<&'a mut EditorPlaySession>,
    play_keys: BTreeSet<String>,
    camera: &'a mut EditorCamera,
    viewport_extent: &'a mut [u32; 2],
    gizmo: &'a mut GizmoSettings,
    gizmo_drag: &'a mut Option<GizmoDrag>,
    gizmo_widget: &'a mut transform_widget::TransformWidget,
    scene_graph: &'a mut scene_graph::SceneGraph,
    model_assets: &'a mut BTreeMap<AssetId, ModelArtifact>,
    project_file_action: &'a mut Option<ProjectFileAction>,
}

#[derive(Clone, Debug)]
enum ViewportDrop {
    File(PathBuf),
    Directory(PathBuf),
    Entries(Vec<PathBuf>),
}

impl ViewportDrop {
    fn project_paths(&self) -> Vec<PathBuf> {
        match self {
            Self::File(path) | Self::Directory(path) => vec![path.clone()],
            Self::Entries(paths) => paths.clone(),
        }
    }
}

impl Behavior<EditorTab> for EditorViewer<'_> {
    fn tab_title_for_pane(&mut self, pane: &EditorTab) -> egui::WidgetText {
        pane.title().into()
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile_id: TileId, pane: &mut EditorTab) -> UiResponse {
        match pane {
            EditorTab::Viewport3d => self.viewport(ui, false),
            EditorTab::Viewport2d => self.viewport(ui, true),
            EditorTab::Hierarchy => self.hierarchy(ui),
            EditorTab::Inspector | EditorTab::Settings => self.inspector(ui),
            // Kept as a compatibility fallback for an in-memory legacy dock.
            EditorTab::ContentBrowser => self.project_explorer(ui),
            EditorTab::Console => self.console(ui),
            EditorTab::SceneGraph => self.scene_graph.show(ui, self.document),
        }
        UiResponse::None
    }
}

impl EditorViewer<'_> {
    #[allow(
        clippy::too_many_lines,
        reason = "viewport allocation, input routing, and drop-zone response share one egui lifetime"
    )]
    fn viewport(&mut self, ui: &mut egui::Ui, is_2d: bool) {
        ui.horizontal(|ui| {
            if self.playing {
                ui.label("Game view · Active scene camera · 16:9");
            }
            if !is_2d && !self.playing {
                ui.selectable_value(&mut self.gizmo.space, GizmoSpace::World, "World");
                ui.selectable_value(&mut self.gizmo.space, GizmoSpace::Local, "Local");
                ui.checkbox(&mut self.gizmo.snapping, "Snap");
                if self.gizmo.snapping {
                    match self.gizmo.operation {
                        GizmoOperation::Translate => {
                            ui.add(
                                egui::DragValue::new(&mut self.gizmo.translation_snap).speed(0.1),
                            );
                        }
                        GizmoOperation::Rotate => {
                            let mut degrees = self.gizmo.rotation_snap_radians.to_degrees();
                            if ui
                                .add(egui::DragValue::new(&mut degrees).suffix("°"))
                                .changed()
                            {
                                self.gizmo.rotation_snap_radians = degrees.max(0.1).to_radians();
                            }
                        }
                        GizmoOperation::Scale => {
                            ui.add(egui::DragValue::new(&mut self.gizmo.scale_snap).speed(0.05));
                        }
                    }
                }
                if ui.button("Frame (F)").clicked() {
                    self.frame_selected();
                }
                if ui.button("Reset (Home)").clicked() {
                    self.camera.reset();
                    self.request_preview = true;
                }
            }
        });
        let frame = egui::Frame::NONE.fill(ui.visuals().extreme_bg_color);
        let (body, dropped) = ui.dnd_drop_zone::<ViewportDrop, _>(frame, |ui| {
            let size = ui.available_size().max(egui::vec2(64.0, 64.0));
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
            if let Some(texture) = self.preview {
                let rect = if self.playing {
                    let aspect = 16.0 / 9.0;
                    let size = if rect.width() / rect.height() > aspect {
                        egui::vec2(rect.height() * aspect, rect.height())
                    } else {
                        egui::vec2(rect.width(), rect.width() / aspect)
                    };
                    egui::Rect::from_center_size(rect.center(), size)
                } else {
                    rect
                };
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                ui.painter()
                    .rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    self.preview_status,
                    egui::TextStyle::Body.resolve(ui.style()),
                    ui.visuals().text_color(),
                );
            }
            response
        });
        let response = body.inner;
        response.clone().on_hover_text(if is_2d {
            "Orthographic 2D authoring viewport; drop content to select it"
        } else {
            "LMB click select/drag look · MMB orbit · Shift+MMB or Q/E pan · WASD move · wheel zoom · F frame · Home reset"
        });
        let pixels_per_point = ui.ctx().pixels_per_point();
        let extent = [
            (response.rect.width() * pixels_per_point)
                .round()
                .max(0.0)
                .to_u32()
                .unwrap_or(u32::MAX),
            (response.rect.height() * pixels_per_point)
                .round()
                .max(0.0)
                .to_u32()
                .unwrap_or(u32::MAX),
        ];
        if extent != *self.viewport_extent {
            *self.viewport_extent = extent;
            self.request_preview = true;
        }
        if self.playing {
            self.play_viewport_input(ui, &response);
        } else if !is_2d {
            let was_dragging = self.gizmo_drag.is_some();
            self.gizmo_overlay(ui, response.rect);
            if !was_dragging && self.gizmo_drag.is_none() && !self.gizmo_widget.is_focused() {
                self.viewport_input(ui, &response);
            }
        }
        if let Some(ViewportDrop::File(path)) = dropped.as_deref() {
            if path.extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("scene") || extension.eq_ignore_ascii_case("rscene")
            }) {
                let relative = path
                    .strip_prefix("scene")
                    .or_else(|_| path.strip_prefix("scenes"))
                    .unwrap_or(path);
                match self.document.insert_scene(relative, None) {
                    Ok(ids) => {
                        self.request_preview = true;
                        self.console.push(simple_console(
                            Severity::Info,
                            "scene",
                            format!("Inserted {} objects from {}", ids.len(), path.display()),
                        ));
                    }
                    Err(error) => self.console.push(simple_console(
                        Severity::Error,
                        "scene",
                        error.to_string(),
                    )),
                }
            } else if is_model_asset(path) {
                if is_2d || self.playing {
                    self.console.push(simple_console(
                        Severity::Warning,
                        "editor",
                        "Models can only be dropped into the editable 3D viewport",
                    ));
                } else {
                    let drop_position = ui
                        .input(|input| input.pointer.interact_pos())
                        .and_then(|pointer| {
                            viewport_ground_position(*self.camera, response.rect, pointer)
                        })
                        .unwrap_or(self.camera.focus);
                    match import_model_asset(self.project_root, path).and_then(
                        |(asset_id, model)| {
                            let name = path
                                .file_stem()
                                .and_then(|value| value.to_str())
                                .unwrap_or("Model");
                            let transform = LocalTransform {
                                translation: drop_position,
                                ..LocalTransform::IDENTITY
                            };
                            let parent = selected_scene_folder(self.document);
                            let entity = self
                                .document
                                .add_model(name, asset_id, parent, transform)
                                .map_err(|error| error.to_string())?;
                            self.model_assets.insert(asset_id, model);
                            Ok(entity)
                        },
                    ) {
                        Ok(_) => {
                            self.request_preview = true;
                            self.console.push(simple_console(
                                Severity::Info,
                                "editor",
                                format!("Added model {} to the 3D world", path.display()),
                            ));
                        }
                        Err(error) => {
                            self.console
                                .push(simple_console(Severity::Error, "assets", error));
                        }
                    }
                }
            } else if ScriptLanguage::from_path(path).is_some() {
                if let Some(entity) = self.document.selected() {
                    match attach_script_to_entity(self.document, entity, path) {
                        Ok(()) => self.console.push(simple_console(
                            Severity::Info,
                            "programming",
                            format!("Attached {} to selected object", path.display()),
                        )),
                        Err(error) => {
                            self.console.push(simple_console(
                                Severity::Error,
                                "programming",
                                error,
                            ));
                        }
                    }
                } else {
                    self.console.push(simple_console(
                        Severity::Warning,
                        "programming",
                        "Select an object before dropping a component script",
                    ));
                }
            } else {
                self.console.push(simple_console(
                    Severity::Info,
                    "editor",
                    format!("Dropped asset {} into viewport", path.display()),
                ));
            }
        }
        // Directories organize project assets and have no viewport representation.
        if matches!(dropped.as_deref(), Some(ViewportDrop::Directory(_))) {
            self.console.push(simple_console(
                Severity::Info,
                "explorer",
                "Folders can only be moved inside the Explorer",
            ));
        }
        if matches!(dropped.as_deref(), Some(ViewportDrop::Entries(_))) {
            self.console.push(simple_console(
                Severity::Info,
                "explorer",
                "Drop one model or script at a time into the viewport",
            ));
        }
        ui.small(self.preview_status);
    }

    fn play_viewport_input(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if response.hovered() && ui.input(|input| input.pointer.any_pressed()) {
            response.request_focus();
        }
        let keyboard_active = viewport_keyboard::active(response);
        let keys = if keyboard_active {
            ui.input(|input| {
                let mut keys = BTreeSet::new();
                for (key, name) in [
                    (egui::Key::A, "KeyA"),
                    (egui::Key::D, "KeyD"),
                    (egui::Key::W, "KeyW"),
                    (egui::Key::S, "KeyS"),
                    (egui::Key::ArrowLeft, "ArrowLeft"),
                    (egui::Key::ArrowRight, "ArrowRight"),
                    (egui::Key::ArrowUp, "ArrowUp"),
                    (egui::Key::ArrowDown, "ArrowDown"),
                ] {
                    if input.key_down(key) {
                        keys.insert(name.to_owned());
                    }
                }
                if input.modifiers.shift {
                    keys.insert("ShiftLeft".to_owned());
                    keys.insert("ShiftRight".to_owned());
                }
                keys
            })
        } else {
            BTreeSet::new()
        };
        self.play_keys.extend(keys);
    }

    fn flush_play_input(&mut self) {
        if let Some(play) = self.play.as_mut()
            && let Err(error) = play.set_input_keys(std::mem::take(&mut self.play_keys))
        {
            self.console
                .push(simple_console(Severity::Error, "runtime.input", error));
        }
    }

    fn viewport_input(&mut self, ui: &egui::Ui, response: &egui::Response) {
        let hovered = response.hovered();
        if hovered && ui.input(|input| input.pointer.any_pressed()) {
            response.request_focus();
        }
        let pointer_delta = ui.input(|input| input.pointer.delta());
        let shift = ui.input(|input| input.modifiers.shift);
        if response.dragged_by(egui::PointerButton::Middle) {
            if shift {
                self.camera.pan(
                    Vec2::new(pointer_delta.x, pointer_delta.y),
                    response.rect.height(),
                );
            } else {
                self.camera
                    .orbit(Vec2::new(pointer_delta.x, pointer_delta.y));
            }
            self.request_preview = true;
        }
        if hovered {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera.dolly(scroll.signum());
                self.request_preview = true;
            }
        }
        let keyboard_active = viewport_keyboard::active(response);
        if keyboard_active {
            ui.input(|input| {
                if input.key_pressed(egui::Key::F) {
                    self.frame_selected();
                }
                if input.key_pressed(egui::Key::Home) {
                    self.camera.reset();
                    self.request_preview = true;
                }
                let motion = Vec3::new(
                    f32::from(input.key_down(egui::Key::D))
                        - f32::from(input.key_down(egui::Key::A)),
                    0.0,
                    f32::from(input.key_down(egui::Key::W))
                        - f32::from(input.key_down(egui::Key::S)),
                );
                if motion.length_squared() > 0.0 {
                    self.camera.fly(
                        motion.normalize(),
                        input.stable_dt,
                        self.camera.distance.max(1.0),
                    );
                    self.request_preview = true;
                    ui.ctx().request_repaint();
                }
                let horizontal_pan = f32::from(input.key_down(egui::Key::E))
                    - f32::from(input.key_down(egui::Key::Q));
                if horizontal_pan != 0.0 {
                    self.camera.pan(
                        Vec2::new(-horizontal_pan * 600.0 * input.stable_dt, 0.0),
                        response.rect.height(),
                    );
                    self.request_preview = true;
                    ui.ctx().request_repaint();
                }
            });
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || (response.dragged_by(egui::PointerButton::Primary) && self.gizmo_drag.is_none())
        {
            self.camera
                .orbit(Vec2::new(pointer_delta.x, pointer_delta.y));
            self.request_preview = true;
        }
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(position) = response.interact_pointer_pos()
        {
            let pixel = Vec2::new(
                position.x - response.rect.left(),
                position.y - response.rect.top(),
            );
            let extent = Vec2::new(response.rect.width(), response.rect.height());
            let picked = self
                .camera
                .world_ray(pixel, extent)
                .and_then(|ray| pick_meshes(ray, &pick_scene(self.document, self.model_assets)));
            let preserve = ui.input(|input| input.modifiers.shift || input.modifiers.command);
            if picked.is_some() || !preserve {
                self.document.select(picked);
                self.request_preview = true;
            }
        }
    }

    fn frame_selected(&mut self) {
        let Some(entity) = self.document.selected() else {
            return;
        };
        let Ok(world) = self.document.world().world_transform(entity) else {
            return;
        };
        let center = world.0.transform_point3(Vec3::ZERO);
        let radius = world.0.transform_vector3(Vec3::ONE).length().max(0.5);
        self.camera.frame_sphere(center, radius);
        self.request_preview = true;
    }

    fn gizmo_overlay(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        if self.document.is_read_only() {
            self.gizmo_widget.reset();
            return;
        }
        let Some(entity) = self.document.selected() else {
            self.gizmo_widget.reset();
            return;
        };
        let Ok(snapshot) = self.document.world().snapshot(entity) else {
            return;
        };
        let Ok(world) = self.document.world().world_transform(entity) else {
            return;
        };
        let parent = snapshot
            .parent
            .and_then(|id| self.document.world().world_transform(id).ok())
            .map_or(Mat4::IDENTITY, |world| world.0);
        if let Some(after) = self.gizmo_widget.interact(
            ui,
            rect,
            *self.camera,
            transform_widget::Target {
                entity,
                world: world.0,
                parent,
            },
            *self.gizmo,
        ) {
            if after
                .matrix()
                .abs_diff_eq(snapshot.local_transform.matrix(), 1.0e-6)
            {
                return;
            }
            let drag = self.gizmo_drag.get_or_insert(GizmoDrag {
                entity,
                before: snapshot.local_transform,
                after: snapshot.local_transform,
            });
            if self.document.preview_transform(entity, after).is_ok() {
                drag.after = after;
                self.request_preview = true;
            }
        }
    }

    fn hierarchy(&mut self, ui: &mut egui::Ui) {
        self.project_explorer(ui);
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the project explorer keeps its compact tree and toolbar interaction together"
    )]
    fn project_explorer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(egui::RichText::new("Project").color(ui.visuals().text_color()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("⚙", |ui| self.project_settings(ui))
                    .response
                    .on_hover_text("Project settings");
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.selectable_value(self.project_panel_mode, ProjectPanelMode::Scene, "Scene");
            ui.selectable_value(
                self.project_panel_mode,
                ProjectPanelMode::Explorer,
                "Explorer",
            );
        });
        ui.separator();

        match *self.project_panel_mode {
            ProjectPanelMode::Scene => self.scene_tree(ui),
            ProjectPanelMode::Explorer => self.file_explorer(ui),
        }
    }

    fn project_settings(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(self.can_switch_scene, |ui| {
            ui.menu_button("Change scene", |ui| {
                let Ok(scenes) = self.document.project().directory(VirtualDirectory::Scenes) else {
                    ui.label("Scenes folder unavailable");
                    return;
                };
                let relative_scenes = scenes.strip_prefix(self.project_root).unwrap_or(&scenes);
                let mut paths = self
                    .project_tree
                    .map_or_else(Vec::new, |tree| tree.scene_paths(relative_scenes));
                let active = self
                    .document
                    .scene_path()
                    .strip_prefix(&scenes)
                    .ok()
                    .map(Path::to_path_buf);
                if let Some(active) = &active {
                    paths.push(active.clone());
                }
                paths.sort();
                paths.dedup();
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        if self.project_tree.is_none() {
                            ui.label("Scanning project scenes…");
                        }
                        for path in paths {
                            let selected = active.as_ref() == Some(&path);
                            let is_start = self
                                .document
                                .project()
                                .start_scene_path()
                                .is_ok_and(|start| start == scenes.join(&path));
                            if let Some(action) = scene_picker_entry(
                                ui,
                                &path,
                                selected,
                                is_start,
                                !self.document.is_read_only(),
                            ) {
                                *self.project_file_action = Some(action);
                            }
                        }
                    });
            });
        })
        .response
        .on_disabled_hover_text("Stop play and review runtime changes before changing scenes");
    }

    #[allow(
        clippy::too_many_lines,
        reason = "scene tree toolbar and recursive hierarchy share the current selection"
    )]
    fn scene_tree(&mut self, ui: &mut egui::Ui) {
        let snapshots = self.document.entity_snapshots();
        let entity_count = snapshots.as_ref().map_or(0, Vec::len);
        let selected_folder = selected_scene_folder(self.document);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{entity_count} entities"))
                    .color(ui.visuals().weak_text_color()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("+ Add", |ui| {
                    if ui
                        .add_enabled(!self.document.is_read_only(), egui::Button::new("Folder"))
                        .clicked()
                    {
                        match self.document.add_folder(selected_folder) {
                            Ok(_) => self.request_preview = true,
                            Err(error) => self.console.push(simple_console(
                                Severity::Error,
                                "editor",
                                error.to_string(),
                            )),
                        }
                        ui.close();
                    }
                    ui.separator();
                    for (label, is_camera) in [("Camera", true), ("Light", false)] {
                        if ui.button(label).clicked() {
                            let transform = if is_camera {
                                camera_placement(*self.camera)
                            } else {
                                LocalTransform::IDENTITY
                            };
                            match self.document.add_camera_or_light(
                                is_camera,
                                transform,
                                selected_folder,
                            ) {
                                Ok(_) => self.request_preview = true,
                                Err(error) => self.console.push(simple_console(
                                    Severity::Error,
                                    "inspector",
                                    error.to_string(),
                                )),
                            }
                            ui.close();
                        }
                    }
                    for (name, primitive) in primitive_palette() {
                        if ui
                            .add_enabled(!self.document.is_read_only(), egui::Button::new(name))
                            .clicked()
                        {
                            match self
                                .document
                                .add_primitive(name, primitive, selected_folder)
                            {
                                Ok(_) => self.request_preview = true,
                                Err(error) => self.console.push(simple_console(
                                    Severity::Error,
                                    "editor",
                                    error.to_string(),
                                )),
                            }
                            ui.close();
                        }
                    }
                });
            });
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("scene-tree")
            .auto_shrink([false, false])
            .show(ui, |ui| match snapshots {
                Ok(snapshots) => {
                    let background = explorer_tree_background(ui, "scene-tree-background");
                    let (root, dropped) =
                        ui.dnd_drop_zone::<SceneTreeDrag, _>(egui::Frame::NONE, |ui| {
                            render_scene_hierarchy(
                                ui,
                                &snapshots,
                                None,
                                self.document.selected(),
                                !self.document.is_read_only() && !self.playing,
                                self.gizmo.space,
                            )
                        });
                    let dropped =
                        dropped.or_else(|| background.dnd_release_payload::<SceneTreeDrag>());
                    background.context_menu(|ui| {
                        let enabled = !self.playing
                            && !self.document.is_read_only()
                            && ui.ctx().data_mut(|data| {
                                data.get_temp::<SceneClipboard>(egui::Id::new("scene-clipboard"))
                                    .is_some()
                            });
                        if ui
                            .add_enabled(enabled, egui::Button::new("Paste at Scene Root"))
                            .clicked()
                        {
                            self.apply_scene_tree_action(ui, SceneTreeAction::Paste(None));
                            ui.close();
                        }
                    });
                    if let Some(id) = root.inner.clicked {
                        self.document.select(Some(id));
                        self.request_preview = true;
                    }
                    if let Some(action) = root.inner.action {
                        self.apply_scene_tree_action(ui, action);
                    }
                    let reparent = root
                        .inner
                        .reparent
                        .or_else(|| dropped.as_deref().map(|payload| (payload.entity, None)));
                    if let Some((entity, parent)) = reparent
                        && !self.playing
                        && !self.document.is_read_only()
                        && let Err(error) = self.document.reparent(entity, parent)
                    {
                        self.console.push(simple_console(
                            Severity::Error,
                            "editor",
                            error.to_string(),
                        ));
                    }
                }
                Err(error) => {
                    ui.colored_label(egui::Color32::RED, error.to_string());
                }
            });
    }

    fn apply_scene_tree_action(&mut self, ui: &egui::Ui, action: SceneTreeAction) {
        if let SceneTreeAction::Space(entity, space) = action {
            self.document.select(Some(entity));
            self.gizmo.space = space;
            self.request_preview = true;
            return;
        }
        if self.playing || self.document.is_read_only() {
            return;
        }
        let clipboard_id = egui::Id::new("scene-clipboard");
        let result = match action {
            SceneTreeAction::Rename(entity, name) => self.document.set_name(entity, name),
            SceneTreeAction::Delete(entity) => self.document.delete_tree(entity),
            action @ (SceneTreeAction::Copy(_) | SceneTreeAction::Cut(_)) => {
                let cut = matches!(action, SceneTreeAction::Cut(_));
                let (SceneTreeAction::Copy(entity) | SceneTreeAction::Cut(entity)) = action else {
                    unreachable!()
                };
                self.document.snapshot_tree(entity).map(|snapshots| {
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(clipboard_id, SceneClipboard { snapshots, cut })
                    });
                })
            }
            SceneTreeAction::Paste(parent) => {
                if let Some(clipboard) = ui
                    .ctx()
                    .data_mut(|data| data.get_temp::<SceneClipboard>(clipboard_id))
                {
                    if clipboard.cut {
                        let result = self.document.reparent(clipboard.snapshots[0].id, parent);
                        if result.is_ok() {
                            ui.ctx()
                                .data_mut(|data| data.remove::<SceneClipboard>(clipboard_id));
                        }
                        result
                    } else {
                        self.document
                            .paste_tree(&clipboard.snapshots, parent)
                            .map(|_| ())
                    }
                } else {
                    Ok(())
                }
            }
            SceneTreeAction::Space(_, _) => Ok(()),
        };
        match result {
            Ok(()) => self.request_preview = true,
            Err(error) => self.console.push(simple_console(
                Severity::Error,
                "explorer",
                error.to_string(),
            )),
        }
    }

    fn file_explorer(&mut self, ui: &mut egui::Ui) {
        begin_explorer_selection(ui);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Filter").color(ui.visuals().weak_text_color()));
            ui.add(
                egui::TextEdit::singleline(self.project_filter)
                    .hint_text("Filter files…")
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if let Some(tree) = self.project_tree {
                ui.label(
                    egui::RichText::new(&tree.name)
                        .strong()
                        .color(ui.visuals().text_color()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} files", tree.file_count()))
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                });
            } else {
                ui.spinner();
            }
        });
        ui.separator();
        let mut action = None;
        if let Some(tree) = self.project_tree {
            let filter = self.project_filter.trim().to_ascii_lowercase();
            egui::ScrollArea::vertical()
                .id_salt("game-project-files")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let background = explorer_tree_background(ui, "project-tree-background");
                    let (root, dropped) =
                        ui.dnd_drop_zone::<ViewportDrop, _>(egui::Frame::NONE, |ui| {
                            render_project_root(
                                ui,
                                tree,
                                &filter,
                                !self.document.is_read_only() && !self.playing,
                                self.document.selected().is_some(),
                            )
                        });
                    action = root.inner;
                    if background.clicked() {
                        clear_explorer_selection(ui);
                    }
                    let dropped =
                        dropped.or_else(|| background.dnd_release_payload::<ViewportDrop>());
                    background.context_menu(|ui| {
                        let enabled = !self.playing
                            && !self.document.is_read_only()
                            && ui.ctx().data_mut(|data| {
                                data.get_temp::<ProjectClipboard>(project_clipboard_id())
                                    .is_some()
                            });
                        if ui
                            .add_enabled(enabled, egui::Button::new("Paste at Project Root"))
                            .clicked()
                        {
                            action = Some(ProjectFileAction::Paste(vec![PathBuf::new()]));
                            ui.close();
                        }
                    });
                    if action.is_none() {
                        action = dropped.as_deref().map(|payload| ProjectFileAction::Move {
                            sources: payload.project_paths(),
                            destination: PathBuf::new(),
                        });
                    }
                    if !filter.is_empty() && !project_directory_matches(tree, &filter) {
                        ui.vertical_centered(|ui| {
                            ui.add_space(24.0);
                            ui.weak("No matching files");
                        });
                    }
                });
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.spinner();
                ui.weak("Scanning project files…");
            });
        }
        if let Some(shortcut) = explorer_keyboard_action(
            ui,
            self.project_root,
            !self.document.is_read_only() && !self.playing,
        ) {
            action = Some(shortcut);
        }
        self.apply_project_file_action(ui, action);
    }

    fn apply_project_file_action(&mut self, ui: &egui::Ui, action: Option<ProjectFileAction>) {
        let can_edit = !self.document.is_read_only() && !self.playing;
        match action {
            Some(ProjectFileAction::Open(path)) => self.open_project_script(&path),
            Some(action @ ProjectFileAction::Attach(_, _)) if can_edit => {
                *self.project_file_action = Some(action);
            }
            Some(entry_action @ (ProjectFileAction::Copy(_) | ProjectFileAction::Cut(_)))
                if can_edit =>
            {
                let cut = matches!(entry_action, ProjectFileAction::Cut(_));
                let (ProjectFileAction::Copy(sources) | ProjectFileAction::Cut(sources)) =
                    entry_action
                else {
                    unreachable!()
                };
                if !sources.is_empty() {
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(
                            project_clipboard_id(),
                            ProjectClipboard {
                                sources: selection_roots(sources),
                                cut,
                            },
                        )
                    });
                }
            }
            Some(action) if can_edit => {
                let renamed = match &action {
                    ProjectFileAction::Rename(source, name) => {
                        Some((source.clone(), source.with_file_name(name)))
                    }
                    _ => None,
                };
                let results = match action {
                    ProjectFileAction::Move {
                        sources,
                        destination,
                    } => selection_roots(sources)
                        .iter()
                        .map(|source| move_project_entry(self.project_root, source, &destination))
                        .collect::<Vec<_>>(),
                    ProjectFileAction::Rename(source, name) => {
                        vec![rename_project_entry(self.project_root, &source, &name)]
                    }
                    ProjectFileAction::Delete(sources) => selection_roots(sources)
                        .iter()
                        .map(|source| delete_document_project_entry(self.document, source))
                        .collect(),
                    ProjectFileAction::Paste(destinations) => {
                        if let Some(mut clipboard) = ui.ctx().data_mut(|data| {
                            data.get_temp::<ProjectClipboard>(project_clipboard_id())
                        }) {
                            let results = paste_project_entries(
                                self.project_root,
                                &mut clipboard,
                                &destinations,
                            );
                            ui.ctx().data_mut(|data| {
                                if clipboard.sources.is_empty() {
                                    data.remove::<ProjectClipboard>(project_clipboard_id());
                                } else {
                                    data.insert_temp(project_clipboard_id(), clipboard);
                                }
                            });
                            results
                        } else {
                            vec![]
                        }
                    }
                    _ => vec![],
                };
                let succeeded = results.iter().all(Result::is_ok);
                for error in results.into_iter().filter_map(Result::err) {
                    self.console
                        .push(simple_console(Severity::Error, "explorer", error));
                }
                update_explorer_selection(ui, self.project_root, renamed.filter(|_| succeeded));
                // Rescan after partial batches too, so completed operations are visible.
                *self.project_file_action = Some(ProjectFileAction::Refresh);
            }
            _ => {}
        }
    }

    fn open_project_script(&mut self, relative_path: &Path) {
        let workspace = self.project_root.join(format!(
            "{}.code-workspace",
            self.document.project().metadata().name
        ));
        let file = self.project_root.join(relative_path);
        let configuration = if discover_code_editor(CodeEditor::VisualStudioCode).is_some() {
            EditorConfiguration::default()
        } else {
            EditorConfiguration {
                preferred: CodeEditor::SystemDefault,
                argument_template: vec!["{file}".into()],
                project_open_behavior: ProjectOpenBehavior::ProjectFolder,
                ..EditorConfiguration::default()
            }
        };
        let result = build_editor_launch(
            &configuration,
            self.project_root,
            Some(&workspace),
            Some(&file),
            Some(1),
            Some(1),
        )
        .map_err(|error| error.to_string())
        .and_then(|launch| {
            open_in_external_editor(&launch)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
        match result {
            Ok(()) => self.console.push(simple_console(
                Severity::Info,
                "programming.editor",
                format!("Opened {} in the project editor", relative_path.display()),
            )),
            Err(error) => {
                self.console
                    .push(simple_console(Severity::Error, "programming.editor", error));
            }
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let playing = self.play.is_some();
                if playing {
                    ui.label("Live runtime values · read only during Play");
                }
                ui.add_enabled_ui(!playing, |ui| self.inspector_contents(ui));
            });
    }

    #[allow(
        clippy::too_many_lines,
        reason = "keep conditional inspector controls and their authoring actions together"
    )]
    fn inspector_contents(&mut self, ui: &mut egui::Ui) {
        self.environment_inspector(ui);
        let Some(entity) = self.document.selected() else {
            ui.vertical_centered(|ui| {
                ui.add_space(48.0);
                ui.label(egui::RichText::new("Nothing selected").heading());
                ui.label(
                    egui::RichText::new(
                        "Choose an entity in the Project panel to edit its properties.",
                    )
                    .color(ui.visuals().weak_text_color()),
                );
            });
            return;
        };
        let Ok(mut snapshot) = self.document.world().snapshot(entity) else {
            ui.label("Selection is no longer available.");
            return;
        };
        if let Some(live) = self.play.as_ref().and_then(|play| play.live_entity())
            && live.entity_id == entity.to_string()
        {
            snapshot.name.clone_from(&live.name);
            snapshot.local_transform.translation = live.translation.into();
            snapshot.local_transform.rotation = glam::Quat::from_array(live.rotation);
            snapshot.local_transform.scale = live.scale.into();
            snapshot.part_attributes = live.part_attributes;
            snapshot.scripts.clone_from(&live.scripts);
        }
        let mut name = snapshot.name.clone().unwrap_or_else(|| "Entity".into());
        if ui.text_edit_singleline(&mut name).changed()
            && !self.document.is_read_only()
            && let Err(error) = self.document.set_name(entity, name)
        {
            self.console.push(simple_console(
                Severity::Error,
                "inspector",
                error.to_string(),
            ));
        }
        ui.label(
            egui::RichText::new(entity.to_string())
                .monospace()
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        let mut transform = snapshot.local_transform;
        let mut translation = transform.translation.to_array();
        let mut rotation = rotation_degrees(transform.rotation);
        let mut scale = transform.scale.to_array();
        ui.add_space(4.0);
        let (changed, rotation_changed, scale_changed) =
            ui_theme::component_card(ui, "Transform", |ui| {
                let changed = transform_vector_row(ui, "Position", &mut translation, 0.1);
                let rotation_changed = transform_vector_row(ui, "Rotation", &mut rotation, 0.5);
                let scale_changed = transform_vector_row(ui, "Scale", &mut scale, 0.05);
                (changed, rotation_changed, scale_changed)
            })
            .inner;
        if changed && !self.document.is_read_only() {
            transform.translation = translation.into();
            if let Err(error) = self.document.set_transform(entity, transform) {
                self.console.push(simple_console(
                    Severity::Error,
                    "inspector",
                    error.to_string(),
                ));
            } else {
                self.request_preview = true;
            }
        }
        if rotation_changed && !self.document.is_read_only() {
            transform.rotation = rotation_from_degrees(rotation);
            if self.document.set_transform(entity, transform).is_ok() {
                self.request_preview = true;
            }
        }
        if scale_changed && !self.document.is_read_only() {
            transform.scale = scale.into();
            if self.document.set_transform(entity, transform).is_ok() {
                self.request_preview = true;
            }
        }
        if let Some(mut camera) = snapshot.camera {
            ui.separator();
            ui_theme::section(ui, "Camera", "Game view during Play");
            ui.label("Looks along local +Z. Highest active priority is used.");
            if ui.button("Align to editor view").clicked()
                && !self.document.is_read_only()
                && self
                    .document
                    .set_transform(entity, camera_placement(*self.camera))
                    .is_ok()
            {
                self.request_preview = true;
            }
            if edit_camera(ui, &mut camera) && !self.document.is_read_only() {
                match self.document.set_camera(entity, camera) {
                    Ok(()) => self.request_preview = true,
                    Err(error) => self.console.push(simple_console(
                        Severity::Error,
                        "inspector",
                        error.to_string(),
                    )),
                }
            }
        }
        if let Some(mut light) = snapshot.light {
            ui.separator();
            ui_theme::section(ui, "Light", "Scene illumination");
            if edit_light(ui, &mut light) && !self.document.is_read_only() {
                match self.document.set_light(entity, light) {
                    Ok(()) => self.request_preview = true,
                    Err(error) => self.console.push(simple_console(
                        Severity::Error,
                        "inspector",
                        error.to_string(),
                    )),
                }
            }
        }
        if snapshot.primitive.is_some() || snapshot.mesh.is_some() {
            let mut attributes = snapshot.part_attributes;
            ui.label("Color");
            let mut color = attributes.color;
            let mut attributes_changed =
                ui.color_edit_button_rgba_unmultiplied(&mut color).changed();
            attributes.color = color;
            attributes_changed |= ui
                .checkbox(&mut attributes.can_touch, "Can touch")
                .on_hover_text("Reserved for touch events. Does not affect solid collisions.")
                .changed();
            attributes_changed |= ui
                .checkbox(&mut attributes.can_collide, "Can collide")
                .on_hover_text("Built-in primitives collide in Play when both objects enable this.")
                .changed();
            attributes_changed |= ui
                .checkbox(&mut attributes.anchored, "Anchored")
                .on_hover_text(
                    "Prevents gravity and collision response from moving this primitive in Play.",
                )
                .changed();
            ui.label(format!(
                "Parent: {}",
                snapshot
                    .parent
                    .map_or_else(|| "Scene root".into(), |id| id.to_string())
            ));
            if attributes_changed
                && !self.document.is_read_only()
                && self
                    .document
                    .set_part_attributes(entity, attributes)
                    .is_ok()
            {
                self.request_preview = true;
            }
        }
        if let Some(mut primitive) = snapshot.primitive {
            ui.separator();
            ui_theme::section(ui, "Geometry", "Parametric primitive");
            if ui_theme::property_grid(ui, |ui| edit_primitive(ui, &mut primitive)).inner
                && !self.document.is_read_only()
            {
                match self.document.set_primitive(entity, primitive) {
                    Ok(()) => self.request_preview = true,
                    Err(error) => self.console.push(simple_console(
                        Severity::Error,
                        "inspector",
                        error.to_string(),
                    )),
                }
            }
            ui.label(
                egui::RichText::new("Changes are validated and can be undone.")
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        }
        self.script_inspector(ui, entity, snapshot.scripts);
    }

    fn environment_inspector(&mut self, ui: &mut egui::Ui) {
        let mut env = self.document.world().environment().clone();
        let before = env.clone();
        ui.collapsing("Scene sky & atmosphere", |ui| {
            ui.add_enabled_ui(!self.document.is_read_only(), |ui| {
                ui.checkbox(&mut env.enabled, "Enable scene environment");
                ui.label("Sky image (2:1 panorama: HDR, PNG, JPEG, TGA)");
                ui.label(if env.sky_image.is_empty() {
                    "Solid sky color"
                } else {
                    &env.sky_image
                });
                ui.horizontal(|ui| {
                    if ui.button("Choose image…").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_directory(self.document.project().root())
                            .add_filter("Sky panorama", &["hdr", "png", "jpg", "jpeg", "tga"])
                            .pick_file()
                    {
                        let root = self.document.project().root().canonicalize();
                        let chosen = path.canonicalize();
                        match root.and_then(|root| chosen.map(|path| (root, path))) {
                            Ok((root, path)) if path.starts_with(&root) => {
                                env.sky_image = path
                                    .strip_prefix(root)
                                    .unwrap()
                                    .to_string_lossy()
                                    .replace('\\', "/");
                            }
                            _ => self.console.push(simple_console(
                                Severity::Error,
                                "scene.environment",
                                "Copy the sky image into your project folder first.",
                            )),
                        }
                    }
                    if ui.button("Clear image").clicked() {
                        env.sky_image.clear();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Sky color");
                    ui.color_edit_button_rgb(&mut env.sky_color);
                });
                ui.add(
                    egui::Slider::new(&mut env.rotation_degrees, -180.0..=180.0)
                        .text("Sky rotation"),
                );
                ui.add(
                    egui::Slider::new(&mut env.exposure, -10.0..=10.0).text("Sky exposure (stops)"),
                );
                ui.horizontal(|ui| {
                    ui.label("Ambient color");
                    ui.color_edit_button_rgb(&mut env.ambient_color);
                });
                ui.add(
                    egui::Slider::new(&mut env.ambient_intensity, 0.0..=5.0)
                        .text("Ambient intensity"),
                );
                ui.horizontal(|ui| {
                    ui.label("Sun color");
                    ui.color_edit_button_rgb(&mut env.sun_color);
                });
                ui.add(egui::Slider::new(&mut env.sun_intensity, 0.0..=10.0).text("Sun intensity"));
                ui.label("Direction towards sun (X, Y, Z)");
                ui.horizontal(|ui| {
                    for v in &mut env.sun_direction {
                        ui.add(egui::DragValue::new(v).speed(0.01).range(-1.0..=1.0));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Haze color");
                    ui.color_edit_button_rgb(&mut env.haze_color);
                });
                ui.add(egui::Slider::new(&mut env.haze_density, 0.0..=0.1).text("Haze density"));
                ui.add(
                    egui::DragValue::new(&mut env.haze_start)
                        .speed(1.0)
                        .range(0.0..=100_000.0)
                        .prefix("Haze starts at "),
                );
                if ui.button("Reset environment").clicked() {
                    env = engine_world::SceneEnvironment::default();
                }
            });
        });
        if before != env {
            match self.document.set_environment(env) {
                Ok(()) => self.request_preview = true,
                Err(error) => self.console.push(simple_console(
                    Severity::Error,
                    "scene.environment",
                    error.to_string(),
                )),
            }
        }
    }

    fn script_inspector(
        &mut self,
        ui: &mut egui::Ui,
        entity: EntityId,
        mut scripts: Vec<ScriptComponent>,
    ) {
        ui.separator();
        ui_theme::section(
            ui,
            "Gameplay scripts",
            format!("{} attached", scripts.len()),
        );
        let available_scripts = self.document.project().root().join("config/scripts.ron");
        let available_scripts = std::fs::read(available_scripts)
            .ok()
            .and_then(|bytes| load_manifest(&bytes).ok())
            .map_or_else(Vec::new, |load| load.manifest.scripts);
        let mut attach = None;
        let mut changed = false;
        ui.menu_button("+ Add Component", |ui| {
            if available_scripts.is_empty() {
                ui.weak("Create a script from the Script menu first");
            }
            for entry in &available_scripts {
                let label = entry.relative_path.display().to_string();
                let already_attached = scripts.iter().any(|script| script.script_id == entry.id);
                if ui
                    .add_enabled(!already_attached, egui::Button::new(label))
                    .clicked()
                {
                    attach = Some(entry.id);
                    ui.close();
                }
            }
        });
        if let Some(script_id) = attach {
            scripts.push(ScriptComponent::new(script_id));
            changed = true;
        }
        for script in &mut scripts {
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut script.enabled, "Enabled").changed();
                ui.label(script_links::script_label(
                    &available_scripts,
                    script.script_id,
                ))
                .on_hover_text(script.script_id.to_string());
            });
            for (name, value) in &mut script.properties {
                ui.horizontal(|ui| {
                    ui.label(name);
                    changed |= edit_engine_value(ui, value);
                });
            }
        }
        if changed
            && !self.document.is_read_only()
            && let Err(error) = self.document.set_scripts(entity, scripts)
        {
            self.console.push(simple_console(
                Severity::Error,
                "inspector.script",
                error.to_string(),
            ));
        }
    }

    fn console(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_sized(
                [300.0, 28.0],
                egui::TextEdit::singleline(&mut self.console.filter.query)
                    .hint_text("Filter messages, subsystem, or process…"),
            );
            egui::ComboBox::from_id_salt("console-severity")
                .selected_text(self.console.filter.minimum_severity.as_str())
                .show_ui(ui, |ui| {
                    for severity in Severity::ALL {
                        ui.selectable_value(
                            &mut self.console.filter.minimum_severity,
                            severity,
                            severity.as_str(),
                        );
                    }
                });
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(self.console.copy_text());
            }
            if ui.button("Clear").clicked() {
                self.console.clear();
            }
        });
        ui.separator();
        let mut open_source = None;
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for entry in self.console.visible() {
                    ui.horizontal_wrapped(|ui| {
                        ui.monospace(format!("{}", entry.timestamp_unix_millis));
                        ui.strong(entry.severity.as_str());
                        ui.monospace(format!("[{}:{}]", entry.process, entry.subsystem));
                        if let Some(language) = &entry.language {
                            ui.monospace(format!("[{language}]"));
                        }
                        ui.label(&entry.message);
                        if entry.duplicate_count > 1 {
                            ui.label(format!("×{}", entry.duplicate_count));
                        }
                        if let Some(source) = &entry.source
                            && ui
                                .link(format!("{}:{}", source.file, source.line))
                                .clicked()
                        {
                            open_source = Some((
                                source.file.clone(),
                                source.line,
                                source.column.unwrap_or(1),
                            ));
                        }
                        for frame in &entry.stack_frames {
                            if ui.link(format!("{}:{}", frame.file, frame.line)).clicked() {
                                open_source = Some((
                                    frame.file.clone(),
                                    frame.line,
                                    frame.column.unwrap_or(1),
                                ));
                            }
                        }
                    });
                }
            });
        if let Some((file, line, column)) = open_source {
            let relative = PathBuf::from(&file);
            let path = if relative.is_absolute() {
                relative
            } else {
                self.project_root.join(relative)
            };
            let config = if discover_code_editor(CodeEditor::VisualStudioCode).is_some() {
                EditorConfiguration::default()
            } else {
                EditorConfiguration {
                    preferred: CodeEditor::SystemDefault,
                    argument_template: vec!["{file}".into()],
                    ..EditorConfiguration::default()
                }
            };
            if let Err(error) = build_editor_launch(
                &config,
                self.project_root,
                None,
                Some(&path),
                Some(line),
                Some(column),
            )
            .and_then(|launch| open_in_external_editor(&launch).map(|_| ()))
            {
                self.console.push(simple_console(
                    Severity::Error,
                    "programming.editor",
                    error.to_string(),
                ));
            }
        }
    }
}

fn transform_vector_row(ui: &mut egui::Ui, label: &str, values: &mut [f32; 3], speed: f64) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized(
            [58.0, ui.spacing().interact_size.y],
            egui::Label::new(egui::RichText::new(label).color(ui.visuals().weak_text_color())),
        );
        let available = ui.available_width();
        let field_width = ((available - ui.spacing().item_spacing.x * 2.0) / 3.0).max(42.0);
        let mut changed = false;
        for (index, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
            changed |= ui
                .add_sized(
                    [field_width, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut values[index])
                        .prefix(format!("{axis} "))
                        .speed(speed),
                )
                .changed();
        }
        changed
    })
    .inner
}

#[derive(Clone, Copy)]
struct GizmoDrag {
    entity: EntityId,
    before: LocalTransform,
    after: LocalTransform,
}

fn rotation_degrees(rotation: Quat) -> [f32; 3] {
    let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
    [x.to_degrees(), y.to_degrees(), z.to_degrees()]
}

fn rotation_from_degrees(rotation: [f32; 3]) -> Quat {
    Quat::from_euler(
        EulerRot::XYZ,
        rotation[0].to_radians(),
        rotation[1].to_radians(),
        rotation[2].to_radians(),
    )
}

#[cfg(test)]
mod gizmo_drag_tests {
    use super::*;

    #[test]
    fn inspector_rotation_degrees_round_trip() {
        let rotation = [25.0, -40.0, 135.0];
        let quaternion = rotation_from_degrees(rotation);
        let round_trip = rotation_degrees(quaternion);

        for (actual, expected) in round_trip.into_iter().zip(rotation) {
            assert!((actual - expected).abs() < 1.0e-4);
        }
    }
}

struct ViewportRenderRequest {
    width: u32,
    height: u32,
    scene: ViewportScene,
}

fn spawn_viewport_renderer(
    project_root: PathBuf,
) -> (
    SyncSender<ViewportRenderRequest>,
    Receiver<Result<Option<RenderedFrame>, String>>,
) {
    let (request_sender, request_receiver) = mpsc::sync_channel::<ViewportRenderRequest>(1);
    let (result_sender, result_receiver) = mpsc::sync_channel(2);
    thread::Builder::new()
        .name("editor-viewport-renderer".to_owned())
        .spawn(move || {
            let mut renderer = match SceneViewportRenderer::new(BackendRequest::Auto) {
                Ok(renderer) => renderer,
                Err(error) => {
                    let _ = result_sender.send(Err(error.to_string()));
                    return;
                }
            };
            renderer.set_environment_root(project_root);
            while let Ok(request) = request_receiver.recv() {
                let result = renderer
                    .render(request.width, request.height, &request.scene)
                    .map_err(|error| error.to_string());
                if result_sender.send(result).is_err() {
                    break;
                }
            }
        })
        .expect("viewport renderer worker");
    (request_sender, result_receiver)
}

fn default_dock(tabs: &[EditorTab]) -> Tree<EditorTab> {
    let mut tiles = egui_tiles::Tiles::default();
    let viewport_tabs: Vec<_> = [
        EditorTab::Viewport3d,
        EditorTab::Viewport2d,
        EditorTab::SceneGraph,
    ]
    .into_iter()
    .filter(|tab| tabs.contains(tab))
    .map(|tab| tiles.insert_pane(tab))
    .collect();
    let viewports = tiles.insert_tab_tile(viewport_tabs);
    let left = tiles.insert_pane(EditorTab::Inspector);
    let console = tiles.insert_pane(EditorTab::Console);
    let center = tiles.insert_vertical_tile(vec![viewports, console]);
    let hierarchy = tiles.insert_pane(EditorTab::Hierarchy);
    let root = tiles.insert_horizontal_tile(vec![left, center, hierarchy]);
    Tree::new("rustic-editor-dock", root, tiles)
}

fn viewport_scene(
    document: &AuthoringDocument,
    camera: EditorCamera,
    extent: [u32; 2],
    render_world_buffer: &mut RenderWorldBuffer,
    model_assets: &BTreeMap<AssetId, ModelArtifact>,
) -> ViewportScene {
    let aspect = extent[0].to_f32().unwrap_or(1.0) / extent[1].max(1).to_f32().unwrap_or(1.0);
    let mut meshes = Vec::new();
    let render_world = render_world_buffer.extract(document.world());
    for extracted in render_world.primitives {
        let entity = extracted.entity;
        let primitive = &extracted.primitive;
        let Ok(generated) = primitive.mesh() else {
            continue;
        };
        let mesh_key = hash_value(&format!("{primitive:?}"));
        let instance_key = hash_value(&entity.to_string());
        let vertices = generated
            .positions
            .iter()
            .enumerate()
            .map(|(index, position)| ViewportVertex {
                position: *position,
                normal: generated
                    .normals
                    .get(index)
                    .copied()
                    .unwrap_or([0.0, 1.0, 0.0]),
            })
            .collect();
        meshes.push(ViewportMesh {
            instance_key,
            mesh_key,
            vertices,
            indices: generated.indices,
            model: extracted.transform,
            color: document
                .world()
                .part_attributes(entity)
                .unwrap_or_default()
                .color,
            selected: document.selected() == Some(entity),
        });
    }
    for extracted in render_world.meshes {
        let Some(model) = model_assets.get(&extracted.mesh) else {
            continue;
        };
        for (mesh_index, imported) in model.meshes.iter().enumerate() {
            let vertices = imported
                .positions
                .iter()
                .enumerate()
                .map(|(index, position)| ViewportVertex {
                    position: *position,
                    normal: imported
                        .normals
                        .get(index)
                        .copied()
                        .unwrap_or([0.0, 1.0, 0.0]),
                })
                .collect();
            let color = model
                .materials
                .get(mesh_index)
                .map_or([0.55, 0.62, 0.72, 1.0], |material| material.base_color);
            meshes.push(ViewportMesh {
                instance_key: hash_value(&(extracted.entity.to_string(), mesh_index)),
                mesh_key: hash_value(&(extracted.mesh, mesh_index)),
                vertices,
                indices: imported.indices.clone(),
                model: extracted.transform,
                color,
                selected: document.selected() == Some(extracted.entity),
            });
        }
    }
    ViewportScene {
        environment: document.world().environment().clone(),
        camera_position: camera.position().to_array(),
        view_projection: camera.view_projection(aspect).to_cols_array(),
        meshes,
        lights: render_world.lights.to_vec(),
        guides: selection_guides(document, aspect),
        grid_vertices: grid_lines(20, 1.0),
        clear_color: [0.045, 0.06, 0.085, 1.0],
    }
}

fn selection_guides(document: &AuthoringDocument, aspect: f32) -> Vec<ViewportGuide> {
    let Some(entity) = document.selected() else {
        return Vec::new();
    };
    let Ok(transform) = document.world().world_transform(entity) else {
        return Vec::new();
    };
    let transform = transform.0;
    if let Ok(Some(camera)) = document.world().camera(entity) {
        return vec![ViewportGuide {
            key: hash_value(&format!("camera-guide-{entity}")),
            vertices: camera_guide_vertices(camera, aspect, transform),
            color: [0.18, 0.82, 1.0, 1.0],
        }];
    }
    if let Ok(Some(light)) = document.world().light(entity) {
        return vec![ViewportGuide {
            key: hash_value(&format!("light-guide-{entity}")),
            vertices: light_guide_vertices(light, transform),
            color: [1.0, 0.78, 0.12, 1.0],
        }];
    }
    Vec::new()
}

fn push_local_line(output: &mut Vec<[f32; 3]>, transform: Mat4, from: Vec3, to: Vec3) {
    output.push(transform.transform_point3(from).to_array());
    output.push(transform.transform_point3(to).to_array());
}

fn push_rectangle(
    output: &mut Vec<[f32; 3]>,
    transform: Mat4,
    half_width: f32,
    half_height: f32,
    z: f32,
) -> [Vec3; 4] {
    let corners = [
        Vec3::new(-half_width, -half_height, z),
        Vec3::new(half_width, -half_height, z),
        Vec3::new(half_width, half_height, z),
        Vec3::new(-half_width, half_height, z),
    ];
    for edge in 0..4 {
        push_local_line(output, transform, corners[edge], corners[(edge + 1) % 4]);
    }
    corners
}

fn camera_guide_vertices(
    camera: engine_world::Camera,
    aspect: f32,
    transform: Mat4,
) -> Vec<[f32; 3]> {
    let mut output = Vec::new();
    let aspect = aspect.max(0.001);
    match camera.projection {
        engine_world::CameraProjection::Perspective {
            vertical_fov_radians,
            near,
            far,
        } => {
            let guide_far = far.min(12.0).max(near);
            let tangent = (vertical_fov_radians * 0.5).tan() / camera.zoom;
            push_rectangle(
                &mut output,
                transform,
                near * tangent * aspect,
                near * tangent,
                near,
            );
            let far_corners = push_rectangle(
                &mut output,
                transform,
                guide_far * tangent * aspect,
                guide_far * tangent,
                guide_far,
            );
            for corner in far_corners {
                push_local_line(&mut output, transform, Vec3::ZERO, corner);
            }
            push_local_line(
                &mut output,
                transform,
                Vec3::new(0.0, 0.0, near),
                Vec3::new(0.0, 0.0, guide_far),
            );
        }
        engine_world::CameraProjection::Orthographic {
            vertical_size,
            near,
            far,
        } => {
            let guide_far = far.min(near + 12.0);
            let half_height = vertical_size * 0.5 / camera.zoom;
            let half_width = half_height * aspect;
            let near_corners =
                push_rectangle(&mut output, transform, half_width, half_height, near);
            let far_corners =
                push_rectangle(&mut output, transform, half_width, half_height, guide_far);
            for index in 0..4 {
                push_local_line(
                    &mut output,
                    transform,
                    near_corners[index],
                    far_corners[index],
                );
            }
            push_local_line(
                &mut output,
                transform,
                Vec3::new(0.0, 0.0, near),
                Vec3::new(0.0, 0.0, guide_far),
            );
        }
    }
    output
}

fn light_guide_vertices(light: engine_world::Light, transform: Mat4) -> Vec<[f32; 3]> {
    let mut output = Vec::new();
    match light.kind {
        engine_world::LightKind::Directional => {
            let end = Vec3::new(0.0, 0.0, 8.0);
            push_local_line(&mut output, transform, Vec3::ZERO, end);
            for offset in [
                Vec3::new(0.45, 0.0, -0.9),
                Vec3::new(-0.45, 0.0, -0.9),
                Vec3::new(0.0, 0.45, -0.9),
                Vec3::new(0.0, -0.45, -0.9),
            ] {
                push_local_line(&mut output, transform, end, end + offset);
            }
        }
        engine_world::LightKind::Spot => {
            let radius = light.range * light.spot_outer_angle_radians.tan();
            let segments = 32u16;
            for index in 0..segments {
                let a = std::f32::consts::TAU * f32::from(index) / f32::from(segments);
                let b = std::f32::consts::TAU * f32::from(index + 1) / f32::from(segments);
                let first = Vec3::new(radius * a.cos(), radius * a.sin(), light.range);
                let second = Vec3::new(radius * b.cos(), radius * b.sin(), light.range);
                push_local_line(&mut output, transform, first, second);
                if index % 8 == 0 {
                    push_local_line(&mut output, transform, Vec3::ZERO, first);
                }
            }
            push_local_line(
                &mut output,
                transform,
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, light.range),
            );
        }
        engine_world::LightKind::Point => {
            let segments = 32u16;
            for axis in 0..3 {
                for index in 0..segments {
                    let a = std::f32::consts::TAU * f32::from(index) / f32::from(segments);
                    let b = std::f32::consts::TAU * f32::from(index + 1) / f32::from(segments);
                    let circle_point = |angle: f32| match axis {
                        0 => Vec3::new(0.0, angle.cos(), angle.sin()) * light.range,
                        1 => Vec3::new(angle.cos(), 0.0, angle.sin()) * light.range,
                        _ => Vec3::new(angle.cos(), angle.sin(), 0.0) * light.range,
                    };
                    push_local_line(&mut output, transform, circle_point(a), circle_point(b));
                }
            }
        }
    }
    output
}

#[cfg(test)]
mod camera_light_guide_tests {
    use super::*;

    #[test]
    fn camera_zoom_narrows_the_perspective_guide() {
        let normal = camera_guide_vertices(engine_world::Camera::default(), 1.0, Mat4::IDENTITY);
        let zoomed = camera_guide_vertices(
            engine_world::Camera {
                zoom: 2.0,
                ..engine_world::Camera::default()
            },
            1.0,
            Mat4::IDENTITY,
        );

        assert_eq!(normal.len(), zoomed.len());
        let normal_width = normal
            .iter()
            .map(|point| point[0].abs())
            .fold(0.0_f32, f32::max);
        let zoomed_width = zoomed
            .iter()
            .map(|point| point[0].abs())
            .fold(0.0_f32, f32::max);
        assert!((zoomed_width * 2.0 - normal_width).abs() < 1.0e-4);
    }

    #[test]
    fn each_light_shape_produces_a_visible_line_guide() {
        for kind in [
            engine_world::LightKind::Directional,
            engine_world::LightKind::Point,
            engine_world::LightKind::Spot,
        ] {
            let vertices = light_guide_vertices(
                engine_world::Light {
                    kind,
                    ..engine_world::Light::default()
                },
                Mat4::IDENTITY,
            );
            assert!(!vertices.is_empty());
            assert_eq!(vertices.len() % 2, 0);
            assert!(vertices.iter().flatten().all(|value| value.is_finite()));
        }
    }
}

fn pick_scene(
    document: &AuthoringDocument,
    model_assets: &BTreeMap<AssetId, ModelArtifact>,
) -> Vec<PickMesh> {
    let mut meshes: Vec<_> = document
        .world()
        .entity_ids()
        .filter_map(|entity| {
            let generated = document
                .world()
                .primitive(entity)
                .ok()
                .flatten()?
                .mesh()
                .ok()?;
            Some(PickMesh {
                entity,
                transform: document.world().world_transform(entity).ok()?.0,
                positions: generated.positions,
                indices: generated.indices,
            })
        })
        .collect();
    for entity in document.world().entity_ids() {
        let Ok(Some(mesh)) = document.world().mesh(entity) else {
            continue;
        };
        let Some(model) = model_assets.get(&mesh.asset) else {
            continue;
        };
        let Ok(transform) = document.world().world_transform(entity) else {
            continue;
        };
        meshes.extend(model.meshes.iter().map(|imported| PickMesh {
            entity,
            transform: transform.0,
            positions: imported.positions.clone(),
            indices: imported.indices.clone(),
        }));
    }
    meshes
}

fn viewport_ground_position(
    camera: EditorCamera,
    rect: egui::Rect,
    pointer: egui::Pos2,
) -> Option<Vec3> {
    if !rect.contains(pointer) {
        return None;
    }
    let pixel = Vec2::new(pointer.x - rect.left(), pointer.y - rect.top());
    let extent = Vec2::new(rect.width(), rect.height());
    ray_ground_intersection(camera.world_ray(pixel, extent)?)
}

fn ray_ground_intersection(ray: WorldRay) -> Option<Vec3> {
    if ray.direction.y.abs() <= 1.0e-5 {
        return None;
    }
    let distance = -ray.origin.y / ray.direction.y;
    (distance >= 0.0)
        .then(|| ray.origin + ray.direction * distance)
        .filter(|position| position.is_finite())
}

fn selected_scene_folder(document: &AuthoringDocument) -> Option<EntityId> {
    let selected = document.selected()?;
    document
        .world()
        .snapshot(selected)
        .ok()
        .filter(|snapshot| snapshot.folder)
        .map(|snapshot| snapshot.id)
}

fn is_model_asset(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "obj" | "gltf" | "glb" | "fbx"
            )
        })
}

fn import_model_asset(root: &Path, relative: &Path) -> Result<(AssetId, ModelArtifact), String> {
    engine_assets::validate_asset_relative_path(relative).map_err(|error| error.to_string())?;
    let source = root.join(relative);
    if !source.is_file() {
        return Err(format!("model asset {} does not exist", relative.display()));
    }
    let contract = ImporterRegistry::contract(relative).map_err(|error| error.to_string())?;
    let meta_path = sidecar_path(&source);
    let meta = if meta_path.is_file() {
        load_meta(&meta_path).map_err(|error| error.to_string())?
    } else {
        let meta = AssetMeta::new(contract.id, contract.version);
        save_meta(&meta_path, &meta).map_err(|error| error.to_string())?;
        meta
    };
    let bytes = std::fs::read(&source)
        .map_err(|error| format!("could not read {}: {error}", source.display()))?;
    let artifact = ImporterRegistry::import(&ImportRequest::new(relative, bytes))
        .map_err(|error| error.to_string())?;
    match artifact {
        DerivedArtifact::Model(model) => Ok((meta.asset_id, model)),
        _ => Err(format!("{} is not a 3D model", relative.display())),
    }
}

fn load_project_model_assets(root: &Path) -> BTreeMap<AssetId, ModelArtifact> {
    fn visit(root: &Path, directory: &Path, output: &mut BTreeMap<AssetId, ModelArtifact>) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if !is_hidden_project_directory(&entry.file_name()) {
                    visit(root, &path, output);
                }
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            if is_model_asset(relative)
                && sidecar_path(&path).is_file()
                && let Ok((asset_id, model)) = import_model_asset(root, relative)
            {
                output.insert(asset_id, model);
            }
        }
    }

    let mut output = BTreeMap::new();
    visit(root, root, &mut output);
    output
}

fn hash_value(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn primitive_palette() -> Vec<(&'static str, Primitive)> {
    vec![
        ("Cube", Primitive::Cube { size: 1.0 }),
        (
            "Rectangular Prism",
            Primitive::RectangularPrism {
                size: [1.0, 1.5, 1.0],
            },
        ),
        ("Plane", Primitive::Plane { size: [2.0, 2.0] }),
        (
            "Sphere",
            Primitive::Sphere {
                radius: 0.5,
                segments: 24,
                rings: 12,
            },
        ),
        (
            "Cylinder",
            Primitive::Cylinder {
                radius: 0.5,
                height: 1.0,
                segments: 24,
            },
        ),
        (
            "Capsule",
            Primitive::Capsule {
                radius: 0.35,
                height: 1.5,
                segments: 24,
                rings: 8,
            },
        ),
        (
            "Cone",
            Primitive::Cone {
                radius: 0.5,
                height: 1.0,
                segments: 24,
            },
        ),
        (
            "Torus",
            Primitive::Torus {
                major_radius: 0.65,
                minor_radius: 0.2,
                segments: 24,
                sides: 12,
            },
        ),
        ("2D Rectangle", Primitive::Rectangle2d { size: [1.0, 1.0] }),
        (
            "2D Circle",
            Primitive::Circle2d {
                radius: 0.5,
                segments: 32,
            },
        ),
        (
            "2D Polygon",
            Primitive::Polygon2d {
                points: vec![[-0.5, -0.5], [0.5, -0.5], [0.0, 0.5]],
            },
        ),
    ]
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive primitive property editor keeps schema coverage visible"
)]
fn edit_primitive(ui: &mut egui::Ui, primitive: &mut Primitive) -> bool {
    let mut changed = false;
    match primitive {
        Primitive::Cube { size } => {
            changed |= ui
                .add(egui::DragValue::new(size).prefix("Size ").speed(0.01))
                .changed();
        }
        Primitive::RectangularPrism { size } => {
            for (label, value) in ["Width", "Height", "Depth"].into_iter().zip(size) {
                changed |= ui
                    .add(
                        egui::DragValue::new(value)
                            .prefix(format!("{label} "))
                            .speed(0.01),
                    )
                    .changed();
            }
        }
        Primitive::Plane { size } | Primitive::Rectangle2d { size } => {
            for (label, value) in ["Width", "Height"].into_iter().zip(size) {
                changed |= ui
                    .add(
                        egui::DragValue::new(value)
                            .prefix(format!("{label} "))
                            .speed(0.01),
                    )
                    .changed();
            }
        }
        Primitive::Sphere {
            radius,
            segments,
            rings,
        } => {
            changed |= ui
                .add(egui::DragValue::new(radius).prefix("Radius ").speed(0.01))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(segments).prefix("Segments "))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(rings).prefix("Rings "))
                .changed();
        }
        Primitive::Cylinder {
            radius,
            height,
            segments,
        }
        | Primitive::Cone {
            radius,
            height,
            segments,
        } => {
            changed |= ui
                .add(egui::DragValue::new(radius).prefix("Radius ").speed(0.01))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(height).prefix("Height ").speed(0.01))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(segments).prefix("Segments "))
                .changed();
        }
        Primitive::Capsule {
            radius,
            height,
            segments,
            rings,
        } => {
            changed |= ui
                .add(egui::DragValue::new(radius).prefix("Radius ").speed(0.01))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(height).prefix("Height ").speed(0.01))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(segments).prefix("Segments "))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(rings).prefix("Rings "))
                .changed();
        }
        Primitive::Torus {
            major_radius,
            minor_radius,
            segments,
            sides,
        } => {
            changed |= ui
                .add(
                    egui::DragValue::new(major_radius)
                        .prefix("Major radius ")
                        .speed(0.01),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(minor_radius)
                        .prefix("Minor radius ")
                        .speed(0.01),
                )
                .changed();
            changed |= ui
                .add(egui::DragValue::new(segments).prefix("Segments "))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(sides).prefix("Sides "))
                .changed();
        }
        Primitive::Circle2d { radius, segments } => {
            changed |= ui
                .add(egui::DragValue::new(radius).prefix("Radius ").speed(0.01))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(segments).prefix("Segments "))
                .changed();
        }
        Primitive::Polygon2d { points } => {
            for (index, point) in points.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("Point {}", index + 1));
                    changed |= ui
                        .add(egui::DragValue::new(&mut point[0]).prefix("X ").speed(0.01))
                        .changed();
                    changed |= ui
                        .add(egui::DragValue::new(&mut point[1]).prefix("Y ").speed(0.01))
                        .changed();
                });
            }
        }
    }
    changed
}

// Register the background before rows so item controls retain pointer priority.
fn explorer_tree_background(ui: &egui::Ui, id: &str) -> egui::Response {
    ui.interact(
        ui.clip_rect(),
        ui.make_persistent_id(id),
        egui::Sense::click(),
    )
}

#[derive(Debug)]
struct ProjectDirectory {
    name: String,
    relative_path: PathBuf,
    directories: Vec<Self>,
    files: Vec<ProjectFile>,
}

#[derive(Clone, Debug)]
struct SceneTreeDrag {
    entity: engine_core::EntityId,
}

#[derive(Debug)]
enum SceneTreeAction {
    Rename(EntityId, String),
    Copy(EntityId),
    Cut(EntityId),
    Paste(Option<EntityId>),
    Delete(EntityId),
    Space(EntityId, GizmoSpace),
}

#[derive(Clone)]
struct SceneClipboard {
    snapshots: Vec<engine_world::EntitySnapshot>,
    cut: bool,
}

#[derive(Default)]
struct SceneTreeResponse {
    clicked: Option<EntityId>,
    reparent: Option<(EntityId, Option<EntityId>)>,
    action: Option<SceneTreeAction>,
}

fn render_scene_hierarchy(
    ui: &mut egui::Ui,
    snapshots: &[engine_world::EntitySnapshot],
    parent: Option<EntityId>,
    selected: Option<EntityId>,
    can_edit: bool,
    space: GizmoSpace,
) -> SceneTreeResponse {
    let mut result = SceneTreeResponse::default();
    for snapshot in snapshots.iter().filter(|item| item.parent == parent) {
        let label = snapshot
            .name
            .clone()
            .unwrap_or_else(|| snapshot.id.to_string());
        let has_children = snapshots
            .iter()
            .any(|item| item.parent == Some(snapshot.id));
        let (drop, dropped) = ui.dnd_drop_zone::<SceneTreeDrag, _>(egui::Frame::NONE, |ui| {
            if snapshot.folder || has_children {
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    ui.make_persistent_id(("scene-entity", snapshot.id)),
                    snapshot.folder,
                )
                .show_header(ui, |ui| {
                    if snapshot.folder {
                        ui.add(Icon::Folder.image(ui.visuals().text_color(), 18.0));
                    }
                    scene_entry_row(
                        ui,
                        snapshot.id,
                        &label,
                        selected,
                        can_edit,
                        space,
                        &mut result,
                    );
                })
                .body(|ui| {
                    let child = render_scene_hierarchy(
                        ui,
                        snapshots,
                        Some(snapshot.id),
                        selected,
                        can_edit,
                        space,
                    );
                    if child.clicked.is_some() {
                        result.clicked = child.clicked;
                    }
                    if child.reparent.is_some() {
                        result.reparent = child.reparent;
                    }
                    if child.action.is_some() {
                        result.action = child.action;
                    }
                });
            } else {
                scene_entry_row(
                    ui,
                    snapshot.id,
                    &label,
                    selected,
                    can_edit,
                    space,
                    &mut result,
                );
            }
        });
        let _ = drop;
        if can_edit
            && let Some(payload) = dropped.as_deref()
            && payload.entity != snapshot.id
        {
            result.reparent = Some((payload.entity, Some(snapshot.id)));
        }
    }
    result
}

fn scene_entry_row(
    ui: &mut egui::Ui,
    entity: EntityId,
    label: &str,
    selected: Option<EntityId>,
    can_edit: bool,
    space: GizmoSpace,
    result: &mut SceneTreeResponse,
) {
    let rename_id = ui.make_persistent_id(("scene-rename", entity));
    let editing = ui.ctx().data_mut(|data| data.get_temp::<String>(rename_id));
    if let Some(mut text) = editing {
        let response = ui.add(egui::TextEdit::singleline(&mut text).id(rename_id));
        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
        if escape || response.lost_focus() || ui.input(|input| input.key_pressed(egui::Key::Enter))
        {
            ui.ctx().data_mut(|data| data.remove::<String>(rename_id));
            if can_edit && !escape && !text.trim().is_empty() && text != label {
                result.action = Some(SceneTreeAction::Rename(entity, text));
            }
        } else {
            ui.ctx().data_mut(|data| data.insert_temp(rename_id, text));
        }
        return;
    }
    let response = ui
        .selectable_label(selected == Some(entity), label)
        .interact(egui::Sense::click_and_drag());
    if can_edit && response.drag_started_by(egui::PointerButton::Primary) {
        response.dnd_set_drag_payload(SceneTreeDrag { entity });
    }
    if response.clicked() {
        result.clicked = Some(entity);
        if can_edit {
            ui.ctx()
                .data_mut(|data| data.insert_temp(rename_id, label.to_owned()));
            ui.memory_mut(|memory| memory.request_focus(rename_id));
        }
    }
    if response.secondary_clicked() {
        result.clicked = Some(entity);
    }
    response.context_menu(|ui| {
        for (label, action) in [
            ("Copy", SceneTreeAction::Copy(entity)),
            ("Cut", SceneTreeAction::Cut(entity)),
            ("Delete", SceneTreeAction::Delete(entity)),
        ] {
            if ui.add_enabled(can_edit, egui::Button::new(label)).clicked() {
                result.action = Some(action);
                ui.close();
            }
        }
        let has_clipboard = ui.ctx().data_mut(|data| {
            data.get_temp::<SceneClipboard>(egui::Id::new("scene-clipboard"))
                .is_some()
        });
        if ui
            .add_enabled(can_edit && has_clipboard, egui::Button::new("Paste"))
            .clicked()
        {
            result.action = Some(SceneTreeAction::Paste(Some(entity)));
            ui.close();
        }
        ui.separator();
        for (label, value) in [
            ("Use World Space", GizmoSpace::World),
            ("Use Local Space", GizmoSpace::Local),
        ] {
            if ui.selectable_label(space == value, label).clicked() {
                result.action = Some(SceneTreeAction::Space(entity, value));
                ui.close();
            }
        }
    });
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ProjectPanelMode {
    #[default]
    Scene,
    Explorer,
}

#[derive(Clone, Debug)]
struct ProjectFile {
    name: String,
    relative_path: PathBuf,
}

#[derive(Debug)]
enum ProjectFileAction {
    ChangeScene(PathBuf),
    RenameScene(PathBuf),
    SetStartScene(PathBuf),
    Refresh,
    Open(PathBuf),
    Attach(PathBuf, ScriptScope),
    Rename(PathBuf, String),
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    Paste(Vec<PathBuf>),
    Delete(Vec<PathBuf>),
    Move {
        sources: Vec<PathBuf>,
        destination: PathBuf,
    },
}

struct SceneRename {
    source: PathBuf,
    name: String,
    error: Option<String>,
    focus: bool,
}

fn scene_picker_entry(
    ui: &mut egui::Ui,
    path: &Path,
    selected: bool,
    is_start: bool,
    can_edit: bool,
) -> Option<ProjectFileAction> {
    let mut action = None;
    let row = ui
        .horizontal(|ui| {
            let response = ui.selectable_label(selected, path.to_string_lossy());
            if is_start {
                ui.weak("Start");
            }
            response
        })
        .inner;
    // Memory-backed context menus close the parent popup. Keep this menu in
    // egui's submenu state so it remains usable inside the scene picker.
    let popup_id = ui.make_persistent_id(("scene-actions", path));
    let secondary_clicked = row.secondary_clicked();
    let open = egui::menu::MenuState::from_ui(ui, |state, _| {
        if secondary_clicked {
            state.open_item = Some(popup_id);
        }
        state.open_item == Some(popup_id)
    });
    let popup = egui::Popup::from_response(&row)
        .id(popup_id)
        .open(open)
        .kind(egui::PopupKind::Menu)
        .align(egui::RectAlign::RIGHT_START)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .show(|ui| {
            if ui
                .add_enabled(can_edit, egui::Button::new("Rename"))
                .clicked()
            {
                action = Some(ProjectFileAction::RenameScene(path.to_path_buf()));
                ui.close();
            }
            if ui
                .add_enabled(can_edit, egui::Button::new("Set as start scene"))
                .clicked()
            {
                action = Some(ProjectFileAction::SetStartScene(path.to_path_buf()));
                ui.close();
            }
        });
    if popup.is_some_and(|popup| popup.response.should_close()) {
        egui::menu::MenuState::from_ui(ui, |state, _| state.open_item = None);
        ui.close();
    }
    if row.clicked() {
        if !selected {
            action = Some(ProjectFileAction::ChangeScene(path.to_path_buf()));
        }
        ui.close();
    }
    action
}

impl ProjectDirectory {
    fn scene_paths(&self, scenes: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<_> = self
            .files
            .iter()
            .filter_map(|file| {
                let path = file.relative_path.strip_prefix(scenes).ok()?;
                let extension = path.extension()?.to_str()?;
                (extension.eq_ignore_ascii_case("scene")
                    || extension.eq_ignore_ascii_case("rscene"))
                .then(|| path.to_path_buf())
            })
            .collect();
        for directory in &self.directories {
            paths.extend(directory.scene_paths(scenes));
        }
        paths
    }

    fn file_count(&self) -> usize {
        self.files.len() + self.directories.iter().map(Self::file_count).sum::<usize>()
    }
}

fn render_project_root(
    ui: &mut egui::Ui,
    directory: &ProjectDirectory,
    filter: &str,
    can_edit: bool,
    has_selected_object: bool,
) -> Option<ProjectFileAction> {
    let mut action = None;
    for child in &directory.directories {
        if project_directory_matches(child, filter)
            && let Some(file_action) =
                render_project_directory(ui, child, filter, true, can_edit, has_selected_object)
        {
            action = Some(file_action);
        }
    }
    for file in &directory.files {
        if project_file_matches(file, filter)
            && let Some(file_action) = render_project_file(ui, file, can_edit, has_selected_object)
        {
            action = Some(file_action);
        }
    }
    action
}

fn render_project_directory(
    ui: &mut egui::Ui,
    directory: &ProjectDirectory,
    filter: &str,
    default_open: bool,
    can_edit: bool,
    has_selected_object: bool,
) -> Option<ProjectFileAction> {
    let folder_text = ui.visuals().text_color();
    let empty_text = ui.visuals().weak_text_color();
    let is_empty = directory.directories.is_empty() && directory.files.is_empty();
    let text_color = if is_empty { empty_text } else { folder_text };
    let mut action = None;
    let id = ui.make_persistent_id(("project-directory", &directory.relative_path));
    let (drop, dropped) = ui.dnd_drop_zone::<ViewportDrop, _>(egui::Frame::NONE, |ui| {
        let header = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            default_open && !is_empty,
        )
        .show_header(ui, |ui| {
            ui.add(
                egui::Image::new(Icon::Folder.source())
                    .fit_to_exact_size(egui::vec2(18.0, 18.0))
                    .tint(text_color),
            );
            action = project_entry_row(
                ui,
                &directory.name,
                &directory.relative_path,
                text_color,
                true,
                can_edit,
                has_selected_object,
            );
            if is_empty {
                ui.label(egui::RichText::new("empty").small().color(empty_text));
            }
        });
        header.body(|ui| {
            for child in &directory.directories {
                if project_directory_matches(child, filter)
                    && let Some(file_action) = render_project_directory(
                        ui,
                        child,
                        filter,
                        !filter.is_empty(),
                        can_edit,
                        has_selected_object,
                    )
                {
                    action = Some(file_action);
                }
            }
            for file in &directory.files {
                if project_file_matches(file, filter)
                    && let Some(file_action) =
                        render_project_file(ui, file, can_edit, has_selected_object)
                {
                    action = Some(file_action);
                }
            }
        })
    });
    let (_, header_response, _) = drop.inner;
    if let Some(payload) = dropped.as_deref() {
        let sources: Vec<_> = payload
            .project_paths()
            .into_iter()
            .filter(|source| source != &directory.relative_path)
            .collect();
        if !sources.is_empty() {
            action = Some(ProjectFileAction::Move {
                sources,
                destination: directory.relative_path.clone(),
            });
        }
    }
    if is_empty {
        header_response
            .response
            .on_hover_text("This folder is empty");
    }
    action
}

fn render_project_file(
    ui: &mut egui::Ui,
    file: &ProjectFile,
    can_edit: bool,
    has_selected_object: bool,
) -> Option<ProjectFileAction> {
    let file_text = ui.visuals().text_color();
    let icon = project_file_icon(&file.relative_path);
    ui.horizontal(|ui| {
        ui.add(
            egui::Image::new(icon)
                .fit_to_exact_size(egui::vec2(18.0, 18.0))
                .tint(file_text),
        );
        project_entry_row(
            ui,
            &file.name,
            &file.relative_path,
            file_text,
            false,
            can_edit,
            has_selected_object,
        )
    })
    .inner
}

#[derive(Clone)]
struct ProjectClipboard {
    sources: Vec<PathBuf>,
    cut: bool,
}

fn project_clipboard_id() -> egui::Id {
    egui::Id::new("project-entry-clipboard")
}

// Only the name owns drag interaction; an expanded folder never owns its children.
fn project_entry_row(
    ui: &mut egui::Ui,
    name: &str,
    path: &Path,
    color: egui::Color32,
    directory: bool,
    can_edit: bool,
    has_selected_object: bool,
) -> Option<ProjectFileAction> {
    let rename_id = ui.make_persistent_id(("project-rename", path));
    let editing = ui.ctx().data_mut(|data| data.get_temp::<String>(rename_id));
    if let Some(mut text) = editing {
        ui.ctx().data_mut(|data| {
            let mut selection = data
                .get_temp::<ExplorerSelection>(explorer_selection_id())
                .unwrap_or_default();
            selection.rendered.push(path.to_path_buf());
            selection.renaming = true;
            data.insert_temp(explorer_selection_id(), selection);
        });
        let response = ui.add(egui::TextEdit::singleline(&mut text).id(rename_id));
        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
        if escape || enter || response.lost_focus() {
            ui.ctx().data_mut(|data| data.remove::<String>(rename_id));
            return (can_edit && !escape && text != name)
                .then(|| ProjectFileAction::Rename(path.to_path_buf(), text));
        }
        ui.ctx().data_mut(|data| data.insert_temp(rename_id, text));
        return None;
    }
    let state_id = explorer_selection_id();
    let mut selection = ui
        .ctx()
        .data_mut(|data| data.get_temp::<ExplorerSelection>(state_id))
        .unwrap_or_default();
    selection.rendered.push(path.to_path_buf());
    let highlighted = selection.highlighted.contains(path);
    let response = ui
        .selectable_label(highlighted, egui::RichText::new(name).color(color))
        .interact(egui::Sense::click_and_drag());
    let (pressed, modifiers) = ui.input(|input| {
        (
            input.pointer.button_pressed(egui::PointerButton::Primary),
            input.modifiers,
        )
    });
    if pressed && response.is_pointer_button_down_on() {
        selection.press(path, modifiers.shift, modifiers.ctrl || modifiers.command);
    }
    if selection.brushing
        && modifiers.shift
        && !modifiers.ctrl
        && !modifiers.command
        && ui.input(|input| input.pointer.primary_down())
        && response.contains_pointer()
    {
        selection.select(path, true, false);
        selection.press = None;
    }
    if can_edit
        && response.drag_started_by(egui::PointerButton::Primary)
        && selection.can_drag(path)
        && !modifiers.shift
        && !modifiers.ctrl
        && !modifiers.command
    {
        let sources = selection_roots(selection.highlighted.iter().cloned());
        response.dnd_set_drag_payload(
            if sources.len() > 1 || sources.first().is_some_and(|source| source != path) {
                ViewportDrop::Entries(sources)
            } else if directory {
                ViewportDrop::Directory(path.to_path_buf())
            } else {
                ViewportDrop::File(path.to_path_buf())
            },
        );
    }
    if response.clicked() && !modifiers.shift && !modifiers.ctrl && !modifiers.command {
        selection.select(path, false, false);
    }
    if response.secondary_clicked() {
        if !selection.highlighted.contains(path) {
            selection.select(path, false, false);
        }
        selection.focused = true;
    }
    if response.double_clicked()
        && can_edit
        && !modifiers.shift
        && !modifiers.ctrl
        && !modifiers.command
    {
        ui.ctx()
            .data_mut(|data| data.insert_temp(rename_id, name.to_owned()));
        ui.memory_mut(|memory| memory.request_focus(rename_id));
        selection.renaming = true;
        ui.ctx().request_repaint();
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(state_id, selection));
    let action = project_entry_menu(&response, path, directory, can_edit, has_selected_object);
    response.on_hover_text(path.display().to_string());
    action
}

fn begin_explorer_selection(ui: &egui::Ui) {
    let outside_press = ui.input(|input| {
        input.pointer.any_pressed()
            && !input
                .pointer
                .latest_pos()
                .is_some_and(|pos| ui.max_rect().contains(pos))
    });
    let released = ui.input(|input| !input.pointer.primary_down());
    ui.ctx().data_mut(|data| {
        let mut selection = data
            .get_temp::<ExplorerSelection>(explorer_selection_id())
            .unwrap_or_default();
        selection.begin_frame();
        if released || outside_press {
            selection.press = None;
            selection.brushing = false;
        }
        if outside_press {
            selection.focused = false;
        }
        data.insert_temp(explorer_selection_id(), selection);
    });
}

fn clear_explorer_selection(ui: &egui::Ui) {
    ui.ctx().data_mut(|data| {
        let mut selection = data
            .get_temp::<ExplorerSelection>(explorer_selection_id())
            .unwrap_or_default();
        selection.highlighted.clear();
        selection.anchor = None;
        selection.focused = true;
        data.insert_temp(explorer_selection_id(), selection);
    });
}

fn update_explorer_selection(ui: &egui::Ui, root: &Path, renamed: Option<(PathBuf, PathBuf)>) {
    ui.ctx().data_mut(|data| {
        let mut selection = data
            .get_temp::<ExplorerSelection>(explorer_selection_id())
            .unwrap_or_default();
        if let Some((source, destination)) = renamed {
            selection.relocate(&source, &destination);
        }
        selection
            .highlighted
            .retain(|path| root.join(path).exists());
        if selection
            .anchor
            .as_ref()
            .is_some_and(|path| !selection.highlighted.contains(path))
        {
            selection.anchor = None;
        }
        selection.press = None;
        data.insert_temp(explorer_selection_id(), selection);
    });
}

fn explorer_selection_id() -> egui::Id {
    egui::Id::new("project-explorer-selection")
}

fn selected_project_entries(context: &egui::Context, fallback: &Path) -> Vec<PathBuf> {
    let selection = context
        .data_mut(|data| data.get_temp::<ExplorerSelection>(explorer_selection_id()))
        .unwrap_or_default();
    if selection.highlighted.contains(fallback) {
        selection_roots(selection.highlighted)
    } else {
        vec![fallback.to_path_buf()]
    }
}

fn explorer_keyboard_action(
    ui: &egui::Ui,
    root: &Path,
    can_edit: bool,
) -> Option<ProjectFileAction> {
    let selection = ui
        .ctx()
        .data_mut(|data| data.get_temp::<ExplorerSelection>(explorer_selection_id()))
        .unwrap_or_default();
    if !can_edit || !selection.focused || selection.renaming || ui.ctx().text_edit_focused() {
        return None;
    }
    let (copy, paste) = ui.input_mut(|input| {
        let copy = input.consume_key(egui::Modifiers::CTRL, egui::Key::C)
            || input.consume_key(egui::Modifiers::COMMAND, egui::Key::C)
            || input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy));
        let paste = input.consume_key(egui::Modifiers::CTRL, egui::Key::V)
            || input.consume_key(egui::Modifiers::COMMAND, egui::Key::V)
            || input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Paste(_)));
        if copy || paste {
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Copy | egui::Event::Paste(_)));
        }
        (copy, paste)
    });
    if copy {
        return Some(ProjectFileAction::Copy(selection_roots(
            selection.highlighted,
        )));
    }
    if paste {
        let mut destinations: Vec<_> = selection
            .highlighted
            .iter()
            .filter(|path| root.join(path).is_dir())
            .cloned()
            .collect();
        if destinations.is_empty() {
            destinations.push(
                selection
                    .anchor
                    .as_deref()
                    .and_then(Path::parent)
                    .unwrap_or(Path::new(""))
                    .to_path_buf(),
            );
        }
        return Some(ProjectFileAction::Paste(destinations));
    }
    None
}

fn paste_project_entries(
    root: &Path,
    clipboard: &mut ProjectClipboard,
    destinations: &[PathBuf],
) -> Vec<Result<(), String>> {
    let destinations: BTreeSet<_> = destinations.iter().collect();
    if clipboard.cut && destinations.len() > 1 {
        return vec![Err("Cut entries can only be pasted into one folder; use Copy to paste into multiple folders".into())];
    }
    let mut results = Vec::new();
    let mut moved = BTreeSet::new();
    for destination in destinations {
        for source in &clipboard.sources {
            let result = if clipboard.cut {
                move_project_entry(root, source, destination)
            } else {
                copy_project_entry(root, source, destination)
            };
            if result.is_ok() && clipboard.cut {
                moved.insert(source.clone());
            }
            results.push(result.map_err(|error| {
                format!("{} → {}: {error}", source.display(), destination.display())
            }));
        }
    }
    clipboard.sources.retain(|source| !moved.contains(source));
    results
}

fn project_entry_menu(
    response: &egui::Response,
    path: &Path,
    directory: bool,
    can_edit: bool,
    has_selected_object: bool,
) -> Option<ProjectFileAction> {
    let mut action = None;
    response.context_menu(|ui| {
        for (label, entry_action) in [
            (
                "Copy",
                ProjectFileAction::Copy(selected_project_entries(&response.ctx, path)),
            ),
            (
                "Cut",
                ProjectFileAction::Cut(selected_project_entries(&response.ctx, path)),
            ),
            (
                "Delete",
                ProjectFileAction::Delete(selected_project_entries(&response.ctx, path)),
            ),
        ] {
            if ui.add_enabled(can_edit, egui::Button::new(label)).clicked() {
                action = Some(entry_action);
                ui.close();
            }
        }
        let has_clipboard = ui.ctx().data_mut(|data| {
            data.get_temp::<ProjectClipboard>(project_clipboard_id())
                .is_some()
        });
        if ui
            .add_enabled(can_edit && has_clipboard, egui::Button::new("Paste"))
            .clicked()
        {
            action = Some(ProjectFileAction::Paste(vec![if directory {
                path.to_path_buf()
            } else {
                path.parent().unwrap_or(Path::new("")).to_path_buf()
            }]));
            ui.close();
        }
        if !directory && ui.button("Open").clicked() {
            action = Some(ProjectFileAction::Open(path.to_path_buf()));
            ui.close();
        }
        if !directory && ScriptLanguage::from_path(path).is_some() {
            ui.separator();
            ui.label(egui::RichText::new("Set As").strong());
            for (label, scope) in [
                ("Global Startup Script", ScriptScope::Global),
                ("Scene Startup Script", ScriptScope::Scene),
                ("Object Component Script", ScriptScope::Component),
            ] {
                if ui
                    .add_enabled(
                        can_edit && (scope != ScriptScope::Component || has_selected_object),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    action = Some(ProjectFileAction::Attach(path.to_path_buf(), scope));
                    ui.close();
                }
            }
            if !has_selected_object {
                ui.weak("Select an object to attach an object component script");
            }
        }
    });
    action
}

const fn script_scope_label(scope: ScriptScope) -> &'static str {
    match scope {
        ScriptScope::Global => "a global startup script",
        ScriptScope::Scene => "a scene startup script",
        ScriptScope::Component => "an object script",
    }
}

fn project_file_matches(file: &ProjectFile, filter: &str) -> bool {
    filter.is_empty()
        || file.name.to_ascii_lowercase().contains(filter)
        || file
            .relative_path
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains(filter)
}

fn project_directory_matches(directory: &ProjectDirectory, filter: &str) -> bool {
    filter.is_empty()
        || directory.name.to_ascii_lowercase().contains(filter)
        || directory
            .files
            .iter()
            .any(|file| project_file_matches(file, filter))
        || directory
            .directories
            .iter()
            .any(|child| project_directory_matches(child, filter))
}

fn move_project_entry(root: &Path, source: &Path, destination: &Path) -> Result<(), String> {
    let source = checked_project_path(root, source, false)?;
    let directory = checked_project_path(root, destination, true)?;
    if !directory.is_dir() {
        return Err("Move destination must be a folder".into());
    }
    if source.is_dir() && directory.starts_with(&source) {
        return Err("A folder cannot be moved inside itself".into());
    }
    let target = directory.join(source.file_name().ok_or("Entry has no name")?);
    if target == source {
        return Ok(());
    }
    relocate_project_entry_with_scripts(root, &source, &target)
}

// Resolve existing entries and reject symlinks, including symlinked ancestors.
fn checked_project_path(root: &Path, relative: &Path, allow_root: bool) -> Result<PathBuf, String> {
    if (!allow_root && relative.as_os_str().is_empty())
        || relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("The project path is not safe to edit".into());
    }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let mut path = root.clone();
    for part in relative.components() {
        path.push(part);
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("Explorer operations do not follow symbolic links".into());
        }
    }
    let resolved = path.canonicalize().map_err(|error| error.to_string())?;
    if !resolved.starts_with(&root) {
        return Err("The project path leaves the project".into());
    }
    Ok(resolved)
}

fn rename_project_entry(root: &Path, source: &Path, name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name != name.trim()
        || name.contains(['/', '\\', ':'])
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.contains(['<', '>', '"', '|', '?', '*'])
        || name.chars().any(char::is_control)
    {
        return Err("Enter a single valid file or folder name".into());
    }
    let source_path = checked_project_path(root, source, false)?;
    let destination = source_path.with_file_name(name);
    if source_path == destination {
        return Ok(());
    }
    relocate_project_entry_with_scripts(root, &source_path, &destination)
}

// Script attachments store IDs; only their registry paths change on relocation.
// Prepare the registry before moving anything and restore files if persistence fails.
fn relocate_project_entry_with_scripts(
    root: &Path,
    source: &Path,
    destination: &Path,
) -> Result<(), String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let manifest_path = root.join("config/scripts.ron");
    if !manifest_path.exists() {
        return relocate_project_entry(source, destination);
    }
    let bytes = std::fs::read(&manifest_path).map_err(|error| error.to_string())?;
    let mut manifest = load_manifest(&bytes)
        .map_err(|error| error.to_string())?
        .manifest;
    let source_relative = source
        .strip_prefix(&root)
        .map_err(|error| error.to_string())?;
    let destination_relative = destination
        .strip_prefix(&root)
        .map_err(|error| error.to_string())?;
    let mut changed = false;
    for entry in &mut manifest.scripts {
        if let Ok(suffix) = entry.relative_path.strip_prefix(source_relative) {
            let path = if suffix.as_os_str().is_empty() {
                destination_relative.to_path_buf()
            } else {
                destination_relative.join(suffix)
            };
            let language = ScriptLanguage::from_path(&path)
                .ok_or("Keep a supported script extension when renaming an attached script")?;
            if !language.accepts_project_path(&path) {
                return Err("Web UI scripts must stay inside the ui folder".into());
            }
            entry.relative_path = path;
            entry.language = language;
            changed = true;
        }
    }
    if changed {
        let mut paths = BTreeSet::new();
        if manifest
            .scripts
            .iter()
            .any(|entry| !paths.insert(&entry.relative_path))
        {
            return Err("A script is already registered at the destination".into());
        }
        // Do not relocate the registry itself along with the source folder.
        if manifest_path.starts_with(source) {
            return Err("Cannot move a folder containing the script registry".into());
        }
    }
    relocate_project_entry(source, destination)?;
    if changed && let Err(error) = save_manifest_atomic(&manifest_path, &manifest) {
        return match relocate_project_entry(destination, source) {
            Ok(()) => Err(format!(
                "Could not update script links; move was undone: {error}"
            )),
            Err(rollback) => Err(format!(
                "Could not update script links: {error}; could not undo move: {rollback}"
            )),
        };
    }
    Ok(())
}

fn project_path_occupied(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn relocate_project_entry(source: &Path, destination: &Path) -> Result<(), String> {
    if project_path_occupied(destination)? {
        return Err("An entry with that name already exists".into());
    }
    let metadata = source.is_file().then(|| sidecar_path(source));
    if metadata.is_some() && project_path_occupied(&sidecar_path(destination))? {
        return Err("Asset metadata already exists at the destination".into());
    }
    std::fs::rename(source, destination).map_err(|error| error.to_string())?;
    if let Some(metadata) = metadata
        && metadata.exists()
        && let Err(error) = std::fs::rename(&metadata, sidecar_path(destination))
    {
        let _ = std::fs::rename(destination, source);
        return Err(format!("Could not move asset metadata: {error}"));
    }
    Ok(())
}

fn delete_document_project_entry(
    document: &mut AuthoringDocument,
    source: &Path,
) -> Result<(), String> {
    if document.is_read_only() {
        return Err("Cannot delete scripts in a read-only project".into());
    }
    let deleted = delete_project_entry(document.project().root(), source)?;
    document
        .remove_script_links(&deleted)
        .map_err(|error| error.to_string())
}

fn delete_project_entry(root: &Path, source: &Path) -> Result<BTreeSet<ScriptId>, String> {
    let source_path = checked_project_path(root, source, false)?;
    let links = script_links::ScriptLinkDeletion::prepare(root, source)?;
    // Keep deleted entries recoverable without showing them in the project tree.
    if root.join(".rustic").exists() {
        checked_project_path(root, Path::new(".rustic"), false)?;
    }
    if root.join(".rustic/trash").exists() {
        checked_project_path(root, Path::new(".rustic/trash"), false)?;
    }
    let trash = root.join(".rustic").join("trash");
    std::fs::create_dir_all(&trash).map_err(|error| error.to_string())?;
    let trash_relative = Path::new(".rustic/trash");
    let trash = checked_project_path(root, trash_relative, false)?;
    if trash.starts_with(&source_path) {
        return Err("Cannot delete the project trash directory".into());
    }
    let entry = trash.join(AssetId::new().to_string());
    std::fs::create_dir(&entry).map_err(|error| error.to_string())?;
    let destination = entry.join(source_path.file_name().ok_or("Entry has no name")?);
    relocate_project_entry(&source_path, &destination)?;
    if let Err(error) = links.save() {
        return match relocate_project_entry(&destination, &source_path) {
            Ok(()) => Err(format!(
                "Could not remove script links; deletion was undone: {error}"
            )),
            Err(rollback) => Err(format!(
                "Could not remove script links: {error}; could not undo deletion: {rollback}"
            )),
        };
    }
    Ok(links.deleted_ids)
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(source).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("Cannot copy symbolic links".into());
    }
    if metadata.is_dir() {
        std::fs::create_dir(target).map_err(|error| error.to_string())?;
        for entry in std::fs::read_dir(source).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            // Copies acquire new asset identities on import/registration.
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "rmeta")
            {
                continue;
            }
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        std::fs::copy(source, target).map_err(|error| error.to_string())?;
    } else {
        return Err("Only regular files and folders can be copied".into());
    }
    Ok(())
}
fn copy_project_entry(root: &Path, source: &Path, destination: &Path) -> Result<(), String> {
    let source = checked_project_path(root, source, false)?;
    let directory = checked_project_path(root, destination, true)?;
    if !directory.is_dir() {
        return Err("Paste destination must be a folder".into());
    }
    if source.is_dir() && directory.starts_with(&source) {
        return Err("A folder cannot be copied inside itself".into());
    }
    let name = source.file_name().ok_or("Entry has no name")?;
    let mut target = directory.join(name);
    let mut index = 1;
    while project_path_occupied(&target)?
        || (source.is_file() && project_path_occupied(&sidecar_path(&target))?)
    {
        let stem = source.file_stem().unwrap_or(name).to_string_lossy();
        let extension = if source.is_file() {
            source
                .extension()
                .map(|value| format!(".{}", value.to_string_lossy()))
                .unwrap_or_default()
        } else {
            String::new()
        };
        target = directory.join(format!("{stem} copy {index}{extension}"));
        index += 1;
    }
    let result = copy_tree(&source, &target);
    if result.is_err() {
        if target.is_dir() {
            let _ = std::fs::remove_dir_all(&target);
        } else {
            let _ = std::fs::remove_file(&target);
        }
    }
    result
}

fn project_file_icon(path: &Path) -> egui::ImageSource<'static> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("c") => egui::include_image!("../../../assets/icons/material-design/language-c.svg"),
        Some("cc" | "cpp" | "cxx") => {
            egui::include_image!("../../../assets/icons/material-design/language-cpp.svg")
        }
        Some("cs") => {
            egui::include_image!("../../../assets/icons/material-design/language-csharp.svg")
        }
        Some("py") => {
            egui::include_image!("../../../assets/icons/material-design/language-python.svg")
        }
        Some("js" | "mjs") => {
            egui::include_image!("../../../assets/icons/material-design/language-javascript.svg")
        }
        Some("lua" | "luau") => {
            egui::include_image!("../../../assets/icons/material-design/language-lua.svg")
        }
        Some("java") => {
            egui::include_image!("../../../assets/icons/material-design/language-java.svg")
        }
        Some("php") => {
            egui::include_image!("../../../assets/icons/material-design/language-php.svg")
        }
        Some("html") => {
            egui::include_image!("../../../assets/icons/material-design/language-html5.svg")
        }
        Some("css") => {
            egui::include_image!("../../../assets/icons/material-design/language-css3.svg")
        }
        Some("rscene") => Icon::Map.source(),
        Some("png" | "jpg" | "jpeg" | "tga" | "hdr") => Icon::Image.source(),
        Some("glb" | "gltf" | "obj") => Icon::Box.source(),
        Some("wav" | "ogg" | "oga" | "opus" | "mp3" | "aac" | "m4a" | "flac") => {
            Icon::FileMusic.source()
        }
        Some("ron" | "toml" | "json") => Icon::FileCog.source(),
        _ => Icon::FileText.source(),
    }
}

fn spawn_project_scan(root: PathBuf) -> Receiver<ProjectDirectory> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("editor-project-scan".to_owned())
        .spawn(move || {
            let tree = scan_project_directory(&root, &root);
            let _ = sender.send(tree);
        })
        .expect("project scan thread");
    receiver
}

fn scan_project_directory(root: &Path, directory: &Path) -> ProjectDirectory {
    let relative_path = directory
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let name = if relative_path.as_os_str().is_empty() {
        root.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Game Project")
            .to_owned()
    } else {
        directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Folder")
            .to_owned()
    };
    let mut node = ProjectDirectory {
        name,
        relative_path,
        directories: Vec::new(),
        files: Vec::new(),
    };
    if let Ok(entries) = std::fs::read_dir(directory) {
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                if !is_hidden_project_directory(&entry.file_name()) {
                    node.directories.push(scan_project_directory(root, &path));
                }
            } else if file_type.is_file()
                && path.extension().and_then(|value| value.to_str()) != Some("rmeta")
                && let Ok(relative_path) = path.strip_prefix(root)
            {
                node.files.push(ProjectFile {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    relative_path: relative_path.to_path_buf(),
                });
            }
        }
    }
    node.directories
        .sort_by_key(|directory| directory.name.to_ascii_lowercase());
    node.files
        .sort_by_key(|file| file.name.to_ascii_lowercase());
    node
}

fn is_hidden_project_directory(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_string_lossy().to_ascii_lowercase().as_str(),
        ".git" | ".tools" | ".rustic" | "target" | "cache" | "temp" | "builds" | "logs"
    )
}

fn simple_console(severity: Severity, subsystem: &str, message: impl Into<String>) -> ConsoleEntry {
    ConsoleEntry {
        timestamp_unix_millis: unix_millis(),
        severity,
        subsystem: subsystem.to_owned(),
        language: None::<Language>,
        process: "editor".to_owned(),
        process_id: std::process::id(),
        message: message.into(),
        source: None,
        stack_frames: Vec::new(),
        duplicate_count: 1,
    }
}

fn attach_script_to_entity(
    document: &mut AuthoringDocument,
    entity: EntityId,
    relative: &Path,
) -> Result<(), String> {
    let manifest_path = document.project().root().join("config/scripts.ron");
    let bytes = std::fs::read(&manifest_path).map_err(|error| {
        format!(
            "could not read script asset registry {}: {error}",
            manifest_path.display()
        )
    })?;
    let manifest = load_manifest(&bytes)
        .map_err(|error| error.to_string())?
        .manifest;
    let entry = manifest
        .scripts
        .iter()
        .find(|entry| entry.relative_path == relative)
        .ok_or_else(|| format!("{} is not a registered script asset", relative.display()))?;
    let mut scripts = document
        .world()
        .snapshot(entity)
        .map_err(|error| error.to_string())?
        .scripts;
    if scripts.iter().any(|script| script.script_id == entry.id) {
        return Err(format!("{} is already attached", relative.display()));
    }
    scripts.push(ScriptComponent::new(entry.id));
    document
        .set_scripts(entity, scripts)
        .map_err(|error| error.to_string())
}

fn edit_engine_value(ui: &mut egui::Ui, value: &mut EngineValue) -> bool {
    match value {
        EngineValue::Boolean(value) => ui.checkbox(value, "").changed(),
        EngineValue::Integer(value) => ui.add(egui::DragValue::new(value)).changed(),
        EngineValue::Number(value) => ui.add(egui::DragValue::new(value)).changed(),
        EngineValue::String(value) => ui.text_edit_singleline(value).changed(),
        EngineValue::Vec2(value) => {
            ui.add(egui::DragValue::new(&mut value[0])).changed()
                | ui.add(egui::DragValue::new(&mut value[1])).changed()
        }
        EngineValue::Vec3(value) => {
            ui.add(egui::DragValue::new(&mut value[0])).changed()
                | ui.add(egui::DragValue::new(&mut value[1])).changed()
                | ui.add(egui::DragValue::new(&mut value[2])).changed()
        }
        EngineValue::Entity(value) => {
            ui.monospace(value.map_or_else(|| "None".to_owned(), |id| id.to_string()));
            false
        }
    }
}

fn gameplay_script_template(language: ScriptLanguage) -> (&'static str, &'static [u8]) {
    match language {
        ScriptLanguage::Lua54 => (
            "lua",
            b"-- Edit in your configured external editor.\nreturn {\n  Start = function() rustic.log(\"info\", \"Behavior started\") end,\n  FixedUpdate = function(dt) end,\n  Update = function(dt) end,\n  OnDestroy = function() rustic.log(\"info\", \"Behavior destroyed\") end,\n}\n",
        ),
        ScriptLanguage::JavaScript => (
            "js",
            b"// Edit in your configured external editor.\nglobalThis.behavior = {\n  Start() { rustic.log(\"info\", \"Behavior started\"); },\n  FixedUpdate(dt) {},\n  Update(dt) {},\n  OnDestroy() { rustic.log(\"info\", \"Behavior destroyed\"); },\n};\n",
        ),
        ScriptLanguage::Python => ("py", include_bytes!("../../../crates/engine-scripting/src/sdk/starter.py")),
        ScriptLanguage::CSharp => ("cs", include_bytes!("../../../crates/engine-scripting/src/sdk/starter.cs")),
        ScriptLanguage::C => ("c", include_bytes!("../../../crates/engine-scripting/src/sdk/starter.c")),
        ScriptLanguage::Cpp => (
            "cpp",
            br#"#include "rustic.hpp"

void on_start() {
    rustic.log("info", "Behavior started");
}

void fixed_update(double dt) {
    const auto movement = rustic.key("KeyW");
    if (movement.held) {
        const auto position = rustic.get_translation();
        rustic.set_translation(position.x, position.y, position.z + dt);
    }

    // The same engine API shape used by Lua and JavaScript:
    // auto table = Game.scene.Find("Room.Table");
    // rustic.EditAttribute("Position", RusticValue::Array{1.0, 2.0, 3.0});
    // instance.clone("assets/models/chair.obj");
}

int main() {
    return rustic_run(RusticBehavior{
        .on_start = on_start,
        .fixed_update = fixed_update,
    });
}
"#,
        ),
        ScriptLanguage::Java => ("java", include_bytes!("../../../crates/engine-scripting/src/sdk/starter.java")),
        ScriptLanguage::Php => ("php", include_bytes!("../../../crates/engine-scripting/src/sdk/starter.php")),
        ScriptLanguage::Web => (
            "html",
            br#"<!doctype html>
<html>
<head><style>body { margin: 0; }</style></head>
<body>
<script>
globalThis.behavior = {
  on_start() { rustic.log("info", "Web behavior started"); },
  update(dt) {},
};
</script>
</body>
</html>
"#,
        ),
        ScriptLanguage::Luau => ("luau", include_bytes!("../../../crates/engine-scripting/src/sdk/starter.luau")),
    }
}

fn optional_toolchain_languages() -> [ScriptLanguage; 6] {
    [
        ScriptLanguage::Python,
        ScriptLanguage::CSharp,
        ScriptLanguage::C,
        ScriptLanguage::Cpp,
        ScriptLanguage::Java,
        ScriptLanguage::Php,
    ]
}

fn missing_project_dependencies(project_root: &Path) -> Vec<ScriptLanguage> {
    let path = project_root.join("config/scripts.ron");
    let used_languages = std::fs::read(path)
        .ok()
        .and_then(|bytes| load_manifest(&bytes).ok())
        .map(|load| {
            load.manifest
                .scripts
                .into_iter()
                .map(|entry| entry.language)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut languages = used_languages
        .into_iter()
        .filter(|language| optional_toolchain_languages().contains(language))
        .filter(|language| !probe_language_toolchain(*language).available)
        .collect::<Vec<_>>();
    languages.sort_by_key(|language| language.display_name());
    languages.dedup();
    languages
}

fn launch_toolchain_manager(languages: &[ScriptLanguage]) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let script = executable
        .ancestors()
        .map(|directory| directory.join("tools/manage-language-toolchains.ps1"))
        .find(|path| path.is_file())
        .ok_or_else(|| "Language Toolchain Manager was not found; rerun Rustic setup".to_owned())?;
    let mut selected = languages
        .iter()
        .filter_map(|language| match language {
            ScriptLanguage::Python => Some("Python"),
            ScriptLanguage::CSharp => Some("CSharp"),
            ScriptLanguage::C | ScriptLanguage::Cpp => Some("CCpp"),
            ScriptLanguage::Java => Some("Java"),
            ScriptLanguage::Php => Some("Php"),
            _ => None,
        })
        .collect::<Vec<_>>();
    selected.sort_unstable();
    selected.dedup();
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script);
    if !selected.is_empty() {
        command.arg("-Languages").arg(selected.join(","));
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not open the dependency installer: {error}"))
}

fn unix_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}

fn project_argument(arguments: &[std::ffi::OsString]) -> Option<PathBuf> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == "--project")
        .map(|pair| PathBuf::from(&pair[1]))
}

fn camera_placement(camera: EditorCamera) -> LocalTransform {
    LocalTransform {
        translation: camera.position(),
        rotation: Quat::from_mat4(&camera.view_matrix().inverse())
            * Quat::from_rotation_y(std::f32::consts::PI),
        ..LocalTransform::IDENTITY
    }
}

fn edit_camera(ui: &mut egui::Ui, camera: &mut engine_world::Camera) -> bool {
    let before = *camera;
    ui.checkbox(&mut camera.active, "Active");
    ui.add(egui::DragValue::new(&mut camera.order).prefix("Priority "));
    ui.add(
        egui::DragValue::new(&mut camera.zoom)
            .range(0.01..=100.0)
            .speed(0.05)
            .prefix("Zoom "),
    )
    .on_hover_text("Optical magnification: values above 1 zoom in; values below 1 zoom out");
    let mut ortho = matches!(
        camera.projection,
        engine_world::CameraProjection::Orthographic { .. }
    );
    if ui.checkbox(&mut ortho, "Orthographic").changed() {
        camera.projection = if ortho {
            engine_world::CameraProjection::Orthographic {
                vertical_size: 10.0,
                near: 0.1,
                far: 1000.0,
            }
        } else {
            engine_world::CameraProjection::default()
        };
    }
    let (near, far) = match &mut camera.projection {
        engine_world::CameraProjection::Perspective {
            vertical_fov_radians,
            near,
            far,
        } => {
            let mut degrees = vertical_fov_radians.to_degrees();
            if ui
                .add(egui::Slider::new(&mut degrees, 1.0..=179.0).text("Field of view (degrees)"))
                .changed()
            {
                *vertical_fov_radians = degrees.to_radians();
            }
            (near, far)
        }
        engine_world::CameraProjection::Orthographic {
            vertical_size,
            near,
            far,
        } => {
            ui.add(
                egui::DragValue::new(vertical_size)
                    .range(0.01..=100_000.0)
                    .speed(0.1)
                    .prefix("Vertical size "),
            );
            (near, far)
        }
    };
    ui.add(
        egui::DragValue::new(near)
            .range(0.001..=(*far - 0.001).max(0.001))
            .speed(0.01)
            .prefix("Near clip "),
    );
    ui.add(
        egui::DragValue::new(far)
            .range((*near + 0.001)..=1_000_000.0)
            .speed(1.0)
            .prefix("Far clip "),
    );
    before != *camera
}

fn edit_light(ui: &mut egui::Ui, light: &mut engine_world::Light) -> bool {
    let before = *light;
    egui::ComboBox::from_label("Type")
        .selected_text(format!("{:?}", light.kind))
        .show_ui(ui, |ui| {
            for kind in [
                engine_world::LightKind::Directional,
                engine_world::LightKind::Point,
                engine_world::LightKind::Spot,
            ] {
                ui.selectable_value(&mut light.kind, kind, format!("{kind:?}"));
            }
        });
    let mut color = light.color.to_array();
    ui.horizontal(|ui| {
        ui.label("Light color");
        ui.color_edit_button_rgb(&mut color);
    });
    light.color = color.into();
    ui.add(
        egui::DragValue::new(&mut light.intensity)
            .range(0.0..=10000.0)
            .speed(0.05)
            .prefix("Intensity "),
    );
    if light.kind != engine_world::LightKind::Directional {
        ui.add(
            egui::DragValue::new(&mut light.range)
                .range(0.01..=100_000.0)
                .speed(0.1)
                .prefix("Range "),
        );
    }
    if light.kind == engine_world::LightKind::Spot {
        let mut degrees = light.spot_outer_angle_radians.to_degrees();
        if ui
            .add(egui::Slider::new(&mut degrees, 1.0..=89.0).text("Cone half-angle (degrees)"))
            .changed()
        {
            light.spot_outer_angle_radians = degrees.to_radians();
        }
    }
    ui.label("Directional and spot lights shine along local +Z.");
    ui.label("Intensity 0 turns this light off. Up to 32 lights per scene.");
    ui.label("Shadow casting is not supported yet.");
    before != *light
}

#[cfg(test)]
mod explorer_tests {
    use super::*;

    fn scene_context_frame(
        context: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
        can_edit: bool,
    ) -> (egui::FullOutput, Option<ProjectFileAction>) {
        let mut action = None;
        let mut output = context.run_ui(
            egui::RawInput {
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                ui.menu_button("Settings", |ui| {
                    ui.menu_button("Change scene", |ui| {
                        action = scene_picker_entry(
                            ui,
                            Path::new("night.scene"),
                            false,
                            false,
                            can_edit,
                        );
                    });
                });
            },
        );
        output.textures_delta.clear();
        (output, action)
    }

    fn scene_context_click(
        context: &egui::Context,
        time: f64,
        position: egui::Pos2,
        button: egui::PointerButton,
        can_edit: bool,
    ) -> (egui::FullOutput, Option<ProjectFileAction>) {
        scene_context_frame(
            context,
            time,
            vec![
                egui::Event::PointerMoved(position),
                pointer(position, button, true),
            ],
            can_edit,
        );
        scene_context_frame(
            context,
            time + 0.01,
            vec![pointer(position, button, false)],
            can_edit,
        )
    }

    #[test]
    fn right_click_scene_offers_rename_and_start_without_loading_the_scene() {
        for (choice, can_edit) in [
            ("Rename", true),
            ("Set as start scene", true),
            ("Rename", false),
            ("Set as start scene", false),
        ] {
            let context = egui::Context::default();
            let (output, _) = scene_context_frame(&context, 0.0, vec![], can_edit);
            let settings = text_position(&output, "Settings");
            scene_context_click(
                &context,
                0.1,
                settings,
                egui::PointerButton::Primary,
                can_edit,
            );
            let (output, _) = scene_context_frame(&context, 0.2, vec![], can_edit);
            let change = text_position(&output, "Change scene");
            scene_context_click(
                &context,
                0.3,
                change,
                egui::PointerButton::Primary,
                can_edit,
            );
            let (output, _) = scene_context_frame(&context, 0.4, vec![], can_edit);
            let night = text_position(&output, "night.scene");
            let (_, action) = scene_context_click(
                &context,
                0.5,
                night,
                egui::PointerButton::Secondary,
                can_edit,
            );
            assert!(action.is_none(), "right click must not switch scenes");
            let (output, _) = scene_context_frame(&context, 0.6, vec![], can_edit);
            text_position(&output, "Rename");
            text_position(&output, "Set as start scene");
            let position = text_position(&output, choice);
            let (_, action) = scene_context_click(
                &context,
                0.7,
                position,
                egui::PointerButton::Primary,
                can_edit,
            );
            if !can_edit {
                assert!(action.is_none());
            } else if choice == "Rename" {
                assert!(
                    matches!(action, Some(ProjectFileAction::RenameScene(path)) if path == Path::new("night.scene"))
                );
            } else {
                assert!(
                    matches!(action, Some(ProjectFileAction::SetStartScene(path)) if path == Path::new("night.scene"))
                );
            }
        }
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the scene picker regression exercises one complete UI interaction"
    )]
    fn project_settings_gear_opens_scene_picker_and_requests_selected_scene() {
        let temporary = TestProject::new();
        let project = Project::create(
            temporary.0.join("game"),
            "Game",
            engine_project::ProjectTemplate::Blank,
        )
        .unwrap();
        let mut document = AuthoringDocument::open(project, false).unwrap();
        document.save().unwrap();
        let other = document.scene_path().with_file_name("night.scene");
        engine_world::save_scene_atomic(&other, &engine_world::SceneDocument::new("Night"))
            .unwrap();
        let root = document.project().root().to_path_buf();
        let tree = scan_project_directory(&root, &root);
        let context = egui::Context::default();
        ui_theme::apply(&context);
        let mut action = None;
        {
            let mut viewer = EditorViewer {
                document: &mut document,
                project_root: &root,
                console: &mut RuntimeConsole::default(),
                project_tree: Some(&tree),
                project_panel_mode: &mut ProjectPanelMode::Scene,
                project_filter: &mut String::new(),
                preview: None,
                preview_status: "",
                request_preview: false,
                playing: false,
                can_switch_scene: true,
                play: None,
                play_keys: BTreeSet::new(),
                camera: &mut EditorCamera::default(),
                viewport_extent: &mut [640, 480],
                gizmo: &mut GizmoSettings::default(),
                gizmo_drag: &mut None,
                gizmo_widget: &mut transform_widget::TransformWidget::default(),
                scene_graph: &mut scene_graph::SceneGraph::default(),
                model_assets: &mut BTreeMap::new(),
                project_file_action: &mut action,
            };
            let mut frame = |time, events| {
                let mut output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(426.0, 586.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ui| viewer.project_explorer(ui),
                );
                output.textures_delta.clear();
                output
            };
            let gear = text_position(&frame(0.0, vec![]), "⚙");
            assert!(
                gear.x > 350.0 && gear.y < 40.0,
                "gear must sit at the top right: {gear:?}"
            );
            frame(
                0.1,
                vec![
                    egui::Event::PointerMoved(gear),
                    pointer(gear, egui::PointerButton::Primary, true),
                ],
            );
            frame(
                0.2,
                vec![pointer(gear, egui::PointerButton::Primary, false)],
            );
            let change = text_position(&frame(0.3, vec![]), "Change scene");
            frame(
                0.4,
                vec![
                    egui::Event::PointerMoved(change),
                    pointer(change, egui::PointerButton::Primary, true),
                ],
            );
            frame(
                0.5,
                vec![pointer(change, egui::PointerButton::Primary, false)],
            );
            let night = text_position(&frame(0.6, vec![]), "night.scene");
            frame(
                0.7,
                vec![
                    egui::Event::PointerMoved(night),
                    pointer(night, egui::PointerButton::Primary, true),
                ],
            );
            frame(
                0.8,
                vec![pointer(night, egui::PointerButton::Primary, false)],
            );
        }
        assert!(
            matches!(action, Some(ProjectFileAction::ChangeScene(path)) if path == Path::new("night.scene"))
        );
    }

    #[test]
    fn scene_picker_lists_nested_and_legacy_scenes_in_the_configured_folder() {
        let project = TestProject::new();
        for path in [
            "levels/main.scene",
            "levels/area/night.scene",
            "levels/old.rscene",
            "levels/upper.SCENE",
            "levels/readme.txt",
            "assets/unrelated.scene",
            "scene/unused.scene",
        ] {
            let path = project.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        let tree = scan_project_directory(&project.0, &project.0);
        let mut paths = tree.scene_paths(Path::new("levels"));
        paths.sort();
        assert_eq!(
            paths,
            [
                "area/night.scene",
                "main.scene",
                "old.rscene",
                "upper.SCENE"
            ]
            .map(PathBuf::from)
        );
    }

    struct TestProject(PathBuf);
    impl TestProject {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("rustic-explorer-{}", AssetId::new()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn registered_script(root: &Path, path: &str) -> ScriptId {
        let path = Path::new(path);
        std::fs::create_dir_all(root.join(path.parent().unwrap())).unwrap();
        std::fs::write(root.join(path), "return { Start = function() end }").unwrap();
        register_script_asset(root, path).unwrap()
    }

    fn assert_script_link(root: &Path, id: ScriptId, path: &str) {
        let manifest = load_manifest(&std::fs::read(root.join("config/scripts.ron")).unwrap())
            .unwrap()
            .manifest;
        let entry = manifest
            .scripts
            .iter()
            .find(|entry| entry.id == id)
            .unwrap();
        assert_eq!(entry.relative_path, Path::new(path));
        assert_eq!(
            Path::new(&script_links::script_label(&manifest.scripts, id)),
            Path::new(path)
        );
        // Play resolves the unchanged attachment ID through this registry and reads the source.
        engine_scripting::source_descriptor(root, entry, 1024).unwrap();
    }

    #[test]
    fn registered_script_links_follow_file_and_folder_relocations() {
        let project = TestProject::new();
        let root = &project.0;
        let id = registered_script(root, "scripts/player.lua");
        let nested = registered_script(root, "scripts/nested/start.lua");
        let other = registered_script(root, "scripts-other/other.lua");
        let attachment = ScriptComponent::new(id);
        std::fs::write(root.join("scripts/player.lua.rmeta"), "identity").unwrap();
        rename_project_entry(root, Path::new("scripts/player.lua"), "controller.lua").unwrap();
        assert_script_link(root, attachment.script_id, "scripts/controller.lua");
        assert!(!root.join("scripts/player.lua").exists());
        rename_project_entry(root, Path::new("scripts"), "behaviors").unwrap();
        assert_script_link(root, id, "behaviors/controller.lua");
        assert_script_link(root, nested, "behaviors/nested/start.lua");
        assert_script_link(root, other, "scripts-other/other.lua");
        std::fs::create_dir(root.join("game")).unwrap();
        move_project_entry(root, Path::new("behaviors"), Path::new("game")).unwrap();
        assert_script_link(root, id, "game/behaviors/controller.lua");
        assert_script_link(root, nested, "game/behaviors/nested/start.lua");
        move_project_entry(
            root,
            Path::new("game/behaviors/controller.lua"),
            Path::new("scripts-other"),
        )
        .unwrap();
        assert_script_link(root, id, "scripts-other/controller.lua");
        assert_eq!(
            std::fs::read_to_string(root.join("scripts-other/controller.lua.rmeta")).unwrap(),
            "identity"
        );
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "covers one delete, undo, save, and reopen lifecycle"
    )]
    fn deleting_scripts_removes_links_from_live_and_saved_scenes_and_settings() {
        let temporary = TestProject::new();
        let project = Project::create(
            temporary.0.join("game"),
            "Script links",
            engine_project::ProjectTemplate::Blank,
        )
        .unwrap();
        let root = project.root().to_path_buf();
        let deleted = registered_script(&root, "scripts/behaviors/player.lua");
        let nested = registered_script(&root, "scripts/behaviors/nested/start.js");
        let retained = registered_script(&root, "scripts-other/keep.lua");
        let mut document = AuthoringDocument::open(project, false).unwrap();
        let entity = document
            .add_primitive("Player", Primitive::Cube { size: 1.0 }, None)
            .unwrap();
        document
            .set_scripts(
                entity,
                vec![
                    ScriptComponent::new(deleted),
                    ScriptComponent::new(retained),
                ],
            )
            .unwrap();
        document
            .set_scene_startup_scripts(vec![
                ScriptReference::new(nested),
                ScriptReference::new(retained),
            ])
            .unwrap();
        document.save().unwrap();
        let scene_path = document.scene_path().to_path_buf();
        let other_scene = scene_path.with_file_name("other.scene");
        std::fs::copy(&scene_path, &other_scene).unwrap();
        let legacy_scene = scene_path.with_file_name("legacy.rscene");
        std::fs::copy(&scene_path, &legacy_scene).unwrap();
        let settings = GameSettings {
            global_scripts: vec![
                ScriptReference::new(deleted),
                ScriptReference::new(retained),
            ],
            autosave: false,
        };
        settings.save(&root).unwrap();
        document.set_name(entity, "Unsaved player".into()).unwrap();
        // This edit must remain unsaved, and undo must not resurrect a dead link.
        document
            .set_scripts(
                entity,
                vec![
                    ScriptComponent::new(deleted),
                    ScriptComponent::new(nested),
                    ScriptComponent::new(retained),
                ],
            )
            .unwrap();
        delete_document_project_entry(&mut document, Path::new("scripts/behaviors")).unwrap();
        assert!(!root.join("scripts/behaviors").exists());
        assert_script_link(&root, retained, "scripts-other/keep.lua");
        let manifest = load_manifest(&std::fs::read(root.join("config/scripts.ron")).unwrap())
            .unwrap()
            .manifest;
        assert_eq!(manifest.scripts.len(), 1);
        let settings = GameSettings::load(&root).unwrap();
        assert!(!settings.autosave);
        assert_eq!(
            settings.global_scripts,
            vec![ScriptReference::new(retained)]
        );
        for path in [&scene_path, &other_scene, &legacy_scene] {
            let loaded = engine_world::load_scene(&std::fs::read(path).unwrap()).unwrap();
            assert!(!loaded.read_only);
            assert_eq!(
                loaded
                    .document
                    .entities
                    .iter()
                    .find(|snapshot| snapshot.id == entity)
                    .unwrap()
                    .name
                    .as_deref(),
                Some("Player")
            );
            assert_eq!(
                loaded.document.startup_scripts,
                vec![ScriptReference::new(retained)]
            );
            assert_eq!(
                loaded
                    .document
                    .entities
                    .iter()
                    .find(|snapshot| snapshot.id == entity)
                    .unwrap()
                    .scripts,
                vec![ScriptComponent::new(retained)]
            );
        }
        assert_eq!(
            document.world().snapshot(entity).unwrap().scripts,
            vec![ScriptComponent::new(retained)]
        );
        assert_eq!(
            document.scene_startup_scripts(),
            &[ScriptReference::new(retained)]
        );
        assert!(document.is_modified());
        assert_eq!(
            document.world().snapshot(entity).unwrap().name.as_deref(),
            Some("Unsaved player")
        );
        document.undo().unwrap();
        assert_eq!(
            document.world().snapshot(entity).unwrap().scripts,
            vec![ScriptComponent::new(retained)]
        );
        document.redo().unwrap();
        assert_eq!(
            document.world().snapshot(entity).unwrap().scripts,
            vec![ScriptComponent::new(retained)]
        );
        assert_eq!(
            document.world().snapshot(entity).unwrap().name.as_deref(),
            Some("Unsaved player")
        );
        document.save().unwrap();
        assert!(!document.is_modified());
        let reopened = AuthoringDocument::open(Project::open(&root).unwrap(), false).unwrap();
        assert_eq!(
            reopened.world().snapshot(entity).unwrap().scripts,
            vec![ScriptComponent::new(retained)]
        );
    }

    #[test]
    fn failed_script_deletion_restores_source_metadata_registry_and_settings() {
        let temporary = TestProject::new();
        let project = Project::create(
            temporary.0.join("game"),
            "Rollback",
            engine_project::ProjectTemplate::Blank,
        )
        .unwrap();
        let root = project.root().to_path_buf();
        let id = registered_script(&root, "scripts/player.lua");
        std::fs::write(root.join("scripts/player.lua.rmeta"), "identity").unwrap();
        let mut document = AuthoringDocument::open(project, false).unwrap();
        let entity = document
            .add_primitive("Player", Primitive::Cube { size: 1.0 }, None)
            .unwrap();
        document
            .set_scripts(entity, vec![ScriptComponent::new(id)])
            .unwrap();
        document.save().unwrap();
        let settings = GameSettings {
            global_scripts: vec![ScriptReference::new(id)],
            ..GameSettings::default()
        };
        settings.save(&root).unwrap();
        let before = [
            root.join("config/scripts.ron"),
            root.join("settings.json"),
            document.scene_path().to_path_buf(),
        ]
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        });
        // Fail after the registry and global settings have already been saved.
        let backup = AtomicFileService::backup_path(document.scene_path());
        if backup.exists() {
            std::fs::remove_file(&backup).unwrap();
        }
        std::fs::create_dir(&backup).unwrap();
        let error = delete_document_project_entry(&mut document, Path::new("scripts/player.lua"))
            .unwrap_err();
        assert!(error.contains("deletion was undone"), "{error}");
        assert_script_link(&root, id, "scripts/player.lua");
        assert_eq!(
            std::fs::read_to_string(root.join("scripts/player.lua.rmeta")).unwrap(),
            "identity"
        );
        assert_eq!(
            document.world().snapshot(entity).unwrap().scripts,
            vec![ScriptComponent::new(id)]
        );
        assert!(!document.is_modified());
        for (path, bytes) in before {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn deleting_one_script_preserves_siblings_and_recreation_gets_a_new_id() {
        let project = TestProject::new();
        let root = &project.0;
        let deleted = registered_script(root, "scripts/player.lua");
        let retained = registered_script(root, "scripts/player2.lua");
        assert_eq!(
            delete_project_entry(root, Path::new("scripts/player.lua")).unwrap(),
            BTreeSet::from([deleted])
        );
        assert_script_link(root, retained, "scripts/player2.lua");
        let recreated = registered_script(root, "scripts/player.lua");
        assert_ne!(recreated, deleted);
        assert_script_link(root, recreated, "scripts/player.lua");
    }

    #[test]
    fn script_registry_write_failure_restores_source_and_sidecar() {
        let project = TestProject::new();
        let root = &project.0;
        let id = registered_script(root, "scripts/player.lua");
        std::fs::write(root.join("scripts/player.lua.rmeta"), "identity").unwrap();
        let before = std::fs::read(root.join("config/scripts.ron")).unwrap();
        // A directory at the atomic writer's temporary-file path forces a portable I/O failure.
        std::fs::create_dir(root.join("config/scripts.ron.tmp")).unwrap();
        let error = rename_project_entry(root, Path::new("scripts/player.lua"), "controller.lua")
            .unwrap_err();
        assert!(error.contains("move was undone"), "{error}");
        assert_script_link(root, id, "scripts/player.lua");
        assert_eq!(
            std::fs::read(root.join("config/scripts.ron")).unwrap(),
            before
        );
        assert!(root.join("scripts/player.lua.rmeta").is_file());
        assert!(!root.join("scripts/controller.lua").exists());
        assert!(!root.join("scripts/controller.lua.rmeta").exists());
    }

    #[test]
    fn registered_scripts_reject_invalid_paths_and_registry_collisions() {
        let project = TestProject::new();
        let root = &project.0;
        let id = registered_script(root, "scripts/player.lua");
        let stale = registered_script(root, "scripts/old.lua");
        std::fs::remove_file(root.join("scripts/old.lua")).unwrap();
        assert!(rename_project_entry(root, Path::new("scripts/player.lua"), "old.lua").is_err());
        assert!(rename_project_entry(root, Path::new("scripts/player.lua"), "player.txt").is_err());
        assert_script_link(root, id, "scripts/player.lua");
        let web = registered_script(root, "ui/hud.html");
        assert!(move_project_entry(root, Path::new("ui/hud.html"), Path::new("scripts")).is_err());
        assert!(rename_project_entry(root, Path::new("ui"), "overlay").is_err());
        assert_script_link(root, web, "ui/hud.html");
        let manifest = load_manifest(&std::fs::read(root.join("config/scripts.ron")).unwrap())
            .unwrap()
            .manifest;
        assert!(manifest.scripts.iter().any(|entry| entry.id == stale));
    }

    #[test]
    fn rename_move_copy_and_delete_preserve_files_and_metadata() {
        let project = TestProject::new();
        let root = &project.0;
        std::fs::write(root.join("script.lua"), "original").unwrap();
        std::fs::write(root.join("script.lua.rmeta"), "asset identity").unwrap();
        rename_project_entry(root, Path::new("script.lua"), "renamed.lua").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("renamed.lua.rmeta")).unwrap(),
            "asset identity"
        );
        copy_project_entry(root, Path::new("renamed.lua"), Path::new("")).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("renamed copy 1.lua")).unwrap(),
            "original"
        );
        assert!(!root.join("renamed copy 1.lua.rmeta").exists());
        std::fs::create_dir(root.join("scripts")).unwrap();
        move_project_entry(root, Path::new("renamed.lua"), Path::new("scripts")).unwrap();
        assert!(root.join("scripts/renamed.lua.rmeta").exists());
        delete_project_entry(root, Path::new("scripts/renamed.lua")).unwrap();
        assert!(!root.join("scripts/renamed.lua").exists());
        let trash = std::fs::read_dir(root.join(".rustic/trash"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            std::fs::read_to_string(trash.join("renamed.lua")).unwrap(),
            "original"
        );
        assert!(trash.join("renamed.lua.rmeta").exists());
    }

    #[test]
    fn file_moves_reject_orphaned_destination_metadata() {
        let project = TestProject::new();
        let root = &project.0;
        std::fs::write(root.join("a.obj"), "source").unwrap();
        std::fs::create_dir(root.join("models")).unwrap();
        for target in [root.join("b.obj"), root.join("models/a.obj")] {
            std::fs::write(sidecar_path(&target), "orphan identity").unwrap();
        }
        assert!(rename_project_entry(root, Path::new("a.obj"), "b.obj").is_err());
        assert!(move_project_entry(root, Path::new("a.obj"), Path::new("models")).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("a.obj")).unwrap(),
            "source"
        );
        for target in [root.join("b.obj"), root.join("models/a.obj")] {
            assert!(!target.exists());
            assert_eq!(
                std::fs::read_to_string(sidecar_path(&target)).unwrap(),
                "orphan identity"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn file_moves_reject_dangling_destination_links_and_sidecars() {
        let project = TestProject::new();
        let root = &project.0;
        std::fs::write(root.join("a.obj"), "source").unwrap();
        std::fs::create_dir(root.join("models")).unwrap();
        let missing = root.join("missing");
        for target in [root.join("b.obj"), root.join("models/a.obj")] {
            std::os::unix::fs::symlink(&missing, &target).unwrap();
        }
        assert!(rename_project_entry(root, Path::new("a.obj"), "b.obj").is_err());
        assert!(move_project_entry(root, Path::new("a.obj"), Path::new("models")).is_err());
        for target in [root.join("b.obj"), root.join("models/a.obj")] {
            assert_eq!(std::fs::read_link(&target).unwrap(), missing);
            std::fs::remove_file(&target).unwrap();
            std::os::unix::fs::symlink(&missing, sidecar_path(&target)).unwrap();
        }
        assert!(rename_project_entry(root, Path::new("a.obj"), "b.obj").is_err());
        assert!(move_project_entry(root, Path::new("a.obj"), Path::new("models")).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("a.obj")).unwrap(),
            "source"
        );
        for target in [root.join("b.obj"), root.join("models/a.obj")] {
            assert!(!target.exists());
            assert_eq!(std::fs::read_link(sidecar_path(&target)).unwrap(), missing);
        }
    }

    #[test]
    fn model_reload_reconciles_moves_unsupported_renames_and_deletion() {
        let project = TestProject::new();
        let root = &project.0;
        std::fs::write(
            root.join("triangle.obj"),
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
        )
        .unwrap();
        let (id, _) = import_model_asset(root, Path::new("triangle.obj")).unwrap();
        assert!(load_project_model_assets(root).contains_key(&id));
        std::fs::create_dir(root.join("models")).unwrap();
        move_project_entry(root, Path::new("triangle.obj"), Path::new("models")).unwrap();
        assert!(load_project_model_assets(root).contains_key(&id));
        rename_project_entry(root, Path::new("models/triangle.obj"), "triangle.txt").unwrap();
        assert!(load_project_model_assets(root).is_empty());
        rename_project_entry(root, Path::new("models/triangle.txt"), "triangle.obj").unwrap();
        assert!(load_project_model_assets(root).contains_key(&id));
        delete_project_entry(root, Path::new("models/triangle.obj")).unwrap();
        assert!(load_project_model_assets(root).is_empty());
        assert!(
            scan_project_directory(root, root)
                .directories
                .iter()
                .all(|entry| entry.name != ".rustic")
        );
        // Generated assets must be excluded too, not only the trash child.
        std::fs::create_dir_all(root.join(".rustic/generated")).unwrap();
        std::fs::write(
            root.join(".rustic/generated/triangle.obj"),
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
        )
        .unwrap();
        import_model_asset(root, Path::new(".rustic/generated/triangle.obj")).unwrap();
        assert!(load_project_model_assets(root).is_empty());
    }

    #[test]
    fn filesystem_operations_reject_collisions_traversal_and_self_moves() {
        let project = TestProject::new();
        let root = &project.0;
        std::fs::write(root.join("a.lua"), "a").unwrap();
        std::fs::write(root.join("b.lua"), "b").unwrap();
        assert!(rename_project_entry(root, Path::new("a.lua"), "b.lua").is_err());
        assert!(rename_project_entry(root, Path::new("a.lua"), "../escape.lua").is_err());
        assert!(delete_project_entry(root, Path::new("")).is_err());
        assert!(delete_project_entry(root, Path::new("../escape")).is_err());
        std::fs::create_dir_all(root.join("folder/child")).unwrap();
        assert!(move_project_entry(root, Path::new("folder"), Path::new("folder/child")).is_err());
        assert!(copy_project_entry(root, Path::new("folder"), Path::new("folder/child")).is_err());
        assert_eq!(std::fs::read_to_string(root.join("a.lua")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(root.join("b.lua")).unwrap(), "b");
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_operations_reject_symlinks_and_clean_partial_copies() {
        let project = TestProject::new();
        let outside = TestProject::new();
        let root = &project.0;
        std::fs::write(outside.0.join("secret"), "untouched").unwrap();
        std::os::unix::fs::symlink(&outside.0, root.join("linked")).unwrap();
        assert!(delete_project_entry(root, Path::new("linked/secret")).is_err());
        assert!(copy_project_entry(root, Path::new("linked"), Path::new("")).is_err());
        std::fs::create_dir(root.join("folder")).unwrap();
        std::os::unix::fs::symlink(outside.0.join("secret"), root.join("folder/link")).unwrap();
        assert!(copy_project_entry(root, Path::new("folder"), Path::new("")).is_err());
        assert!(!root.join("folder copy 1").exists());
        assert_eq!(
            std::fs::read_to_string(outside.0.join("secret")).unwrap(),
            "untouched"
        );
    }

    fn row_frame(
        context: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, egui::Rect, Option<ProjectFileAction>) {
        let mut rect = egui::Rect::NOTHING;
        let mut action = None;
        let mut output = context.run_ui(
            egui::RawInput {
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                action = project_entry_row(
                    ui,
                    "script.lua",
                    Path::new("script.lua"),
                    egui::Color32::WHITE,
                    false,
                    true,
                    true,
                );
                rect = ui.min_rect();
            },
        );
        output.textures_delta.clear();
        (output, rect, action)
    }

    fn pointer(position: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn explorer_hover_does_not_drag_but_primary_press_and_move_does() {
        let context = egui::Context::default();
        let (_, rect, _) = row_frame(&context, 0.0, vec![]);
        let position = rect.center();
        let (output, _, _) = row_frame(&context, 0.1, vec![egui::Event::PointerMoved(position)]);
        assert_ne!(output.platform_output.cursor_icon, egui::CursorIcon::Move);
        assert!(!egui::DragAndDrop::has_any_payload(&context));
        row_frame(
            &context,
            0.2,
            vec![pointer(position, egui::PointerButton::Primary, true)],
        );
        row_frame(
            &context,
            0.3,
            vec![egui::Event::PointerMoved(position + egui::vec2(20.0, 0.0))],
        );
        assert!(!egui::DragAndDrop::has_any_payload(&context));
        row_frame(
            &context,
            0.4,
            vec![pointer(
                position + egui::vec2(20.0, 0.0),
                egui::PointerButton::Primary,
                false,
            )],
        );
        row_frame(
            &context,
            0.8,
            vec![
                egui::Event::PointerMoved(position),
                pointer(position, egui::PointerButton::Primary, true),
            ],
        );
        row_frame(
            &context,
            0.9,
            vec![egui::Event::PointerMoved(position + egui::vec2(20.0, 0.0))],
        );
        assert!(egui::DragAndDrop::has_payload_of_type::<ViewportDrop>(
            &context
        ));
    }

    #[test]
    fn explorer_single_click_highlights_and_double_click_renames() {
        let context = egui::Context::default();
        let (_, rect, _) = row_frame(&context, 0.0, vec![]);
        let position = rect.center();
        row_frame(
            &context,
            0.1,
            vec![
                egui::Event::PointerMoved(position),
                pointer(position, egui::PointerButton::Primary, true),
            ],
        );
        row_frame(
            &context,
            0.15,
            vec![pointer(position, egui::PointerButton::Primary, false)],
        );
        let selection = context
            .data_mut(|data| data.get_temp::<ExplorerSelection>(explorer_selection_id()))
            .unwrap();
        assert!(selection.highlighted.contains(Path::new("script.lua")));
        assert!(!selection.renaming);
        row_frame(
            &context,
            0.2,
            vec![pointer(position, egui::PointerButton::Primary, true)],
        );
        let (_, _, action) = row_frame(
            &context,
            0.25,
            vec![pointer(position, egui::PointerButton::Primary, false)],
        );
        assert!(action.is_none(), "double click must not open the file");
        assert!(
            context
                .data_mut(|data| data.get_temp::<ExplorerSelection>(explorer_selection_id()))
                .unwrap()
                .renaming
        );
        let (_, _, action) = row_frame(
            &context,
            0.3,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(action.is_none(), "Escape cancels rename");
    }
    fn collect_text(shape: &egui::epaint::Shape, text: &mut String) {
        match shape {
            egui::epaint::Shape::Text(shape) => {
                text.push_str(&shape.galley.job.text);
                text.push('\n');
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, text);
                }
            }
            _ => {}
        }
    }
    #[test]
    fn secondary_click_opens_file_actions_and_script_scopes() {
        let context = egui::Context::default();
        let (_, rect, _) = row_frame(&context, 0.0, vec![]);
        let position = rect.center();
        row_frame(
            &context,
            0.1,
            vec![
                egui::Event::PointerMoved(position),
                pointer(position, egui::PointerButton::Secondary, true),
            ],
        );
        row_frame(
            &context,
            0.2,
            vec![pointer(position, egui::PointerButton::Secondary, false)],
        );
        let (output, _, _) = row_frame(&context, 0.3, vec![]);
        let mut text = String::new();
        for shape in output.shapes {
            collect_text(&shape.shape, &mut text);
        }
        for expected in [
            "Copy",
            "Cut",
            "Delete",
            "Paste",
            "Global Startup Script",
            "Scene Startup Script",
            "Object Component Script",
        ] {
            assert!(text.contains(expected), "Missing {expected}: {text}");
        }
        assert!(!egui::DragAndDrop::has_any_payload(&context));
    }
    #[test]
    fn expanded_folder_child_owns_its_drag_payload() {
        let context = egui::Context::default();
        egui_extras::install_image_loaders(&context);
        let directory = ProjectDirectory {
            name: "scripts".into(),
            relative_path: "scripts".into(),
            directories: vec![],
            files: vec![ProjectFile {
                name: "child.lua".into(),
                relative_path: "scripts/child.lua".into(),
            }],
        };
        let frame = |time, events| {
            let mut output = context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    render_project_directory(ui, &directory, "", true, true, true);
                },
            );
            output.textures_delta.clear();
            output
        };
        let output = frame(0.0, vec![]);
        let position = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::epaint::Shape::Text(text) = &shape.shape
                    && text.galley.job.text == "child.lua"
                {
                    Some(text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .expect("expanded folder must show its child");
        let output = frame(0.1, vec![egui::Event::PointerMoved(position)]);
        assert_ne!(output.platform_output.cursor_icon, egui::CursorIcon::Move);
        assert_ne!(output.platform_output.cursor_icon, egui::CursorIcon::Grab);
        assert!(!egui::DragAndDrop::has_any_payload(&context));
        context.data_mut(|data| {
            let mut selection = data
                .get_temp::<ExplorerSelection>(explorer_selection_id())
                .unwrap_or_default();
            selection.select(Path::new("scripts/child.lua"), false, false);
            data.insert_temp(explorer_selection_id(), selection);
        });
        frame(
            0.2,
            vec![pointer(position, egui::PointerButton::Primary, true)],
        );
        frame(
            0.3,
            vec![egui::Event::PointerMoved(position + egui::vec2(25.0, 0.0))],
        );
        let payload =
            egui::DragAndDrop::payload::<ViewportDrop>(&context).expect("child starts dragging");
        assert!(
            matches!(payload.as_ref(), ViewportDrop::File(path) if path == Path::new("scripts/child.lua"))
        );
    }
    #[test]
    fn explorer_background_menu_opens_in_empty_space_without_stealing_row_clicks() {
        let context = egui::Context::default();
        let frame = |time, events| {
            let mut clicked = false;
            let mut output = context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let background = explorer_tree_background(ui, "background");
                    clicked = ui.button("Object").clicked();
                    background.context_menu(|ui| {
                        let _ = ui.button("Paste at Project Root");
                    });
                },
            );
            output.textures_delta.clear();
            (output, clicked)
        };
        let (output, _) = frame(0.0, vec![]);
        let position = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::epaint::Shape::Text(text) = &shape.shape
                    && text.galley.job.text == "Object"
                {
                    Some(text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .unwrap();
        frame(
            0.1,
            vec![
                egui::Event::PointerMoved(position),
                pointer(position, egui::PointerButton::Primary, true),
            ],
        );
        let (_, clicked) = frame(
            0.2,
            vec![pointer(position, egui::PointerButton::Primary, false)],
        );
        assert!(clicked, "the item must keep its click interaction");
        let empty_space = position + egui::vec2(100.0, 100.0);
        frame(
            0.3,
            vec![
                egui::Event::PointerMoved(empty_space),
                pointer(empty_space, egui::PointerButton::Secondary, true),
            ],
        );
        frame(
            0.4,
            vec![pointer(empty_space, egui::PointerButton::Secondary, false)],
        );
        let (output, _) = frame(0.5, vec![]);
        let mut text = String::new();
        for shape in output.shapes {
            collect_text(&shape.shape, &mut text);
        }
        assert!(
            text.contains("Paste at Project Root"),
            "Missing root paste menu: {text}"
        );
    }
    fn selection_frame(
        context: &egui::Context,
        root: &Path,
        time: f64,
        mut events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<ProjectFileAction>) {
        let modifiers = events
            .iter()
            .rev()
            .find_map(|event| match event {
                egui::Event::PointerButton { modifiers, .. }
                | egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_else(|| context.input(|input| input.modifiers));
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let mut action = None;
        let mut output = context.run_ui(
            egui::RawInput {
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                begin_explorer_selection(ui);
                for (name, directory) in [
                    ("a.lua", false),
                    ("b.lua", false),
                    ("c.lua", false),
                    ("one", true),
                    ("two", true),
                ] {
                    if let Some(row_action) = project_entry_row(
                        ui,
                        name,
                        Path::new(name),
                        egui::Color32::WHITE,
                        directory,
                        true,
                        true,
                    ) {
                        action = Some(row_action);
                    }
                }
                if let Some(shortcut) = explorer_keyboard_action(ui, root, true) {
                    action = Some(shortcut);
                }
            },
        );
        output.textures_delta.clear();
        (output, action)
    }

    fn text_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::epaint::Shape::Text(text) = &shape.shape
                    && text.galley.job.text == label
                {
                    Some(text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Missing UI text: {label}"))
    }

    fn modifier_pointer(
        position: egui::Pos2,
        pressed: bool,
        modifiers: egui::Modifiers,
    ) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        }
    }

    fn ctrl_key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        }
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "verify keyboard selection, copy and fan-out paste as a single user flow"
    )]
    fn explorer_ctrl_range_copy_and_multi_folder_paste_work_end_to_end() {
        let project = TestProject::new();
        for name in ["a.lua", "b.lua", "c.lua"] {
            std::fs::write(project.0.join(name), name).unwrap();
        }
        for name in ["one", "two"] {
            std::fs::create_dir(project.0.join(name)).unwrap();
        }
        let context = egui::Context::default();
        let (output, _) = selection_frame(&context, &project.0, 0.0, vec![]);
        let a = text_position(&output, "a.lua");
        let c = text_position(&output, "c.lua");
        let one = text_position(&output, "one");
        let two = text_position(&output, "two");
        selection_frame(
            &context,
            &project.0,
            0.1,
            vec![
                egui::Event::PointerMoved(a),
                modifier_pointer(a, true, egui::Modifiers::NONE),
            ],
        );
        selection_frame(
            &context,
            &project.0,
            0.2,
            vec![modifier_pointer(a, false, egui::Modifiers::NONE)],
        );
        selection_frame(
            &context,
            &project.0,
            0.4,
            vec![
                egui::Event::PointerMoved(c),
                modifier_pointer(c, true, egui::Modifiers::CTRL),
            ],
        );
        selection_frame(
            &context,
            &project.0,
            0.5,
            vec![modifier_pointer(c, false, egui::Modifiers::CTRL)],
        );
        let (_, copy) = selection_frame(&context, &project.0, 0.6, vec![ctrl_key(egui::Key::C)]);
        let Some(ProjectFileAction::Copy(sources)) = copy else {
            panic!("Ctrl+C must copy selected rows");
        };
        assert_eq!(sources, ["a.lua", "b.lua", "c.lua"].map(PathBuf::from));
        let mut clipboard = ProjectClipboard {
            sources,
            cut: false,
        };
        selection_frame(
            &context,
            &project.0,
            0.8,
            vec![
                egui::Event::PointerMoved(one),
                modifier_pointer(one, true, egui::Modifiers::NONE),
            ],
        );
        selection_frame(
            &context,
            &project.0,
            0.9,
            vec![modifier_pointer(one, false, egui::Modifiers::NONE)],
        );
        selection_frame(
            &context,
            &project.0,
            1.1,
            vec![
                egui::Event::PointerMoved(two),
                modifier_pointer(two, true, egui::Modifiers::SHIFT),
            ],
        );
        selection_frame(
            &context,
            &project.0,
            1.2,
            vec![modifier_pointer(two, false, egui::Modifiers::SHIFT)],
        );
        let (_, paste) = selection_frame(
            &context,
            &project.0,
            1.3,
            vec![ctrl_key(egui::Key::V), egui::Event::Paste(String::new())],
        );
        let Some(ProjectFileAction::Paste(destinations)) = paste else {
            panic!("Ctrl+V must paste into selected folders");
        };
        assert_eq!(destinations, ["one", "two"].map(PathBuf::from));
        assert!(
            paste_project_entries(&project.0, &mut clipboard, &destinations)
                .into_iter()
                .all(|result| result.is_ok())
        );
        for folder in ["one", "two"] {
            for name in ["a.lua", "b.lua", "c.lua"] {
                assert_eq!(
                    std::fs::read_to_string(project.0.join(folder).join(name)).unwrap(),
                    name
                );
            }
        }
        assert_eq!(
            clipboard.sources.len(),
            3,
            "copy remains available for repeated paste"
        );
        assert!(
            paste_project_entries(&project.0, &mut clipboard, &destinations)
                .into_iter()
                .all(|result| result.is_ok())
        );
        assert!(project.0.join("one/a copy 1.lua").exists());
        assert!(project.0.join("two/a copy 1.lua").exists());
    }

    #[test]
    fn explorer_dragging_selected_row_moves_the_whole_highlighted_group() {
        let context = egui::Context::default();
        let (output, _) = selection_frame(&context, Path::new(""), 0.0, vec![]);
        let a = text_position(&output, "a.lua");
        let b = text_position(&output, "b.lua");
        selection_frame(
            &context,
            Path::new(""),
            0.1,
            vec![
                egui::Event::PointerMoved(a),
                modifier_pointer(a, true, egui::Modifiers::NONE),
            ],
        );
        selection_frame(
            &context,
            Path::new(""),
            0.2,
            vec![modifier_pointer(a, false, egui::Modifiers::NONE)],
        );
        selection_frame(
            &context,
            Path::new(""),
            0.4,
            vec![
                egui::Event::PointerMoved(b),
                modifier_pointer(b, true, egui::Modifiers::SHIFT),
            ],
        );
        selection_frame(
            &context,
            Path::new(""),
            0.5,
            vec![modifier_pointer(b, false, egui::Modifiers::SHIFT)],
        );
        selection_frame(
            &context,
            Path::new(""),
            0.8,
            vec![
                egui::Event::PointerMoved(a),
                modifier_pointer(a, true, egui::Modifiers::NONE),
            ],
        );
        selection_frame(
            &context,
            Path::new(""),
            0.9,
            vec![egui::Event::PointerMoved(a + egui::vec2(40.0, 0.0))],
        );
        let payload = egui::DragAndDrop::payload::<ViewportDrop>(&context).unwrap();
        assert_eq!(
            payload.project_paths(),
            ["a.lua", "b.lua"].map(PathBuf::from)
        );
    }

    #[test]
    fn paste_failures_preserve_cut_clipboard_and_process_other_copy_destinations() {
        let project = TestProject::new();
        std::fs::write(project.0.join("a.lua"), "a").unwrap();
        std::fs::create_dir(project.0.join("one")).unwrap();
        let mut clipboard = ProjectClipboard {
            sources: vec!["a.lua".into()],
            cut: true,
        };
        assert!(
            paste_project_entries(&project.0, &mut clipboard, &["one".into(), "two".into()])[0]
                .is_err()
        );
        assert!(project.0.join("a.lua").exists());
        assert_eq!(clipboard.sources.len(), 1);
        clipboard.cut = false;
        let results = paste_project_entries(
            &project.0,
            &mut clipboard,
            &["missing".into(), "one".into()],
        );
        assert!(results[0].is_err());
        assert!(results[1].is_ok());
        assert!(project.0.join("one/a.lua").exists());
    }
    #[test]
    fn shift_hold_can_highlight_rows_without_starting_a_drag() {
        let context = egui::Context::default();
        let (output, _) = selection_frame(&context, Path::new(""), 0.0, vec![]);
        let a = text_position(&output, "a.lua");
        let c = text_position(&output, "c.lua");
        selection_frame(
            &context,
            Path::new(""),
            0.1,
            vec![
                egui::Event::PointerMoved(a),
                modifier_pointer(a, true, egui::Modifiers::SHIFT),
            ],
        );
        selection_frame(
            &context,
            Path::new(""),
            0.2,
            vec![egui::Event::PointerMoved(c)],
        );
        let selection = context
            .data_mut(|data| data.get_temp::<ExplorerSelection>(explorer_selection_id()))
            .unwrap();
        assert!(selection.highlighted.contains(Path::new("a.lua")));
        assert!(selection.highlighted.contains(Path::new("c.lua")));
        assert!(!egui::DragAndDrop::has_any_payload(&context));
    }

    #[test]
    fn explorer_shortcuts_are_scoped_to_selection_and_leave_text_editing_alone() {
        let context = egui::Context::default();
        let frame = |focused, renaming, can_edit| {
            context.data_mut(|data| {
                data.insert_temp(
                    explorer_selection_id(),
                    ExplorerSelection {
                        highlighted: [PathBuf::from("a.lua")].into(),
                        focused,
                        renaming,
                        ..Default::default()
                    },
                )
            });
            let mut action = None;
            let mut output = context.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Copy],
                    ..Default::default()
                },
                |ui| {
                    action = explorer_keyboard_action(ui, Path::new(""), can_edit);
                },
            );
            output.textures_delta.clear();
            action
        };
        assert!(frame(false, false, true).is_none());
        assert!(frame(true, true, true).is_none());
        assert!(frame(true, false, false).is_none());
        assert!(matches!(
            frame(true, false, true),
            Some(ProjectFileAction::Copy(_))
        ));
    }

    #[test]
    fn double_click_rename_commits_enter_text() {
        let context = egui::Context::default();
        let (_, rect, _) = row_frame(&context, 0.0, vec![]);
        let position = rect.center();
        for (time, pressed) in [(0.1, true), (0.15, false), (0.2, true), (0.25, false)] {
            row_frame(
                &context,
                time,
                vec![
                    egui::Event::PointerMoved(position),
                    pointer(position, egui::PointerButton::Primary, pressed),
                ],
            );
        }
        row_frame(&context, 0.3, vec![]);
        row_frame(
            &context,
            0.4,
            vec![
                egui::Event::ModifiersChanged(egui::Modifiers::CTRL | egui::Modifiers::COMMAND),
                ctrl_key(egui::Key::A),
                egui::Event::Text("renamed.lua".into()),
            ],
        );
        let (_, _, action) = row_frame(
            &context,
            0.5,
            vec![
                egui::Event::ModifiersChanged(egui::Modifiers::NONE),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(
            matches!(action.as_ref(), Some(ProjectFileAction::Rename(path, name)) if path == Path::new("script.lua") && name == "renamed.lua"),
            "Rename action: {action:?}"
        );
    }
}
