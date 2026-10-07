#![forbid(unsafe_code)]
use mlua::{Function, Lua, LuaSerdeExt, Value, Variadic};
use std::io::{BufRead, Read, Write};

fn main() {
    if let Err(error) = run() {
        eprintln!("Rustic Luau: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--version") {
        println!("Rustic Luau host {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let validate = args.first().is_some_and(|a| a == "--validate");
    let path = args
        .get(usize::from(validate))
        .ok_or("missing source path")?;
    let source = std::fs::read(path)?;
    if source.len() > 1024 * 1024 {
        return Err("source exceeds 1 MiB".into());
    }
    let source = std::str::from_utf8(&source)?;
    let lua = Lua::new();
    lua.set_memory_limit(64 * 1024 * 1024)?;
    let script = lua.load(source).set_name(path).into_function()?;
    if validate {
        return Ok(());
    }
    // SDK output is explicitly flushed; no CLI prompt or console buffering.
    lua.globals().set(
        "print",
        lua.create_function(|_, values: Variadic<Value>| {
            let mut out = std::io::stdout().lock();
            for (index, value) in values.into_iter().enumerate() {
                if index > 0 {
                    out.write_all(b"\t").map_err(mlua::Error::external)?;
                }
                match value {
                    Value::String(s) => out
                        .write_all(&s.as_bytes())
                        .map_err(mlua::Error::external)?,
                    _ => return Err(mlua::Error::external("SDK output must be text")),
                }
            }
            out.write_all(b"\n").map_err(mlua::Error::external)?;
            out.flush().map_err(mlua::Error::external)
        })?,
    )?;
    lua.sandbox(true)?;
    let environment = lua.create_table()?;
    let metatable = lua.create_table()?;
    metatable.set("__index", lua.globals())?;
    environment.set_metatable(Some(metatable))?;
    lua.load(include_str!(
        "../../../crates/engine-scripting/src/sdk/rustic.luau"
    ))
    .set_environment(environment.clone())
    .exec()?;
    script.set_environment(environment.clone())?;
    environment.set("__rustic_load", script)?;
    let invoke: Function = environment.get("__rustic_invoke")?;
    let mut input = std::io::stdin().lock();
    loop {
        let mut line = Vec::new();
        if input
            .by_ref()
            .take(1024 * 1024 + 1)
            .read_until(b'\n', &mut line)?
            == 0
        {
            break;
        }
        if line.len() > 1024 * 1024 {
            return Err("request exceeds 1 MiB".into());
        }
        let state: serde_json::Value = serde_json::from_slice(&line)?;
        invoke.call::<()>(
            lua.to_value_with(
                &state,
                mlua::SerializeOptions::new()
                    .serialize_none_to_null(false)
                    .serialize_unit_to_null(false),
            )?,
        )?;
    }
    Ok(())
}
