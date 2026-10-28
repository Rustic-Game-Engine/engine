# Load, unload and switch scenes (Rust)

`engine_world::SceneManager` owns explicitly loaded scene worlds. Use this API
in a Rust host with a path dependency on `crates/engine-world`. It is not exposed
as a Lua, JavaScript, C, C++, C# or Python script function. No script attachment
is required. Editor Play still uses its existing single-scene runtime.

```rust
use engine_world::{SceneManager, SceneManagerError};

fn main() -> Result<(), SceneManagerError> {
    let mut scenes = SceneManager::new();
    let menu = scenes.load_file("assets/menu.rscene")?;
    let level = scenes.load_file("assets/level.rscene")?;
    scenes.switch(menu)?;
    scenes.switch(level)?;
    scenes.unload(menu)?;
    assert!(!scenes.is_loaded(menu));
    assert!(scenes.switch(menu).is_err());
    assert_eq!(scenes.active_id(), Some(level));
    Ok(())
}
```

Replace the paths with valid saved scene files. The example leaves the level
active and loaded, and releases the menu world. `load(SceneDocument)` also
accepts an owned document created or loaded by the host.

| Operation | Result |
| --- | --- |
| `load(document)` / `load_file(path)` | Build a world and return its scene ID, without selecting it. |
| `switch(id)` | Select a loaded world. An unknown or unloaded ID returns `NotLoaded`. |
| `unload(id)` | Drop that world and its entities/components. Unsaved edits are discarded. |
| `is_loaded(id)` / `loaded_ids()` | Inspect the worlds currently owned by the manager. |
| `active_id()` / `active_world()` / `active_world_mut()` | Inspect or edit the selected world; initially `None`. |
| `world(id)` / `world_mut(id)` | Borrow any loaded world by ID. |

Switching retains loaded worlds and their edits. Switching to the current world
is allowed. Unloading the active world clears the selection, even if other worlds
remain loaded. Load or switch another world explicitly. Duplicate loads return
`AlreadyLoaded`; unloading an unknown ID returns `NotLoaded`. Failed operations
preserve the loaded set and selection.

The manager retains no unloaded entry, document, serialized bytes, or world cache.
Reloading reads the file again and builds a fresh world. File loading rejects
incompatible versions, damaged data and checksum mismatches, and does not read
backup files. Inspect `SceneManagerError` for I/O, parsing or world validation
failures; open and save externally edited scenes in the editor to refresh their
checksum before runtime loading.

The host must tick and render the selected world, and coordinate script callbacks,
physics, audio and GPU resources separately. The manager owns ECS world data only;
assets or resources retained elsewhere in the host are outside its ownership.
Loading a document does not execute its startup scripts. Save authored changes
explicitly before unloading if they should survive.
