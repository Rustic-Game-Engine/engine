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

## Static website

Prepare an existing static site in the project's `web/` folder, or select another project-relative folder. It must contain `index.html`.

```text
MyProject/
  web/
    index.html
    styles.css
    images/
      logo.png
```

Select **Website (static files)** and build. Upload the contents of the output's `website/` folder to a static web host. For a local preview, run `python -m http.server 8080 --directory <output>/website` and open `http://localhost:8080`.

This target copies deployable website files. Framework projects must first produce a static export; server-rendered applications need their own server deployment. This does not compile Rustic scenes to WebAssembly or supply the Rustic gameplay API in the browser. Browser game exports need a separate compatible web runtime. Symbolic links are rejected, and `.git`, `.gradle`, and `node_modules` directories are excluded. Android template copies also exclude `build` and `builds` directories to avoid stale artifacts.

## Android APK and AAB

This target builds an **existing compatible Android app template**; the desktop engine currently does not supply an Android runtime or automatically turn scenes into a mobile game. Your template must implement the runtime that consumes the staged snapshot. Installing the Android SDK alone is insufficient.

1. Place the compatible Gradle project in `android/` (or select another project-relative folder). It must contain an `app/` module, the Gradle wrapper script (`gradlew` on Unix or `gradlew.bat` on Windows), `gradle/wrapper/gradle-wrapper.jar`, and its wrapper properties. Keep the usual `settings.gradle[.kts]` and build scripts.
2. Install a JDK compatible with the template's Android Gradle plugin. Set `JAVA_HOME` to that JDK. Install the Android SDK, accept its licenses, and set `ANDROID_HOME` or `ANDROID_SDK_ROOT` to its directory. Install the platform/build-tools/NDK versions required by your template; Gradle reports any remaining missing resources.
3. Leave `app/src/main/assets/rustic` absent in the template: the build reserves it for the snapshot. The template's runtime must read its manifest and game files from that assets path.
4. Select **Android APK (debug)** to run `assembleDebug`, or **Android App Bundle (release)** to run `bundleRelease`. Configure package ID, SDK levels, version codes, ABIs, and release signing in your Android template. Configure signing with environment variables or a local credential provider; keep credentials and keystores outside game assets.
5. Build. The template is copied into the new output folder and built there, leaving the original unchanged. The snapshot is added to the copied app's assets. Completed `.apk` or `.aab` files appear under `artifacts/`. The temporary copied Android project is removed after success. Release signing follows the template; an AAB is not guaranteed store-ready or signed by this dialog.

An APK can be installed on a compatible device with `adb install <output>/artifacts/app-0.apk`. An AAB is a publishing artifact, not directly installable with adb. Test with your template's supported device/ABI and store tooling before distribution.

## Troubleshooting

- **Build is disabled:** check the Required messages. Read-only editors cannot build; close the other writable editor and reopen the project. Windows packaging requires a Windows host and its bundled runtime. Inno Setup is required only when creating setup.
- **Installer compiler unavailable:** enter the full existing `ISCC.exe` path. Inspect the displayed compiler failure for invalid version or image resources.
- **Website index missing:** select the folder containing the final `index.html`, not the framework source directory.
- **Resource path rejected:** use a path inside the project without `..` or absolute components. Remove symbolic links from website/Android build inputs. Choose a source folder outside the builds directory; outputs cannot be nested in their input.
- **Gradle fails:** the dialog includes its exit status and output. Check JDK/SDK compatibility, wrapper resources, required platform packages, SDK licenses, network access for Gradle dependencies, and release signing configuration.
- **Game fails to launch:** distribute the whole `game/` folder or run setup; do not move only the executable. Restore modified/missing data files, check game logs, graphics drivers, and external language toolchains.
