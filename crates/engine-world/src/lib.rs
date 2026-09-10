//! Backend-independent world, scene graph, scene persistence, and render extraction.

#![forbid(unsafe_code)]

mod components;
mod ids;
mod primitive;
mod render;
mod scene;
mod undo;
mod world;

pub use components::{
    Camera, CameraProjection, Children, Light, LightKind, LocalTransform, Material, Mesh, Name,
    Parent, PartAttributes, ScriptComponent, ScriptComponents, StableEntity, WorldTransform,
};
pub use ids::{AssetId, EntityId, RuntimeEntityRef, SceneId, SceneInstanceId, WorldId};
pub use primitive::{Primitive, PrimitiveError, PrimitiveMesh};
pub use render::{
    RenderCamera, RenderLight, RenderMesh, RenderPrimitive, RenderWorld, RenderWorldBuffer,
};
pub use scene::{
    CURRENT_SCENE_VERSION, EntitySnapshot, RawComponent, RecoverySource, SceneDiagnostic,
    SceneDiagnosticSeverity, SceneDocument, SceneError, SceneInstance, SceneLoad, load_scene,
    load_scene_recovering, save_scene_atomic,
};
pub use undo::{SceneEdit, UndoStack};
pub use world::{EngineSystem, SceneWorld, SchedulePhase, WorldCommand, WorldError, WorldSystem};
