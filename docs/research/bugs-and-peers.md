## Report: Unikey bug classes, peer IMEs, and what oh-key should adopt

Evidence tags: **[measured]** was read on a fetched page, **[inferred]** is from general knowledge of how these IMEs work, **[unverified]** is a claim I could not confirm. Several pages were summarized by a fast model, so issue titles are exact but descriptions are thin. I did not get fcitx5-bamboo (the fcitx-contrib URL returned 404) or the claudeissues.com threads (host unreachable). I did not reach voz, tinhte or reddit.

### 1. Root cause shared by most classes
Unikey, OpenKey, EVKey and goxkey on Windows and macOS use a global keyboard hook. They do not use a text-service composition. When a tone key arrives, they swallow it, then inject N backspaces plus the replacement characters (SendInput on Windows, CGEvent on macOS). **[inferred]** This puts the IME and the app in a race over text the app owns.
- OpenKey's "Gửi từng phím" (send key step by step) option is a workaround for apps that cannot take the batch send. It is off by default because batch is faster. **[measured]** https://github.com/tuyenvm/OpenKey
- ibus-bamboo keeps per-application input modes for the same reason: Pre-edit (default), Surrounding text and ForwardKeyEvent. Apps respond differently, so the mode is saved per app. **[measured]** https://github.com/BambooEngine/ibus-bamboo
- goxkey (macOS) needs Accessibility permission. Revoking it while the app runs can freeze the machine (issue #94), and Secure Keyboard Input silently disables it (issue #9). **[measured]** https://github.com/huytd/goxkey/issues

**Fix oh-key should adopt:** make the real composition the primary path.
- Windows: a TSF text service (ITfContext composition range, with a TIP profile). Marked composition text is owned by the app, so there are no backspaces.
- macOS: an IMKInputController using `setMarkedText` and `insertText`.
- Linux: IBus and fcitx5 addons with preedit.
- Keep a hook-plus-backspace fallback only for apps with no TSF/IMK (some games, legacy controls), switched per app.
- Put the engine in a pure `no_std`-friendly crate that never sees the injection method. Frontends only receive a "replace composition with X" result.

### 2. Bug classes: cause and fix

| Class | Evidence | Root cause | oh-key fix |
|---|---|---|---|
| Chrome address bar: accented char duplicated or moved to the start, "đ" can't be typed | OpenKey #194 "Lỗi từ đầu có dấu trên address bar của Chrome", #27, #37, #99, #22. A mytour.vn blog describes accents repeating at the start of the address bar when typing fast. **[measured titles]** https://github.com/tuyenvm/OpenKey/issues?q=is%3Aissue+chrome | Omnibox inline autocomplete selects the suggested tail. A backspace then deletes the selected suggestion rather than the typed letter, so the backspace count is off by one. **[inferred]** | TSF composition removes this. In fallback mode, detect the omnibox (UIA role/class) and send a Delete or Right-arrow first to clear the selected suggestion. Commonly suggested workarounds are Ctrl+E/Ctrl+K and clearing history. |
| Autocorrect in Chrome, Excel and Sheets | OpenKey has an "Autocorrect fixed" option for browsers and Excel. **[measured]** | The app's autocomplete or autocorrect reacts to the transient intermediate text between the backspaces and the retype. **[inferred]** | Replace in one atomic edit (TSF composition, or one SendInput batch with no gaps). Never expose intermediate states. |
| Double letters or dropped characters in Electron, Discord, Edge, Firefox | ibus-bamboo #467 "Lỗi duplicate kí tự trên Microsoft Edge", #256 "chữ cuối cùng nhảy lên đầu" (last letter jumps to the front). **[measured titles]** | Backspace and commit events arrive out of order. Chromium handles delete-surrounding-text and commit asynchronously. **[inferred]** | Use preedit, not surrounding-text deletion. Sequence-number each edit and ignore stale acks. |
| Terminals, TUIs, CLIs | Search found reports titled "Vietnamese IME drops characters in terminal (Unikey/ibus-unikey)" and "doubles characters on Windows (\x08 BS not handled, only \x7F DEL)". **[unverified]** The pages were unreachable, only the titles came through a search. | Terminals treat BS (0x08) and DEL (0x7F) differently, and a PTY may not treat them as a backspace. **[inferred]** | Per-app profile for terminals: either commit whole words on a boundary, or send DEL (0x7F). Prefer a composition-aware path (Windows Terminal supports TSF). |
| Games and anti-cheat | Not found in a source. **[unverified]** | Hooks are blocked or filtered. SendInput is flagged as injected. A game using DirectInput or raw input ignores the IME. | Default Vietnamese off in full-screen exclusive apps via a foreground-app list. Ship a quick toggle and an exclusion list. OpenKey issue #104 is a feature request for app exclusion. **[measured]** |
| UAC and admin apps | goxkey and OpenKey issues give no direct evidence. **[inferred]** | A medium-integrity hook cannot inject into high-integrity windows (UIPI). | A TSF text service loads into the target process and has no UIPI problem. For the fallback, offer an elevated helper or `uiAccess=true` with a signed binary in `Program Files`. |
| Wrong tone placement | goxkey #84 (free tone toggle), #65 (old style request). vi-rs places tones per the new style (`hoà`, not `hòa`). **[measured]** | Hard-coded placement, with no old/new switch. | Setting `ToneStyle::{Modern, Classic}` (bamboo-core: `EstdToneStyle`). Placement is a pure function of the syllable, tested against a table. |
| English words mangled ("text" to "tẽt") | OpenKey has "restore keys for invalid words". bamboo-core has `RestoreLastWord(false)`. **[measured]** | No syllable validation before commit. | Validate the Vietnamese syllable at each key. On a word boundary, if the syllable is invalid, replay the raw keys. Also provide an explicit undo (the same tone key twice, or Esc) and a bundled English-word exception list for common collisions (text, six, mix). |
| "đ" inconsistency | OpenKey #22 (address bar), goxkey #72 (special characters should start a new word). **[measured titles]** | `dd` handled in the wrong state or at a word boundary. Leftover state after punctuation. | `d`+`d` is a pure composition rule. Punctuation and digits hard-reset the composition (the PunctuationMode idea in bamboo-core). |
| Caps Lock and Shift | Not found in a source. **[unverified]** | Case taken from the key rather than from the key's case state. | Record the case per raw key (bamboo-core `IsUpperCase`). Handle "Caps Lock on, user types lowercase" explicitly. Test all four combinations. |
| Mode-switch and persistence | goxkey #109 (first switch sometimes fails), #110, #88 (remember mode per app). Microsoft Q&A: Ctrl+Shift toggles both the IME and the Windows layout, and they conflict. **[measured]** | The IME toggle shortcut collides with the OS layout switch. | Use a dedicated hotkey or TSF-native switching. Persist the mode per app. |
| Remote desktop | goxkey #121 (diacritics fail over RDP). **[measured]** | Injected keys are scancode-remapped by the RDP client. | TSF/IMK composition works over RDP; in the fallback, send Unicode text events (`KEYEVENTF_UNICODE`), not scancodes. |
| Macros and clipboard | OpenKey macros have no length limit, where macOS's built-in text replacement is capped at 20 characters. **[measured]** | Many IMEs paste via the clipboard and clobber it. | Commit macro text through TSF/IMK. If a paste is unavoidable, save and restore all clipboard formats and be careful with sensitive clipboard apps. |
| macOS NFD | OpenKey lists "Unicode Compound" as a separate code table. **[measured]** | The app expects the combining form; HFS+ and some editors normalize to NFD. | The engine outputs NFC internally. Give the frontend an `OutputForm::{NFC, NFD}` option, and test both. |
| Microsoft's own Telex | Microsoft's IME is composition-based (TSF). In Telex, `uw`→ư, `ofw`→ờ and `vuwofwn`→vườn, and the Number Key-based IME is also offered. **[measured]** https://learn.microsoft.com/globalization/input/vietnamese-ime | Fewer app-compat bugs, but fewer features (macros, quick telex, per-app memory, restore). | oh-key should keep the composition architecture and add the features users get from Unikey (macros, quick telex, spell check) on top. |

### 3. Peer engine design: what to copy

**bamboo-core** (Go, MIT, © 2018 Luong Thanh Lam) https://github.com/BambooEngine/bamboo-core **[measured, from source files]**
- Composition is a slice of `Transformation{Rule, Target, IsUpperCase}`. Appending transformations add a raw letter, and Mark/Tone transformations point at an earlier one through `Target`.
- `Flatten` renders the slice to a string. A tone or mark can move to a different vowel without rewriting history, and undo is just removing a node. `RemoveLastChar` rebuilds the word without the removed node and everything targeting it.
- Rules are parsed from a data definition (`input_method_def.go`, `rules_parser.go`), not hard-coded. A new input method is data.
- A `uow` shortcut handles `uơ`/`ươ` through a virtual horn rule.
- Mode bitflags: English, Full text, Punctuation, ToneLess, MarkLess, LowerCase. Engine flags: free tone marking, standard tone style, autocorrect.
- `RestoreLastWord(bool)` replays the raw keys back (false) or forward to Vietnamese (true).
- Tests: `bamboo_test.go`, `rules_parser_test.go`, `utils_test.go`. I did not see the corpus itself. **[unverified size]**
- Copy: the target-pointer composition and the "keep raw keys, derive the display" idea. Reimplement it in Rust rather than translate it. The MIT licence allows it with attribution.

**vi-rs** (Rust, MIT, `vi` crate 0.8.0, by zerox-dg; "95% functional", "some minor bugs") https://github.com/zerox-dg/vi-rs **[measured]**
- API is `transform_buffer(method, keys, &mut out)`.
- Modules: `editing`, `maps`, `parsing`, `processor`, `syllable`, `util`, `validation`, plus `tests/`, `testdata/` and `benches/`.
- Tone goes on the new accent (`hoà`).
- Copy the syllable-parse then validate then place-tone pipeline and the benches/testdata layout. No performance numbers are published. **[measured]** Treat the crate as a reference, not a dependency, given its own maturity statement.

**goxkey** (Rust, BSD-3-Clause, built on the vi-rs lineage) https://github.com/huytd/goxkey **[measured]**
- macOS only in practice (core-graphics hooks). An IBus branch is in development. Telex and VNI only.
- Its issue list is a useful list of what to avoid: crash at startup if another input source is selected (#48), freeze on revoking Accessibility (#94).

**Other peers**
- OpenKey (C++, GPL-3.0): the feature list is the benchmark (macros, per-app code table, quick telex, spell check, restore).
- ibus-bamboo (Go, GPL-3.0, maintenance stalled; fcitx5-unikey is the suggested alternative) and fcitx5-unikey (GPLv2+, embeds the Unikey `ukengine`): both are strong-copyleft.
- Unikey's original engine is GPL, so oh-key should not port its source if it needs a permissive licence. **[inferred]**

**Licence decision for oh-key:** take ideas and test cases from MIT/BSD projects with attribution. For GPL projects (Unikey, OpenKey, ibus-bamboo), take only behaviours, as black-box test expectations written from observing the output. Keep the repo licence choice (MIT or GPL) in line with what is ported.

### 4. Concrete recommendations
1. Engine crate `oh-key-engine`: pure and allocation-light, with a `Transformation`-style composition. Inputs are a key and a case. Outputs are `Composition { display, committed }`. It has no OS dependency.
2. Syllable validator before commit, and raw-key replay for invalid words, with a configurable exception list.
3. Frontends, in priority order: Windows TSF, macOS IMK, IBus/fcitx5. Per-app compat table only for the fallback hook path.
4. Test corpus: a golden TSV of (key sequence, expected output) in both tone styles. Cover Caps Lock and Shift, `dd`, `uow`, undo, and the NFC/NFD output. Property tests: committing never changes more than one syllable, and undo is the inverse.
5. Benchmarks in `benches/` with a target of microseconds per key.
6. Do not copy: the single-batch backspace-injection primary path, a toggle that conflicts with the OS layout switch, and clipboard-based macros.

### Sources
- https://github.com/BambooEngine/bamboo-core (and `flattener.go`, `bamboo.go` raw)
- https://github.com/zerox-dg/vi-rs
- https://github.com/huytd/goxkey and https://github.com/huytd/goxkey/issues
- https://github.com/tuyenvm/OpenKey and https://github.com/tuyenvm/OpenKey/issues?q=is%3Aissue+chrome
- https://github.com/BambooEngine/ibus-bamboo and the address bar issue search
- https://github.com/fcitx/fcitx5-unikey
- https://learn.microsoft.com/globalization/input/vietnamese-ime
- https://learn.microsoft.com/answers/a/12220396 (Ctrl+Shift layout conflict)
- https://mytour.vn/en/blog/bai-viet/fixing-the-issue-of-typing-vietnamese-on-the-chrome-address-bar.html (secondhand)
- https://bugzilla.mozilla.org/show_bug.cgi?id=1319018 (Firefox/ibus underlined-mode autocomplete)
