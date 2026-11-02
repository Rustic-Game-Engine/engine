# Hosted website game

This package runs all Rustic gameplay scripting languages through the native game
runtime. Players need only a browser with WebSocket and Canvas support. The host
needs the same OS/architecture as this exported runtime, a compatible GPU/driver,
and the external language toolchains used by the game (Luau, Python, C/C++, .NET,
JDK including javac, and/or PHP). Lua 5.4 and JavaScript are bundled. The Luau host
and sibling runtime libraries are copied when present. No scripts are translated
or silently omitted.

1. Keep the complete `website/` folder together.
2. On Windows run `start-website.cmd`. On Linux/macOS run `sh start-website.sh`.
3. Open `http://127.0.0.1:8080` in a browser. The game starts automatically.
4. To choose a port, pass `--website-address 127.0.0.1:9090` to the launcher.

For remote players, run the package on a compatible GPU-capable server and put an
HTTPS reverse proxy in front of the listening address. Forward WebSocket upgrades
for `/play` and preserve the external Host and Origin headers. Host at the origin
root. The listener defaults to loopback; to listen on a server network interface,
pass `--website-address 0.0.0.0:8080`. Static hosts such as GitHub Pages cannot run
this package. Use the engine's Static WebAssembly mode for those hosts, with its
smaller set of supported languages.

The host renders 640 × 360 frames at up to 30 fps and streams PNG-compressed
frames and stereo 48 kHz audio. Bandwidth depends on scene complexity; latency
and bandwidth affect playability. Click/tap the canvas to enable browser audio.
Keyboard, primary mouse and touch input work. There is a maximum of eight
connections and idle connections time out after 30 seconds. Each player has an
independent simulation and temporary writable game copy; disconnecting removes
that copy. Saves do not persist between connections. Game UI follows the native
runtime's HTML/CSS/PHP rendering rules. Microphone and device selection refer to
the host, not the player's device; browser microphone forwarding is unavailable.

Only the packaged game's scripts execute. Do not run untrusted game packages:
native scripts have the host account's ordinary privileges. The listener exposes
only the player, game title and play endpoint, never the game data or scripts.
Use a dedicated host account and your reverse proxy's access controls when needed.

If startup fails, read the terminal output for missing toolchains, snapshot
integrity errors or GPU errors. Runtime initialization errors also appear in the
browser. If the browser disconnects behind a proxy, check WebSocket forwarding,
matching Host/Origin, timeouts and bandwidth. Close the launcher to stop hosting.
