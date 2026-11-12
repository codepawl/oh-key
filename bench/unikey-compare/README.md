# oh-key against UniKey

Replays one Telex keystroke corpus through UniKey's `ukengine` and through `oh-key-engine`, then compares speed and the text each one produces.

## Run it

1. Put the `ukengine` source files of `github.com/vn-input/ibus-unikey` (the `ukengine/` folder, `.cpp` and `.h` only) in an empty folder outside the repository. They are not committed and nothing from that folder is executed except the `cl` command line in `run.ps1`.
2. `pwsh bench/unikey-compare/run.ps1 -UniKeySource <that folder>`

It needs Visual Studio Build Tools (found with `vswhere`) and cargo. Output lands in `bench/unikey-compare/out/` (ignored by git).

## What runs

- `crates/oh-key-engine/examples/compare_corpus.rs` writes `corpus.keys` (about 300,000 Telex keystrokes) and `corpus.expected` (the text a correct engine should show). Same seed, same bytes. The corpus is 42 sentences of prose plus 8% hard words (người, khuỷu, thuở, quốc, giữ, Nguyễn, đường, thuyền and so on), about 5% English words, about 3% numbers, capitals and ALL CAPS words, and punctuation. Typing styles vary: tone key at the word end (85%) or right after the vowels (15%), ươ typed `uwow` or `uow`, and 8% of words with all modifier keys typed after the letters (`duongwd`).
- `unikey_bench.cpp` links the ukengine sources (compiled with `cl /O2 /EHsc /MT`, plus `windows_shim.h`, a stub for the POSIX `mkstemp` that only the unused file-conversion helper calls) and replays the corpus.
- `crates/oh-key-engine/examples/compare.rs` replays the same file through `oh_key_engine`.
- `crates/oh-key-engine/examples/compare_report.rs` prints the table and the differences.

## Settings given to UniKey

Telex input, output charset `XUTF8` (Unicode, backspace counts are characters), `freeMarking = 1`, `modernStyle = 1`, `spellCheckEnabled = 1`, `autoNonVnRestore = 1`, `macroEnabled = 0`. These match `oh_key_engine::Config::default()`: free tone placement, modern tone style, spell check, restore of invalid words, no macros. `UnikeySetCapsState` gets shift for uppercase letters.

## How keys are applied

Both sides keep a text buffer and apply each key as a frontend does. UniKey: call `UnikeyFilter`, delete `UnikeyBackspaces` characters, type the output bytes, or type the key itself when the engine returned nothing. A newline resets the engine and is typed. oh-key: `Commit(text)` is appended, `PassThrough` types the key, `Composing` types nothing yet; the pending word is committed at the end. Backspace is not part of the corpus.

## Speed

Each side replays the whole corpus 3 times as warmup, then 20 timed times. The number per replay is total time divided by key count, so the timer overhead is not in it. Both include applying the result to the text buffer. `run.ps1` raises its priority class to High, which the replays inherit. The table shows median, p10 and p90 of the 20 replays, in nanoseconds per key (`QueryPerformanceCounter` on the C++ side, `Instant` on the Rust side).

## Differences

The three outputs are split on whitespace and compared word by word with the expected text. Each differing word falls in one group: only oh-key right, only UniKey right, both wrong and different, both wrong and the same. Decide the cause (UniKey bug, oh-key bug, or a policy difference such as English restore) by reading the listed keystrokes.

Another seed gives another corpus: `compare_corpus <keys> <expected> <seed>`.
