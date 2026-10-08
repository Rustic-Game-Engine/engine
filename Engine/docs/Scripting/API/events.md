# Events and signals API

Communicate through queued global events and object-scoped signals across scripting languages.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Events.on(name, callback)`
- `Events.once(name, callback)`
- `Events.connect(entity, name, callback)`
- `Events.emit(name, arguments = [], source = null)`
- `connection.disconnect() / Events.disconnect(connection)`

## Parameters

Name is the exact signal name. Callback receives payload arguments. Source is an optional stable entity ID or resolvable scene path. Payload supports booleans, finite numbers, strings/IDs, vectors and quaternions, not arbitrary nested objects.

## Return and timing

On/once/connect return a connection with disconnect. Emit queues delivery during Update before the user frame callback; it never synchronously enters another language runtime.

## Example

```javascript
globalThis.behavior = {
  Start() {
    Events.once("DoorOpened", player => print("opened by", player));
    Events.emit("DoorOpened", [rustic.entity_id()]);
  }
};
```

## Behavior and limitations

- The Console prints opened by followed by the owner ID on queued delivery. Once disconnects after its single delivery.
- Global on/once subscriptions are distinct from object signals. For an object signal use connect(object, name, callback) and emit(name, args, object).
- Language instances in the same play scene can communicate; no network or inter-scene transport is implied.
- Payload limits are 64 arguments, 4096 bytes per string and 16 KiB combined. The scene retains at most 4096 subscriptions and 4096 callbacks per queue.
- Destroying an object removes its scoped connections. Unrelated object destruction does not remove a global script subscription.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
