# oh-key

A Vietnamese input method written in Rust. Fast, small and with fewer bugs than Unikey.

Bộ gõ tiếng Việt viết bằng Rust: nhanh, nhẹ, ít lỗi hơn Unikey.

## Why oh-key

- **Composition, not backspace hacks.** Unikey types a letter, then erases and retypes it to add a tone. Many apps (browsers, terminals, remote desktops) mishandle that and show doubled or lost letters. oh-key uses the system text service (TSF on Windows) and keeps the word as an in-progress composition that the application sees as one unit.
- **Spell check and restore.** If the keys do not make a Vietnamese word (for example `window` or `github`), oh-key gives back the exact keys you typed.
- **Speed.** The engine is built to avoid allocation on the typing path and has a benchmark (`crates/oh-key-engine/examples/bench.rs`).
- **Ported from Unikey where it was right.** Charsets (Unicode, TCVN3, VNI, VIQR and others), macros and typing rules follow Unikey's behaviour, with tests for each.

## Status

Early. The engine and charset conversion are in place; the Windows text service is in progress. Do not rely on it for daily typing yet.

## Install (Windows)

While the repository is private, install needs the [GitHub CLI](https://cli.github.com) signed in:

```powershell
gh auth login
gh repo clone codepawl/oh-key
.\oh-key\install.ps1
```

`install.ps1` downloads the latest release for your processor (x64, x86 or arm64), checks its SHA-256 and unpacks it into `%LOCALAPPDATA%\Programs\oh-key`. It does not register the text service unless you pass `-Register`.

## Build

You need a current stable Rust toolchain.

```powershell
cargo build --release
cargo test --workspace
```

## Roadmap

- Windows text service (TSF TIP) for x64, x86 and arm64.
- macOS frontend with InputMethodKit (IMK).
- Linux frontends for fcitx5 and ibus.
- Settings app and a quick toggle.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE). Parts follow the behaviour of Unikey, which is GPL as well.
