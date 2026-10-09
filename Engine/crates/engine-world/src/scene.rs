use crate::{
    Camera, CameraProjection, EntityId, Light, LightKind, LocalTransform, Material, Mesh,
    PartAttributes, Primitive, SceneId, SceneInstanceId, SceneWorld, ScriptComponent, WorldCommand,
    WorldError,
};
use glam::{Quat, Vec3};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

pub const CURRENT_SCENE_VERSION: u32 = 2;
const RESOURCE_KIND: &str = "rustic_scene";

const NAME_SCHEMA: &str = "rustic.name";
const FOLDER_SCHEMA: &str = "rustic.folder";
const TRANSFORM_SCHEMA: &str = "rustic.transform";
const PARENT_SCHEMA: &str = "rustic.parent";
const MESH_SCHEMA: &str = "rustic.mesh";
const MATERIAL_SCHEMA: &str = "rustic.material";
const CAMERA_SCHEMA: &str = "rustic.camera";
const LIGHT_SCHEMA: &str = "rustic.light";
const PRIMITIVE_SCHEMA: &str = "rustic.primitive";
const SCRIPTS_SCHEMA: &str = "rustic.scripts";
const PART_ATTRIBUTES_SCHEMA: &str = "rustic.part_attributes";

/// One opaque component retained when its schema is unknown or its payload is damaged.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RawComponent {
    pub schema: String,
    pub version: u32,
    pub payload: String,
}

/// Serializable representation of one authored entity.
#[derive(Clone, Debug, PartialEq)]
pub struct EntitySnapshot {
    pub id: EntityId,
    pub name: Option<String>,
    pub folder: bool,
    pub local_transform: LocalTransform,
    pub parent: Option<EntityId>,
    pub mesh: Option<Mesh>,
    pub material: Option<Material>,
    pub camera: Option<Camera>,
    pub light: Option<Light>,
    pub primitive: Option<Primitive>,
    pub part_attributes: PartAttributes,
    pub scripts: Vec<ScriptComponent>,
    pub unknown_components: Vec<RawComponent>,
}

impl Default for EntitySnapshot {
    fn default() -> Self {
        Self {
            id: EntityId::new(),
            name: None,
            folder: false,
            local_transform: LocalTransform::IDENTITY,
            parent: None,
            mesh: None,
            material: None,
            camera: None,
            light: None,
            primitive: None,
            part_attributes: PartAttributes::default(),
            scripts: Vec::new(),
            unknown_components: Vec::new(),
        }
    }
}

impl EntitySnapshot {
    pub(crate) fn validate(&self) -> Result<(), WorldError> {
        if !self.local_transform.is_finite() {
            return Err(WorldError::InvalidTransform(self.id));
        }
        if let Some(primitive) = &self.primitive {
            primitive
                .validate()
                .map_err(|error| WorldError::InvalidPrimitive {
                    entity: self.id,
                    message: error.to_string(),
                })?;
        }
        for script in &self.scripts {
            script
                .validate()
                .map_err(|message| WorldError::InvalidScript {
                    entity: self.id,
                    message,
                })?;
        }
        Ok(())
    }
}

/// Persisted mapping for one additive scene instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SceneInstance {
    pub id: SceneInstanceId,
    pub source_scene: SceneId,
    pub entity_map: BTreeMap<EntityId, EntityId>,
}

/// Version-independent in-memory scene document.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneDocument {
    pub id: SceneId,
    pub name: String,
    /// Scene-lifetime scripts, referenced exclusively by persistent asset ID.
    pub startup_scripts: Vec<engine_scripting::ScriptReference>,
    pub entities: Vec<EntitySnapshot>,
    pub instances: Vec<SceneInstance>,
}

impl SceneDocument {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: SceneId::new(),
            name: name.into(),
            startup_scripts: Vec::new(),
            entities: Vec::new(),
            instances: Vec::new(),
        }
    }

    /// Captures stable authored data in deterministic ID order.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] if a world entity cannot be captured consistently.
    pub fn from_world(
        id: SceneId,
        name: impl Into<String>,
        world: &SceneWorld,
    ) -> Result<Self, WorldError> {
        let entities = world
            .entity_ids()
            .map(|entity| world.snapshot(entity))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            id,
            name: name.into(),
            startup_scripts: Vec::new(),
            entities,
            instances: world.instances().to_vec(),
        })
    }

    /// Creates a new live world without exposing persistence or Bevy types.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] if the serialized hierarchy or components are invalid.
    pub fn create_world(&self) -> Result<SceneWorld, WorldError> {
        let mut world = SceneWorld::new();
        let commands: Vec<_> = self
            .entities
            .iter()
            .cloned()
            .map(|snapshot| WorldCommand::Spawn(Box::new(snapshot)))
            .collect();
        world.apply_commands(&commands)?;
        world.propagate_transforms();
        world.replace_instances(self.instances.clone());
        Ok(world)
    }

    /// Serializes current schema bytes with a checksum over the checksum-free envelope.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError`] when component validation or RON serialization fails.
    pub fn to_bytes(&self) -> Result<Vec<u8>, SceneError> {
        let mut wire = SceneWire::from_document(self)?;
        wire.checksum = Some(checksum_wire(&wire)?);
        serialize_wire(&wire)
    }
}

/// Severity of a localized scene-load diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneDiagnosticSeverity {
    Warning,
    Error,
}

/// Diagnostic that identifies the damaged entity/component whenever possible.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneDiagnostic {
    pub severity: SceneDiagnosticSeverity,
    pub entity: Option<EntityId>,
    pub component: Option<String>,
    pub message: String,
}

/// File selected by recoverable loading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoverySource {
    Primary,
    Backup,
}

/// Loaded document, including compatibility mode and non-fatal corruption details.
#[derive(Clone, Debug)]
pub struct SceneLoad {
    pub document: SceneDocument,
    pub schema_version: u32,
    pub read_only: bool,
    pub diagnostics: Vec<SceneDiagnostic>,
    pub source: RecoverySource,
    corrupt: bool,
}

impl SceneLoad {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == SceneDiagnosticSeverity::Error)
    }

    /// Saves only compatible, fully valid loads; future/corrupt data remains read-only.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError::ReadOnly`] for incompatible or damaged input, or a
    /// persistence error from [`save_scene_atomic`].
    pub fn save_atomic(&self, path: impl AsRef<Path>) -> Result<(), SceneError> {
        if self.read_only {
            return Err(SceneError::ReadOnly);
        }
        save_scene_atomic(path, &self.document)
    }
}

/// Scene encoding, compatibility, and recoverable persistence errors.
#[derive(Debug, Error)]
pub enum SceneError {
    #[error("scene I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("scene serialization failed: {0}")]
    Serialize(String),
    #[error("scene is not valid RON: {0}")]
    Parse(String),
    #[error("resource kind `{0}` is not a Rustic scene")]
    WrongResourceKind(String),
    #[error("scene schema version {0} is too old to migrate")]
    UnsupportedOldVersion(u32),
    #[error("scene cannot be saved because it was opened read-only")]
    ReadOnly,
    #[error("scene world rejected loaded data: {0}")]
    World(#[from] WorldError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SceneWire {
    resource_kind: String,
    schema_version: u32,
    resource_id: SceneId,
    name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    startup_scripts: Vec<engine_scripting::ScriptReference>,
    checksum: Option<String>,
    entities: Vec<EntityWire>,
    #[serde(default)]
    instances: Vec<SceneInstance>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SceneWireV1 {
    resource_kind: String,
    schema_version: u32,
    resource_id: SceneId,
    name: String,
    entities: Vec<EntityWire>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct EntityWire {
    id: EntityId,
    components: Vec<RawComponent>,
}

#[derive(Deserialize)]
struct VersionProbe {
    resource_kind: String,
    schema_version: u32,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct TransformWire {
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct CameraWire {
    projection: CameraProjection,
    #[serde(default = "default_camera_zoom")]
    zoom: f32,
    active: bool,
    order: i32,
}

const fn default_camera_zoom() -> f32 {
    1.0
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct LightWire {
    kind: LightKind,
    color: [f32; 3],
    intensity: f32,
    range: f32,
    spot_outer_angle_radians: f32,
    casts_shadows: bool,
}

impl SceneWire {
    fn from_document(document: &SceneDocument) -> Result<Self, SceneError> {
        let mut entities = document.entities.clone();
        entities.sort_by_key(|entity| entity.id);
        let entities = entities
            .into_iter()
            .map(EntityWire::from_snapshot)
            .collect::<Result<Vec<_>, _>>()?;
        let mut instances = document.instances.clone();
        instances.sort_by_key(|instance| instance.id);
        Ok(Self {
            resource_kind: RESOURCE_KIND.to_owned(),
            schema_version: CURRENT_SCENE_VERSION,
            resource_id: document.id,
            name: document.name.clone(),
            startup_scripts: document.startup_scripts.clone(),
            checksum: None,
            entities,
            instances,
        })
    }
}

impl EntityWire {
    fn from_snapshot(snapshot: EntitySnapshot) -> Result<Self, SceneError> {
        snapshot.validate()?;
        let mut components = Vec::with_capacity(8 + snapshot.unknown_components.len());
        if let Some(name) = snapshot.name {
            components.push(encode_component(NAME_SCHEMA, &name)?);
        }
        if snapshot.folder {
            components.push(encode_component(FOLDER_SCHEMA, &true)?);
        }
        components.push(encode_component(
            TRANSFORM_SCHEMA,
            &TransformWire {
                translation: snapshot.local_transform.translation.to_array(),
                rotation: snapshot.local_transform.rotation.to_array(),
                scale: snapshot.local_transform.scale.to_array(),
            },
        )?);
        if let Some(parent) = snapshot.parent {
            components.push(encode_component(PARENT_SCHEMA, &parent)?);
        }
        if let Some(mesh) = snapshot.mesh {
            components.push(encode_component(MESH_SCHEMA, &mesh.asset)?);
        }
        if let Some(material) = snapshot.material {
            components.push(encode_component(MATERIAL_SCHEMA, &material.asset)?);
        }
        if let Some(camera) = snapshot.camera {
            components.push(encode_component(
                CAMERA_SCHEMA,
                &CameraWire {
                    projection: camera.projection,
                    zoom: camera.zoom,
                    active: camera.active,
                    order: camera.order,
                },
            )?);
        }
        if let Some(light) = snapshot.light {
            components.push(encode_component(
                LIGHT_SCHEMA,
                &LightWire {
                    kind: light.kind,
                    color: light.color.to_array(),
                    intensity: light.intensity,
                    range: light.range,
                    spot_outer_angle_radians: light.spot_outer_angle_radians,
                    casts_shadows: light.casts_shadows,
                },
            )?);
        }
        if let Some(primitive) = snapshot.primitive {
            components.push(encode_component(PRIMITIVE_SCHEMA, &primitive)?);
        }
        components.push(encode_component(
            PART_ATTRIBUTES_SCHEMA,
            &snapshot.part_attributes,
        )?);
        if !snapshot.scripts.is_empty() {
            components.push(encode_component(SCRIPTS_SCHEMA, &snapshot.scripts)?);
        }
        components.extend(snapshot.unknown_components);
        components.sort_by(|left, right| {
            left.schema
                .cmp(&right.schema)
                .then_with(|| left.version.cmp(&right.version))
                .then_with(|| left.payload.cmp(&right.payload))
        });
        Ok(Self {
            id: snapshot.id,
            components,
        })
    }
}

fn encode_component<T: Serialize>(schema: &str, value: &T) -> Result<RawComponent, SceneError> {
    Ok(RawComponent {
        schema: schema.to_owned(),
        version: 1,
        payload: ron::to_string(value).map_err(|error| SceneError::Serialize(error.to_string()))?,
    })
}

fn decode_component<T: DeserializeOwned>(
    raw: &RawComponent,
    entity: EntityId,
    diagnostics: &mut Vec<SceneDiagnostic>,
) -> Option<T> {
    if raw.version != 1 {
        diagnostics.push(SceneDiagnostic {
            severity: SceneDiagnosticSeverity::Error,
            entity: Some(entity),
            component: Some(raw.schema.clone()),
            message: format!("unsupported component version {}", raw.version),
        });
        return None;
    }
    match ron::from_str(&raw.payload) {
        Ok(value) => Some(value),
        Err(error) => {
            diagnostics.push(SceneDiagnostic {
                severity: SceneDiagnosticSeverity::Error,
                entity: Some(entity),
                component: Some(raw.schema.clone()),
                message: format!("component payload is corrupt: {error}"),
            });
            None
        }
    }
}

/// Parses scene bytes, migrates schema v1, preserves unknown components, and reports
/// component-level failures without discarding other valid entities.
///
/// # Errors
///
/// Returns [`SceneError`] when the outer envelope is malformed, has the wrong resource
/// kind, is too old to migrate, or cannot be decoded as UTF-8/RON.
#[allow(clippy::too_many_lines)]
pub fn load_scene(bytes: &[u8]) -> Result<SceneLoad, SceneError> {
    let text = std::str::from_utf8(bytes).map_err(|error| SceneError::Parse(error.to_string()))?;
    let probe: VersionProbe =
        ron::from_str(text).map_err(|error| SceneError::Parse(error.to_string()))?;
    if probe.resource_kind != RESOURCE_KIND {
        return Err(SceneError::WrongResourceKind(probe.resource_kind));
    }

    let (wire, migrated) = match probe.schema_version {
        0 => return Err(SceneError::UnsupportedOldVersion(0)),
        1 => {
            let old: SceneWireV1 =
                ron::from_str(text).map_err(|error| SceneError::Parse(error.to_string()))?;
            (
                SceneWire {
                    resource_kind: old.resource_kind,
                    schema_version: CURRENT_SCENE_VERSION,
                    resource_id: old.resource_id,
                    name: old.name,
                    startup_scripts: Vec::new(),
                    checksum: None,
                    entities: old.entities,
                    instances: Vec::new(),
                },
                true,
            )
        }
        _ => (
            ron::from_str(text).map_err(|error| SceneError::Parse(error.to_string()))?,
            false,
        ),
    };

    let future = probe.schema_version > CURRENT_SCENE_VERSION;
    let mut diagnostics = Vec::new();
    let mut corrupt = false;
    if migrated {
        diagnostics.push(SceneDiagnostic {
            severity: SceneDiagnosticSeverity::Warning,
            entity: None,
            component: None,
            message: "scene schema v1 migrated to v2 in memory".to_owned(),
        });
    }
    if future {
        diagnostics.push(SceneDiagnostic {
            severity: SceneDiagnosticSeverity::Warning,
            entity: None,
            component: None,
            message: format!(
                "scene schema {} is newer than supported schema {}; opened read-only",
                probe.schema_version, CURRENT_SCENE_VERSION
            ),
        });
    } else if let Some(expected) = &wire.checksum {
        let actual = checksum_wire(&wire)?;
        if *expected != actual {
            corrupt = true;
            diagnostics.push(SceneDiagnostic {
                severity: SceneDiagnosticSeverity::Error,
                entity: None,
                component: None,
                message: format!(
                    "scene checksum mismatch: expected {expected}, calculated {actual}"
                ),
            });
        }
    }

    let mut seen_entities = BTreeSet::new();
    let mut entities = Vec::with_capacity(wire.entities.len());
    for entity in wire.entities {
        if !seen_entities.insert(entity.id) {
            corrupt = true;
            diagnostics.push(SceneDiagnostic {
                severity: SceneDiagnosticSeverity::Error,
                entity: Some(entity.id),
                component: None,
                message: "duplicate entity identity; later record ignored".to_owned(),
            });
            continue;
        }
        let before = diagnostics.len();
        let snapshot = decode_entity(entity, &mut diagnostics);
        corrupt |= diagnostics[before..]
            .iter()
            .any(|value| value.severity == SceneDiagnosticSeverity::Error);
        entities.push(snapshot);
    }
    entities.sort_by_key(|entity| entity.id);
    let document = SceneDocument {
        id: wire.resource_id,
        name: wire.name,
        startup_scripts: wire.startup_scripts,
        entities,
        instances: wire.instances,
    };
    Ok(SceneLoad {
        document,
        schema_version: probe.schema_version,
        read_only: future || corrupt,
        diagnostics,
        source: RecoverySource::Primary,
        corrupt,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "the component dispatch table is intentionally centralized for localized diagnostics"
)]
fn decode_entity(entity: EntityWire, diagnostics: &mut Vec<SceneDiagnostic>) -> EntitySnapshot {
    let mut snapshot = EntitySnapshot {
        id: entity.id,
        ..EntitySnapshot::default()
    };
    let mut seen = BTreeSet::new();
    for raw in entity.components {
        if !seen.insert(raw.schema.clone()) {
            diagnostics.push(SceneDiagnostic {
                severity: SceneDiagnosticSeverity::Error,
                entity: Some(entity.id),
                component: Some(raw.schema.clone()),
                message: "duplicate component schema; later value preserved as opaque data"
                    .to_owned(),
            });
            snapshot.unknown_components.push(raw);
            continue;
        }
        match raw.schema.as_str() {
            NAME_SCHEMA => assign_or_preserve(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |value| snapshot.name = Some(value),
            ),
            FOLDER_SCHEMA => assign_or_preserve::<bool>(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |value| snapshot.folder = value,
            ),
            TRANSFORM_SCHEMA => assign_or_preserve::<TransformWire>(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |value| {
                    snapshot.local_transform = LocalTransform {
                        translation: Vec3::from_array(value.translation),
                        rotation: Quat::from_array(value.rotation),
                        scale: Vec3::from_array(value.scale),
                    };
                },
            ),
            PARENT_SCHEMA => assign_or_preserve(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |value| snapshot.parent = Some(value),
            ),
            MESH_SCHEMA => assign_or_preserve(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |asset| snapshot.mesh = Some(Mesh { asset }),
            ),
            MATERIAL_SCHEMA => assign_or_preserve(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |asset| snapshot.material = Some(Material { asset }),
            ),
            CAMERA_SCHEMA => assign_or_preserve::<CameraWire>(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |value| {
                    snapshot.camera = Some(Camera {
                        projection: value.projection,
                        zoom: value.zoom,
                        active: value.active,
                        order: value.order,
                    });
                },
            ),
            LIGHT_SCHEMA => assign_or_preserve::<LightWire>(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |value| {
                    snapshot.light = Some(Light {
                        kind: value.kind,
                        color: Vec3::from_array(value.color),
                        intensity: value.intensity,
                        range: value.range,
                        spot_outer_angle_radians: value.spot_outer_angle_radians,
                        casts_shadows: value.casts_shadows,
                    });
                },
            ),
            PRIMITIVE_SCHEMA => assign_or_preserve(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |value| snapshot.primitive = Some(value),
            ),
            PART_ATTRIBUTES_SCHEMA => assign_or_preserve(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |value| snapshot.part_attributes = value,
            ),
            SCRIPTS_SCHEMA => assign_or_preserve(
                &raw,
                entity.id,
                diagnostics,
                &mut snapshot.unknown_components,
                |value| snapshot.scripts = value,
            ),
            _ => {
                diagnostics.push(SceneDiagnostic {
                    severity: SceneDiagnosticSeverity::Warning,
                    entity: Some(entity.id),
                    component: Some(raw.schema.clone()),
                    message: "unknown component preserved as opaque data".to_owned(),
                });
                snapshot.unknown_components.push(raw);
            }
        }
    }
    if !snapshot.local_transform.is_finite() {
        diagnostics.push(SceneDiagnostic {
            severity: SceneDiagnosticSeverity::Error,
            entity: Some(entity.id),
            component: Some(TRANSFORM_SCHEMA.to_owned()),
            message: "transform is non-finite or singular; identity used".to_owned(),
        });
        snapshot.local_transform = LocalTransform::IDENTITY;
    }
    snapshot
}

fn assign_or_preserve<T: DeserializeOwned>(
    raw: &RawComponent,
    entity: EntityId,
    diagnostics: &mut Vec<SceneDiagnostic>,
    unknown: &mut Vec<RawComponent>,
    assign: impl FnOnce(T),
) {
    if let Some(value) = decode_component(raw, entity, diagnostics) {
        assign(value);
    } else {
        unknown.push(raw.clone());
    }
}

fn serialize_wire<T: Serialize>(wire: &T) -> Result<Vec<u8>, SceneError> {
    let pretty = ron::ser::PrettyConfig::new()
        .depth_limit(32)
        .separate_tuple_members(true)
        .enumerate_arrays(true);
    let mut bytes = ron::ser::to_string_pretty(wire, pretty)
        .map_err(|error| SceneError::Serialize(error.to_string()))?
        .into_bytes();
    bytes.push(b'\n');
    Ok(bytes)
}

fn checksum_wire(wire: &SceneWire) -> Result<String, SceneError> {
    let mut canonical = wire.clone();
    canonical.checksum = None;
    let digest = Sha256::digest(serialize_wire(&canonical)?);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    Ok(output)
}

/// Atomically writes a scene and retains the prior valid primary as `.bak`.
///
/// # Errors
///
/// Returns [`SceneError`] if validation, serialization, flushing, backup staging, or
/// atomic installation fails. A failed replacement restores the prior primary.
pub fn save_scene_atomic(
    path: impl AsRef<Path>,
    document: &SceneDocument,
) -> Result<(), SceneError> {
    let path = path.as_ref();
    let parent = path.parent().ok_or_else(|| {
        io_error(
            path,
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "scene path has no parent"),
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let bytes = document.to_bytes()?;
    let token = Uuid::new_v4();
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("scene"),
        token
    ));
    let backup = backup_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| io_error(&temporary, error))?;
        file.write_all(&bytes)
            .map_err(|error| io_error(&temporary, error))?;
        file.sync_all()
            .map_err(|error| io_error(&temporary, error))?;
        drop(file);
        if path.exists() {
            if backup.exists() {
                fs::remove_file(&backup).map_err(|error| io_error(&backup, error))?;
            }
            fs::rename(path, &backup).map_err(|error| io_error(path, error))?;
            if let Err(error) = fs::rename(&temporary, path) {
                let _ = fs::rename(&backup, path);
                return Err(io_error(path, error));
            }
        } else {
            fs::rename(&temporary, path).map_err(|error| io_error(path, error))?;
        }
        Ok(())
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Loads the primary scene, falling back to the retained backup when the primary is
/// missing, structurally invalid, or fails component/checksum validation.
///
/// # Errors
///
/// Returns the primary [`SceneError`] when neither the primary nor backup is recoverable.
pub fn load_scene_recovering(path: impl AsRef<Path>) -> Result<SceneLoad, SceneError> {
    let path = path.as_ref();
    let primary = fs::read(path)
        .map_err(|error| io_error(path, error))
        .and_then(|bytes| load_scene(&bytes));
    match primary {
        Ok(load) if !load.corrupt => Ok(load),
        primary_result => {
            let backup = backup_path(path);
            let recovered = fs::read(&backup)
                .map_err(|error| io_error(&backup, error))
                .and_then(|bytes| load_scene(&bytes));
            match recovered {
                Ok(mut load) if !load.corrupt => {
                    load.source = RecoverySource::Backup;
                    load.diagnostics.push(SceneDiagnostic {
                        severity: SceneDiagnosticSeverity::Warning,
                        entity: None,
                        component: None,
                        message: format!("recovered scene from {}", backup.display()),
                    });
                    Ok(load)
                }
                _ => primary_result,
            }
        }
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(".bak");
    PathBuf::from(name)
}

fn io_error(path: impl Into<PathBuf>, source: std::io::Error) -> SceneError {
    SceneError::Io {
        path: path.into(),
        source,
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare exact round trips and deterministic values"
)]
mod tests {
    use super::*;

    fn fixed_id(value: u128) -> EntityId {
        EntityId::from(Uuid::from_u128(value))
    }

    fn sample_document() -> SceneDocument {
        let root = fixed_id(1);
        let child = fixed_id(2);
        SceneDocument {
            id: SceneId::from(Uuid::from_u128(10)),
            name: "Golden".to_owned(),
            startup_scripts: Vec::new(),
            entities: vec![
                EntitySnapshot {
                    id: root,
                    name: Some("Root".to_owned()),
                    ..EntitySnapshot::default()
                },
                EntitySnapshot {
                    id: child,
                    name: Some("Child".to_owned()),
                    parent: Some(root),
                    primitive: Some(Primitive::Cube { size: 2.0 }),
                    ..EntitySnapshot::default()
                },
            ],
            instances: Vec::new(),
        }
    }

    #[test]
    fn deterministic_round_trip_is_exact_across_entity_insertion_order() {
        let document = sample_document();
        let first = document.to_bytes().unwrap();
        let mut reordered = document.clone();
        reordered.entities.reverse();
        assert_eq!(first, reordered.to_bytes().unwrap());
        let loaded = load_scene(&first).unwrap();
        assert!(!loaded.read_only);
        assert_eq!(loaded.document, document);
        assert_eq!(loaded.document.to_bytes().unwrap(), first);
    }

    #[test]
    fn scene_startup_scripts_round_trip_as_asset_ids_without_source() {
        let mut document = sample_document();
        let reference = engine_scripting::ScriptReference::new(engine_scripting::ScriptId::new());
        document.startup_scripts.push(reference.clone());
        let bytes = document.to_bytes().unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains(&reference.asset_id.to_string()));
        assert!(!text.contains("source_code"));
        assert_eq!(
            load_scene(&bytes).unwrap().document.startup_scripts,
            [reference]
        );
    }

    #[test]
    fn camera_zoom_round_trips_and_defaults_for_older_component_payloads() {
        let wire = CameraWire {
            projection: CameraProjection::default(),
            zoom: 2.5,
            active: true,
            order: 3,
        };
        let encoded = ron::to_string(&wire).unwrap();
        assert_eq!(ron::from_str::<CameraWire>(&encoded).unwrap().zoom, 2.5);

        let old_payload = ron::to_string(&CameraWire { zoom: 1.0, ..wire })
            .unwrap()
            .replace("zoom:1.0,", "");
        assert_eq!(ron::from_str::<CameraWire>(&old_payload).unwrap().zoom, 1.0);
    }

    #[test]
    fn schema_v1_migrates_in_memory_and_remains_writable() {
        let current = SceneWire::from_document(&sample_document()).unwrap();
        let old = SceneWireV1 {
            resource_kind: current.resource_kind,
            schema_version: 1,
            resource_id: current.resource_id,
            name: current.name,
            entities: current.entities,
        };
        let loaded = load_scene(&serialize_wire(&old).unwrap()).unwrap();
        assert_eq!(loaded.schema_version, 1);
        assert!(!loaded.read_only);
        assert!(
            loaded
                .diagnostics
                .iter()
                .any(|value| value.message.contains("migrated"))
        );
        assert_eq!(
            load_scene(&loaded.document.to_bytes().unwrap())
                .unwrap()
                .schema_version,
            CURRENT_SCENE_VERSION
        );
    }

    #[test]
    fn future_schema_opens_read_only() {
        let mut wire = SceneWire::from_document(&sample_document()).unwrap();
        wire.schema_version = CURRENT_SCENE_VERSION + 1;
        wire.checksum = None;
        let loaded = load_scene(&serialize_wire(&wire).unwrap()).unwrap();
        assert!(loaded.read_only);
        assert!(!loaded.has_errors());
    }

    #[test]
    fn corrupt_component_is_localized_and_other_entities_survive() {
        let mut wire = SceneWire::from_document(&sample_document()).unwrap();
        let damaged_id = wire.entities[0].id;
        let component = wire.entities[0]
            .components
            .iter_mut()
            .find(|value| value.schema == TRANSFORM_SCHEMA)
            .unwrap();
        component.payload = "(translation: definitely-not-a-vector)".to_owned();
        wire.checksum = Some(checksum_wire(&wire).unwrap());
        let loaded = load_scene(&serialize_wire(&wire).unwrap()).unwrap();
        assert!(loaded.read_only);
        assert_eq!(loaded.document.entities.len(), 2);
        assert!(loaded.diagnostics.iter().any(|value| {
            value.entity == Some(damaged_id)
                && value.component.as_deref() == Some(TRANSFORM_SCHEMA)
                && value.severity == SceneDiagnosticSeverity::Error
        }));
    }

    #[test]
    fn atomic_save_retains_backup_and_recovers_corrupt_primary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.rscene");
        let first = sample_document();
        save_scene_atomic(&path, &first).unwrap();
        let mut second = first.clone();
        second.name = "Second".to_owned();
        save_scene_atomic(&path, &second).unwrap();
        assert!(backup_path(&path).is_file());
        fs::write(&path, b"not ron").unwrap();
        let recovered = load_scene_recovering(&path).unwrap();
        assert_eq!(recovered.source, RecoverySource::Backup);
        assert_eq!(recovered.document.name, first.name);
    }

    #[test]
    fn scene_instance_remaps_internal_parent_references() {
        let source = sample_document();
        let mut world = SceneWorld::new();
        let instance = world
            .instantiate_scene(&source, SceneInstanceId::new(), None)
            .unwrap();
        assert_eq!(instance.entity_map.len(), 2);
        for original in &source.entities {
            let mapped = instance.entity_map[&original.id];
            assert!(world.contains(mapped));
            assert_eq!(
                world.parent(mapped).unwrap(),
                original.parent.map(|parent| instance.entity_map[&parent])
            );
        }
    }
}
