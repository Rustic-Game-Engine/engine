//! External-editor-first gameplay programming with sandboxed Lua 5.4 and JavaScript adapters.
//!
//! Source validation never executes gameplay callbacks. Execution belongs exclusively
//! in `rustic-runtime`; editor callers use the manifest, validation, workspace, and
//! external-editor services exposed here.

#![forbid(unsafe_code)]

mod cpp_sdk;
mod external_editor;
mod external_runtime;
mod input;
mod javascript;
mod lua;
mod manifest;
mod model;
mod runtime;
mod settings;
mod workspace;

pub use engine_core::{EntityId, ScriptId};
pub use external_editor::{
    CodeEditor, EditorConfiguration, EditorError, EditorLaunch, ProjectOpenBehavior,
    build_editor_launch, discover_code_editor, open_in_external_editor,
};
pub use external_runtime::{
    ExternalBehavior, ExternalRuntimeError, LanguageAvailability, WebBehavior,
    probe_language_toolchain, validate_external,
};
pub use input::{
    ActionBinding, ActionMap, ActionState, InputAction, InputFrame, InputSystem, KeyEvent,
    KeyEventState, KeyboardInput,
};
pub use javascript::{JavaScriptBehavior, JavaScriptRuntimeError, validate_javascript};
pub use lua::{GameplayHost, LuaBehavior, LuaRuntimeError, validate_lua};
pub use manifest::{
    CURRENT_SCRIPT_MANIFEST_VERSION, ManifestLoad, ScriptManifest, ScriptManifestEntry,
    load_manifest, move_script_asset, register_script_asset, save_manifest_atomic,
    source_descriptor,
};
pub use model::{
    CompilationState, Diagnostic, DiagnosticSeverity, EngineValue, PublicProperties,
    PublicProperty, ReloadCompatibility, RuntimeException, RuntimeInstanceState, ScriptApiVersion,
    ScriptCallback, ScriptEvent, ScriptLanguage, ScriptReference, ScriptScope, ScriptSource,
    SourceLocation, StackFrame, reload_compatibility,
};
pub use runtime::{
    ScriptInstanceDescriptor, ScriptInstanceHandle, ScriptRuntimeAdapter, ScriptScheduler,
};
pub use settings::{GAME_SETTINGS_FILE, GameSettings};
pub use workspace::{
    WorkspaceGeneration, generate_programming_workspace, install_user_agent_integrations,
};

/// Compiles source without running gameplay code using the registered adapter.
///
/// # Errors
/// Returns a language-specific compiler error or an explicit unavailable-adapter error.
pub fn validate_script(
    language: ScriptLanguage,
    source: &[u8],
    name: &str,
    maximum_bytes: usize,
) -> Result<(), String> {
    match language {
        ScriptLanguage::Lua54 => {
            validate_lua(source, name, maximum_bytes).map_err(|e| e.to_string())
        }
        ScriptLanguage::JavaScript => {
            validate_javascript(source, name, maximum_bytes).map_err(|e| e.to_string())
        }
        other => validate_external(other, source, name, maximum_bytes).map_err(|e| e.to_string()),
    }
}
