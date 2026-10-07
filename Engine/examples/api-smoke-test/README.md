# Gameplay API smoke-test entry scripts

This directory contains entries for all supported language types. Import the
chosen file into your project's scripts/ directory (ui/ for PHP/Web), then attach
it through Programming or the Inspector. Keep the editor-assigned Asset ID.

Lua, JavaScript, C++ and HTML demonstrate additional API operations. Python, C#,
C, Java, PHP and Luau use the editor's API-only movement starters described below.
All scripts call engine-owned functions; none require a user-written JSON loop.

`entry.css` is included because CSS is an accepted Web entry type, but CSS has no
callable gameplay API. Use `entry.html` to test the Web JavaScript bridge.


## Built-in API starters

Python, C#, C, Java, PHP, and Luau entries use the same API-only starter as the
editor. Attach each entry to a Part using the editor, press Play, focus the embedded
viewport, and hold W. Each should log Behavior started once and move along positive
Z at one unit per second. Rustic supplies all SDK files; do not add transport code.
The engine-scripting integration suite separately checks callback persistence,
multiple commands, enable/disable callbacks and string escaping across toolchains.
