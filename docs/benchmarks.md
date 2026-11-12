# oh-key against UniKey: speed and correctness

Measured on one machine: Intel i7-14700KF, 64 GB DDR4-3600, Windows 11, release builds, `x86_64-pc-windows-msvc`. The machine was not idle during the runs (other sessions were working), so every speed figure below is the median of 5 runs and the range is shown. Numbers marked (measured) came from the commands in [How to reproduce](#how-to-reproduce).

UniKey here is `ukengine` from `github.com/vn-input/ibus-unikey`, compiled with `cl /O2`, with Telex, free marking, modern tone style, spell check on, restore of invalid words on and macros off. oh-key runs with `Config::default()`, which is the same set of options.

## Speed

Both sides replay the same keystrokes and apply the result to a text buffer the way a frontend would. Each run is 3 warmup replays and 20 timed replays. ns per key, lower is better. Measured on 2026-11-12, 5 runs per corpus. Each cell is the median of the 5 run medians, with the lowest and highest run median in brackets.

| corpus | keys | UniKey | oh-key | oh-key / UniKey |
|---|---:|---:|---:|---:|
| Vietnamese prose, hard words, some English | 300,035 | 35.82 (35.27 to 43.62) | 44.59 (43.77 to 74.70) | 1.24x |
| Mixed English / Vietnamese, 7 categories | 303,588 | 33.77 (33.45 to 35.23) | 34.80 (34.33 to 36.00) | 1.03x |

- The oh-key value 74.70 is the first run of the Vietnamese series, while the machine was busy. Without that run the oh-key range is 43.77 to 47.05. The UniKey range is wide because of its second run (43.62); the other four runs are 35.27 to 38.92.
- The output files of these runs are in `bench/unikey-compare/out/` (not committed): `runs/ohkey.N.timing` and `runs/unikey.N.timing` (first number = median ns/key of the 20 replays), `run-vietnamese.log`, `mixed/*.median` and `run-mixed.log`.
- oh-key takes 3% more time per key than UniKey on the mixed corpus and 24% more on the Vietnamese corpus. The probable cause (inferred, not profiled): `process_key` returns a `String` for every committed word, and the harness copies it into the text buffer. UniKey writes into a fixed buffer. Both are far below one microsecond per key, so neither is perceptible while typing.
- The engine alone, with no result handling (`examples/bench.rs`, 10 million keys, 5 runs): Telex median 34.4 ns/key (32.4 to 35.7), VNI median 33.2 ns/key (32.6 to 34.0). Output: `bench/unikey-compare/out/engine-alone.log`. The engine allocates nothing per key. It allocates one `String` per committed word.
- Before the engine work, the commit message of `85e49ea` says "Engine: 2-3x faster per key". No saved number backs a more exact figure, so this document gives none.

## Correctness on the Vietnamese corpus

51,140 words of Vietnamese prose (300,035 keystrokes) plus 8% hard words (người, khuỷu, thuở, quốc, giữ, Nguyễn, đường, thuyền...), 5% English, 3% numbers and capitals, with varied typing styles (tone at the end or after the vowels, `uwow` and `uow`, all modifiers typed after the letters). The expected text is known word by word (measured).

| | words wrong |
|---|---:|
| oh-key | 0 |
| UniKey | 179 |

All 179 UniKey errors are English words with a doubled key that UniKey mangles: `pass` → `pas` (55 times), `boss` → `bos` (54), `coffee` → `cofee` (46), and the same words followed by punctuation. oh-key keeps a word list of such words (`src/english.rs`).

## Mixed English / Vietnamese corpus

303,588 keystrokes, 52 passes over 7 categories of tokens. A token is one whitespace-separated word, with its punctuation. English tokens must come out as the exact keys typed (an English word typed while Telex is on should not change); Vietnamese tokens as the Vietnamese word. "Distinct" counts each typed token once. Category b is not judged (see below), so its "wrong" columns only count differences from the typed keys.

| category | tokens | distinct | oh-key wrong | UniKey wrong | oh-key wrong (distinct) | UniKey wrong (distinct) |
|---|---:|---:|---:|---:|---:|---:|
| a. English words that collide with Telex keys | 10192 | 196 | 1508 | 2480 | 29 | 92 |
| b. English words that are also Telex Vietnamese (ambiguous) | 5408 | 104 | 1976 | 2072 | 38 | 44 |
| c. Brands and proper nouns | 5356 | 103 | 104 | 447 | 2 | 27 |
| d. Code and tech tokens | 4836 | 93 | 364 | 467 | 7 | 16 |
| e. Mixed Vietnamese / English sentences | 15912 | 318 | 208 | 728 | 4 | 13 |
| f. All caps and capitalised English | 4264 | 82 | 260 | 878 | 5 | 35 |
| g. English followed by punctuation | 4004 | 77 | 728 | 1114 | 14 | 24 |

Reading the table: oh-key is wrong on fewer distinct tokens than UniKey in every category, and there is no token where oh-key is wrong and UniKey right except `#ff00aa` (below). After removing the tokens that are valid Vietnamese when read as Telex (`best` → `bét`, `see` → `sê`, `box` → `bõ`, `how` → `hơ`...), which both engines turn into Vietnamese and which cannot be told apart from Vietnamese without a dictionary or context, what is left for oh-key is shown in "Open items".

### a. English words that collide with Telex keys

oh-key (29 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `away` | `away` | `ây` |
| `best` | `best` | `bét` |
| `box` | `box` | `bõ` |
| `dd` | `dd` | `đ` |
| `deep` | `deep` | `dêp` |
| `door` | `door` | `dổ` |

UniKey (92 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `away` | `away` | `ây` |
| `best` | `best` | `bét` |
| `boss` | `boss` | `bos` |
| `box` | `box` | `bõ` |
| `coffee` | `coffee` | `cofee` |
| `dd` | `dd` | `đ` |

### b. English words that are also Telex Vietnamese (ambiguous)

oh-key (38 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `as` | `as` | `á` |
| `ass` | `ass` | `as` |
| `ax` | `ax` | `ã` |
| `bar` | `bar` | `bả` |
| `bus` | `bus` | `bú` |
| `car` | `car` | `cả` |

UniKey (44 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `as` | `as` | `á` |
| `ass` | `ass` | `as` |
| `ax` | `ax` | `ã` |
| `bar` | `bar` | `bả` |
| `bass` | `bass` | `bas` |
| `bus` | `bus` | `bú` |

### c. Brands and proper nouns

oh-key (2 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `Paris` | `Paris` | `Pái` |
| `Rust` | `Rust` | `Rút` |

UniKey (27 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `Messi` | `Messi` | `Mesi` |
| `Paris` | `Paris` | `Pái` |
| `Russia` | `Russia` | `Rusia` |
| `Rust` | `Rust` | `Rút` |
| `Zoom` | `Zoom` | `Zôm` |
| `Airbnb` | `Airbnb` | `Ảibnb` |

### d. Code and tech tokens

oh-key wrong, UniKey right (1 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `#ff00aa` | `#ff00aa` | `#ff00â` |

oh-key (7 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `#ff00aa` | `#ff00aa` | `#ff00â` |
| `/usr/local/bin` | `/usr/local/bin` | `/ủ/local/bin` |
| `class="box"` | `class="box"` | `class="bõ"` |
| `foo(bar)` | `foo(bar)` | `foo(bả)` |
| `foo.bar.baz` | `foo.bar.baz` | `foo.bả.baz` |
| `if(x>0){` | `if(x>0){` | `ì(x>0){` |

UniKey (16 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `/usr/local/bin` | `/usr/local/bin` | `/ủ/local/bin` |
| `class="box"` | `class="box"` | `class="bõ"` |
| `foo(bar)` | `foo(bar)` | `foo(bả)` |
| `foo.bar.baz` | `foo.bar.baz` | `foo.bả.baz` |
| `if(x>0){` | `if(x>0){` | `ì(x>0)Ơ` |
| `max_value` | `max_value` | `mã_value` |

### e. Mixed Vietnamese / English sentences

oh-key (4 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `down` | `down` | `dơn` |
| `reset` | `reset` | `rết` |
| `see` | `see` | `sê` |
| `test` | `test` | `tét` |

UniKey (13 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `password` | `password` | `pasword` |
| `Samsung` | `Samsung` | `Sámung` |
| `coffee` | `coffee` | `cofee` |
| `down` | `down` | `dơn` |
| `excel` | `excel` | `ễcl` |
| `google` | `google` | `gôgle` |

### f. All caps and capitalised English

oh-key (5 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `ASAP` | `ASAP` | `ẤP` |
| `NASA` | `NASA` | `NẤ` |
| `NOW` | `NOW` | `NƠ` |
| `SEE` | `SEE` | `SÊ` |
| `TOO` | `TOO` | `TÔ` |

UniKey (35 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `ASAP` | `ASAP` | `ẤP` |
| `BOSS` | `BOSS` | `BOS` |
| `Boss` | `Boss` | `Bos` |
| `ISSUE` | `ISSUE` | `ISUE` |
| `Issue` | `Issue` | `Isue` |
| `NASA` | `NASA` | `NẤ` |

### g. English followed by punctuation

oh-key (14 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `&more` | `&more` | `&moẻ` |
| `(see` | `(see` | `(sê` |
| `boxes;` | `boxes;` | `boé;` |
| `how?` | `how?` | `hơ?` |
| `last.` | `last.` | `lát.` |
| `law?` | `law?` | `lă?` |

UniKey (24 distinct wrong tokens)

| keys typed | expected | produced |
|---|---|---|
| `&more` | `&more` | `&moẻ` |
| `(see` | `(see` | `(sê` |
| `[sic]` | `[sic]` | `sic]` |
| `boss)` | `boss)` | `bos)` |
| `boxes;` | `boxes;` | `boé;` |
| `how?` | `how?` | `hơ?` |


## Ambiguous words (category b)

English words that are also Telex Vietnamese, or become Vietnamese with one tone key. Neither reading is called wrong. 104 distinct words (measured):

- Typed as written by both engines (59): ah an at ban can care den dim do done eh fan for four fur go gone han he hen hi him hour jar la lam lone ma man me men mom nam no none oh ok on one our pan pen phone rim sam share sim so son sure ta tam tan ten tim tin to ton tone uh van where win your.
- Turned into Vietnamese by both engines:

| typed | result | typed | result | typed | result | typed | result |
|---|---|---|---|---|---|---|---|
| `as` | `á` | `ax` | `ã` | `bar` | `bả` | `bus` | `bú` |
| `car` | `cả` | `does` | `doé` | `ex` | `ẽ` | `gas` | `gá` |
| `goes` | `goé` | `has` | `há` | `her` | `hẻ` | `here` | `hể` |
| `his` | `hí` | `if` | `ì` | `is` | `í` | `lax` | `lã` |
| `mar` | `mả` | `mas` | `má` | `mix` | `mĩ` | `moms` | `móm` |
| `more` | `moẻ` | `nor` | `nỏ` | `of` | `ò` | `or` | `ỏ` |
| `ox` | `õ` | `pas` | `pá` | `per` | `pẻ` | `sax` | `sã` |
| `sir` | `sỉ` | `tas` | `tá` | `there` | `thể` | `toes` | `toé` |
| `tux` | `tũ` | `us` | `ú` | `war` | `ửa` | `was` | `ứa` |
| `wax` | `ữa` | | | | | | |

- `ass` gives `as` in both: the second `s` undoes the tone, which is the Telex way to type a literal `as`. oh-key keeps this on purpose (a test pins it), so `ass` is not in the English word list.
- Where they differ: `bass` → UniKey `bas`, oh-key `bass`; `were` → UniKey `ưere`, oh-key `were`.

Options, none applied (policy is unchanged):

1. Keep it. Telex users already get a literal word by typing the tone key twice (`iss` → `is`, `off` → `off`), and nothing Vietnamese is lost.
2. Restore at the end of the word for a short list of very frequent English function words that are rare or absent as Vietnamese words (`is as of or if us his has was her here there more`, about 15 words). Costs a few lines in `english.rs`; the risk is a Vietnamese user who really means `á` or `ò` as an interjection, who would then type the tone key twice.
3. Make option 2 a setting ("English words stay as typed") with the list editable, off by default.
4. A per-application list already exists (`excluded_apps`); a terminal or code editor can be put there.

## What changed in the engine for this report

- Bracket keys: `[ ] { }` make `ơ ư Ơ Ư` only at the start of a word that follows a space (or the start of the text). After a symbol or digit they are typed as they are: `item[0]`, `a[i]`, `foo(){}` were `item[0ư` and so on (UniKey already kept `item[0]`). Test: `crates/oh-key-engine/tests/english_mixed.rs`.
- English list: about 50 more words with doubled keys were added to `src/english.rs` (`Russia`, `Russian`, `classroom`, `successfully`, `passion`, `missing`...). `Russia` came out as `Rusia` in both engines.

## Open items

Tokens that remain wrong for oh-key in the mixed corpus, all of which UniKey gets wrong as well (except the first):

- `#ff00aa`, `0xaa`, `a1aa`: letters typed straight after digits start a new word, so `aa` becomes `â`. UniKey keeps `#ff00aa` literal but composes `8dd` to `8đ`. oh-key composes `5trieeju` → `5triệu` and `100ngafn` → `100ngàn`, which UniKey leaves raw; that is the reason it was not changed. Option: stop composing after a digit only when the digit run follows a letter or `#` or `0x`.
- Valid-syllable readings: `best` `bét`, `test` `tét`, `rest` `rét`, `see` `sê`, `too` `tô`, `moon` `môn`, `deep` `dêp`, `box` `bõ`, `tax` `tã`, `max` `mã`, `how` `hơ`, `now` `nơ`, `law` `lă`, `saw` `să`, `rows` `rớ`, `down` `dơn`, `reset` `rết`, `Rust` `Rút`, `Paris` `Pái`. A Vietnamese word is a Vietnamese word, so only a dictionary of English words could separate them, at the cost of Vietnamese words that are also English. Same options as category b.
- `w` at the start of a word is `ư`: `was` `ứa`, `war` `ửa`, `won't` `ươn't`, `wow` `ươ`, `worst` `ướt`.
- `{` and `}` at the start of a word after a space or a line start still make `Ơ` and `Ư` when the result is a Vietnamese syllable (`{i}` → `Ơi}`); UniKey also turns `{key:value}` into `Ơkey:value}` where oh-key keeps it. Option: drop the shifted bracket mapping, since nobody types Ơ that way.
- Speed: in this harness oh-key takes 3% to 24% more time per key (see Speed).

## Footprint

Two sets of numbers, from two different measurements. Do not mix them.

### Installed copy (measured 2026-11-12, hook on)

`tools/measure-footprint.ps1` reads the running, installed `oh-key.exe` and the files next to it. It is read only: it starts nothing, stops nothing and types nothing. The process started at 17:07 and was measured at 17:34, with the keyboard hook on, while the machine was in use. The CPU time therefore includes the keys typed during the 30 seconds and is not an idle figure. Output: `bench/unikey-compare/out/footprint.txt`.

| What | Value |
|---|---|
| Working set | 7.31 MB |
| Private bytes | 3.32 MB |
| Threads | 2 |
| Handles | 205 |
| CPU time over 30 s | 93.8 ms |
| `oh-key.exe` | 834,560 bytes (815 KiB) |

### Trim work, hook off

Measured during the trim work with `target/trim/measure/measure.ps1` (a copy is `tools/measure-trim.ps1`), with the hook off (`OHKEY_NO_HOOK=1`) and a separate instance. That script opens the settings window, so it was not rerun for this document.

| What | Before the trim | After the trim |
|---|---:|---:|
| Idle working set | 21.7 MB | 3.6 MB |
| Context switches per 60 s idle | 876 | 0 |
| Working set after the settings window is closed | 49.9 MB | 6.6 MB |

UniKey's file size is not given: no measurement of a downloaded release binary was saved.

## Hook latency

`examples/hook_suite.rs` prints the time of each key callback in the hook (`hook callback over N keys: p50, p99, max`), for the first 16,384 keys. The callback includes the `SendInput` call when a key edits text. CI run 43195361798 (`hook-e2e` job, commit `dc5cae8`, windows-latest) printed:

| Part of the suite | Keys | p50 | p99 | max |
|---|---:|---:|---:|---:|
| basic, hard-rhymes, uppercase, capitalised, caps-lock | 1068 | 0 us | 170 us | 460 us |
| fast-5ms, backspace, english | 1290 | 0 us | 193 us | 466 us |
| toggle, digits, enter, tab, addr- | 580 | 1 us | 201 us | 337 us |

A hosted runner is not this machine. A figure of "p99 5 to 7 ms including SendInput" and "pass-through under 1 us" came from earlier local runs and is not reproduced here, so it is dropped. The p50 of 0 to 1 us is the nearest measured value for pass-through keys, at a timer resolution of 1 us.

## Tests

Counted on 2026-11-12 with `cargo test --workspace --target x86_64-pc-windows-msvc` (218 tests, all pass):

| Crate | Tests |
|---|---:|
| `oh-key-app` (unit) | 101 |
| `oh-key-charset` | 18 |
| `oh-key-cli` | 2 |
| `oh-key-engine` (unit 1, `adversarial` 19, `english_mixed` 6, `typing` 25) | 51 |
| `oh-key-ffi` | 18 |
| `oh-key-ipc` | 13 |
| `oh-key-macro` | 15 |

- The hook suite has 34 cases (`all_cases()` in `crates/oh-key-app/src/hook_cases.rs`).
- The adversarial test builds 13,833 Vietnamese syllables. It types them in Telex and VNI, in several key orders and case styles. That makes about 206,000 typed syllables in total (41,499 Telex and 41,499 VNI in canonical order with three case styles, 40,959 each in free orders, plus the classic tone style and simple Telex).

## How to reproduce

1. Put the `ukengine` sources (`.cpp` and `.h` only) of `github.com/vn-input/ibus-unikey` in a folder outside the repository.
2. `pwsh bench/unikey-compare/run.ps1 -UniKeySource <folder> -Runs 5` builds both sides and runs the Vietnamese corpus (table and differences). The per-run timings land in `out/runs/`.
3. `pwsh bench/unikey-compare/run-mixed.ps1 -Runs 5` runs the mixed corpus and prints the per-category tables above (`MIXED_REPORT_ROWS=500` for every failure).
4. Engine speed alone: `cargo run --release -p oh-key-engine --example bench --target x86_64-pc-windows-msvc`.
5. Typing tests for this report: `cargo test -p oh-key-engine --test english_mixed --target x86_64-pc-windows-msvc`.

Close other programs before timing. The figures above moved by up to 70% between runs on a busy machine (first Vietnamese run). The corpora are deterministic (fixed seed), so the wrong-token counts do not change between runs, only the speed does.
