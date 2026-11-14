# oh-key

Vietnamese typing app for Windows in Rust, in the Unikey style (a tray app with a low-level keyboard hook): faster and with fewer bugs than Unikey. Repository: `codepawl/oh-key` (private).

## Layout

- `Cargo.toml`: workspace, `members = ["crates/*"]`, edition 2024. Version, licence and repository live in `[workspace.package]`.
- `crates/oh-key-charset`: charset conversion (Unicode, TCVN3, VNI, VIQR and others). Pure, no system calls.
- `crates/oh-key-engine`: the typing engine (Telex, VNI), spell check and restore. Pure; `examples/bench.rs` is the benchmark. `bench/unikey-compare` replays one Telex corpus through UniKey's ukengine and through this engine for speed and output (`run.ps1 -UniKeySource <folder>`, see its README); the UniKey sources are never committed.
- `crates/oh-key-macro`: macro (abbreviation) expansion. Pure.
- `crates/oh-key-cli`: the `oh-key-cli` binary (`oh-key-cli type <keys>`).
- `crates/oh-key-ipc`: what the parts of the app share: `config.toml` format (`Settings`, including `excluded_apps`; an old `hook_apps` line is ignored) and `SharedState`, a named memory block (`Local\oh-key-state`) with a seqlock-guarded settings copy, a generation counter and the global VI/EN mode, used by the tray, the hook and the second-instance launch.
- `crates/oh-key-ffi`: the C interface (staticlib + rlib, header in `include/oh_key.h`) to the engine, macros and `config.toml` settings for non-Rust hosts; a session API returns backspaces, text to insert and a consume flag per key. Pure and tested through the C functions on every OS.
- `macos/`: the macOS input method (InputMethodKit, Swift, no Xcode project). `build.sh` builds the universal `target/mac/oh-key.app` and `oh-key-macos.zip` (ad hoc signed), `verify.sh` checks the bundle; install by copying to `~/Library/Input Methods`. Live typing is checked by hand, not in CI.
- `crates/oh-key-app`: the `oh-key` tray app. `src/hook.rs` is the `WH_KEYBOARD_LL` hook that types Vietnamese in every app whose exe is not in `excluded_apps` (the pure half, diff plus key decision plus session, is unit tested); the hotkeys (Ctrl+Shift, Alt+Z) work in every app. Mode icon and control panel; the look is copied from oh-snap. `oh-key --render-png settings|tray-menu-preview|excluded-apps <out.png> [--scale 1-4]` draws a window offscreen (the scale gives a sharper PNG with the same layout); `docs/settings.png` is the settings render. `src/logo.rs` draws the mark (the website's orange tile with a dark V) in every window title; `oh-key --render-png logo <folder>` writes it at each icon size, `tools/make-icon.py <folder>` packs those into `assets/oh-key.ico`, and `build.rs` writes that icon as a `.res` file for the MSVC linker, so the exe carries it with no resource compiler. `src/hook_cases.rs` is the case table shared by `examples/hook_suite.rs` (types ~34 cases into its own windows through the real hook, CI only) and `src/hook_headless.rs` (the same cases through `KeyFlow`, the type `HookState` drives, into a simulated text field with the suggestion landing at `{WAIT}` and after every key, run by `cargo test-all`); `examples/hook_e2e.rs` types into its own window through the hook. The hook has no timer: it checks the foreground window and the shared-state counters at the start of each key event; the macro file is read on a short thread and program names are cached by process id, so the callback does no file I/O. A watchdog notices a hook that Windows removed: a second low-level hook on its own thread (the sentinel, which does no work and never reads the key) pings the hook thread for each key, and keys that keep coming for 1 s with no main-hook callback mean the hook is gone, so it is reinstalled (new before old), the typing state is reset, and the app reports it once per run. A reinstall that brings no callback back is retried only after 60 s. Do not use `GetLastInputInfo` (it counts the mouse), raw keyboard input on the hook thread (it stopped every callback in CI) or a reinstall after each slow call (Windows keeps the hook through 350 ms calls, and the swap dropped keys). The app reports a reinstall once per run.
- macOS IMK and Linux fcitx5/ibus frontends come later.
- `crates/oh-key-app/src/sound.rs` and `mode_flash.rs`: the mode switch sound (synthesised, no audio file ships; `oh-key --write-sounds <folder>` writes it as WAV) and the badge by the caret (a layered, click-through window that is never activated, on its own short thread; `oh-key --render-png mode-flash <folder>` writes every frame). `mode_effects.rs` decides whether either plays. Only the caret rectangle is read, never text.
- `docs/research`: notes on Unikey behaviour and design decisions. `windows-tsf.md` is kept for reference only: the TSF text service was dropped on 2026-11-12.
- `install.ps1` (release install into `%LOCALAPPDATA%\Programs\oh-key`, no admin, `-StartAtSignIn`) and `tools/dev-install.ps1` (same from a local `target\lead` build; stops the running app and starts the new one through explorer).
- `.github/workflows`: `ci.yml` (fmt, clippy, tests, Windows builds of the app and CLI for x64/x86/arm64) and `release.yml` (tag `v*` builds, packages `oh-key-windows-<arch>.zip` with `oh-key.exe` and `oh-key-cli.exe`, `oh-key-setup.exe` from `installer/oh-key.iss` (Inno Setup, per user, all three architectures, installed and uninstalled once in CI) and the macOS zip, and publishes a GitHub Release here and in the public `codepawl/oh-key-releases` through the `RELEASES_TOKEN` secret of the `public-release` environment; a pull request touching the workflow, `installer/` or `macos/` runs it as a dry run without publishing; a tag with a hyphen is a public pre-release).
- `web/`: the download page at `ohkey.codepawl.com`, a Cloudflare Worker (custom domain). It reads the latest public release from the `releases/latest` redirect and its `SHA256SUMS.txt`, so a new release needs no page change; `/download/windows`, `/download/macos` and `/install.ps1` redirect or proxy to the latest files. `npm test` in `web/`, deploy with `npx wrangler deploy` there.

## Checks to run before handing off

```
cargo fmt --check
cargo check-all   # clippy, msvc target, -D warnings
cargo test-all    # tests, msvc target
```

The aliases in `.cargo/config.toml` pin the msvc target: this machine defaults to the gnu host. The hook logic is checked headless by `cargo test-all` (the hook suite cases replayed through `Session` into a simulated text field). Never run `hook_e2e` or `hook_suite` on the owner's machine: they open a window, take the foreground and steal focus while the owner works (An, 2026-11-12). They run in CI (`hook-e2e` job on windows-latest); run them locally only when An asks.

Lag (keys repeated, tones missing under load): `hook_headless.rs` models a loaded machine (one input queue shared by the keyboard and our `SendInput` batches, late callbacks, blocked input) and runs headless in `cargo test-all`; `Mitigations` switches each fix off to show the symptom (`legacy_*` tests fail on the old logic, `fixed_*` pass). The `hook-e2e` CI job runs `hook_suite` under stress with `OHKEY_TEST_HOOK_DELAY_MS` / `OHKEY_TEST_HOOK_DELAY_EVERY` (slow callbacks), `OHKEY_TEST_CPU_STRESS_THREADS` (busy cores) and `OHKEY_DISABLE_MITIGATIONS=1` (negative control). Never run these on a developer PC.

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

Once CI is green on a pushed commit, build it and install it with `tools/dev-install.ps1` without asking (An, 2026-11-12: "CI xanh thì cài luôn nha"). Never install a build whose CI is red or still running.

1. Build with `CARGO_TARGET_DIR=target/lead`: `cargo build -p oh-key-app --release --target x86_64-pc-windows-msvc`.
2. Run `tools/dev-install.ps1` (no elevation). It stops the running `oh-key.exe` (Program Files or `%LOCALAPPDATA%\Programs\oh-key`), copies the build into `%LOCALAPPDATA%\Programs\oh-key`, refreshes an existing "start with Windows" value to that path and starts the app through explorer.
3. Type in Notepad or a browser. Alt+Z or Ctrl+Shift switches V and E; an app on the "Ứng dụng tắt oh-key" list keeps its keys untouched.
4. For a repeatable check without the tray, use the headless tests or the CI `hook-e2e` job (see Checks). Do not drive the keyboard of the user's session from a script: the hook is global.
