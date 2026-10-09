# Shared gameplay API architecture

`engine-core::gameplay` owns easing, interpolation, paths, actions, composition,
clip evaluation, marker delivery, IK, collision queries, PCM mixing and signals.
Language adapters construct typed requests and retain VM-local callbacks. One
scheduler, physics world, mixer and clip registry belong to each play scene.

An operation carries scene/entity/script ownership, an opaque handle, playback
controls and an action tree. Trees contain property/value tweens, movement,
paths, shake, waits, callbacks, animation, sequence and parallel nodes. Validation
bounds data size, depth, node counts and callback queues before scheduling.
Sequences consume leftover elapsed time; parallel nodes wait for their longest
child. Callbacks are queued for the owning VM and run outside scheduler locks.

A property target is an entity ID plus a property name. `GameplayWorld` resolves
properties and rejects unsupported targets. Scalars and vectors interpolate in
Rust; quaternion interpolation follows the shortest arc and supports eased
overshoot. Scene writes validate f32 conversion and backend property ranges.
Numeric keyframe callbacks adapt script/component/UI state to the same scheduler.

Every time-based system uses the same 31 `Ease` curves. Geometry is separate from
path traversal: global and segment easing change progress, never control points.
The validated scene clock drives actions, physics and audio source consumption;
voice and operation pause are independent controls. Audio output has a bounded
queue and consumes mixed PCM outside scene/script locks.

Imported glTF/GLB/FBX data retains nodes, tracks, skin palettes and inverse binds.
The model importer validates hierarchy and skin data. Source nodes bind before
script Start; named clip references resolve entirely in the engine before a tree
is scheduled. Source clip clocks stay linear unless progression easing is opted
into. Blend-in/out weights, masked layers and additive deltas are separate from
source timing. The renderer deforms vertices from live joint matrices.

All ten script types use these services. Lua/JS use in-process calls; external
hosts use bounded synchronous queries for math, casts and resource handles, and
queued requests for actions. Queries flush earlier commands before reading state.
Script authors use SDK functions, typed values/options and callbacks; the SDK owns
the transport protocol. Operation and signal snapshots release VM callback
references when owners or targets disappear.

Scene stop/destruction, owner disable/failure and successful reload remove actions,
voices and subscriptions. Failed reload preserves the prior owner's live actions.
Public serializable types form the extension boundary for future property adapters,
animation graphs, timelines, path editors and visual easing previews. User-facing
setup, API examples and backend limits are in [Gameplay actions](Scripting/gameplayActions.md).
