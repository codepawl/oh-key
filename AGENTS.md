# oh-key

Vietnamese typing app for Windows in Rust, in the Unikey style (a tray app with a low-level keyboard hook): faster and with fewer bugs than Unikey. Repository: `codepawl/oh-key` (private).

## Layout

- `Cargo.toml`: workspace, `members = ["crates/*"]`, edition 2024. Version, licence and repository live in `[workspace.package]`.
- `crates/oh-key-charset`: charset conversion (Unicode, TCVN3, VNI, VIQR and others). Pure, no system calls.
- `crates/oh-key-engine`: the typing engine (Telex, VNI), spell check and restore. Pure; `examples/bench.rs` is the benchmark. `bench/unikey-compare` replays one Telex corpus through UniKey's ukengine and through this engine for speed and output (`run.ps1 -UniKeySource <folder>`, see its README); the UniKey sources are never committed.
- `crates/oh-key-macro`: macro (abbreviation) expansion. Pure.
- `crates/oh-key-cli`: the `oh-key-cli` binary (`oh-key-cli type <keys>`).
- `crates/oh-key-ipc`: what the parts of the app share: `config.toml` format (`Settings`, including `excluded_apps`; an old `hook_apps` line is ignored) and `SharedState`, a named memory block (`Local\oh-key-state`) with a seqlock-guarded settings copy, a generation counter and the global VI/EN mode, used by the tray, the hook and the second-instance launch.
- `crates/oh-key-app`: the `oh-key` tray app. `src/hook.rs` is the `WH_KEYBOARD_LL` hook that types Vietnamese in every app whose exe is not in `excluded_apps` (the pure half, diff plus key decision plus session, is unit tested); the hotkeys (Ctrl+Shift, Alt+Z) work in every app. Mode icon and control panel; the look is copied from oh-snap. `oh-key --render-png settings|tray-menu-preview|excluded-apps <out.png>` draws a window offscreen; `docs/settings.png` is the settings render. `examples/hook_e2e.rs` types into its own window through the hook.
- macOS IMK and Linux fcitx5/ibus frontends come later.
- `docs/research`: notes on Unikey behaviour and design decisions. `windows-tsf.md` is kept for reference only: the TSF text service was dropped on 2026-11-12.
- `install.ps1` (release install into `%LOCALAPPDATA%\Programs\oh-key`, no admin, `-StartAtSignIn`) and `tools/dev-install.ps1` (same from a local `target\lead` build; stops the running app and starts the new one through explorer).
- `.github/workflows`: `ci.yml` (fmt, clippy, tests, Windows builds of the app and CLI for x64/x86/arm64) and `release.yml` (tag `v*` builds, packages `oh-key-windows-<arch>.zip` with `oh-key.exe` and `oh-key-cli.exe`, and publishes a GitHub Release).

## Checks to run before handing off

```
cargo fmt --check
cargo check-all   # clippy, msvc target, -D warnings
cargo test-all    # tests, msvc target
```

The aliases in `.cargo/config.toml` pin the msvc target: this machine defaults to the gnu host. With the hook, also run the end-to-end check once: `cargo run -p oh-key-app --example hook_e2e --target x86_64-pc-windows-msvc` (exit 0 = pass; it installs the hook for under 30 seconds and types only into its own window).

CI runs the pure crates on Windows, Linux and macOS, so keep them free of platform code. Engine changes need a typing test (keys in, text out).

## Rules

- The keyboard hook sees every key. Install it only inside a short-lived test process that types into its own windows, never leave a test build hooked, and never log or store typed text.
- Code style: full descriptive names, small functions, one statement per line, comments only where the reason is not obvious.
- No allocations on the engine's per-keystroke path unless a benchmark shows it does not matter.
- Always use absolute paths; do not `cd` into subfolders.
- Do not commit secrets or personal data.

## Measuring beside an installed copy

`OHKEY_NO_HOOK=1` skips the keyboard hook and `OHKEY_INSTANCE=<name>` renames the mutex, tray window title, shared state and mode event, so a build can run next to the installed app without talking to it. Both are for measurement only and do nothing when unset. The tray holds no Direct2D, DirectWrite or WIC objects while idle (windows create their own and the last one to close trims the working set), and it sleeps on the mode event instead of a timer.

## Licence

GPL-3.0-or-later. Code ported from Unikey keeps its GPL terms. Do not copy in code under an incompatible licence.

## Trying the app

1. Build with `CARGO_TARGET_DIR=target/lead`: `cargo build -p oh-key-app --release --target x86_64-pc-windows-msvc`.
2. Run `tools/dev-install.ps1` (no elevation). It stops the running `oh-key.exe` (Program Files or `%LOCALAPPDATA%\Programs\oh-key`), copies the build into `%LOCALAPPDATA%\Programs\oh-key`, refreshes an existing "start with Windows" value to that path and starts the app through explorer.
3. Type in Notepad or a browser. Alt+Z or Ctrl+Shift switches V and E; an app on the "Ứng dụng tắt oh-key" list keeps its keys untouched.
4. For a repeatable check without the tray, run `hook_e2e` (see Checks). Do not drive the keyboard of the user's session from a script: the hook is global.
