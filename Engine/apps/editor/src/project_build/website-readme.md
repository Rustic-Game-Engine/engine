# Run this game

This folder contains a complete playable website. Keep all files and the `data/`
folder together. The game starts automatically when you open the served website.

## Run locally

1. Install Python 3 if it is not already installed.
2. Open a terminal in this folder (the folder containing `index.html`).
3. Start a local web server:

   ```sh
   python -m http.server 8080
   ```

   On Windows, use `py -m http.server 8080` if `python` is not available.
   On macOS/Linux, use `python3 -m http.server 8080` if needed.
4. Open http://localhost:8080 in a recent browser with WebGL 2 and WebAssembly
   exception handling support. The game loads automatically. Click, tap, or press
   a key in the game to enable sound; browsers require interaction before audio.

Keep the terminal running while playing. Press Ctrl+C in the terminal to stop
the server. If port 8080 is busy, use another port such as 8081 in both the command
and browser address.

Opening `index.html` directly (`file://`) does not work because browsers restrict
file loading. If the game fails to start, check its displayed error and the
browser developer console, and confirm that all exported files are present.

## Publish online

Upload the complete contents of this folder to a static website host, preserving
filenames and subfolders. Serve `index.html` as the entry page over HTTP/HTTPS
and `.wasm` files as `application/wasm`. Subdirectory hosting is supported.
Players do not need the editor, Python, or a separate game server to play online.
Runtime file writes are temporary and disappear when the page reloads.
