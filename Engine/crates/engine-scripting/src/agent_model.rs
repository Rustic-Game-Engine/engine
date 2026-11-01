//! Agent-only authoring scripts; never attached to a game or executed at play startup.
use mlua::{Lua, LuaOptions, StdLib, Table};
use std::fmt::Write as _;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// Execute a bounded Lua mesh recipe, returning an OBJ asset for the normal importer.
/// The recipe returns `vertices` (xyz triples) and `triangles` (one-based index triples).
/// # Errors
/// Rejects invalid geometry, unavailable APIs, excess memory and instruction budgets.
pub fn build_agent_model(source: &str) -> Result<String, String> {
    if source.len() > 256 * 1024 {
        return Err("model script exceeds 256 KiB".into());
    }
    let run = || -> mlua::Result<String> {
        let lua = Lua::new_with(
            StdLib::MATH | StdLib::TABLE | StdLib::STRING,
            LuaOptions::default(),
        )?;
        lua.set_memory_limit(32 * 1024 * 1024)?;
        // No filesystem, OS, package loader, debug library or game API is exposed.
        for name in ["dofile", "loadfile", "load", "collectgarbage"] {
            lua.globals().set(name, mlua::Value::Nil)?;
        }
        let budget = Arc::new(AtomicUsize::new(0));
        lua.set_hook(
            mlua::HookTriggers::new().every_nth_instruction(1000),
            move |_, _| {
                if budget.fetch_add(1000, Ordering::Relaxed) >= 2_000_000 {
                    return Err(mlua::Error::RuntimeError(
                        "model instruction budget exceeded".into(),
                    ));
                }
                Ok(mlua::VmState::Continue)
            },
        )?;
        let model: Table = lua.load(source).set_name("agent-model").eval()?;
        let vertices: Vec<[f32; 3]> = model
            .get::<Table>("vertices")?
            .sequence_values()
            .map(|v| {
                let row: Table = v?;
                Ok([row.get(1)?, row.get(2)?, row.get(3)?])
            })
            .collect::<mlua::Result<_>>()?;
        let triangles: Vec<[usize; 3]> = model
            .get::<Table>("triangles")?
            .sequence_values()
            .map(|v| {
                let row: Table = v?;
                Ok([row.get(1)?, row.get(2)?, row.get(3)?])
            })
            .collect::<mlua::Result<_>>()?;
        if vertices.len() < 3
            || vertices.len() > 50_000
            || triangles.is_empty()
            || triangles.len() > 100_000
        {
            return Err(mlua::Error::RuntimeError(
                "expected 3..50000 vertices and 1..100000 triangles".into(),
            ));
        }
        if vertices
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 100_000.0)
            || triangles
                .iter()
                .flatten()
                .any(|i| *i == 0 || *i > vertices.len())
        {
            return Err(mlua::Error::RuntimeError(
                "non-finite/out-of-range vertex or triangle index".into(),
            ));
        }
        let mut obj = String::from("# Rustic agent model\n");
        for [x, y, z] in vertices {
            let _ = writeln!(obj, "v {x} {y} {z}");
        }
        for [a, b, c] in triangles {
            let _ = writeln!(obj, "f {a} {b} {c}");
        }
        Ok(obj)
    };
    run().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builds_real_mesh_and_bounds_scripts() {
        let obj =
            build_agent_model("return {vertices={{0,0,0},{1,0,0},{0,1,0}},triangles={{1,2,3}}}")
                .unwrap();
        assert!(obj.contains("f 1 2 3"));
        for source in [
            "while true do end",
            "return io.open('project.engine')",
            "return os.execute('whoami')",
            "return {vertices={{0,0,0},{1,0,0},{0,1,0}},triangles={{1,2,9}}}",
        ] {
            assert!(build_agent_model(source).is_err());
        }
    }
}
