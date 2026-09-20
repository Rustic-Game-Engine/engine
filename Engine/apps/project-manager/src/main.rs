mod model;
mod settings;
mod ui_theme;

use eframe::egui;
use engine_core::logging::{AsyncLogger, LogConfig, LogStream, Severity};
use engine_core::{
    ApplicationIdentity, ApplicationRole, EngineVersion, run_headless_smoke_from_arguments,
};
use engine_platform::{
    ConfigLoad, ConfigStore, NativePlatformPaths, NativeProcessLauncher, PlatformPaths,
    ProcessLauncher, ProcessRequest, StartupMarker,
};
use engine_project::{CatalogSort, Project, ProjectRecord, ProjectTemplate};
use engine_scripting::{ScriptLanguage, probe_language_toolchain};
use model::ProjectManagerModel;
use settings::{LauncherSettings, ThemePreference};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rustic-project-manager: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let identity =
        ApplicationIdentity::new("rustic-project-manager", ApplicationRole::ProjectManager);
    if run_headless_smoke_from_arguments(&identity, std::env::args_os().skip(1))
        .map_err(|error| format!("headless smoke failed: {error}"))?
    {
        return Ok(());
    }
    let platform_paths = NativePlatformPaths::discover()
        .map_err(|error| format!("could not discover platform paths: {error}"))?;
    let app = ProjectManagerApp::new(platform_paths)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Rustic Game Engine — Projects")
            .with_inner_size([1_080.0, 700.0])
            .with_min_inner_size([760.0, 480.0])
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "org.rusticengine.project-manager",
        options,
        Box::new(move |_context| Ok(Box::new(app))),
    )
    .map_err(|error| format!("native launcher failed: {error}"))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectSort {
    Recent,
    Name,
    Location,
}

impl ProjectSort {
    const ALL: [Self; 3] = [Self::Recent, Self::Name, Self::Location];

    const fn label(self) -> &'static str {
        match self {
            Self::Recent => "Recently opened",
            Self::Name => "Name",
            Self::Location => "Location",
        }
    }

    const fn catalog(self) -> CatalogSort {
        match self {
            Self::Recent => CatalogSort::Recent,
            Self::Name => CatalogSort::Name,
            Self::Location => CatalogSort::Location,
        }
    }
}

struct CreateProjectForm {
    visible: bool,
    name: String,
    parent: String,
    template: ProjectTemplate,
}

impl Default for CreateProjectForm {
    fn default() -> Self {
        Self {
            visible: false,
            name: "My Game".to_owned(),
            parent: String::new(),
            template: ProjectTemplate::Empty3d,
        }
    }
}

#[derive(Default)]
struct DialogVisibility {
    import: bool,
    settings: bool,
    diagnostics: bool,
}

struct ProjectManagerApp {
    platform_paths: NativePlatformPaths,
    process_launcher: NativeProcessLauncher,
    model: ProjectManagerModel,
    logger: AsyncLogger,
    startup_marker: Option<StartupMarker>,
    previous_unclean_exit: bool,
    settings: LauncherSettings,
    search: String,
    sort: ProjectSort,
    create: CreateProjectForm,
    dialogs: DialogVisibility,
    import_path: String,
    ui_status: Option<String>,
}

impl ProjectManagerApp {
    fn new(platform_paths: NativePlatformPaths) -> Result<Self, String> {
        let logger = AsyncLogger::start_resilient(LogConfig {
            directory: platform_paths.launcher_log_directory().to_path_buf(),
            stream: LogStream::Editor,
            ..LogConfig::default()
        })
        .map_err(|error| format!("could not start launcher diagnostics: {error}"))?;
        let (startup_marker, previous_unclean_exit, marker_warning) =
            match StartupMarker::begin(platform_paths.launcher_startup_marker(), "project_manager")
            {
                Ok(marker) => {
                    let previous = marker.previous_unclean_exit();
                    (Some(marker), previous, None)
                }
                Err(error) => (
                    None,
                    false,
                    Some(format!("Startup marker unavailable: {error}")),
                ),
            };
        let (settings, settings_warning) = match ConfigStore::default()
            .load::<LauncherSettings>(platform_paths.launcher_settings_file())
        {
            Ok(load) => {
                let warning = match &load {
                    ConfigLoad::Migrated { from_version, .. } => Some(format!(
                        "Migrated launcher settings from version {from_version}."
                    )),
                    ConfigLoad::RecoveredBackup { quarantine, .. } => Some(format!(
                        "Recovered launcher settings; quarantined {}.",
                        quarantine.display()
                    )),
                    ConfigLoad::Missing | ConfigLoad::Current(_) => None,
                };
                (load.value_or_else(LauncherSettings::default), warning)
            }
            Err(error) => (
                LauncherSettings::default(),
                Some(format!("Using default launcher settings: {error}")),
            ),
        };
        let model = ProjectManagerModel::load(
            platform_paths.project_registry_file().to_path_buf(),
            platform_paths.thumbnail_cache_directory().to_path_buf(),
            usize::from(settings.background_workers),
            settings.thumbnail_cache_enabled,
        );
        let create = CreateProjectForm {
            parent: platform_paths
                .default_project_parent()
                .display()
                .to_string(),
            ..CreateProjectForm::default()
        };
        let ui_status = marker_warning.or(settings_warning).or_else(|| {
            previous_unclean_exit.then(|| {
                "The previous launcher session did not shut down cleanly; data was recovered."
                    .to_owned()
            })
        });
        let _ = logger.log(
            Severity::Info,
            "launcher",
            None,
            format!(
                "Project Manager started with engine {}",
                EngineVersion::CURRENT
            ),
        );
        Ok(Self {
            platform_paths,
            process_launcher: NativeProcessLauncher,
            model,
            logger,
            startup_marker,
            previous_unclean_exit,
            settings,
            search: String::new(),
            sort: ProjectSort::Recent,
            create,
            dialogs: DialogVisibility::default(),
            import_path: String::new(),
            ui_status,
        })
    }

    fn launch_project(&mut self, project: &Project) {
        let editor = match self.platform_paths.sibling_executable("rustic-editor") {
            Ok(editor) => editor,
            Err(error) => {
                self.ui_status = Some(format!("Could not locate editor: {error}"));
                return;
            }
        };
        let request = ProcessRequest::new(&editor)
            .argument("--project")
            .argument(project.root().as_os_str())
            .working_directory(project.root());
        match self.process_launcher.spawn(&request) {
            Ok(process) => {
                self.ui_status = Some(format!("Started editor process {}.", process.id()));
                let _ = self.logger.log(
                    Severity::Info,
                    "launcher",
                    None,
                    format!(
                        "Started editor {} for {}",
                        process.id(),
                        project.root().display()
                    ),
                );
            }
            Err(error) => {
                self.ui_status = Some(format!(
                    "Could not launch editor {}: {error}",
                    editor.display()
                ));
            }
        }
    }

    fn save_settings(&mut self) {
        match ConfigStore::default()
            .save(self.platform_paths.launcher_settings_file(), &self.settings)
        {
            Ok(()) => self.ui_status = Some("Launcher settings saved.".to_owned()),
            Err(error) => self.ui_status = Some(format!("Could not save settings: {error}")),
        }
    }

    fn current_status(&self) -> &str {
        self.ui_status
            .as_deref()
            .or_else(|| self.model.status())
            .unwrap_or("Ready")
    }

    fn clear_status(&mut self) {
        self.ui_status = None;
        self.model.clear_status();
    }
}

impl Drop for ProjectManagerApp {
    fn drop(&mut self) {
        let _ = self.logger.log(
            Severity::Info,
            "launcher",
            None,
            "Project Manager shut down cleanly",
        );
        let _ = self.logger.shutdown();
        if let Some(marker) = &mut self.startup_marker {
            let _ = marker.clear();
        }
    }
}

impl eframe::App for ProjectManagerApp {
    #[allow(
        clippy::too_many_lines,
        reason = "panel and modal declaration order is the launcher shell's visual layout"
    )]
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.model.poll();
        if let Some(project) = self.model.take_project_to_launch() {
            self.launch_project(&project);
        }
        let context = ui.ctx().clone();
        context.request_repaint_after(std::time::Duration::from_millis(50));
        let dark = match self.settings.theme {
            ThemePreference::System => context.system_theme() != Some(egui::Theme::Light),
            ThemePreference::Dark => true,
            ThemePreference::Light => false,
        };
        ui_theme::apply(&context, dark);

        egui::Panel::top("project_manager_header")
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("Projects");
                        ui.label(
                            egui::RichText::new("RUSTIC GAME ENGINE")
                                .size(11.0)
                                .strong()
                                .color(ui_theme::MUTED),
                        );
                    });
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("v{}", EngineVersion::CURRENT))
                            .small()
                            .color(ui_theme::MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(ui_theme::primary_button("＋  New project"))
                            .clicked()
                            && self.model.catalog_writable()
                        {
                            self.create.visible = true;
                        }
                        if ui.button("Import…").clicked() {
                            self.dialogs.import = true;
                        }
                    });
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search by project name or location…")
                            .desired_width(360.0),
                    );
                    egui::ComboBox::from_id_salt("project_sort")
                        .selected_text(self.sort.label())
                        .show_ui(ui, |ui| {
                            for sort in ProjectSort::ALL {
                                ui.selectable_value(&mut self.sort, sort, sort.label());
                            }
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Diagnostics").clicked() {
                            self.dialogs.diagnostics = true;
                        }
                        if ui.button("Settings").clicked() {
                            self.dialogs.settings = true;
                        }
                    });
                });
            });

        egui::Panel::bottom("project_manager_status")
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(18, 8))
                    .stroke(egui::Stroke::new(1.0, ui_theme::BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.model.pending_jobs() > 0 {
                        ui.spinner();
                        ui.label(format!("{} background job(s)", self.model.pending_jobs()));
                        ui.separator();
                    }
                    ui.label(self.current_status());
                    if (self.ui_status.is_some() || self.model.status().is_some())
                        && ui.small_button("Clear").clicked()
                    {
                        self.clear_status();
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(24)))
            .show(ui, |ui| {
                let projects = self
                    .model
                    .visible_records(&self.search, self.sort.catalog());
                if projects.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(110.0);
                        ui.heading(if self.search.trim().is_empty() {
                            "Your projects will appear here"
                        } else {
                            "No matching projects"
                        });
                        ui.label(
                            egui::RichText::new(if self.search.trim().is_empty() {
                                "Create a new project or import an existing project.engine folder."
                            } else {
                                "Try another project name or location."
                            })
                            .color(ui_theme::MUTED),
                        );
                        ui.add_space(14.0);
                        if self.search.trim().is_empty()
                            && ui
                                .add(ui_theme::primary_button("Create your first project"))
                                .clicked()
                        {
                            self.create.visible = true;
                        }
                    });
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for record in projects {
                            self.project_card(ui, &record);
                            ui.add_space(8.0);
                        }
                    });
                }
            });
        self.create_dialog(ui);
        self.import_dialog(ui);
        self.settings_dialog(ui);
        self.diagnostics_dialog(ui);
    }
}

impl ProjectManagerApp {
    fn project_card(&mut self, ui: &mut egui::Ui, record: &ProjectRecord) {
        egui::Frame::new()
            .fill(ui_theme::SURFACE)
            .stroke(egui::Stroke::new(1.0, ui_theme::BORDER))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(69, 48, 34))
                        .corner_radius(7.0)
                        .show(ui, |ui| {
                            ui.allocate_ui(egui::vec2(68.0, 68.0), |ui| {
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        egui::RichText::new("R")
                                            .size(28.0)
                                            .strong()
                                            .color(ui_theme::ACCENT),
                                    );
                                })
                            });
                        });
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(&record.name)
                                .size(18.0)
                                .strong()
                                .color(ui_theme::TEXT),
                        );
                        ui.label(
                            egui::RichText::new(record.path.display().to_string())
                                .color(ui_theme::MUTED),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "{:?}   ·   Engine {}{}",
                                record.template,
                                record.engine_version.as_deref().unwrap_or("current"),
                                if record.thumbnail.is_some() {
                                    "  ·  thumbnail cached"
                                } else {
                                    ""
                                }
                            ))
                            .small()
                            .color(ui_theme::MUTED),
                        );
                        if !record.available {
                            ui.colored_label(ui_theme::WARNING, "Project unavailable or invalid");
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("Remove")
                            .on_hover_text("Remove from this list; project files stay on disk")
                            .clicked()
                        {
                            self.model.remove(record.id);
                        }
                        if ui
                            .add_enabled(record.available, ui_theme::primary_button("Open project"))
                            .clicked()
                        {
                            self.model.request_open(record.id);
                        }
                    });
                });
            });
    }

    fn create_dialog(&mut self, ui: &mut egui::Ui) {
        if !self.create.visible {
            return;
        }
        let mut visible = true;
        egui::Window::new("Create project")
            .collapsible(false)
            .resizable(false)
            .open(&mut visible)
            .show(ui, |ui| {
                ui.label("Project name");
                ui.text_edit_singleline(&mut self.create.name);
                ui.label("Parent folder");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.create.parent).desired_width(390.0),
                    );
                    if ui.button("Browse…").clicked()
                        && let Some(parent) = rfd::FileDialog::new()
                            .set_title("Choose parent folder")
                            .pick_folder()
                    {
                        self.create.parent = parent.display().to_string();
                    }
                });
                egui::ComboBox::from_label("Template")
                    .selected_text(format!("{:?}", self.create.template))
                    .show_ui(ui, |ui| {
                        for template in all_templates() {
                            ui.selectable_value(
                                &mut self.create.template,
                                template,
                                format!("{template:?}"),
                            );
                        }
                    });
                ui.label(self.create.template.brief());
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.create.visible = false;
                    }
                    let valid = !self.create.name.trim().is_empty()
                        && !self.create.parent.trim().is_empty();
                    if ui
                        .add_enabled(valid, ui_theme::primary_button("Create project"))
                        .clicked()
                    {
                        let name = self.create.name.trim().to_owned();
                        let root = PathBuf::from(self.create.parent.trim()).join(&name);
                        self.model.request_create(root, name, self.create.template);
                        self.create.visible = false;
                    }
                });
            });
        self.create.visible &= visible;
    }

    fn import_dialog(&mut self, ui: &mut egui::Ui) {
        if !self.dialogs.import {
            return;
        }
        let mut visible = true;
        egui::Window::new("Import project")
            .collapsible(false)
            .resizable(false)
            .open(&mut visible)
            .show(ui, |ui| {
                ui.label("Project folder containing project.engine");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.import_path).desired_width(390.0));
                    if ui.button("Browse…").clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        self.import_path = path.display().to_string();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.dialogs.import = false;
                    }
                    if ui
                        .add_enabled(
                            !self.import_path.trim().is_empty(),
                            ui_theme::primary_button("Import project"),
                        )
                        .clicked()
                    {
                        self.model
                            .request_import(PathBuf::from(self.import_path.trim()));
                        self.dialogs.import = false;
                    }
                });
            });
        self.dialogs.import &= visible;
    }

    fn settings_dialog(&mut self, ui: &mut egui::Ui) {
        if !self.dialogs.settings {
            return;
        }
        let mut visible = true;
        egui::Window::new("Application settings")
            .default_width(480.0)
            .open(&mut visible)
            .show(ui, |ui| {
                ui.heading("Project manager");
                ui.label("Theme");
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.settings.theme,
                        ThemePreference::System,
                        "System",
                    );
                    ui.selectable_value(&mut self.settings.theme, ThemePreference::Dark, "Dark");
                    ui.selectable_value(&mut self.settings.theme, ThemePreference::Light, "Light");
                });
                ui.add(
                    egui::Slider::new(&mut self.settings.background_workers, 1..=8)
                        .text("Background workers (next launch)"),
                );
                ui.checkbox(
                    &mut self.settings.thumbnail_cache_enabled,
                    "Enable thumbnail cache",
                );
                if ui.button("Save settings").clicked() {
                    self.save_settings();
                }
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.heading("Gameplay dependencies");
                ui.label(
                    egui::RichText::new(
                        "Manage optional language toolchains used by project scripts.",
                    )
                    .color(ui_theme::MUTED),
                );
                for language in optional_toolchain_languages() {
                    let availability = probe_language_toolchain(language);
                    ui.horizontal(|ui| {
                        ui.label(language.display_name());
                        if availability.available {
                            ui.colored_label(
                                ui_theme::SUCCESS,
                                availability.version.as_deref().unwrap_or("Installed"),
                            );
                        } else {
                            ui.colored_label(ui_theme::WARNING, "Not installed");
                            if ui.button("Install").clicked() {
                                self.ui_status =
                                    Some(match launch_toolchain_manager(&[language]) {
                                        Ok(()) => format!(
                                            "Opened installer for {}.",
                                            language.display_name()
                                        ),
                                        Err(error) => error,
                                    });
                            }
                        }
                    });
                }
                if ui.button("Edit installed dependencies…").clicked() {
                    self.ui_status = Some(match launch_toolchain_manager(&[]) {
                        Ok(()) => "Opened the Language Toolchain Manager.".to_owned(),
                        Err(error) => error,
                    });
                }
            });
        self.dialogs.settings &= visible;
    }

    fn diagnostics_dialog(&mut self, ui: &mut egui::Ui) {
        if !self.dialogs.diagnostics {
            return;
        }
        let mut visible = true;
        egui::Window::new("Launcher diagnostics")
            .open(&mut visible)
            .show(ui, |ui| {
                let logger = self.logger.stats();
                let jobs = self.model.scheduler_stats();
                ui.monospace(format!("Engine: {}", EngineVersion::CURRENT));
                ui.monospace(format!(
                    "Catalog: {}",
                    self.platform_paths.project_registry_file().display()
                ));
                ui.monospace(format!(
                    "Logs: {}",
                    self.platform_paths.launcher_log_directory().display()
                ));
                ui.label(format!(
                    "Previous unclean exit: {}",
                    self.previous_unclean_exit
                ));
                ui.label(format!(
                    "Log sink available: {} · dropped: {} · I/O errors: {}",
                    logger.sink_available, logger.dropped, logger.io_errors
                ));
                ui.label(format!(
                    "Jobs queued: {} · active: {} · completed: {} · panics: {}",
                    jobs.queued, jobs.active, jobs.completed, jobs.panicked
                ));
                if ui.button("Run background responsiveness check").clicked() {
                    self.model.request_synthetic_diagnostic();
                }
            });
        self.dialogs.diagnostics &= visible;
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
        .map_err(|error| format!("Could not open the dependency installer: {error}"))
}

const fn all_templates() -> [ProjectTemplate; 8] {
    [
        ProjectTemplate::Empty3d,
        ProjectTemplate::Empty2d,
        ProjectTemplate::FirstPerson,
        ProjectTemplate::ThirdPerson,
        ProjectTemplate::Platformer,
        ProjectTemplate::TopDown,
        ProjectTemplate::UiApplication,
        ProjectTemplate::Blank,
    ]
}
