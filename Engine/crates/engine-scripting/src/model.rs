use crate::{EntityId, ScriptId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Where an engine-owned script instance participates in the lifecycle.
///
/// Scope is attachment data, not a property of the source asset: the same script
/// asset can safely be used by a scene and by one or more objects.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptScope {
    Global,
    Scene,
    #[default]
    Component,
}

/// An editor-authored reference to a script asset. Source paths never cross the
/// scene/runtime boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptReference {
    pub asset_id: ScriptId,
    #[serde(default = "script_reference_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub execution_order: i32,
}

const fn script_reference_enabled() -> bool {
    true
}

impl ScriptReference {
    pub const fn new(asset_id: ScriptId) -> Self {
        Self {
            asset_id,
            enabled: true,
            execution_order: 0,
        }
    }
}

/// Language-neutral callback names understood by every runtime adapter.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ScriptCallback {
    Start,
    Update,
    FixedUpdate,
    CollisionEnter,
    CollisionStay,
    CollisionExit,
    Enable,
    Disable,
    Destroy,
}

impl ScriptCallback {
    /// Canonical public spelling used in documentation and language adapters.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Update => "Update",
            Self::FixedUpdate => "FixedUpdate",
            Self::CollisionEnter => "OnCollisionEnter",
            Self::CollisionStay => "OnCollisionStay",
            Self::CollisionExit => "OnCollisionExit",
            Self::Enable => "OnEnable",
            Self::Disable => "OnDisable",
            Self::Destroy => "OnDestroy",
        }
    }

    /// Original 0.1 callback spelling, retained for existing projects.
    pub const fn legacy_name(self) -> &'static str {
        match self {
            Self::Start => "on_start",
            Self::Update => "update",
            Self::FixedUpdate => "fixed_update",
            Self::CollisionEnter => "on_collision_enter",
            Self::CollisionStay => "on_collision_stay",
            Self::CollisionExit => "on_collision_exit",
            Self::Enable => "on_enable",
            Self::Disable => "on_disable",
            Self::Destroy => "on_destroy",
        }
    }
}

/// Cross-language payload carried by the common Engine Events / Script API.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptEvent {
    pub name: String,
    #[serde(default)]
    pub sender: Option<ScriptId>,
    #[serde(default)]
    pub target: Option<ScriptId>,
    #[serde(default)]
    pub arguments: Vec<EngineValue>,
}

/// Current stable gameplay API. Major changes are incompatible; minor additions are compatible.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ScriptApiVersion {
    pub major: u16,
    pub minor: u16,
}

impl ScriptApiVersion {
    pub const CURRENT: Self = Self { major: 1, minor: 0 };

    pub const fn accepts(self, requested: Self) -> bool {
        requested.major == self.major && requested.minor <= self.minor
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptLanguage {
    Lua54,
    Luau,
    JavaScript,
    Python,
    C,
    Cpp,
    CSharp,
    Java,
    Php,
    Web,
}

impl ScriptLanguage {
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Lua54 => "Lua 5.4",
            Self::Luau => "Luau",
            Self::JavaScript => "JavaScript",
            Self::Python => "Python",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::CSharp => "C#",
            Self::Java => "Java",
            Self::Php => "PHP",
            Self::Web => "HTML/CSS",
        }
    }

    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "lua" => Some(Self::Lua54),
            "luau" => Some(Self::Luau),
            "js" | "mjs" => Some(Self::JavaScript),
            "py" => Some(Self::Python),
            "c" => Some(Self::C),
            "cc" | "cpp" | "cxx" => Some(Self::Cpp),
            "cs" => Some(Self::CSharp),
            "java" => Some(Self::Java),
            "php" => Some(Self::Php),
            "html" | "htm" | "css" => Some(Self::Web),
            _ => None,
        }
    }

    pub fn accepts_path(self, path: &std::path::Path) -> bool {
        Self::from_path(path) == Some(self)
    }

    /// Returns whether a source path is valid inside a game project.
    ///
    /// Browser-facing languages are deliberately isolated below `ui/`; they are
    /// content for the game's overlay, not general gameplay scripts. Standalone
    /// JavaScript remains available below `scripts/` for non-DOM behaviors.
    pub fn accepts_project_path(self, path: &std::path::Path) -> bool {
        if !self.accepts_path(path) {
            return false;
        }
        if matches!(self, Self::Php | Self::Web) {
            return path
                .components()
                .next()
                .is_some_and(|component| component.as_os_str() == "ui");
        }
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptSource {
    pub format_version: u32,
    pub id: ScriptId,
    pub language: ScriptLanguage,
    pub relative_path: PathBuf,
    pub api_version: ScriptApiVersion,
    pub content_hash: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompilationState {
    Valid,
    Validating,
    Invalid,
    Stale,
    Missing,
    Incompatible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeInstanceState {
    Created,
    Running,
    Disabled,
    Failed,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum EngineValue {
    Boolean(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Entity(Option<EntityId>),
}

impl EngineValue {
    pub fn same_type(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublicProperty {
    pub name: String,
    pub default: EngineValue,
}

pub type PublicProperties = BTreeMap<String, EngineValue>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceLocation {
    pub path: PathBuf,
    pub line: u32,
    pub column: u32,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StackFrame {
    pub function: Option<String>,
    pub source: SourceLocation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub format_version: u32,
    pub severity: DiagnosticSeverity,
    pub language: ScriptLanguage,
    pub subsystem: String,
    pub process: String,
    pub message: String,
    pub script_id: Option<ScriptId>,
    pub source: Option<SourceLocation>,
    pub stack: Vec<StackFrame>,
    pub timestamp_unix_millis: u64,
    pub duplicate_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeException {
    pub format_version: u32,
    pub script_id: ScriptId,
    pub message: String,
    pub stack: Vec<StackFrame>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum ReloadCompatibility {
    Compatible {
        added_properties: Vec<String>,
        removed_properties: Vec<String>,
    },
    Incompatible {
        reason: String,
    },
}

pub fn reload_compatibility(
    api: ScriptApiVersion,
    next_api: ScriptApiVersion,
    current: &[PublicProperty],
    next: &[PublicProperty],
) -> ReloadCompatibility {
    if api.major != next_api.major {
        return ReloadCompatibility::Incompatible {
            reason: format!(
                "gameplay API major changed from {} to {}",
                api.major, next_api.major
            ),
        };
    }
    let old: BTreeMap<_, _> = current.iter().map(|p| (&p.name, &p.default)).collect();
    let new: BTreeMap<_, _> = next.iter().map(|p| (&p.name, &p.default)).collect();
    for (name, value) in &old {
        if let Some(next_value) = new.get(name)
            && !value.same_type(next_value)
        {
            return ReloadCompatibility::Incompatible {
                reason: format!("public property `{name}` changed type"),
            };
        }
    }
    ReloadCompatibility::Compatible {
        added_properties: new
            .keys()
            .filter(|name| !old.contains_key(*name))
            .map(|s| (*s).clone())
            .collect(),
        removed_properties: old
            .keys()
            .filter(|name| !new.contains_key(*name))
            .map(|s| (*s).clone())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn all_supported_source_extensions_resolve_to_their_language() {
        for (path, expected) in [
            ("behavior.lua", ScriptLanguage::Lua54),
            ("behavior.luau", ScriptLanguage::Luau),
            ("behavior.js", ScriptLanguage::JavaScript),
            ("behavior.py", ScriptLanguage::Python),
            ("behavior.c", ScriptLanguage::C),
            ("behavior.cpp", ScriptLanguage::Cpp),
            ("behavior.cs", ScriptLanguage::CSharp),
            ("RusticBehavior.java", ScriptLanguage::Java),
            ("behavior.php", ScriptLanguage::Php),
            ("behavior.html", ScriptLanguage::Web),
            ("behavior.css", ScriptLanguage::Web),
        ] {
            assert_eq!(ScriptLanguage::from_path(Path::new(path)), Some(expected));
            assert!(expected.accepts_path(Path::new(path)));
        }
    }

    #[test]
    fn web_languages_are_confined_to_the_ui_directory() {
        assert!(ScriptLanguage::Web.accepts_project_path(Path::new("ui/hud.html")));
        assert!(ScriptLanguage::Web.accepts_project_path(Path::new("ui/theme.css")));
        assert!(ScriptLanguage::Php.accepts_project_path(Path::new("ui/menu.php")));
        assert!(!ScriptLanguage::Web.accepts_project_path(Path::new("scripts/hud.html")));
        assert!(!ScriptLanguage::Php.accepts_project_path(Path::new("menu.php")));
        assert!(ScriptLanguage::JavaScript.accepts_project_path(Path::new("scripts/player.js")));
    }

    #[test]
    fn reload_preserves_compatible_properties_and_rejects_type_changes() {
        let old = vec![PublicProperty {
            name: "speed".into(),
            default: EngineValue::Number(1.0),
        }];
        let next = vec![
            PublicProperty {
                name: "speed".into(),
                default: EngineValue::Number(2.0),
            },
            PublicProperty {
                name: "active".into(),
                default: EngineValue::Boolean(true),
            },
        ];
        assert!(matches!(
            reload_compatibility(
                ScriptApiVersion::CURRENT,
                ScriptApiVersion::CURRENT,
                &old,
                &next
            ),
            ReloadCompatibility::Compatible { .. }
        ));
        let bad = vec![PublicProperty {
            name: "speed".into(),
            default: EngineValue::String("fast".into()),
        }];
        assert!(matches!(
            reload_compatibility(
                ScriptApiVersion::CURRENT,
                ScriptApiVersion::CURRENT,
                &old,
                &bad
            ),
            ReloadCompatibility::Incompatible { .. }
        ));
    }
}
