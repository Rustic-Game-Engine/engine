mod play_session;
mod ui_theme;

use eframe::egui;
use egui_tiles::{Behavior, TileId, Tree, UiResponse};
use engine_core::logging::{AsyncLogger, Language, LogConfig, LogStream, Severity};
use engine_core::{ApplicationIdentity, ApplicationRole, run_headless_smoke_from_arguments};
use engine_editor::{
    AuthoringDocument, ConsoleEntry, EditorCamera, EditorTab, EditorWorkspace, GizmoAxis,
    GizmoOperation, GizmoSettings, GizmoSpace, PickMesh, ProjectAccess, ProjectLock,
    RuntimeConsole, apply_gizmo_delta, grid_lines, pick_meshes,
};
use engine_platform::{AtomicFileService, ConfigStore};
use engine_play::{ControlRequest, PlayMode, RuntimeChangeSet, RuntimeState};
use engine_project::{Project, VirtualDirectory};
use engine_scripting::{
    CodeEditor, EditorConfiguration, EngineValue, GameSettings, ProjectOpenBehavior,
    ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
    build_editor_launch, discover_code_editor, generate_programming_workspace, load_manifest,
    open_in_external_editor, probe_language_toolchain, save_manifest_atomic,
};
use engine_world::ScriptComponent;
use engine_world::{EntityId, LocalTransform, Primitive, RenderWorldBuffer};
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use num_traits::ToPrimitive as _;
use play_session::EditorPlaySession;
use renderer_wgpu::{
    BackendRequest, RenderedFrame, SceneViewportRenderer, ViewportGuide, ViewportMesh,
    ViewportScene, ViewportVertex,
};
use std::collections::hash_map::DefaultHasher;
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
    let title = format!("{} — Rustic Editor", project.metadata().name);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
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

struct EditorApp {
    document: AuthoringDocument,
    _project_lock: Option<engine_editor::ProjectLock>,
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
    render_world: RenderWorldBuffer,
    detach_inspector: bool,
    game_settings: GameSettings,
    game_name_text: String,
    entry_script_text: String,
    autosave_enabled: bool,
    last_autosave: Option<Instant>,
    show_game_settings: bool,
    missing_dependencies: Vec<ScriptLanguage>,
    show_dependency_prompt: bool,
}

impl EditorApp {
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
        let (viewport_request, preview_result) = spawn_viewport_renderer();
        let game_settings = GameSettings::load(document.project().root()).unwrap_or_else(|_| {
            let manifest_path = document.project().root().join("config/scripts.ron");
            std::fs::read(manifest_path)
                .ok()
                .and_then(|bytes| load_manifest(&bytes).ok())
                .and_then(|load| load.manifest.scripts.first().cloned())
                .map_or_else(GameSettings::default, |entry| GameSettings {
                    entry_script: entry.relative_path,
                    ..GameSettings::default()
                })
        });
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
        let entry_script_text = game_settings
            .entry_script
            .to_string_lossy()
            .replace('\\', "/");
        let autosave_enabled = game_settings.autosave;
        let missing_dependencies = missing_project_dependencies(document.project().root());
        let show_dependency_prompt = !missing_dependencies.is_empty();
        let mut app = Self {
            document,
            _project_lock: project_lock,
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
            render_world: RenderWorldBuffer::new(),
            detach_inspector: false,
            game_settings,
            game_name_text,
            entry_script_text,
            autosave_enabled,
            last_autosave: Some(Instant::now()),
            show_game_settings: false,
            missing_dependencies,
            show_dependency_prompt,
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
            Ok(()) => self.log(Severity::Info, "scene", "Scene saved transactionally"),
            Err(error) => self.log(Severity::Error, "scene", error.to_string()),
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
            Ok(()) => self.log(Severity::Info, "scene.autosave", "Scene autosaved"),
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

    fn create_gameplay_script(&mut self, language: ScriptLanguage) {
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
        if let Some(entity) = self.document.selected() {
            let mut scripts = self
                .document
                .world()
                .snapshot(entity)
                .map(|s| s.scripts)
                .unwrap_or_default();
            scripts.push(ScriptComponent::new(id));
            if let Err(error) = self.document.set_scripts(entity, scripts) {
                self.log(Severity::Error, "programming", error.to_string());
                return;
            }
        }
        self.log(
            Severity::Info,
            "programming",
            format!("Created {} with stable ID {id}", relative.display()),
        );
        self.open_script_path(&relative, 1, 1);
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
                ui.label("Panels");
                for panel in [EditorTab::Viewport3d, EditorTab::Viewport2d] {
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
                ui.menu_button("Create Behavior", |ui| {
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
                            self.create_gameplay_script(language);
                            ui.close();
                        }
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
                    .color(ui_theme::TEXT),
            );
            if self.document.is_modified() {
                ui.label(egui::RichText::new("●  Unsaved").color(ui_theme::WARNING));
            }
            if self.document.is_read_only() {
                ui.colored_label(ui_theme::WARNING, "READ ONLY");
            }
            ui.separator();
            if self.play.is_none() {
                if ui
                    .add(egui::Button::new("▶  Play").fill(egui::Color32::from_rgb(45, 100, 70)))
                    .clicked()
                {
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
                if ui
                    .add(egui::Button::new("■  Stop").fill(egui::Color32::from_rgb(115, 53, 53)))
                    .clicked()
                {
                    self.stop_play();
                }
                let paused = self
                    .play
                    .as_ref()
                    .is_some_and(|play| play.state() == RuntimeState::Paused);
                if ui
                    .button(if paused { "▶ Resume" } else { "Ⅱ Pause" })
                    .clicked()
                {
                    self.control_play(if paused {
                        ControlRequest::Resume
                    } else {
                        ControlRequest::Pause
                    });
                }
                if ui
                    .add_enabled(paused, egui::Button::new("▹ Frame"))
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
                ui.label("Entry script");
                ui.add_enabled(
                    !self.document.is_read_only(),
                    egui::TextEdit::singleline(&mut self.entry_script_text)
                        .desired_width(420.0)
                        .hint_text("scripts/main.lua"),
                );
                ui.small("Project-relative path. Its top-level code runs once when play starts.");
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
                            entry_script: PathBuf::from(self.entry_script_text.trim()),
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
                            .and_then(|()| {
                                let path = root.join(&candidate.entry_script);
                                if !path.is_file() {
                                    return Err(format!(
                                        "entry script does not exist: {}",
                                        path.display()
                                    ));
                                }
                                candidate.save(&root)
                            })
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
                                    "Saved game name, entry script, and autosave settings",
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
                                self.log(Severity::Error, "programming.dependencies", error)
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
    let desired_size = egui::vec2(48.0, 46.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    let visuals = ui
        .style()
        .interact_selectable(&response, current == operation);
    ui.painter().rect_filled(rect, 5.0, visuals.weak_bg_fill);

    let icon_source = match operation {
        GizmoOperation::Translate => {
            egui::include_image!("../../../assets/icons/material-design/axis-arrow.svg")
        }
        GizmoOperation::Rotate => {
            egui::include_image!("../../../assets/icons/material-design/rotate-orbit.svg")
        }
        GizmoOperation::Scale => {
            egui::include_image!("../../../assets/icons/material-design/resize.svg")
        }
    };
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 15.0),
        egui::vec2(24.0, 24.0),
    );
    ui.put(
        icon_rect,
        egui::Image::new(icon_source).tint(visuals.fg_stroke.color),
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 5.0),
        egui::Align2::CENTER_BOTTOM,
        label,
        egui::FontId::proportional(10.5),
        visuals.fg_stroke.color,
    );
    response.on_hover_text(format!("{label} tool"))
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_background(ui.ctx());
        let scene_was_modified = self.document.is_modified();
        egui::Panel::top("editor-toolbar")
            .frame(
                egui::Frame::new()
                    .fill(ui_theme::PANEL)
                    .stroke(egui::Stroke::new(1.0, ui_theme::BORDER))
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ui, |ui| self.top_bar(ui));
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
            camera: &mut self.camera,
            viewport_extent: &mut self.viewport_extent,
            gizmo: &mut self.gizmo,
            gizmo_drag: &mut self.gizmo_drag,
        };
        self.dock.ui(&mut viewer, ui);
        if viewer.request_preview {
            self.refresh_preview();
        }
        self.runtime_changes_dialog(ui.ctx());
        self.game_settings_dialog(ui.ctx());
        self.dependency_prompt(ui.ctx());
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
    camera: &'a mut EditorCamera,
    viewport_extent: &'a mut [u32; 2],
    gizmo: &'a mut GizmoSettings,
    gizmo_drag: &'a mut Option<GizmoDrag>,
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
            EditorTab::Inspector => self.inspector(ui),
            // Kept as a compatibility fallback for an in-memory legacy dock.
            EditorTab::ContentBrowser => self.project_explorer(ui),
            EditorTab::Console => self.console(ui),
            // Legacy workspace files can still deserialize this removed pane.
            EditorTab::Settings => self.inspector(ui),
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
        let frame = egui::Frame::NONE.fill(egui::Color32::from_rgb(23, 27, 34));
        let (body, dropped) = ui.dnd_drop_zone::<PathBuf, _>(frame, |ui| {
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
                    .rect_filled(rect, 0.0, egui::Color32::from_rgb(23, 27, 34));
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    self.preview_status,
                    egui::FontId::proportional(16.0),
                    egui::Color32::LIGHT_GRAY,
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
        if !is_2d && !self.playing {
            self.viewport_input(ui, &response);
            self.gizmo_overlay(ui, response.rect);
        }
        if let Some(path) = dropped {
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("rscene"))
            {
                let relative = path.strip_prefix("scenes").unwrap_or(&path);
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
            } else {
                self.console.push(simple_console(
                    Severity::Info,
                    "editor",
                    format!("Dropped asset {} into viewport", path.display()),
                ));
            }
        }
        ui.small(self.preview_status);
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
        // Hovering is sufficient to steer the scene. Requiring an earlier LMB
        // click made RMB-look and WASD appear broken because the viewport never
        // acquired keyboard focus.
        let keyboard_active =
            (response.has_focus() || hovered) && !ui.ctx().egui_wants_keyboard_input();
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
                .and_then(|ray| pick_meshes(ray, &pick_scene(self.document)));
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
        let Some(entity) = self.document.selected() else {
            return;
        };
        let Ok(snapshot) = self.document.world().snapshot(entity) else {
            return;
        };
        let Ok(world) = self.document.world().world_transform(entity) else {
            return;
        };
        let aspect = rect.width() / rect.height().max(1.0);
        let view_projection = self.camera.view_projection(aspect);
        let origin_world = world.0.transform_point3(Vec3::ZERO);
        let Some(origin) = project_to_rect(view_projection, origin_world, rect) else {
            return;
        };
        if self.gizmo.operation == GizmoOperation::Rotate {
            self.rotation_gizmo_overlay(
                ui,
                rect,
                entity,
                snapshot.local_transform,
                view_projection,
                origin_world,
                origin,
            );
            return;
        }
        for (axis, color) in [
            (GizmoAxis::X, egui::Color32::RED),
            (GizmoAxis::Y, egui::Color32::GREEN),
            (GizmoAxis::Z, egui::Color32::BLUE),
        ] {
            let axis_world = if self.gizmo.space == GizmoSpace::Local {
                snapshot.local_transform.rotation * axis.vector()
            } else {
                axis.vector()
            };
            let Some(projected) = project_to_rect(view_projection, origin_world + axis_world, rect)
            else {
                continue;
            };
            let direction = (projected - origin).normalized();
            let end = origin + direction * 72.0;
            ui.painter()
                .line_segment([origin, end], egui::Stroke::new(4.0, color));
            ui.painter().circle_filled(end, 6.0, color);
            let interaction = ui.interact(
                egui::Rect::from_two_pos(origin, end).expand(8.0),
                egui::Id::new(("viewport-gizmo", entity.to_string(), axis as u8)),
                if self.document.is_read_only() {
                    egui::Sense::hover()
                } else {
                    egui::Sense::drag()
                },
            );
            if interaction.drag_started() {
                *self.gizmo_drag = Some(GizmoDrag {
                    entity,
                    axis,
                    before: snapshot.local_transform,
                    after: snapshot.local_transform,
                    amount: 0.0,
                    pointer_direction: direction,
                    last_pointer: interaction.interact_pointer_pos().unwrap_or(origin),
                });
            }
            if interaction.dragged()
                && let Some(mut drag) = *self.gizmo_drag
                && drag.entity == entity
                && drag.axis == axis
            {
                drag.amount = accumulate_gizmo_pointer_delta(
                    drag.amount,
                    interaction.drag_delta(),
                    direction,
                    self.gizmo.operation,
                );
                if let Some(after) = apply_gizmo_delta(drag.before, axis, drag.amount, *self.gizmo)
                {
                    drag.after = after;
                    if self.document.preview_transform(entity, after).is_ok() {
                        *self.gizmo_drag = Some(drag);
                        self.request_preview = true;
                    }
                }
            }
            if interaction.drag_stopped()
                && let Some(drag) = self.gizmo_drag.take()
                && let Err(error) =
                    self.document
                        .commit_transform_drag(drag.entity, drag.before, drag.after)
            {
                self.console
                    .push(simple_console(Severity::Error, "gizmo", error.to_string()));
            }
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the rotation overlay needs the viewport projection and selected transform"
    )]
    fn rotation_gizmo_overlay(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        entity: EntityId,
        local_transform: LocalTransform,
        view_projection: Mat4,
        origin_world: Vec3,
        origin: egui::Pos2,
    ) {
        const RING_RADIUS: f32 = 72.0;
        const HIT_RADIUS: f32 = 9.0;
        let axes = [
            (GizmoAxis::X, egui::Color32::RED),
            (GizmoAxis::Y, egui::Color32::GREEN),
            (GizmoAxis::Z, egui::Color32::BLUE),
        ];
        let rings = axes.map(|(axis, color)| {
            let axis_world = if self.gizmo.space == GizmoSpace::Local {
                local_transform.rotation * axis.vector()
            } else {
                axis.vector()
            };
            (
                axis,
                color,
                projected_rotation_ring(
                    view_projection,
                    origin_world,
                    origin,
                    axis_world,
                    rect,
                    RING_RADIUS,
                ),
            )
        });

        let pointer = ui.input(|input| input.pointer.hover_pos());
        let active_axis = self.gizmo_drag.as_ref().map(|drag| drag.axis);
        let hovered = pointer.and_then(|pointer| {
            rings
                .iter()
                .filter_map(|(axis, _, points)| {
                    ring_pointer_distance(points, pointer)
                        .map(|(distance, tangent)| (*axis, distance, tangent))
                })
                .filter(|(_, distance, _)| *distance <= HIT_RADIUS)
                .min_by(|left, right| left.1.total_cmp(&right.1))
        });

        ui.painter().circle_stroke(
            origin,
            RING_RADIUS + 5.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(125)),
        );

        for (axis, color, points) in rings {
            if points.len() < 2 {
                continue;
            }
            let is_active = active_axis == Some(axis);
            let is_hovered = hovered.is_some_and(|(hovered_axis, _, _)| hovered_axis == axis);
            let stroke_width = if is_active || is_hovered { 5.0 } else { 3.0 };
            let pointer_tangent = pointer.and_then(|pointer| {
                ring_pointer_distance(&points, pointer).map(|(_, tangent)| tangent)
            });
            ui.painter().add(egui::Shape::closed_line(
                points,
                egui::Stroke::new(stroke_width, color),
            ));

            let interaction_rect = if is_active {
                rect
            } else if is_hovered {
                egui::Rect::from_center_size(pointer.unwrap_or(origin), egui::Vec2::splat(20.0))
            } else {
                egui::Rect::NOTHING
            };
            let interaction = ui.interact(
                interaction_rect,
                egui::Id::new(("viewport-gizmo", entity.to_string(), axis as u8)),
                if self.document.is_read_only() || (!is_active && !is_hovered) {
                    egui::Sense::hover()
                } else {
                    egui::Sense::drag()
                },
            );
            if interaction.drag_started() {
                let tangent = hovered.map_or(egui::Vec2::RIGHT, |(_, _, tangent)| tangent);
                *self.gizmo_drag = Some(GizmoDrag {
                    entity,
                    axis,
                    before: local_transform,
                    after: local_transform,
                    amount: 0.0,
                    pointer_direction: tangent,
                    last_pointer: interaction.interact_pointer_pos().unwrap_or(origin),
                });
            }
            if interaction.dragged()
                && let Some(mut drag) = *self.gizmo_drag
                && drag.entity == entity
                && drag.axis == axis
            {
                let current_pointer = interaction
                    .interact_pointer_pos()
                    .unwrap_or(drag.last_pointer);
                let tangent = pointer_tangent.unwrap_or(drag.pointer_direction);
                drag.amount = accumulate_gizmo_pointer_delta(
                    drag.amount,
                    current_pointer - drag.last_pointer,
                    tangent,
                    GizmoOperation::Rotate,
                );
                drag.pointer_direction = tangent;
                drag.last_pointer = current_pointer;
                if let Some(after) = apply_gizmo_delta(drag.before, axis, drag.amount, *self.gizmo)
                {
                    drag.after = after;
                    if self.document.preview_transform(entity, after).is_ok() {
                        *self.gizmo_drag = Some(drag);
                        self.request_preview = true;
                    }
                }
            }
            if interaction.drag_stopped()
                && let Some(drag) = self.gizmo_drag.take()
                && let Err(error) =
                    self.document
                        .commit_transform_drag(drag.entity, drag.before, drag.after)
            {
                self.console
                    .push(simple_console(Severity::Error, "gizmo", error.to_string()));
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
        ui.heading(egui::RichText::new("Project").color(ui_theme::TEXT));
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

    fn scene_tree(&mut self, ui: &mut egui::Ui) {
        let snapshots = self.document.entity_snapshots();
        let entity_count = snapshots.as_ref().map_or(0, Vec::len);
        let selected_folder = snapshots.as_ref().ok().and_then(|items| {
            let selected = self.document.selected()?;
            items
                .iter()
                .find(|item| item.id == selected && item.folder)
                .map(|item| item.id)
        });
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{entity_count} entities")).color(ui_theme::MUTED),
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
                    if let Some(id) =
                        render_scene_hierarchy(ui, &snapshots, None, self.document.selected())
                    {
                        self.document.select(Some(id));
                        self.request_preview = true;
                    }
                }
                Err(error) => {
                    ui.colored_label(egui::Color32::RED, error.to_string());
                }
            });
    }

    fn file_explorer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Filter").color(ui_theme::MUTED));
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
                        .color(ui_theme::TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} files", tree.file_count()))
                            .small()
                            .color(ui_theme::MUTED),
                    );
                });
            } else {
                ui.spinner();
            }
        });
        ui.separator();
        if let Some(tree) = self.project_tree {
            let filter = self.project_filter.trim().to_ascii_lowercase();
            egui::ScrollArea::vertical()
                .id_salt("game-project-files")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(file) = render_project_root(ui, tree, &filter) {
                        self.open_project_script(&file);
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
        let Some(entity) = self.document.selected() else {
            ui.vertical_centered(|ui| {
                ui.add_space(48.0);
                ui.label(egui::RichText::new("Nothing selected").size(16.0).strong());
                ui.label(
                    egui::RichText::new(
                        "Choose an entity in the Project panel to edit its properties.",
                    )
                    .color(ui_theme::MUTED),
                );
            });
            return;
        };
        let Ok(snapshot) = self.document.world().snapshot(entity) else {
            ui.label("Selection is no longer available.");
            return;
        };
        let mut name = snapshot.name.clone().unwrap_or_else(|| "Entity".into());
        if ui.text_edit_singleline(&mut name).changed() && !self.document.is_read_only() {
            if let Err(error) = self.document.set_name(entity, name) {
                self.console.push(simple_console(
                    Severity::Error,
                    "inspector",
                    error.to_string(),
                ));
            }
        }
        ui.label(
            egui::RichText::new(entity.to_string())
                .monospace()
                .small()
                .color(ui_theme::MUTED),
        );
        ui.separator();
        let mut transform = snapshot.local_transform;
        let mut translation = transform.translation.to_array();
        ui_theme::section(ui, "Transform", "Translation");
        let changed = ui
            .horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut translation[0]).prefix("X "))
                    .changed()
                    | ui.add(egui::DragValue::new(&mut translation[1]).prefix("Y "))
                        .changed()
                    | ui.add(egui::DragValue::new(&mut translation[2]).prefix("Z "))
                        .changed()
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
        let mut rotation = rotation_degrees(transform.rotation);
        ui.label("Rotation (degrees)");
        let rotation_changed = ui
            .horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut rotation[0])
                        .prefix("X ")
                        .speed(0.5),
                )
                .changed()
                    | ui.add(
                        egui::DragValue::new(&mut rotation[1])
                            .prefix("Y ")
                            .speed(0.5),
                    )
                    .changed()
                    | ui.add(
                        egui::DragValue::new(&mut rotation[2])
                            .prefix("Z ")
                            .speed(0.5),
                    )
                    .changed()
            })
            .inner;
        if rotation_changed && !self.document.is_read_only() {
            transform.rotation = rotation_from_degrees(rotation);
            if self.document.set_transform(entity, transform).is_ok() {
                self.request_preview = true;
            }
        }
        let mut scale = transform.scale.to_array();
        ui.label("Size (X, Y, Z)");
        let scale_changed = ui
            .horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut scale[0]).prefix("X "))
                    .changed()
                    | ui.add(egui::DragValue::new(&mut scale[1]).prefix("Y "))
                        .changed()
                    | ui.add(egui::DragValue::new(&mut scale[2]).prefix("Z "))
                        .changed()
            })
            .inner;
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
            if ui.button("Align to editor view").clicked() && !self.document.is_read_only() {
                if self
                    .document
                    .set_transform(entity, camera_placement(*self.camera))
                    .is_ok()
                {
                    self.request_preview = true;
                }
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
                .changed();
            attributes_changed |= ui
                .checkbox(&mut attributes.can_collide, "Can collide")
                .changed();
            attributes_changed |= ui.checkbox(&mut attributes.anchored, "Anchored").changed();
            ui.label(format!(
                "Parent: {}",
                snapshot
                    .parent
                    .map_or_else(|| "Scene root".into(), |id| id.to_string())
            ));
            if attributes_changed && !self.document.is_read_only() {
                if self
                    .document
                    .set_part_attributes(entity, attributes)
                    .is_ok()
                {
                    self.request_preview = true;
                }
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
                    .color(ui_theme::MUTED),
            );
        }
        if !snapshot.scripts.is_empty() {
            self.script_inspector(ui, entity, snapshot.scripts);
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
        let mut changed = false;
        for script in &mut scripts {
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut script.enabled, "Enabled").changed();
                ui.monospace(script.script_id.to_string());
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

#[derive(Clone, Copy)]
struct GizmoDrag {
    entity: EntityId,
    axis: GizmoAxis,
    before: LocalTransform,
    after: LocalTransform,
    amount: f32,
    pointer_direction: egui::Vec2,
    last_pointer: egui::Pos2,
}

fn projected_rotation_ring(
    view_projection: Mat4,
    origin_world: Vec3,
    origin: egui::Pos2,
    axis_world: Vec3,
    rect: egui::Rect,
    radius: f32,
) -> Vec<egui::Pos2> {
    const SEGMENTS: usize = 64;
    let reference = if axis_world.dot(Vec3::Y).abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let tangent = axis_world.cross(reference).normalize();
    let bitangent = axis_world.cross(tangent).normalize();
    let mut points = Vec::with_capacity(SEGMENTS);
    for index in 0..SEGMENTS {
        let angle = index as f32 * std::f32::consts::TAU / SEGMENTS as f32;
        let circle_point = origin_world + tangent * angle.cos() + bitangent * angle.sin();
        let Some(projected) = project_to_rect(view_projection, circle_point, rect) else {
            return Vec::new();
        };
        points.push(projected);
    }
    let projected_radius = points
        .iter()
        .map(|point| point.distance(origin))
        .fold(0.0_f32, f32::max);
    if projected_radius <= f32::EPSILON {
        return Vec::new();
    }
    let scale = radius / projected_radius;
    points
        .into_iter()
        .map(|point| origin + (point - origin) * scale)
        .collect()
}

fn ring_pointer_distance(points: &[egui::Pos2], pointer: egui::Pos2) -> Option<(f32, egui::Vec2)> {
    if points.len() < 2 {
        return None;
    }
    let mut closest = (f32::INFINITY, egui::Vec2::RIGHT);
    for index in 0..points.len() {
        let start = points[index];
        let end = points[(index + 1) % points.len()];
        let segment = end - start;
        let length_sq = segment.length_sq();
        if length_sq <= f32::EPSILON {
            continue;
        }
        let amount = ((pointer - start).dot(segment) / length_sq).clamp(0.0, 1.0);
        let distance = pointer.distance(start + segment * amount);
        if distance < closest.0 {
            closest = (distance, segment.normalized());
        }
    }
    closest.0.is_finite().then_some(closest)
}

fn accumulate_gizmo_pointer_delta(
    amount: f32,
    pointer_delta: egui::Vec2,
    axis_direction: egui::Vec2,
    operation: GizmoOperation,
) -> f32 {
    let sensitivity = match operation {
        GizmoOperation::Rotate => 0.012,
        GizmoOperation::Translate | GizmoOperation::Scale => 0.015,
    };
    amount + pointer_delta.dot(axis_direction) * sensitivity
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
    fn pointer_motion_accumulates_along_the_visible_axis() {
        let direction = egui::Vec2::new(-1.0, 0.0);
        let first = accumulate_gizmo_pointer_delta(
            0.0,
            egui::Vec2::new(-10.0, 4.0),
            direction,
            GizmoOperation::Translate,
        );
        let second = accumulate_gizmo_pointer_delta(
            first,
            egui::Vec2::new(-5.0, -8.0),
            direction,
            GizmoOperation::Translate,
        );

        assert!((first - 0.15).abs() < 1.0e-6);
        assert!((second - 0.225).abs() < 1.0e-6);
    }

    #[test]
    fn rotation_motion_accumulates_as_the_ring_tangent_changes() {
        let first = accumulate_gizmo_pointer_delta(
            0.0,
            egui::Vec2::new(10.0, 0.0),
            egui::Vec2::RIGHT,
            GizmoOperation::Rotate,
        );
        let second = accumulate_gizmo_pointer_delta(
            first,
            egui::Vec2::new(0.0, 10.0),
            egui::Vec2::DOWN,
            GizmoOperation::Rotate,
        );

        assert!((first - 0.12).abs() < 1.0e-6);
        assert!((second - 0.24).abs() < 1.0e-6);
    }

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

fn spawn_viewport_renderer() -> (
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
    let viewport_tabs: Vec<_> = [EditorTab::Viewport3d, EditorTab::Viewport2d]
        .into_iter()
        .filter(|tab| tabs.contains(tab))
        .map(|tab| tiles.insert_pane(tab))
        .collect();
    let viewports = tiles.insert_tab_tile(viewport_tabs);
    let left = tiles.insert_pane(EditorTab::Hierarchy);
    let console = tiles.insert_pane(EditorTab::Console);
    let center = tiles.insert_vertical_tile(vec![viewports, console]);
    let inspector = tiles.insert_pane(EditorTab::Inspector);
    let root = tiles.insert_horizontal_tile(vec![left, center, inspector]);
    Tree::new("rustic-editor-dock", root, tiles)
}

fn viewport_scene(
    document: &AuthoringDocument,
    camera: EditorCamera,
    extent: [u32; 2],
    render_world_buffer: &mut RenderWorldBuffer,
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
    ViewportScene {
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
            let segments = 32;
            for index in 0..segments {
                let a = std::f32::consts::TAU * index as f32 / segments as f32;
                let b = std::f32::consts::TAU * (index + 1) as f32 / segments as f32;
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
            let segments = 32;
            for axis in 0..3 {
                for index in 0..segments {
                    let a = std::f32::consts::TAU * index as f32 / segments as f32;
                    let b = std::f32::consts::TAU * (index + 1) as f32 / segments as f32;
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

fn pick_scene(document: &AuthoringDocument) -> Vec<PickMesh> {
    document
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
        .collect()
}

fn project_to_rect(view_projection: Mat4, point: Vec3, rect: egui::Rect) -> Option<egui::Pos2> {
    let clip = view_projection * point.extend(1.0);
    if clip.w <= 0.0 || !clip.is_finite() {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(egui::pos2(
        rect.left() + ndc.x.midpoint(1.0) * rect.width(),
        rect.top() + (-ndc.y).midpoint(1.0) * rect.height(),
    ))
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

#[derive(Debug)]
struct ProjectDirectory {
    name: String,
    relative_path: PathBuf,
    directories: Vec<Self>,
    files: Vec<ProjectFile>,
}

fn render_scene_hierarchy(
    ui: &mut egui::Ui,
    snapshots: &[engine_world::EntitySnapshot],
    parent: Option<engine_core::EntityId>,
    selected: Option<engine_core::EntityId>,
) -> Option<engine_core::EntityId> {
    let mut clicked = None;
    for snapshot in snapshots.iter().filter(|item| item.parent == parent) {
        let label = snapshot
            .name
            .as_deref()
            .map_or_else(|| snapshot.id.to_string(), str::to_owned);
        let has_children = snapshots
            .iter()
            .any(|item| item.parent == Some(snapshot.id));
        if snapshot.folder || has_children {
            let title = if snapshot.folder {
                format!("📁 {label}")
            } else {
                label
            };
            let response =
                egui::CollapsingHeader::new(egui::RichText::new(title).color(ui_theme::TEXT))
                    .id_salt(("scene-entity", snapshot.id))
                    .default_open(snapshot.folder)
                    .show(ui, |ui| {
                        render_scene_hierarchy(ui, snapshots, Some(snapshot.id), selected)
                    });
            if response.header_response.clicked() {
                clicked = Some(snapshot.id);
            }
            if let Some(child) = response.body_returned.flatten() {
                clicked = Some(child);
            }
            if selected == Some(snapshot.id) {
                response.header_response.highlight();
            }
        } else if ui
            .selectable_label(
                selected == Some(snapshot.id),
                egui::RichText::new(label).color(ui_theme::TEXT),
            )
            .clicked()
        {
            clicked = Some(snapshot.id);
        }
    }
    clicked
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ProjectPanelMode {
    #[default]
    Scene,
    Explorer,
}

#[derive(Debug)]
struct ProjectFile {
    name: String,
    relative_path: PathBuf,
}

impl ProjectDirectory {
    fn file_count(&self) -> usize {
        self.files.len() + self.directories.iter().map(Self::file_count).sum::<usize>()
    }
}

fn render_project_root(
    ui: &mut egui::Ui,
    directory: &ProjectDirectory,
    filter: &str,
) -> Option<PathBuf> {
    let mut opened_file = None;
    for child in &directory.directories {
        if project_directory_matches(child, filter)
            && let Some(path) = render_project_directory(ui, child, filter, true)
        {
            opened_file = Some(path);
        }
    }
    for file in &directory.files {
        if project_file_matches(file, filter)
            && let Some(path) = render_project_file(ui, file)
        {
            opened_file = Some(path);
        }
    }
    opened_file
}

fn render_project_directory(
    ui: &mut egui::Ui,
    directory: &ProjectDirectory,
    filter: &str,
    default_open: bool,
) -> Option<PathBuf> {
    const FOLDER_TEXT: egui::Color32 = egui::Color32::from_rgb(207, 215, 225);
    const EMPTY_TEXT: egui::Color32 = egui::Color32::from_rgb(105, 115, 128);
    let is_empty = directory.directories.is_empty() && directory.files.is_empty();
    let text_color = if is_empty { EMPTY_TEXT } else { FOLDER_TEXT };
    let mut opened_script = None;
    let id = ui.make_persistent_id(("project-directory", &directory.relative_path));
    let header = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        default_open && !is_empty,
    )
    .show_header(ui, |ui| {
        ui.add(
            egui::Image::new(egui::include_image!(
                "../../../assets/icons/material-design/folder.svg"
            ))
            .fit_to_exact_size(egui::vec2(18.0, 18.0))
            .tint(text_color),
        );
        ui.label(egui::RichText::new(&directory.name).color(text_color));
        if is_empty {
            ui.label(egui::RichText::new("empty").small().color(EMPTY_TEXT));
        }
    });
    let (_, header_response, _) = header.body(|ui| {
        if !is_empty {
            for child in &directory.directories {
                if project_directory_matches(child, filter)
                    && let Some(path) =
                        render_project_directory(ui, child, filter, !filter.is_empty())
                {
                    opened_script = Some(path);
                }
            }
            for file in &directory.files {
                if project_file_matches(file, filter)
                    && let Some(path) = render_project_file(ui, file)
                {
                    opened_script = Some(path);
                }
            }
        }
    });
    if is_empty {
        header_response
            .response
            .on_hover_text("This folder is empty");
    }
    opened_script
}

fn render_project_file(ui: &mut egui::Ui, file: &ProjectFile) -> Option<PathBuf> {
    const FILE_TEXT: egui::Color32 = egui::Color32::from_rgb(190, 199, 211);
    let icon = project_file_icon(&file.relative_path);
    let response = ui.dnd_drag_source(
        egui::Id::new(("project-file", &file.relative_path)),
        file.relative_path.clone(),
        |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Image::new(icon)
                        .fit_to_exact_size(egui::vec2(18.0, 18.0))
                        .tint(FILE_TEXT),
                );
                ui.label(egui::RichText::new(&file.name).color(FILE_TEXT))
                    .on_hover_text(file.relative_path.display().to_string());
            });
        },
    );
    response
        .response
        .double_clicked()
        .then(|| file.relative_path.clone())
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
        Some("rscene") => {
            egui::include_image!("../../../assets/icons/material-design/map-outline.svg")
        }
        Some("png" | "jpg" | "jpeg" | "tga" | "hdr") => {
            egui::include_image!("../../../assets/icons/material-design/image-outline.svg")
        }
        Some("glb" | "gltf" | "obj") => {
            egui::include_image!("../../../assets/icons/material-design/cube-outline.svg")
        }
        Some("wav" | "ogg" | "mp3") => {
            egui::include_image!("../../../assets/icons/material-design/music-note-outline.svg")
        }
        Some("ron" | "toml" | "json") => {
            egui::include_image!("../../../assets/icons/material-design/file-cog-outline.svg")
        }
        _ => {
            egui::include_image!("../../../assets/icons/material-design/file-document-outline.svg")
        }
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
        ".git" | ".tools" | "target" | "cache" | "temp" | "builds" | "logs"
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
            b"-- Edit in your configured external editor.\nreturn {\n  on_start = function() rustic.log(\"info\", \"Behavior started\") end,\n  fixed_update = function(dt) end,\n  update = function(dt) end,\n  on_stop = function() rustic.log(\"info\", \"Behavior stopped\") end,\n}\n",
        ),
        ScriptLanguage::JavaScript => (
            "js",
            b"// Edit in your configured external editor.\nglobalThis.behavior = {\n  on_start() { rustic.log(\"info\", \"Behavior started\"); },\n  fixed_update(dt) {},\n  update(dt) {},\n  on_stop() { rustic.log(\"info\", \"Behavior stopped\"); },\n};\n",
        ),
        ScriptLanguage::Python => (
            "py",
            br#"import json, sys

class InstanceApi:
    def __init__(self): self.commands = []
    def add(self, source, parent=None):
        self.commands.append({"op": "add_instance", "source": source, "parent": parent})
    def clone(self, source, parent=None):
        self.commands.append({"op": "clone_instance", "source": source, "parent": parent})

instance = InstanceApi()

class RusticApi:
    def __init__(self): self.state = {}; self.commands = instance.commands
    def entity_id(self): return self.state["entity_id"]
    def delta_time(self): return self.state["delta_time"]
    def fixed_delta_time(self): return self.state["fixed_delta_time"]
    def get_translation(self): return self.state["translation"][:]
    def set_translation(self, x, y, z): self.commands.append({"op":"set_translation","value":[x,y,z]})
    def get_property(self, name): return self.state["properties"].get(name)
    def set_property(self, name, value): self.commands.append({"op":"set_property","name":name,"value":value})
    def get_attribute(self, name): return self.state["attributes"].get(name)
    GetAttribute = get_attribute
    def edit_attribute(self, name, value): self.commands.append({"op":"edit_attribute","name":name,"value":value})
    EditAttribute = edit_attribute
    def input(self, name): return self.state.get("actions", {}).get(name, {"pressed":False,"released":False,"held":False,"axis":0})
    def key(self, name): return self.state["keys"].get(name, {"pressed":False,"released":False,"held":False,"axis":0})
    def key_events(self): return self.state["key_events"][:]
    def any_key_pressed(self): return self.state["any_key_pressed"]
    def log(self, level, message): self.commands.append({"op":"log","level":str(level),"message":str(message)})
    def set_enabled(self, enabled): self.commands.append({"op":"set_enabled","enabled":bool(enabled)})

class SceneApi:
    def __init__(self): self.state = {}
    def Find(self, path): return self.state.get(path)
    def List(self, path="Game.scene"):
        prefix = "" if path in ("", "Game.scene") else path.rstrip("./") + "."
        return [entity for name, entity in self.state.items() if not prefix or name.startswith(prefix)]

class GameApi:
    def setCurrentCamera(self, source):
        instance.commands.append({"op":"set_current_camera","source":source})
    set_current_camera = setCurrentCamera
rustic = RusticApi()
Game = GameApi()
Game.scene = SceneApi()

for line in sys.stdin:
    request = json.loads(line)
    instance.commands = []
    rustic.commands = instance.commands
    rustic.state = request
    Game.scene.state = request.get("scene_paths", {})
    if request["callback"] == "on_start":
        instance.commands.append({"op": "log", "level": "info", "message": "Behavior started"})
    print(json.dumps({"format_version": 1, "commands": instance.commands}), flush=True)
"#,
        ),
        ScriptLanguage::CSharp => (
            "cs",
            br#"using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;

var instance = new InstanceApi();
var rustic = new RusticApi(instance.Commands);
var Game = new GameApi(instance.Commands);
string? line;
while ((line = Console.ReadLine()) is not null) {
    instance.Commands.Clear();
    rustic.State = JsonDocument.Parse(line).RootElement.Clone();
    Game.scene.State = rustic.State.GetProperty("scene_paths");
    Console.WriteLine(JsonSerializer.Serialize(new { format_version = 1, commands = instance.Commands }));
}

sealed class InstanceApi {
    public List<object> Commands { get; } = new();
    public void Add(string source, string? parent = null) => Commands.Add(new { op = "add_instance", source, parent });
    public void Clone(string source, string? parent = null) => Commands.Add(new { op = "clone_instance", source, parent });
    public void add(string source, string? parent = null) => Add(source, parent);
    public void clone(string source, string? parent = null) => Clone(source, parent);
}

sealed class RusticApi {
    public JsonElement State { get; set; }
    public List<object> Commands { get; }
    public RusticApi(List<object> commands) => Commands = commands;
    public string entity_id() => State.GetProperty("entity_id").GetString()!;
    public double delta_time() => State.GetProperty("delta_time").GetDouble();
    public double fixed_delta_time() => State.GetProperty("fixed_delta_time").GetDouble();
    public double[] get_translation() => State.GetProperty("translation").EnumerateArray().Select(x => x.GetDouble()).ToArray();
    public void set_translation(double x,double y,double z) => Commands.Add(new { op="set_translation", value=new[]{x,y,z} });
    public JsonElement get_property(string name) => State.GetProperty("properties").GetProperty(name);
    public void set_property(string name, object value) => Commands.Add(new { op="set_property", name, value });
    public JsonElement GetAttribute(string name) => State.GetProperty("attributes").GetProperty(name);
    public void EditAttribute(string name, object value) => Commands.Add(new { op="edit_attribute", name, value });
    public JsonElement get_attribute(string name) => GetAttribute(name);
    public void edit_attribute(string name, object value) => EditAttribute(name, value);
    public JsonElement input(string name) => State.GetProperty("actions").TryGetProperty(name, out var value) ? value : default;
    public JsonElement key(string name) => State.GetProperty("keys").TryGetProperty(name, out var value) ? value : default;
    public IEnumerable<JsonElement> key_events() => State.GetProperty("key_events").EnumerateArray();
    public bool any_key_pressed() => State.GetProperty("any_key_pressed").GetBoolean();
    public void log(string level,string message) => Commands.Add(new { op="log", level, message });
    public void set_enabled(bool enabled) => Commands.Add(new { op="set_enabled", enabled });
}

sealed class SceneApi {
    public JsonElement State { get; set; }
    public string? Find(string path) => State.TryGetProperty(path, out var value) ? value.GetString() : null;
    public IEnumerable<string> List(string path="Game.scene") => State.EnumerateObject().Where(x => path=="Game.scene" || x.Name.StartsWith(path+".")).Select(x => x.Value.GetString()!);
}
sealed class GameApi {
    public SceneApi scene { get; } = new();
    private readonly List<object> commands;
    public GameApi(List<object> commands) => this.commands = commands;
    public void SetCurrentCamera(string source) => commands.Add(new { op="set_current_camera", source });
    public void setCurrentCamera(string source) => SetCurrentCamera(source);
}
"#,
        ),
        ScriptLanguage::C => (
            "c",
            br#"#include <stdio.h>
#include <string.h>

static char instance_commands[1048576];
static void instance_command(const char *op, const char *source, const char *parent) {
    snprintf(instance_commands, sizeof instance_commands,
        "{\"op\":\"%s\",\"source\":\"%s\",\"parent\":%s}",
        op, source, parent ? parent : "null");
}
static void instance_add(const char *source, const char *parent) { instance_command("add_instance", source, parent); }
static void instance_clone(const char *source, const char *parent) { instance_command("clone_instance", source, parent); }
static void Game_setCurrentCamera(const char *source) {
    char escaped[1048500];
    size_t n = 0;
    for (const unsigned char *p = (const unsigned char *)source; *p; ++p) {
        if (n + 6 >= sizeof escaped) { fputs("camera path too long\n", stderr); return; }
        if (*p < 32 || *p == '"' || *p == '\\') {
            n += (size_t)snprintf(escaped + n, sizeof escaped - n, "\\u%04x", *p);
        } else escaped[n++] = (char)*p;
    }
    escaped[n] = '\0';
    instance_command("set_current_camera", escaped, NULL);
}

int main(void) {
    char request[1048577];
    while (fgets(request, sizeof request, stdin)) {
        instance_commands[0] = '\0';
        printf("{\"format_version\":1,\"commands\":[%s]}\n", instance_commands);
        fflush(stdout);
    }
    return 0;
}
"#,
        ),
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
        ScriptLanguage::Java => (
            "java",
            br#"class RusticBehavior {
    static final class InstanceApi {
        final java.util.List<String> commands = new java.util.ArrayList<>();
        void add(String source) { add(source, null); }
        void add(String source, String parent) { push("add_instance", source, parent); }
        void clone(String source) { clone(source, null); }
        void clone(String source, String parent) { push("clone_instance", source, parent); }
        void push(String op, String source, String parent) {
            commands.add("{\"op\":\""+op+"\",\"source\":\""+source+"\",\"parent\":"+(parent==null?"null":"\""+parent+"\"")+"}");
        }
    }
    static final class GameApi {
        final InstanceApi instance;
        GameApi(InstanceApi instance) { this.instance = instance; }
        void setCurrentCamera(String source) {
            var escaped = new StringBuilder();
            for (char c : source.toCharArray()) {
                if (c < 32 || c == '"' || c == '\\') escaped.append(String.format("\\u%04x", (int)c));
                else escaped.append(c);
            }
            instance.push("set_current_camera", escaped.toString(), null);
        }
    }
    public static void main(String[] args) throws Exception {
        var instance = new InstanceApi();
        var Game = new GameApi(instance);
        var input = new java.io.BufferedReader(new java.io.InputStreamReader(System.in));
        while (input.readLine() != null) {
            instance.commands.clear();
            System.out.println("{\"format_version\":1,\"commands\":["+String.join(",",instance.commands)+"]}");
            System.out.flush();
        }
    }
}
"#,
        ),
        ScriptLanguage::Php => (
            "php",
            br#"<?php
final class InstanceApi {
    public array $commands = [];
    public function add(string $source, ?string $parent = null): void {
        $this->commands[] = ["op" => "add_instance", "source" => $source, "parent" => $parent];
    }
    public function clone(string $source, ?string $parent = null): void {
        $this->commands[] = ["op" => "clone_instance", "source" => $source, "parent" => $parent];
    }
}
final class RusticApi {
    public array $state = [];
    public function __construct(public InstanceApi $instance) {}
    public function entity_id(): string { return $this->state["entity_id"]; }
    public function delta_time(): float { return $this->state["delta_time"]; }
    public function fixed_delta_time(): float { return $this->state["fixed_delta_time"]; }
    public function get_translation(): array { return $this->state["translation"]; }
    public function set_translation(float $x,float $y,float $z): void { $this->instance->commands[]=["op"=>"set_translation","value"=>[$x,$y,$z]]; }
    public function get_property(string $name): mixed { return $this->state["properties"][$name] ?? null; }
    public function set_property(string $name,mixed $value): void { $this->instance->commands[]=["op"=>"set_property","name"=>$name,"value"=>$value]; }
    public function GetAttribute(string $name): mixed { return $this->state["attributes"][$name] ?? null; }
    public function get_attribute(string $name): mixed { return $this->GetAttribute($name); }
    public function EditAttribute(string $name,mixed $value): void { $this->instance->commands[]=["op"=>"edit_attribute","name"=>$name,"value"=>$value]; }
    public function edit_attribute(string $name,mixed $value): void { $this->EditAttribute($name,$value); }
    public function key(string $name): array { return $this->state["keys"][$name] ?? ["pressed"=>false,"released"=>false,"held"=>false,"axis"=>0]; }
    public function key_events(): array { return $this->state["key_events"]; }
    public function any_key_pressed(): bool { return $this->state["any_key_pressed"]; }
    public function log(string $level,string $message): void { $this->instance->commands[]=["op"=>"log","level"=>$level,"message"=>$message]; }
    public function set_enabled(bool $enabled): void { $this->instance->commands[]=["op"=>"set_enabled","enabled"=>$enabled]; }
}
final class SceneApi {
    public array $state=[];
    public function Find(string $path): ?string { return $this->state[$path] ?? null; }
    public function List(string $path="Game.scene"): array { return array_values(array_filter($this->state, fn($id,$name)=>$path==="Game.scene" || str_starts_with($name,$path."."), ARRAY_FILTER_USE_BOTH)); }
}
final class GameApi {
    public SceneApi $scene;
    public function __construct(private InstanceApi $instance){ $this->scene=new SceneApi(); }
    public function setCurrentCamera(string $source): void { $this->instance->commands[]=["op"=>"set_current_camera","source"=>$source]; }
}
$instance = new InstanceApi();
$rustic = new RusticApi($instance);
$Game = new GameApi($instance);
while (($line = fgets(STDIN)) !== false) {
    $request = json_decode($line, true, flags: JSON_THROW_ON_ERROR);
    $instance->commands = [];
    $rustic->state = $request;
    $Game->scene->state = $request["scene_paths"] ?? [];
    // Example: $instance->clone("assets/models/chair.obj");
    echo json_encode(["format_version" => 1, "commands" => $instance->commands], JSON_THROW_ON_ERROR), PHP_EOL;
    flush();
}
"#,
        ),
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
        ScriptLanguage::Luau => (
            "luau",
            br#"-- Luau uses the isolated CLI host protocol.
local commands = {}
local Game = {}
function Game.setCurrentCamera(source)
    if type(source) == "table" then source = source[1] end
    assert(type(source) == "string", "expected a camera path or {cameraPath}")
    local escaped = string.gsub(source, '[%c\\"]', function(c)
        return string.format("\\u%04x", string.byte(c))
    end)
    table.insert(commands, '{"op":"set_current_camera","source":"' .. escaped .. '"}')
end
-- Example: Game.setCurrentCamera({"Game.scene.Camera"})
print('{"format_version":1,"commands":[' .. table.concat(commands, ",") .. ']}')
"#,
        ),
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
    let mut used_languages = std::fs::read(path)
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
    if let Ok(settings) = GameSettings::load(project_root)
        && let Some(language) = ScriptLanguage::from_path(&settings.entry_script)
    {
        used_languages.push(language);
    }
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
                    .range(0.01..=100000.0)
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
            .range((*near + 0.001)..=1000000.0)
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
                .range(0.01..=100000.0)
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
