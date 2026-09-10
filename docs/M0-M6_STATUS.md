# M0-M6 implementation and qualification ledger

j

Last updated: 2026-08-31 (Pacific/Auckland)

Post-M6 addition (2026-09-09): the first external-editor gameplay slice is implemented
with Lua 5.4 and QuickJS. See `GAMEPLAY_PROGRAMMING.md`, ADR-0002, and ADR-0003. Automated qualification is
recorded by the current workspace run; native external-editor walkthrough remains a
separate release qualification item when VS Code is available.

The M0-M6 code slice is implemented and its automated workspace gates pass. Full
milestone acceptance is not yet claimed because this supplied workspace has no Git
metadata, only one Windows GPU is present, no Linux runner was available locally, and
broader native multi-monitor/DPI/IME/accessibility checks need hardware that was not
available in this session.

| Milestone | Implementation evidence | Automated result | Remaining qualification |
|---|---|---|---|
| M0 | Pinned resolver-3 workspace, central dependency/lint/profile policy, CI, governance, process smoke contract, `xtask` | Complete: format, Clippy, tests, checks, deny, doctor/test, four-app smoke and injected failure all pass | Clean-checkout Windows/Linux run is blocked because the delivered directory is not a Git checkout |
| M1 | Platform paths/processes, safe VFS, transactional files/config/crash marker, bounded jobs/cancellation, resilient logging, versioned catalog/thumbnail cache, async launcher model | Complete automated recovery, corruption, saturation, path, model, and process tests; launcher catalog and completed background diagnostics were exercised interactively | DPI, IME, accessibility, and native multi-monitor behavior require broader physical qualification |
| M2 | Engine-owned RHI/types/handles/lifecycle, deterministic null model, isolated `renderer-wgpu`, Naga reflection, triangle/indexed textured/depth offscreen path, a real `winit`/`wgpu` surface path, and callback-driven device recreation | Complete automated lifecycle/device-loss model tests; explicit ignored GPU test passes on Intel Iris Xe through DX12; New Window and Standalone rendered indexed textured/depth frames to native surfaces and a maximize resize reconfigured successfully | Second distinct Windows GPU/backend candidate plus minimize/DPI/physical device-loss drills remain manual |
| M3 | ECS schedules, stable IDs, hierarchy/cycle validation, dirty propagation, `.rscene` migration/recovery, instances, transactional undo, immutable extraction, primitives | Complete: all automated tests pass, including the 10,000-static-transform warm-up allocation gate | None for the defined headless gate |
| M4 | `.rmeta`, SQLite index, checksummed DDC, async handles/placeholders/watcher, required importers, worker isolation, moves/remap/missing-reference report | Complete automated pipeline, corruption, dependency, malformed-corpus, thread-affinity, real worker-process and reaping tests | Broader production corpora for every codec/driver remain release qualification; FBX/EXR remain explicitly deferred |
| M5 | Versioned Blank/Empty 2D/Empty 3D templates, project locking, dockable data-backed editor, 2D/3D views, hierarchy/inspector/content/console/settings, workspace recovery, primitives and centralized undo | Complete automated template, lock, workspace, primitive, persistence, read-only, and lossless authoring tests | Interactive drag/drop, native detached viewport, multi-monitor/DPI/IME/accessibility walkthrough remains manual |
| M6 | Authenticated bounded IPC 1.1, immutable snapshots, bounded BGRA frame ring, supervised runtime, embedded Play plus distinct native New Window/Standalone `wgpu` surfaces, exact controls, live console, review/discard/apply workflow, and all four executables in the installer | Complete automated protocol/state/ring tests plus real child authentication, frames, all modes, pause/step/resume/stop, crash and forced-reap tests; runtime changes apply as one undoable edit | Complete on the available Windows desktop: all three modes, native external surface rendering, exact pause/frame/resume, Stop/reap, live console, and review/discard were exercised interactively |

## Verification record

All commands used the checked-in Rust 1.98.0 toolchain through the workspace-local
Cargo/Rustup homes on Windows 10.0.26200.

| Command | Result |
|---|---|
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace` | Pass; the physical-GPU test is intentionally ignored in the ordinary suite and run separately |
| `cargo check --workspace --all-targets` | Pass |
| `cargo build --workspace --locked` | Pass |
| `cargo deny check --hide-inclusion-graph` | Pass: advisories, bans, licenses, and sources all OK; duplicate-version diagnostics remain configured warnings |
| `cargo xtask doctor` | Pass |
| `cargo xtask test` | Pass, including locked repeats and expected smoke failure |
| `powershell -ExecutionPolicy Bypass -File .\tools\build-windows-installer.ps1` | Pass; Inno Setup 6.7.3 compiled a 12,538,058-byte installer containing project manager, editor, runtime, and asset worker |
| Per-user silent install under `target`, installed four-app smoke matrix, native installed editor/runtime launch, and silent uninstall | Pass: all four files installed; every success/failure smoke returned 0/1; the installed editor found its sibling runtime and displayed a native DX12 frame; uninstall returned 0 and removed the qualification directory |
| `cargo test -p renderer-wgpu renders_triangle_and_indexed_textured_mesh -- --ignored --nocapture` | Pass on `Intel(R) Iris(R) Xe Graphics`, DX12 |
| Four native packages with `--headless-smoke` and `--headless-smoke-fail` | All success paths exit 0; all injected failures exit 1 |
| `cargo test -p engine-editor -p engine-play -p rustic-runtime` | Pass, including real runtime frame transport and centralized runtime-change apply |
| Native launcher/editor/runtime walkthrough | Pass on Windows: launcher opened its catalog and diagnostics with background jobs completed; embedded Play displayed; Pause froze tick 494; one Frame Advance produced 495; Resume worked; New Window and Standalone rendered distinct color-coded indexed textured/depth frames through real `wgpu` surfaces; maximizing New Window preserved rendering after surface reconfiguration; Stop closed/reaped both and displayed explicit review/discard |
| Source boundary scan for `wgpu::` | Pass: source references occur only in `crates/renderer-wgpu` |
| Manifest wildcard-version scan | Pass: none found |

## Manual qualification checklist

- Run the CI workflow from an actual clean Git checkout on Windows and Linux.
- Exercise launcher/editor native windows for DPI changes, IME, accessibility,
  detached/missing monitors, drag/drop, and persistence.
- Qualify live surface minimize/DPI/device-loss recovery and a second Windows
  backend/GPU candidate; ordinary maximize resize is qualified on the available system.
- Walk through create/import, primitive editing, and non-empty runtime-change apply on a
  representative packaged installation; packaged open/play is qualified and source-level
  tests cover apply as one undo.
- Code-sign the Windows installer in a release environment with the project's signing
  identity; the locally produced qualification installer is intentionally unsigned.
