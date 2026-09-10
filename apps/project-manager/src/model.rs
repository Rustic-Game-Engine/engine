use engine_core::{JobPriority, JobScheduler};
use engine_project::{
    CatalogLoadStatus, CatalogSort, Project, ProjectCatalog, ProjectId, ProjectRecord,
    ProjectTemplate, ThumbnailCache,
};
use std::path::PathBuf;
use std::sync::Arc;

enum ModelEvent {
    Scanned {
        id: ProjectId,
        project: Result<Project, String>,
    },
    Created(Result<Project, String>),
    Imported(Result<Project, String>),
    Opened(Result<Project, String>),
    Saved(Result<(), String>),
    Thumbnail {
        id: ProjectId,
        reference: Result<engine_project::ThumbnailRef, String>,
    },
    SyntheticComplete,
}

struct ModelCapabilities {
    catalog_writable: bool,
    thumbnails_enabled: bool,
}

#[derive(Default)]
struct SaveState {
    in_flight: bool,
    dirty: bool,
}

/// Testable launcher state. Native widgets only issue commands and render snapshots.
pub struct ProjectManagerModel {
    catalog_path: PathBuf,
    catalog: ProjectCatalog,
    capabilities: ModelCapabilities,
    scheduler: JobScheduler,
    events_tx: crossbeam_channel::Sender<ModelEvent>,
    events_rx: crossbeam_channel::Receiver<ModelEvent>,
    thumbnail_cache: Arc<ThumbnailCache>,
    pending_jobs: usize,
    status: Option<String>,
    project_to_launch: Option<Project>,
    save: SaveState,
}

impl ProjectManagerModel {
    pub fn load(
        catalog_path: PathBuf,
        thumbnail_root: PathBuf,
        worker_count: usize,
        thumbnail_enabled: bool,
    ) -> Self {
        let (catalog, status, catalog_writable) = match ProjectCatalog::load(&catalog_path) {
            Ok(load) => {
                let status = match load.status {
                    CatalogLoadStatus::Missing | CatalogLoadStatus::Current => None,
                    CatalogLoadStatus::Migrated { from_version } => Some(format!(
                        "Migrated project catalog from version {from_version}."
                    )),
                    CatalogLoadStatus::RecoveredBackup { quarantine } => Some(format!(
                        "Recovered the project catalog; corrupt bytes are in {}.",
                        quarantine.display()
                    )),
                    CatalogLoadStatus::RecoveredEmpty { quarantine } => Some(format!(
                        "Reset a corrupt project catalog; quarantined {}.",
                        quarantine.display()
                    )),
                };
                (load.catalog, status, true)
            }
            Err(error) => (
                ProjectCatalog::default(),
                Some(format!(
                    "Catalog opened read-only because it could not be loaded safely: {error}"
                )),
                false,
            ),
        };
        let scheduler = JobScheduler::new(worker_count.max(1), 256);
        let (events_tx, events_rx) = crossbeam_channel::unbounded();
        let mut model = Self {
            catalog_path,
            catalog,
            capabilities: ModelCapabilities {
                catalog_writable,
                thumbnails_enabled: thumbnail_enabled,
            },
            scheduler,
            events_tx,
            events_rx,
            thumbnail_cache: Arc::new(ThumbnailCache::new(thumbnail_root)),
            pending_jobs: 0,
            status,
            project_to_launch: None,
            save: SaveState::default(),
        };
        model.queue_full_scan();
        model
    }

    pub fn visible_records(&self, search: &str, sort: CatalogSort) -> Vec<ProjectRecord> {
        self.catalog
            .query(search, sort)
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn request_create(&mut self, root: PathBuf, name: String, template: ProjectTemplate) {
        let sender = self.events_tx.clone();
        self.submit(JobPriority::High, move || {
            let result = Project::create(root, name, template).map_err(|error| error.to_string());
            let _ = sender.send(ModelEvent::Created(result));
        });
    }

    pub fn request_import(&mut self, path: PathBuf) {
        let sender = self.events_tx.clone();
        self.submit(JobPriority::High, move || {
            let result = Project::open(path).map_err(|error| error.to_string());
            let _ = sender.send(ModelEvent::Imported(result));
        });
    }

    pub fn request_open(&mut self, id: ProjectId) {
        let Some(path) = self
            .catalog
            .projects
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.path.clone())
        else {
            return;
        };
        let sender = self.events_tx.clone();
        self.submit(JobPriority::High, move || {
            let result = Project::open(path).map_err(|error| error.to_string());
            let _ = sender.send(ModelEvent::Opened(result));
        });
    }

    pub fn remove(&mut self, id: ProjectId) {
        if !self.capabilities.catalog_writable {
            self.status = Some("Catalog is read-only; remove was not persisted.".to_owned());
            return;
        }
        if self.catalog.remove(id) {
            self.persist_async();
            self.status = Some("Removed from launcher. Project files were not deleted.".to_owned());
        }
    }

    pub fn request_synthetic_diagnostic(&mut self) {
        let sender = self.events_tx.clone();
        self.submit(JobPriority::Low, move || {
            let mut checksum = 0_u64;
            for value in 0..250_000_u64 {
                checksum = checksum.wrapping_add(value.rotate_left(7));
            }
            std::hint::black_box(checksum);
            let _ = sender.send(ModelEvent::SyntheticComplete);
        });
    }

    pub fn poll(&mut self) {
        while let Ok(event) = self.events_rx.try_recv() {
            self.pending_jobs = self.pending_jobs.saturating_sub(1);
            match event {
                ModelEvent::Scanned { id, project } => match project {
                    Ok(project) => {
                        let prior_opened = self
                            .catalog
                            .projects
                            .iter()
                            .find(|record| record.id == id)
                            .map_or(0, |record| record.last_opened_unix_seconds);
                        let thumbnail = self
                            .catalog
                            .projects
                            .iter()
                            .find(|record| record.id == id)
                            .and_then(|record| record.thumbnail.clone());
                        let mut record = ProjectRecord::from_project(&project);
                        record.last_opened_unix_seconds = prior_opened;
                        record.thumbnail = thumbnail;
                        if let Some(existing) = self
                            .catalog
                            .projects
                            .iter_mut()
                            .find(|record| record.id == id)
                        {
                            *existing = record;
                        }
                        self.queue_thumbnail(id, project.metadata().name.clone());
                    }
                    Err(_) => {
                        if let Some(record) = self
                            .catalog
                            .projects
                            .iter_mut()
                            .find(|record| record.id == id)
                        {
                            record.available = false;
                        }
                    }
                },
                ModelEvent::Created(result) => self.accept_project(result, "Created"),
                ModelEvent::Imported(result) => self.accept_project(result, "Imported"),
                ModelEvent::Opened(result) => match result {
                    Ok(project) => {
                        self.catalog.add_or_touch(&project);
                        self.persist_async();
                        self.project_to_launch = Some(project);
                    }
                    Err(error) => self.status = Some(format!("Open failed: {error}")),
                },
                ModelEvent::Saved(result) => {
                    self.save.in_flight = false;
                    if let Err(error) = result {
                        self.status = Some(format!("Catalog save failed: {error}"));
                    }
                    if self.save.dirty {
                        self.save.dirty = false;
                        self.persist_async();
                    }
                }
                ModelEvent::Thumbnail { id, reference } => {
                    if let Ok(reference) = reference
                        && let Some(record) = self
                            .catalog
                            .projects
                            .iter_mut()
                            .find(|record| record.id == id)
                    {
                        record.thumbnail = Some(reference);
                    }
                }
                ModelEvent::SyntheticComplete => {
                    self.status = Some("Background scheduler diagnostic completed.".to_owned());
                }
            }
        }
    }

    pub fn take_project_to_launch(&mut self) -> Option<Project> {
        self.project_to_launch.take()
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn clear_status(&mut self) {
        self.status = None;
    }

    pub const fn pending_jobs(&self) -> usize {
        self.pending_jobs
    }

    pub fn scheduler_stats(&self) -> engine_core::JobSchedulerStats {
        self.scheduler.stats()
    }

    pub const fn catalog_writable(&self) -> bool {
        self.capabilities.catalog_writable
    }

    fn accept_project(&mut self, result: Result<Project, String>, verb: &str) {
        match result {
            Ok(project) => {
                let path = project.root().to_path_buf();
                self.catalog.add_or_touch(&project);
                self.persist_async();
                self.queue_thumbnail(project.id(), project.metadata().name.clone());
                self.status = Some(format!("{verb} {}", path.display()));
            }
            Err(error) => self.status = Some(format!("{} failed: {error}", verb.to_lowercase())),
        }
    }

    fn queue_full_scan(&mut self) {
        let records = self.catalog.projects.clone();
        for record in records {
            let sender = self.events_tx.clone();
            self.submit(JobPriority::Normal, move || {
                let project = Project::open(&record.path).map_err(|error| error.to_string());
                let _ = sender.send(ModelEvent::Scanned {
                    id: record.id,
                    project,
                });
            });
        }
    }

    fn queue_thumbnail(&mut self, id: ProjectId, name: String) {
        if !self.capabilities.thumbnails_enabled {
            return;
        }
        let cache = Arc::clone(&self.thumbnail_cache);
        let sender = self.events_tx.clone();
        self.submit(JobPriority::Low, move || {
            let bytes = format!("Rustic project thumbnail\n{name}\n");
            let revision = stable_revision(bytes.as_bytes());
            let reference = cache
                .put(id, revision, bytes.as_bytes())
                .map_err(|error| error.to_string());
            let _ = sender.send(ModelEvent::Thumbnail { id, reference });
        });
    }

    fn persist_async(&mut self) {
        if !self.capabilities.catalog_writable {
            return;
        }
        if self.save.in_flight {
            self.save.dirty = true;
            return;
        }
        self.save.in_flight = true;
        let catalog = self.catalog.clone();
        let path = self.catalog_path.clone();
        let sender = self.events_tx.clone();
        let submitted = self.submit(JobPriority::High, move || {
            let result = catalog.save(&path).map_err(|error| error.to_string());
            let _ = sender.send(ModelEvent::Saved(result));
        });
        if !submitted {
            self.save.in_flight = false;
        }
    }

    fn submit(&mut self, priority: JobPriority, work: impl FnOnce() + Send + 'static) -> bool {
        match self.scheduler.submit(priority, move |_| work()) {
            Ok(_handle) => {
                self.pending_jobs = self.pending_jobs.saturating_add(1);
                true
            }
            Err(error) => {
                self.status = Some(format!("Background job rejected: {error}"));
                false
            }
        }
    }
}

impl Drop for ProjectManagerModel {
    fn drop(&mut self) {
        self.scheduler.shutdown();
        if self.capabilities.catalog_writable {
            let _ = self.catalog.save(&self.catalog_path);
        }
    }
}

fn stable_revision(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn synthetic_work_is_submitted_without_blocking_the_caller() {
        let directory = tempdir().unwrap();
        let mut model = ProjectManagerModel::load(
            directory.path().join("projects.ron"),
            directory.path().join("thumbs"),
            1,
            true,
        );
        model.request_synthetic_diagnostic();
        assert!(model.pending_jobs() > 0);
        for _ in 0..2_000 {
            model.poll();
            if model.pending_jobs() == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(model.pending_jobs(), 0);
    }

    #[test]
    fn remove_never_deletes_project_files() {
        let directory = tempdir().unwrap();
        let project_root = directory.path().join("project");
        let project = Project::create(&project_root, "Keep", ProjectTemplate::Blank).unwrap();
        let catalog_path = directory.path().join("projects.ron");
        let mut catalog = ProjectCatalog::default();
        catalog.add_or_touch(&project);
        catalog.save(&catalog_path).unwrap();
        let mut model =
            ProjectManagerModel::load(catalog_path, directory.path().join("thumbs"), 1, true);
        model.remove(project.id());
        assert!(project_root.is_dir());
    }
}
