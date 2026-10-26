# Required UI implementation

Follow the mandatory UI stack and appearance rules in [../AGENTS.md](../AGENTS.md).
Use the [egui demo](https://www.egui.rs/#demo) for standard widgets and built-in
themes, [egui_tiles](https://github.com/rerun-io/egui_crates/tree/main/crates/egui_tiles)
for docking, [egui-snarl](https://zakarumych.github.io/egui-snarl/) for node graphs,
and [transform-gizmo-egui](https://urholaukkarinen.github.io/transform-gizmo/) for
transform handles. Preserve their default styles and interactions. Do not add
custom UI palettes, skins, or hand-drawn replacements. Use active egui style
values so light and dark themes remain consistent across native applications.

# Workspace completion requirement

After completing code changes in this repository, build the Windows installer before reporting the task as finished:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\build-windows-installer.ps1
```

Confirm that a fresh `dist\RusticGameEngine-Setup-*.exe` was produced. If the installer build fails, report the failure and do not describe the task as fully complete.
