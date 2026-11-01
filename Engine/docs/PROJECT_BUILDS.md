# Build and distribute a project

Open a writable project in the editor, choose **File → Build Project…**, and select a target. Every build gets a new `project-*` folder in the project's configured builds directory. The dialog displays requirements, build failures, and the successful output path. Settings are saved in `config/build.json` when you build. Failed output folders are removed; previous successful builds remain available.

Game packages contain a validated snapshot of the **active scene, including unsaved edits**, other scenes in `scene/` or `scenes/`, assets in `assets/`, HTML/CSS UI in `ui/`, game settings, and registered script sources. Save other scenes before building. This packages the engine runtime and scripts; it does not translate scripts into machine code. The current snapshot collector uses these standard content folders. Projects with custom content paths should move referenced content into the standard folders before exporting.

## Windows game and custom installer

Build on Windows with `rustic-runtime.exe` beside the editor. Installed engine distributions include it; source users should build the workspace. A compatible Windows GPU/driver is required to run the game.

1. Select **Windows game (.exe)** and enter the application name.
2. Leave **Create a custom installer** enabled to build setup, or disable it for a portable game folder.
3. For setup, install **Inno Setup 6**. Expand **Design installer** and set the full path to `ISCC.exe`, for example `C:\Program Files (x86)\Inno Setup 6\ISCC.exe`. A compiler on PATH also works.
4. Set a numeric version such as `1.0.0`, publisher and welcome message. Choose modern/classic wizard styling and whether to offer a desktop shortcut. The dialog shows a text preview.
5. Optionally enter project-relative paths for a plain text license, `.ico` setup icon, and `.bmp` wizard image. For example, `branding/license.txt`, `branding/setup.ico`, and `branding/wizard.bmp`. Inno Setup validates image formats/dimensions; its standard large wizard image is 164 × 314 pixels. The compiled wizard displays these images; the editor preview displays text.
6. Click **Build Project**. Distribute `setup.exe`, or distribute the complete `game/` folder containing `<Application name>.exe` and `data/`. Launch the game by opening the executable; the editor is not required.

Setup installs per user into `%LOCALAPPDATA%\Programs\<Application name>`, adds a Start menu shortcut and an uninstaller, and optionally offers a desktop shortcut. The generated `installer.iss` is kept beside setup for inspection. The setup icon customizes the installer, not the game executable's embedded icon. These are unsigned builds; signing and store publication are separate steps.

The game opens its own window and isolated simulation, forwards keyboard, primary mouse and touch input, and stops the simulation when the window closes. Logs are written under `%LOCALAPPDATA%\RusticGames\<Application name>\logs` on Windows. Lua 5.4, JavaScript and HTML/CSS script support are bundled. Available sibling DLLs and `rustic-luau-host.exe` are copied. Other external script languages still require their supported toolchains on the player's machine; test the result on a clean machine.

## Playable website

Select **Website (playable game)** and click **Build Project**. The editor exports the current game automatically: its scene snapshot, registered scripts, assets, HTML/CSS game UI, browser player and compiled WebAssembly runtime. **Do not create a `web/` folder or prepare a separate website.** No game server is required.

The output's `website/` directory contains `index.html`, `player.js`, `rustic-browser.js`, `rustic-browser.wasm`, the game file manifest, and `data/`. Publish the complete directory to any static web host, including a subdirectory. For a local preview:

```sh
python -m http.server 8080 --directory <output>/website
```

Open `http://localhost:8080` and choose **Play game**. The browser runs the shared Rustic game simulation, physics, scene loading, renderer, Lua/JavaScript behaviors and audio locally. Keyboard input goes to the focused game canvas; mouse and touch input are mapped to the letterboxed game image. The Play button also enables browser audio. Opening `index.html` directly through `file://` does not work because browsers restrict file fetches.

Use a recent browser with WebAssembly exception handling and WebGL 2. The generated player reports missing/corrupted files, unsupported graphics and runtime initialization failures. Game files are verified before scripts execute. Browser file writes use the runtime's in-memory filesystem and disappear on reload. Native microphone capture and native audio-device selection are unavailable; ordinary game audio uses Web Audio.

**Script compatibility:** direct web and Android exports currently support Lua 5.4, JavaScript and HTML/CSS behaviors. Scripts that need external desktop processes (including Luau, Python, C/C++, C#, Java and PHP) cause an explicit build error. Standalone PHP pages also cause an error. They are not silently omitted or converted. Use supported behaviors for these targets.

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

Install an APK with `adb install <output>/artifacts/app-0.apk`. An AAB is a publishing artifact, not directly installable with adb. The app requires Android 8.0/API 26 or newer and an updated Android System WebView supporting WebGL 2 and WebAssembly exception handling. The same script/API compatibility limits as website builds apply.

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

- **Build is disabled:** check the Required messages. Read-only editors cannot build; close the other writable editor and reopen the project. Windows packaging requires a Windows host and its bundled runtime. Inno Setup is required only when creating setup. Web and Android exports require the engine's compiled `browser-runtime/` resources.
- **Installer compiler unavailable:** enter the full existing `ISCC.exe` path. Inspect the displayed compiler failure for invalid version or image resources.
- **Browser runtime missing:** install a distribution that includes `browser-runtime/` or build those engine resources from source. There is no project `web/` source folder to configure.
- **Website fails to load:** serve the whole generated `website/` directory over HTTP/HTTPS, check browser WebGL 2/WebAssembly support, and inspect the displayed error and browser console.
- **Unsupported script:** replace external-toolchain behaviors with supported Lua 5.4/JavaScript/HTML behaviors for direct web/Android exports.
- **Resource path rejected:** use a path inside the project without `..` or absolute components. Remove symbolic links from packaged game inputs. Outputs cannot be nested in their input.
- **Gradle fails:** the dialog includes its exit status and output. Check JDK/SDK compatibility, required platform/build-tools packages, SDK licenses, network access for Gradle dependencies, and release signing environment variables.
- **Game fails to launch:** distribute the whole `game/` folder or run setup; do not move only the executable. Restore modified/missing data files, check game logs, graphics drivers, and external language toolchains.
