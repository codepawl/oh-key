# oh-key

Vietnamese input method in Rust: faster and with fewer bugs than Unikey. Repository: `codepawl/oh-key` (private).

## Layout

- `Cargo.toml`: workspace, `members = ["crates/*"]`, edition 2024. Version, licence and repository live in `[workspace.package]`.
- `crates/oh-key-charset`: charset conversion (Unicode, TCVN3, VNI, VIQR and others). Pure, no system calls.
- `crates/oh-key-engine`: the typing engine (Telex, VNI), spell check and restore. Pure; `examples/bench.rs` is the benchmark.
- `crates/oh-key-macro`: macro (abbreviation) expansion. Pure.
- `crates/oh-key-windows`: the TSF text service DLL (`ohkey_tip.dll`), COM glue only; key decisions live in `src/keys.rs` and are unit tested.
- `crates/oh-key-cli`: the `oh-key` binary (`oh-key type <keys>`, `oh-key register|unregister`).
- macOS IMK and Linux fcitx5/ibus frontends come later.
- `docs/research`: notes on Unikey behaviour and design decisions.
- `.github/workflows`: `ci.yml` (fmt, clippy, tests, Windows builds) and `release.yml` (tag `v*` builds zips and a GitHub Release).

## Checks to run before handing off

```
cargo fmt --check
cargo check-all   # clippy, msvc target, -D warnings
cargo test-all    # tests, msvc target
```

The aliases in `.cargo/config.toml` pin the msvc target: this machine defaults to the gnu host, where the TSF crate's test binary crashes before main.

CI runs the pure crates on Windows, Linux and macOS, so keep them free of platform code. Engine changes need a typing test (keys in, text out).

## Rules

- Never register the TIP (`regsvr32`, `install.ps1 -Register`) on the dev machine without asking. A broken text service can break typing system-wide.
- Code style: full descriptive names, small functions, one statement per line, comments only where the reason is not obvious.
- No allocations on the engine's per-keystroke path unless a benchmark shows it does not matter.
- Always use absolute paths; do not `cd` into subfolders.
- Do not commit secrets or personal data.

## Licence

GPL-3.0-or-later. Code ported from Unikey keeps its GPL terms. Do not copy in code under an incompatible licence.

## Trying the Windows input method

1. Build the DLLs with `CARGO_TARGET_DIR=target/lead` for `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`
   (`cargo build -p oh-key-windows --release --target <target>`).
2. Run `tools/dev-install.ps1` elevated. It swaps the DLLs even while apps hold the old ones, registers both,
   adds oh-key under Vietnamese and creates `C:\Users\Public\Documents\oh-key.log`, which turns on the
   diagnostics log (event names and HRESULTs only, never typed text). Delete that file to turn logging off.
3. Test with `cargo run -p oh-key-windows --example probe --target x86_64-pc-windows-msvc -- edit "Vieetj Nam "`
   (or `richedit`). The probe activates oh-key for its own process only and prints what its window holds.
   Do not switch the input language with Alt+Shift or Win+Space from a script: that switch is global and
   moves every open app, including the user's, onto the build under test.
