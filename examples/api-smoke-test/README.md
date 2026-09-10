# Gameplay API smoke-test entry scripts

This directory contains one minimal entry script for every language accepted by
Rustic Game Engine. Each script reads the API's state, exercises the harmless
mutation commands, logs a success message, and leaves the behavior enabled.

Copy the desired file into a project's `scripts/` directory (or `ui/` for PHP and
Web), add it through **Programming > Create Behavior**, and attach it to an entity.
The behavior should declare a public Number property named `smoke_value` so the
`set_property` check has a valid target.

The native Lua/JavaScript/Web scripts exercise the complete in-process API. External
languages exercise every host-protocol command (`set_translation`, `set_property`,
`edit_attribute`, `log`, `set_enabled`, `add_instance`, and `clone_instance`) and
read every field supplied by protocol version 1. Structural `add`/`clone` checks run
only once, during `on_start`.

`entry.css` is included because CSS is an accepted Web entry type, but CSS has no
callable gameplay API. Use `entry.html` to test the Web JavaScript bridge.

