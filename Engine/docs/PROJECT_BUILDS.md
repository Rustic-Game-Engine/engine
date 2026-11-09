# Build and distribute a project

Open a writable project in the editor, choose **File → Build Project…**, and select a target. Every build gets a new `project-*` folder in the project's configured builds directory. The dialog displays requirements, build failures, and the successful output path. Settings are saved in `config/build.json` when you build. Failed output folders are removed; previous successful builds remain available.

Game packages contain a validated snapshot of the **active scene, including unsaved edits**, other scenes in `scene/` or `scenes/`, assets in `assets/`, HTML/CSS UI in `ui/`, game settings, and registered script sources. Save other scenes before building. This packages the engine runtime and scripts; it does not translate scripts into machine code. The current snapshot collector uses these standard content folders. Projects with custom content paths should move referenced content into the standard folders before exporting.

## Windows game and custom installer

Build on Windows with `rustic-runtime.exe` beside the editor. Installed engine distributions include it; source users should build the workspace. A compatible Windows GPU/driver is required to run the game.

1. Select **Windows game (.exe)** and enter the application name.
2. Leave **Create a custom installer** enabled to build setup, or disable it for a portable game folder.
3. For setup, leave the **Inno Setup compiler** field as `ISCC.exe` (or blank). When you click **Build Project**, the background worker finds Inno Setup on PATH, in standard installation folders, or through its Windows uninstall registration. If missing, it downloads the signed Inno Setup 6.7.3 installer from the official GitHub release and installs it for the current user without administrator access. The compiler folder is added to your user PATH, and this build continues without restarting the editor. First installation requires internet access. To use a custom installation, expand **Design installer** and enter its existing absolute `ISCC.exe` path.
4. Set a numeric version such as `1.0.0`, publisher and welcome message. Choose modern/classic wizard styling and whether to offer a desktop shortcut. The dialog shows a text preview.
5. Optionally use **Browse…** beside the license, setup icon, and wizard image fields to select a plain text (`.txt`) license, `.ico` setup icon, and `.bmp` wizard image. The file explorer starts in your project folder and stores the selected path relative to it. Copy files from outside the project into it before selecting them; otherwise the dialog reports an error and keeps the previous value. You can also type or clear these optional paths directly. For example, `branding/license.txt`, `branding/setup.ico`, and `branding/wizard.bmp`. Inno Setup validates image formats/dimensions; its standard large wizard image is 164 × 314 pixels. The compiled wizard displays these images; the editor preview displays text.
6. Click **Build Project**. Distribute `setup.exe`, or distribute the complete `game/` folder containing `<Application name>.exe` and `data/`. Launch the game by opening the executable; the editor is not required.

Setup installs per user into `%LOCALAPPDATA%\Programs\<Application name>`, adds a Start menu shortcut and an uninstaller, and optionally offers a desktop shortcut. The generated `installer.iss` is kept beside setup for inspection. The setup icon customizes the installer, not the game executable's embedded icon. These are unsigned builds; signing and store publication are separate steps.

The game opens its own window and isolated simulation, forwards keyboard, primary mouse and touch input, and stops the simulation when the window closes. Logs are written under `%LOCALAPPDATA%\RusticGames\<Application name>\logs` on Windows. Lua 5.4, JavaScript and HTML/CSS script support are bundled. Available sibling DLLs and `rustic-luau-host.exe` are copied. Other external script languages still require their supported toolchains on the player's machine; test the result on a clean machine.

## Playable website

Select **Website (playable game)** in **File → Build Project…**. Choose the website mode:

| Mode | Languages | Hosting |
| --- | --- | --- |
| Hosted runtime (default for new build settings) | Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, HTML/CSS | Native runtime on a matching OS/architecture with GPU and required language toolchains |
| Static WebAssembly | Lua 5.4, JavaScript, HTML/CSS | Any static HTTP/HTTPS host |

Existing saved build settings retain static mode until you select hosted mode.

### Hosted runtime: all scripting languages

Choose **Hosted runtime (all scripting languages)** and click **Build Project**.
The generated `website/` folder contains the native runtime, available sibling
runtime libraries and Luau host, a verified game snapshot in `data/`, player files,
launchers, game title and `readme.md`. It requires no browser-runtime resources.
Registered scripts are retained in their original language and use the same
native gameplay adapters as desktop play; no translation is performed.

1. Keep the complete folder together. Run `start-website.cmd` on Windows, or
   `sh start-website.sh` on Linux/macOS. Use a host matching the exported binary.
2. Install the external toolchains used by the game on that host: Luau, Python,
   C/C++ compiler, .NET SDK, JDK including `javac`, and/or PHP. The included Luau
   host is used when present. Lua 5.4 and JavaScript are bundled. Missing
   registered-script toolchains produce a startup error in the host terminal.
3. Open `http://127.0.0.1:8080`. Each connection starts an independent native game
   session and temporary writable game copy; disconnecting removes that copy.
4. For another port pass `--website-address 127.0.0.1:9090` to the launcher.

For remote players, run on a GPU-capable server with a compatible graphics driver.
Use an HTTPS reverse proxy at the origin root; forward WebSocket upgrades for
`/play` and preserve the public Host and Origin headers. The default listener is
loopback. To bind a server interface pass `--website-address 0.0.0.0:8080`.
Static hosting alone cannot run this mode. No player-side language installation
is needed.

Keyboard, primary mouse, touch, native game UI (including PHP pages), and game
audio use the native simulation. Click/tap or press a key on the canvas to enable
sound. The host streams PNG-compressed 640 × 360 frames at up to 30 fps and
stereo 48 kHz audio. Bandwidth varies with scene complexity; network latency
adds input delay. Up to eight connections are accepted; inactive connections time
out after 30 seconds. Saves are connection-local and not persistent. Browser
microphone forwarding and player-side audio-device selection are unavailable;
native device APIs refer to the host. Run only trusted game packages under a
suitable host account, since native scripts have that account's privileges.

If the page fails, inspect both the browser error and host terminal. Restore
missing/corrupted game data, install reported toolchains, and check GPU drivers,
WebSocket forwarding, matching Host/Origin, proxy timeouts and bandwidth. Reload
after a disconnect to start a fresh session. Stop the launcher to stop hosting.

### Static WebAssembly

Choose **Static WebAssembly (Lua 5.4, JavaScript, HTML/CSS)** and click **Build Project**. The editor exports the current game automatically: its scene snapshot, registered scripts, assets, HTML/CSS game UI, browser player and compiled WebAssembly runtime. **Do not create a `web/` folder or prepare a separate website.** No game server is required.

The output's `website/` directory contains `index.html`, `player.js`, `rustic-browser.js`, `rustic-browser.wasm`, the game file manifest, `data/`, and a `readme.md` with local preview and publishing instructions. Publish the complete directory to any static web host, including a subdirectory. For a local preview:

```sh
python -m http.server 8080 --directory <output>/website
```

Open `http://localhost:8080`. The game starts automatically with no engine-added menu, Play button, or loading overlay; only startup failures display an error. The browser runs the shared Rustic game simulation, physics, scene loading, renderer, Lua/JavaScript behaviors and audio locally. Keyboard input goes to the focused game canvas; mouse and touch input are mapped to the letterboxed game image. Click, tap, or press a key in the game to enable audio; browsers require a player interaction before sound can start. Opening `index.html` directly through `file://` does not work because browsers restrict file fetches.

Use a recent browser with WebAssembly exception handling and WebGL 2. The generated player reports missing/corrupted files, unsupported graphics and runtime initialization failures. Game files are verified before scripts execute. Browser file writes use the runtime's in-memory filesystem and disappear on reload. Native microphone capture and native audio-device selection are unavailable; ordinary game audio uses Web Audio.

**Static script compatibility:** static web and Android exports currently support Lua 5.4, JavaScript and HTML/CSS behaviors. Scripts that need external desktop processes (including Luau, Python, C/C++, C#, Java and PHP) cause an explicit build error. Standalone PHP pages also cause an error. They are not silently omitted or converted. Use supported behaviors for these targets, or choose the hosted website mode for all scripting languages.

Installed engine distributions bundle `browser-runtime/` beside the editor. Game authors do not need Emscripten to export when those resources are installed. Source developers must build that runtime once with the setup below.

## Android APK and AAB

Select **Android APK (debug)** or **Android App Bundle (release)**. The editor **generates the Android project itself** and packages the same compiled game as local app assets. Do not create an `android/` folder or supply an Android app template. The generated app plays locally in Android System WebView, without a game server or an Internet permission. Touch, mouse and hardware-keyboard input use the shared browser player.

1. Install JDK 17 or newer and Android SDK **platform 36** plus **build-tools 36.0.0**. Accept SDK licenses with `sdkmanager --licenses`. `JAVA_HOME` can point to the JDK, or `java` can be on PATH. Set `ANDROID_HOME`/`ANDROID_SDK_ROOT` if the SDK is not in the standard Android Studio SDK directory.
2. Enter your own **Android package ID**, for example `com.mystudio.mygame`, a positive **version code**, and a numeric **version name** such as `1.0.0`. Increase the version code for each store update.
3. Build. The bundled Gradle 8.13 wrapper downloads its checksum-verified distribution and Android build dependencies on first use, so that first build requires network access unless those tools are already cached. No user Gradle template, Gradle installation or Android NDK is required.
4. Completed `.apk` or `.aab` files appear under `artifacts/`. The generated temporary Android project is removed after success. APK uses `assembleDebug` and Android's debug signing key. AAB uses `bundleRelease`.

For signed release bundles, set these environment variables before launching the editor:

```text
RUSTIC_ANDROID_KEYSTORE=<absolute path to your keystore>
RUSTIC_ANDROID_STORE_PASSWORD=<store password>
RUSTIC_ANDROID_KEY_ALIAS=<key alias>
RUSTIC_ANDROID_KEY_PASSWORD=<key password>
```

The generated build reads signing credentials from the environment rather than writing them into the project. Keep the keystore outside game assets. Without release signing configuration, an AAB can be produced unsigned; store publication still requires the appropriate signing and testing steps.

Install an APK with `adb install <output>/artifacts/app-0.apk`. An AAB is a publishing artifact, not directly installable with adb. The app requires Android 8.0/API 26 or newer and an updated Android System WebView supporting WebGL 2 and WebAssembly exception handling. The same script/API compatibility limits as static website builds apply.

## Build browser resources from engine source

This is an engine-development step, not a requirement to author a separate website or Android app. Install Python 3, Rust, **Emscripten SDK 6.0.13**, and libclang (for QuickJS bindings). Activate the SDK's `emsdk_env` script, then run from `Engine/`:

```sh
python tools/build-browser-runtime.py
```

The helper installs the Rust Emscripten target and compiles the shared runtime into `target/distribution/browser-runtime/`. For a development editor built with the `dev-fast` profile, use `python tools/build-browser-runtime.py --profile dev-fast`. Keep that directory beside the editor executable. The Windows installer build performs the browser-runtime build and includes its outputs.

The browser runtime uses the engine's single-threaded game session and GLES renderer, rather than recreating gameplay in an unrelated JavaScript engine. Portable snapshots use normalized path separators and fixed-width checksum lengths so Windows/64-bit builds can be verified in WebAssembly/32-bit execution. The desktop runtime still reads legacy version-1 snapshots on their originating architecture.

To run the browser export smoke test after building the development runtime:

```sh
npm ci --prefix tools/browser-tests
npx --prefix tools/browser-tests playwright install chromium
npm test --prefix tools/browser-tests
```

The test exports a real scene and checks rendering, Lua/JavaScript startup, keyboard movement, touch/object clicks and rejection of corrupted game data.

## Troubleshooting

- **Build is disabled:** check the Required messages. Read-only editors cannot build; close the other writable editor and reopen the project. Windows packaging requires a Windows host and its bundled runtime. Inno Setup is configured automatically only when creating setup; a missing default compiler does not disable the build. Static website and Android exports require the engine's compiled `browser-runtime/` resources; hosted websites require the native runtime.
- **Inno Setup configuration failed:** check the displayed error, internet access to GitHub releases, and permission to install in your local Programs folder and update your user PATH, then retry **Build Project**. A failed signature check stops installation. You can also install Inno Setup 6 manually and enter its existing absolute `ISCC.exe` path. Invalid custom paths are reported in **Required**; reset the field to `ISCC.exe` for automatic setup. Inspect compiler failures for invalid version or image resources.
- **Browser runtime missing:** install a distribution that includes `browser-runtime/` or build those engine resources from source. There is no project `web/` source folder to configure.
- **Website fails to load:** serve the whole generated `website/` directory over HTTP/HTTPS, check browser WebGL 2/WebAssembly support, and inspect the displayed error and browser console.
- **Unsupported script:** select Hosted runtime for a website using external languages; static web/Android builds require Lua 5.4/JavaScript/HTML behaviors.
- **Resource path rejected:** use a path inside the project without `..` or absolute components. Remove symbolic links from packaged game inputs. Outputs cannot be nested in their input.
- **Gradle fails:** the dialog includes its exit status and output. Check JDK/SDK compatibility, required platform/build-tools packages, SDK licenses, network access for Gradle dependencies, and release signing environment variables.
- **Game fails to launch:** distribute the whole `game/` folder or run setup; do not move only the executable. Restore modified/missing data files, check game logs, graphics drivers, and external language toolchains.
