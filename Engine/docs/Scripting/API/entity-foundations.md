# Entity foundations

Use the shared runtime world to label, organize, reparent, query and destroy objects.
These are live synchronous queries, so a change is visible to the next query in the
same callback, including queries from another scripting language.

## Setup and language support

Create an object in the editor. Use **Programming > New Script > Object Component
Script**, choose a language, and attach the script to that object with **+ Add
Component** or by dragging it from Explorer. Press **Play**. Do not edit scene or
manifest files to attach a behavior. Stop Play to restore the authored scene.

The SDKs provide these calls in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java,
and PHP; inline HTML JavaScript shares the JavaScript calls. External languages
require their existing toolchains. PHP and HTML remain restricted to `ui/` assets;
for those scripts pass a real scene entity ID/path rather than the UI script owner.
The HTML adapter does not provide a browser DOM or interactive HTML controls.

Python imports services from `rustic`; C includes `rustic.h`; C++ includes
`rustic.hpp` and uses `Tags::add`; C# uses `using static Rustic`; Java extends
`Rustic` or qualifies `Rustic.Tags`; PHP uses `Tags::add`. Lua and JavaScript use
`Tags.add`. New service methods use the exact snake_case spellings below.

## Parameters

| Argument | Type and meaning |
| --- | --- |
| entity | Live stable UUID string or scene path; SDK object-path handles are accepted in Lua, JavaScript and Python. Passing null/nil/None to an entity parameter selects the component owner. A global/UI owner need not be a real scene entity. |
| name | Case-sensitive tag/group string, or exact component schema string for Components. Tag/group names must contain non-whitespace text, be at most 128 UTF-8 bytes, and contain no control characters. |
| parent | Entity ID/path or null to detach. C uses NULL, C++ uses an empty string, Python None, Lua/Luau nil. Required argument. |
| keep_world | Boolean, default true. With false, keep the local transform. C requires the explicit boolean; Java has a true-default overload. |
| delay | Finite nonnegative seconds on the scaled fixed simulation clock. |
| center | Three finite numbers in world coordinates. C/C++ use RusticVector3; other SDKs use arrays/tables. Values must fit finite f32 scene coordinates. |
| radius | Finite nonnegative world-space distance. |

## Return and timing

| Signature | Return |
| --- | --- |
| Tags.add(entity, name) | bool: true if newly added |
| Tags.remove(entity, name) | bool: true if removed |
| Tags.find_all(name) | entity ID list |
| Groups.create(name) | bool: true if newly created |
| Groups.add(name, entity) | bool: true if newly added |
| Groups.get_members(name) | entity ID list |
| Hierarchy.set_parent(entity, parent, keep_world = true) | void |
| Hierarchy.get_children(entity) | direct child entity ID list |
| Hierarchy.detach(entity, keep_world = true) | void |
| Entity.destroy(entity) | void |
| Entity.destroy_after(entity, delay) | void |
| Entity.is_valid(entity) | bool |
| Components.has(entity, name) | bool |
| Components.get_all(entity) | component schema/script ID string list |
| Components.find_in_children(entity, name) | descendant entity ID list |
| World.find_by_tag(name) | entity ID list; same implementation as Tags.find_all |
| World.find_in_radius(center, radius) | entity ID list |

Lists are deterministic in ascending stable ID order (component names sort
lexically). Lua/Luau use tables, JavaScript/Python/PHP arrays/lists, Java and C#
string arrays, C++ `std::vector<std::string>`, and C `RusticList` with `count` and
`items`. C query strings/lists live until the next callback; copy them to retain
across callbacks. An empty query returns an empty list, never a fake entity.

## Behavior and limitations

- Tags, groups and destruction deadlines belong to the runtime world. They reset
  on Stop/new Play and are not serialized into authored scenes or save files.
  Tags are idempotent, with at most 128 tags per entity; at most 4096 groups exist
  per world. A group must be created before use. Empty groups remain available.
- Destruction removes the entire subtree in one validated command batch and
  invalidates generational references. Tags, group membership and deadlines clean
  up even when another engine subsystem despawns an object. A query that uses a
  destroyed entity errors, except `Entity.is_valid`, which returns false for a
  well-formed absent UUID. Malformed UUIDs are errors. IDs are never raw pointers.
- A destroyed component owner stops receiving Update/FixedUpdate; OnDisable and
  OnDestroy run once at the runtime cleanup boundary, and owned operations/signals
  are released. The current callback can finish; do not read owner properties
  after destroying it. Global scripts do not end when an unrelated object dies.
- A new `destroy_after` replaces the old deadline. Zero destroys immediately;
  positive delays advance once per fixed step using the existing Clock time
  scale. Clock pause and paused Play stop countdowns. Destruction occurs at a fixed
  boundary, so timing is quantized to simulation ticks and does not depend on
  render frequency. Deadlines survive the scheduling script's successful reload;
  they belong to the entity/world, not a script callback.
- Reparenting defaults to preserving the world pose, including descendants. It
  validates both entities and rejects cycles before mutation. A nonuniform parent
  scale combined with rotation can require shear; the current TRS representation
  cannot preserve that pose, and the call errors without mutation. Pass false
  only if keeping the local transform is intended.
- Components queries report the existing authored built-ins: rustic.transform,
  rustic.part_attributes, rustic.name, rustic.folder, rustic.parent, rustic.mesh,
  rustic.material, rustic.camera, rustic.light, rustic.primitive, rustic.scripts;
  each attached script also reports `script:<Asset ID>`. Missing names return
  false/no matches. These are presence queries, not component payload handles;
  disabled script components are still present. Unknown preserved serialized
  schemas and derived private ECS bookkeeping are not reported.
- find_in_children searches every descendant and excludes the root. get_children
  returns only immediate children. Radius queries measure world **origins**, not
  collider extents: use [Physics queries](/docs/api/physics-actions/queries) for
  collision overlap. A zero radius matches coincident origins.
- Invalid names, absent objects/groups, non-finite/negative delays or radii, bad
  vector sizes, cycles and unsupported transforms produce script errors through
  the existing adapter. Lua/PHP/Python/etc. can catch errors using their normal
  mechanisms; C's SDK reports a callback failure. Uncaught callback failures use
  the existing script disabling/logging behavior. Check the Play Console.
- Tag/radius/component searches are linear scans. Hierarchy changes/destruction
  validate a whole command batch; descendant operations scale with the subtree.
  Cache query results for a frame and use direct IDs for frequent queries. This
  implementation adds no new dependencies or GPU/backend-specific requirements.
  Linux headless tests are verification evidence, not Windows/macOS hardware
  qualification or a guarantee of exported-game packaging.
- Dynamic component attachment/activation, prefabs, pooling, scene streaming,
  navigation, advanced physics/rendering and DOM UI are separate unfinished work.
  See the [expansion status](/docs/guides/scripting-expansion-status). Existing
  [events](/docs/api/events) and [timers](/docs/api/timer) remain the callback tools.

## Example

Create and attach this Lua Object Component Script to one object. Press Play.
Expect `entity foundations ready` in the Console; after one scaled fixed second,
the object disappears. Stop Play to restore it. No extra assets are required.

```lua
return {Start=function()
    local e=rustic.entity_id()
    Tags.add(e,'enemy')
    Groups.create('enemies')
    Groups.add('enemies',e)
    assert(Entity.is_valid(e))
    assert(Components.has(e,'rustic.transform'))
    assert(#Tags.find_all('enemy')==1)
    rustic.log('info','entity foundations ready')
    Entity.destroy_after(e,1)
end}
```

Python uses the same live services:

```python
from rustic import rustic, Tags, Groups, Entity, Components, run

def on_start():
    e = rustic.entity_id()
    Tags.add(e, 'enemy')
    Groups.create('enemies')
    Groups.add('enemies', e)
    assert Entity.is_valid(e)
    assert Components.has(e, 'rustic.transform')
    assert len(Tags.find_all('enemy')) == 1
    rustic.log('info', 'entity foundations ready')
    Entity.destroy_after(e, 1)

run(globals())
```

For a world-preserving hierarchy edit in JavaScript, use live IDs returned by
`Game.scene.Find` for existing objects:

```javascript
const child = Game.scene.Find('Child');
const parent = Game.scene.Find('Parent');
Hierarchy.set_parent(child, parent); // child keeps its world pose
Hierarchy.detach(child);            // child keeps that pose as a root
```

A missing path returns null from Find; check the lookup result before passing it
as an entity parameter, because null selects the behavior owner.

## Tags.add

Add a runtime tag. Repeated additions return false.

Signature: `Tags.add(entity, name)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Tags.add(rustic.entity_id(), "enemy")
```

## Tags.remove

Remove a runtime tag; return false if it was absent.

Signature: `Tags.remove(entity, name)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Tags.remove(rustic.entity_id(), "enemy")
```

## Tags.find_all

Find all live tagged entities in stable ID order.

Signature: `Tags.find_all(name)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Tags.find_all("enemy")
```

## Groups.create

Create an empty runtime group; return false if it already exists.

Signature: `Groups.create(name)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Groups.create("enemies")
```

## Groups.add

Add an entity to an existing group; return false for existing membership.

Signature: `Groups.add(name, entity)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Groups.add("enemies", rustic.entity_id())
```

## Groups.get_members

Return live members of an existing group.

Signature: `Groups.get_members(name)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Groups.get_members("enemies")
```

## Hierarchy.set_parent

Change the parent transactionally, rejecting cycles and unrepresentable world poses.

Signature: `Hierarchy.set_parent(entity, parent, keep_world = true)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Hierarchy.set_parent(rustic.entity_id(), nil, true)
```

## Hierarchy.get_children

Return direct children; use Components.find_in_children to search descendants.

Signature: `Hierarchy.get_children(entity)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Hierarchy.get_children(rustic.entity_id())
```

## Hierarchy.detach

Make an entity a root, preserving the world pose by default.

Signature: `Hierarchy.detach(entity, keep_world = true)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Hierarchy.detach(rustic.entity_id())
```

## Entity.destroy

Immediately remove the entity and its entire subtree. Further reads fail safely.

Signature: `Entity.destroy(entity)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Entity.destroy(rustic.entity_id())
```

## Entity.destroy_after

Replace the entity destruction deadline on the scaled fixed clock. Zero destroys now.

Signature: `Entity.destroy_after(entity, delay)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Entity.destroy_after(rustic.entity_id(), 1)
```

## Entity.is_valid

Check a well-formed stable UUID; absent IDs return false, malformed IDs error.

Signature: `Entity.is_valid(entity)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Entity.is_valid(rustic.entity_id())
```

## Components.has

Check presence of an exact component schema/script ID. Missing names return false.

Signature: `Components.has(entity, name)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Components.has(rustic.entity_id(), "rustic.transform")
```

## Components.get_all

Return sorted component schema and script ID strings, not mutable component handles.

Signature: `Components.get_all(entity)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Components.get_all(rustic.entity_id())
```

## Components.find_in_children

Return all descendants having the component; exclude the root.

Signature: `Components.find_in_children(entity, name)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
Components.find_in_children(rustic.entity_id(), "rustic.transform")
```

## World.find_by_tag

Use the existing Tags.find_all implementation to query tagged objects.

Signature: `World.find_by_tag(name)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
World.find_by_tag("enemy")
```

## World.find_in_radius

Find entity world origins inside an inclusive radius; no physics collider test.

Signature: `World.find_in_radius(center, radius)`. Parameter types, return values, lifecycle, errors and
performance are specified in the tables and limitations above.

```lua
World.find_in_radius({0, 0, 0}, 5)
```
