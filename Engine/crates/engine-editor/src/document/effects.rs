//! Shared assets refresh authored defaults while saved instance overrides survive.
use engine_world::{SceneDiagnostic, SceneDiagnosticSeverity, SceneWorld};
use std::{collections::BTreeSet, path::Path};

pub(super) fn refresh_sources(world: &mut SceneWorld, root: &Path) -> Vec<SceneDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with('.')
                    && !matches!(
                        name.as_ref(),
                        "target" | "node_modules" | "temp" | "cache" | "dist"
                    )
                {
                    pending.push(path);
                }
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("effect"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut loaded = BTreeSet::new();
    for path in files {
        let sidecar = engine_assets::sidecar_path(&path);
        if !sidecar.is_file() {
            continue;
        }
        let result = (|| {
            let meta = engine_assets::load_meta(&sidecar).map_err(|error| error.to_string())?;
            if !loaded.insert(meta.asset_id) {
                return Err(format!(
                    "Duplicate stable effect asset identity {}",
                    meta.asset_id
                ));
            }
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            let asset = engine_world::effects::EffectAsset::from_bytes(&bytes)?;
            world
                .register_effect_asset(meta.asset_id, asset)
                .map_err(|error| error.to_string())
        })();
        if let Err(error) = result {
            diagnostics.push(SceneDiagnostic { severity:SceneDiagnosticSeverity::Warning, entity:None, component:Some("Effect".into()),
                message:format!("Could not refresh effect asset {}: {error}; saved configuration remains available",path.display()) });
        }
    }
    for entity in world.entity_ids() {
        if let Ok(Some(effect)) = world.effect(entity)
            && let Some(source) = effect.source
            && !world.has_effect_asset(source.asset)
        {
            diagnostics.push(SceneDiagnostic {
                severity: SceneDiagnosticSeverity::Warning,
                entity: Some(entity),
                component: Some("Effect".into()),
                message: format!(
                    "Shared effect asset {} is unavailable; using saved instance configuration",
                    source.asset
                ),
            });
        }
    }
    diagnostics
}
