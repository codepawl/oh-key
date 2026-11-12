> **Dropped on 2026-11-12.** The TSF text service (`oh-key-windows`) was removed: it was hard to get right and risky to test (a broken text service can break typing system-wide), and oh-key is now a standalone tray app that types through a keyboard hook, like Unikey. These notes are kept for reference only.

# Guide: Windows TSF text input processor (TIP) in Rust for "oh-key"

Provenance: items marked [V] were read in real repos or docs on 2026-11-12. Items marked [K] come from knowledge of TSF and need testing. Newest `windows` crate on crates.io is 0.62.2 [V, crates.io API]. Pin `windows = "0.62"` and `windows-core = "0.62"`.

## Implementation status (crates/oh-key-windows)

Built and checked on 2026-11-12 (measured):
- `cargo build -p oh-key-windows --release --target x86_64-pc-windows-msvc` succeeds; the DLL is `ohkey_tip.dll`, about 290 KB, static CRT, no VC runtime dependency.
- `dumpbin /exports` shows `DllCanUnloadNow`, `DllGetClassObject`, `DllRegisterServer`, `DllUnregisterServer` (and `DllMain`).
- Clippy with `-D warnings` is clean; unit tests cover the key translation layer (`keys.rs`) and the config parser (`settings.rs`).
- Not done yet (guessed, untested in a real app): registration, typing in Notepad/Word/Chrome/UWP, the language bar button, the Ctrl+Shift toggle and the display attribute. Registration was deliberately not run on this machine.
- The default Rust host on this machine is `stable-x86_64-pc-windows-gnu`: always pass `--target x86_64-pc-windows-msvc`. A plain `cargo test -p oh-key-windows` on the gnu host crashes with an access violation; the msvc run passes.
- Layout: `keys.rs` (pure key decisions and the Ctrl+Shift chord), `settings.rs` (`%APPDATA%\oh-key\config.toml`), `registry.rs` (CLSID, profile, categories, `InstallLayoutOrTip`), `edit.rs` (edit sessions and the composition), `service.rs` (the COM object), `langbar.rs`, `display_attribute.rs`, `exports.rs` (the `Dll*` entry points, compiled out of the test build).
- Deviations from the guide: the TIP is `crates/oh-key-windows` (DLL name `ohkey_tip`); `InstallLayoutOrTip` is not in the `windows` bindings, so it is imported with `windows_link::link!`; category list is the six in section 3 of "Needed"; the lang bar manager is taken by `cast` from the thread manager with a `CoCreateInstance(CLSID_TF_LangBarItemMgr)` fallback [K, check which one works].
- Config keys: `input_method`, `tone_style`, `free_tone_placement`, `spell_check`, `restore_invalid_word`, `macro_enabled`, `start_vietnamese`, `toggle_hotkey` (`"ctrl+shift"` or `"none"`).
- The CLI is `oh-key type [--method ...] <keystrokes>`, `oh-key register [dll]`, `oh-key unregister [dll]` (runs `regsvr32 /s`, needs an elevated terminal).

## 0. Real Rust TSF references (read them, mind the licences)
| Repo | Why | Licence / freshness |
|---|---|---|
| https://github.com/chewing/windows-chewing-tsf | Best reference. Uses windows 0.62.2, `#[implement]` with 8 interfaces on one struct, lang bar button, edit sessions, display attributes, keyboard open/close compartment, a separate `tsfreg` registrar, a WiX installer with x64, ARM64 and x86 DLLs | GPL-3.0, pushed 2026-11-11. Read only; do not copy code into a non-GPL project |
| https://github.com/fkunn1326/azooKey-Windows | `ITfTextInputProcessorEx_Impl`, `ITfKeyEventSink_Impl`, `ITfCompositionSink_Impl` in `crates/client/src/tsf/` and `crates/client/src/engine/composition.rs` | MIT, 178 stars, pushed 2026-07 |
| https://github.com/dec32/ajemi | Clean small TIP. `src/tsf/{text_input_processor,key_event_sink,composition,edit_session}.rs` and `src/register.rs` (CLSID registry, `ITfInputProcessorProfiles`, a full list of categories) | No licence file (read only). Uses windows 0.54 (old API) |
| https://github.com/mominul/tsf-example | Minimal cdylib TIP installed via `regsvr32 TextService.dll` | MIT, windows 0.58 |
| https://github.com/OMAMA-Taioan/khiin-rs (`windows/ime/src/tip/`), https://github.com/determ1ne/weasel-rs, https://github.com/ledao/lufly-im, https://github.com/mkpoli/ainuKey | More `ITfTextInputProcessorEx_Impl` / `ITfKeyEventSink_Impl` examples (found by GitHub code search; not read in depth) | Check each licence |
| Microsoft C++ SampleIME: https://github.com/microsoft/Windows-classic-samples/tree/main/Samples/IME | Reference for semantics (it is the canonical sample) | MIT |
| Docs: https://learn.microsoft.com/en-us/windows/win32/api/msctf/nn-msctf-itftextinputprocessorex and https://learn.microsoft.com/en-us/windows/win32/api/_tsf | Interface semantics | |
| windows-rs bindings: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/UI/TextServices/index.html | | |

## 1. Crate layout
Use a cdylib TIP crate holding only COM glue. The Vietnamese engine goes in a separate pure-Rust crate with no `windows` dependency, so tests run anywhere. The TIP calls `engine.process_key(...)` and gets back an action: `Commit(String)`, `Composing` (preedit) or `PassThrough`.

```toml
[lib] crate-type = ["cdylib"]
[dependencies]
windows = { version = "0.62", features = [
  "Win32_Foundation","Win32_Security","Win32_System_Com","Win32_System_Registry","Win32_System_LibraryLoader",
  "Win32_System_SystemServices","Win32_System_Variant","Win32_System_Ole","Win32_Globalization",
  "Win32_UI_TextServices","Win32_UI_Input_KeyboardAndMouse","Win32_UI_WindowsAndMessaging","Win32_Graphics_Gdi" ] }
windows-core = "0.62"
```
`implement` is on by default in 0.62 [V, compiled]. `RegCreateKeyExW` needs `Win32_Security` [V, compiled].

## 2. COM in-proc server DLL
- Exports `DllMain`, `DllGetClassObject`, `DllCanUnloadNow`, `DllRegisterServer`, `DllUnregisterServer`. Use `#[unsafe(no_mangle)] extern "system" fn` (edition 2024) [V]. Export names come out right without a .def file [V, dumpbin]. rustc prints LNK4104 ("should be PRIVATE") for them; silence with `#![allow(linker_messages)]`.
- In `DllMain`, store the HMODULE in an `AtomicUsize` (needed for the DLL path when registering) [V]. Do no COM or loader work there.
- Keep the exports in a module that is compiled out of the test build (`#[cfg(not(test))]`), so the test executable does not export `DllMain` [V].
- `DllGetClassObject` pattern [V, chewing `com.rs`]: compare the CLSID, return `CLASS_E_CLASSNOTAVAILABLE` otherwise, build the factory with `ClassFactory.into()` and `query` the requested IID. `CreateInstance` rejects aggregation (`CLASS_E_NOAGGREGATION`).
- `DllCanUnloadNow`: `S_OK` only when the live-object count and lock count are both 0. Returning `S_FALSE` is always safe for a TIP.
- Threading model: `Apartment` in the registry [V]. TSF calls the TIP on the app's UI thread, so `RefCell` and `Cell` inside the service are fine, with no Mutex. Never hold a `RefCell` borrow across a call that can re-enter you. `RequestEditSession` with `TF_ES_SYNC` and `SetText` can fire `OnCompositionTerminated`.

## 3. Registration
Two separate steps:
1. COM class registration: `HKLM\SOFTWARE\Classes\CLSID\{CLSID}` default value is the name. Its `InprocServer32` default is the DLL path, with `ThreadingModel=Apartment` [V, ajemi `register.rs`]. Per-user alternative: `HKCU\Software\Classes\CLSID\...` works for COM, but a TIP seen by all apps (especially elevated ones and UWP) must live in HKLM with the DLL in a protected location such as Program Files [K].
2. TIP profile (the per-user list of keyboards, kept under `HKCU\Software\Microsoft\CTF\TIP`). Use `ITfInputProcessorProfileMgr::RegisterProfile`, as chewing's `tsfreg` does [V]: locale `vi-VN` (LANGID 0x042A), `HKL::default()`, `bEnabledByDefault = false`. Then call `InstallLayoutOrTip` (input.dll, `ILOT_INSTALL` = 0) with `"0x042A:{CLSID}{PROFILE}"` to enable it for the current user, as chewing's `enable()` does [V]. Without this the profile is registered but not shown in the language list.
3. Categories, using `ITfCategoryMgr::RegisterCategory(&clsid, &catid, &clsid)` for each [V]. chewing registers: `GUID_TFCAT_TIP_KEYBOARD`, `GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER`, `GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT`, `UIELEMENTENABLED`, `IMMERSIVESUPPORT`, `SYSTRAYSUPPORT`, `COMLESS`. ajemi registers all 16, including `SECUREMODE`, `WOW16`, `GUID_TFCAT_CATEGORY_OF_TIP`, `DISPLAYATTRIBUTEPROPERTY` and `PROPSTYLE_STATIC`, with the comment that without them the IME fails in some apps such as Word [V].
   - Needed: `TIP_KEYBOARD`, `IMMERSIVESUPPORT` (UWP/Store apps, the Start menu, the Win11 shell), `SYSTRAYSUPPORT` (tray icon), `UIELEMENTENABLED` (only if you draw candidates via ITfUIElement), `COMLESS` (activation without COM in some processes), `DISPLAYATTRIBUTEPROVIDER` (underline). `SECUREMODE` is for password fields (secure desktop). `INPUTMODECOMPARTMENT` is for the Win10/11 mode indicator.
   - A TIP must be implemented for `ITfTextInputProcessorEx` if it registers COMLESS / SECUREMODE / UIELEMENTENABLED / WOW16 [V, MS docs].
4. `DllRegisterServer` / `DllUnregisterServer` do steps 1 to 3 here, which makes `regsvr32 ohkey_tip.dll` enough for development, as mominul/tsf-example does [V]. For production prefer a separate `tsfreg.exe` run by the installer, as chewing does. Reasons: step 2 is per user, and `regsvr32` runs elevated as an admin, who might not be the logged-in user [K].

## 4. Implementing the TIP with windows 0.62
One struct, many interfaces [V, chewing `text_service/mod.rs`]:
```rust
#[implement(ITfTextInputProcessorEx, ITfKeyEventSink, ITfThreadMgrEventSink, ITfCompositionSink,
            ITfDisplayAttributeProvider, ITfCompartmentEventSink)]
struct TextService { /* Cell / RefCell fields */ }
```
- Trait names are `X_Impl` for the user-facing trait, and the generated struct is `TextService_Impl`. You write `impl ITfKeyEventSink_Impl for TextService_Impl` [V, compiled].
- Argument types in 0.62: interface parameters are `Ref<ITfThreadMgr>` (use `.ok()?`), not `Option<&T>`. `BOOL` comes from `windows::core::BOOL`. Return `Result<BOOL>` from key sinks. `ITfTextInputProcessorEx_Impl::ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, dwflags: u32)`. Getting self as another interface: `self.to_interface::<ITfKeyEventSink>()` with `windows::core::IUnknownImpl` in scope [V, compiled].
- `ActivateEx` is called instead of `Activate` if you implement the Ex interface. Implement both traits, with `Activate` doing the work and `ActivateEx` delegating [V].
- Activate: save `tid` and the thread manager. Then `thread_mgr.cast::<ITfKeystrokeMgr>()?.AdviseKeyEventSink(tid, &key_sink, true)`. Register the thread manager sink with `thread_mgr.cast::<ITfSource>()?.AdviseSink(&ITfThreadMgrEventSink::IID, &sink)` and keep the cookie. Register the compartment sink the same way (`ITfCompartmentMgr`, then `GetCompartment`, then `ITfSource::AdviseSink`) for open/close. Register the display attribute with `ITfCategoryMgr::RegisterGUID(&DISPLAY_ATTR_GUID)`, which returns the atom, and wrap it with `VARIANT::from(atom as i32)`. `Deactivate` must undo everything: `UnadviseKeyEventSink`, `UnadviseSink`, remove lang bar items, drop the thread manager, and end any composition. Leaks here cause the classic crash on app exit. Mind the cycle service to lang bar button to compartment to service: it is broken in `Deactivate`.

## 5. Keys: `ITfKeyEventSink`
Semantics, stated well in ajemi's comments [V]:
- `OnTestKeyDown`: return whether the key will be eaten if `OnKeyDown` is called. The caller may ignore later results, and may skip `OnKeyDown` if you return FALSE, so gather state here.
- `OnKeyDown` can be called without a preceding `OnTestKeyDown` and can be called after FALSE. Both paths decide with the same pure function (`keys::decide_key`).
- Read modifiers with `GetKeyState(VK_CONTROL/VK_MENU/VK_SHIFT)` (high bit set means down) [K]. Rules: if Ctrl or Alt (and not AltGr) is held, return FALSE and end/commit the composition (so Ctrl+C, Ctrl+V, Alt+Tab shortcuts reach the app). Shift+letter is a capital letter and goes through the engine. Win key: pass through. Keys with no composition active and no Vietnamese meaning (arrows, function keys, Esc with no composition): FALSE.
- Translate with `ToUnicodeEx` using `GetKeyboardState`, so layouts and Caps Lock behave. ajemi does this in `key_event_sink.rs` [V]. Pass the "no change" flag (bit 2 = 0x4 on Win10 1607+) so a pending dead key is not consumed [K].
- Commit keys (space, punctuation): the engine returns `Commit(text)` including the boundary char, which is inserted and the key is consumed. Enter, Tab, Esc, arrows: commit first, then the key goes to the app.
- `OnSetFocus`, `OnPreservedKey`: return FALSE. `ITfKeystrokeMgr::PreserveKey` + `OnPreservedKey` is the alternative for a global toggle hotkey [K].
- Re-entrancy: guard with a `key_busy: Cell<bool>` as chewing does [V].
- Performance: this is the hot path. Keep the engine allocation-free per key. Edit sessions are the real cost, so issue one `RequestEditSession` per key at most.

## 6. Composition: `ITfContextComposition` + `ITfEditSession`
All text changes require an edit session. Pattern [V, chewing `edit_session.rs`; this crate's `edit.rs` compiles against 0.62.2]:
- Start: `ITfContext::RequestEditSession(tid, &session, TF_ES_SYNC | TF_ES_READWRITE)`. The outer `Result` is the call, the inner `HRESULT` is the session's own result; check both. `TF_ES_SYNC` can be refused (`TF_E_SYNCHRONOUS`) in some contexts: fall back to `TF_ES_ASYNC` [K].
- First key: `ITfInsertAtSelection::InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])` gives the caret range, then `ITfContextComposition::StartComposition(ec, &range, &sink)`.
- Update: `SetText(ec, 0, text)` on `composition.GetRange()`, set `GUID_PROP_ATTRIBUTE` on the range for the underline, move the caret to the end.
- End/commit: leave the final text in the range, clear the attribute property, then `composition.EndComposition(ec)`. For cancel, set empty text then End.
- `SetSelection` takes `TF_SELECTION { range: ManuallyDrop<Option<ITfRange>>, style }`; wrap and `ManuallyDrop::into_inner` after the call to avoid leaking or double release [V].
- `ITfCompositionSink::OnCompositionTerminated(ecwrite, pcomposition)` fires when the app kills the composition (focus change, click). Drop your composition and reset the engine syllable.
- Focus change (`ITfThreadMgrEventSink::OnSetFocus`) and mouse clicks: commit the preedit.
- Some apps (terminals, games, Chrome in certain modes) behave badly with composition. Provide a fallback "direct commit" mode, committing each syllable via `InsertTextAtSelection` without an underline. Chewing has a `quirk.rs` for per-app workarounds [V file exists; not read].

## 7. Display attributes (underline)
Implement `ITfDisplayAttributeProvider` (`EnumDisplayAttributeInfo`, `GetDisplayAttributeInfo`) with an `ITfDisplayAttributeInfo` object that returns a `TF_DISPLAYATTRIBUTE` (underline `TF_LS_DOT` / `TF_LS_SOLID`, `bBoldLine` false, colors `TF_CT_NONE`) and your attribute GUID. Register the CLSID with category `GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER`. At runtime: `RegisterGUID` gives an atom, then set `GUID_PROP_ATTRIBUTE` on the composition range [V: chewing `display_attribute.rs`, ajemi `DISPLAY_ATTR_ID`].

## 8. VI/EN toggle, tray and compartment
- Open/close state: `GUID_COMPARTMENT_KEYBOARD_OPENCLOSE` on the thread manager's compartment via `ITfCompartmentMgr::GetCompartment`, then `ITfCompartment::SetValue(tid, &VARIANT::from(1i32))`. Listen with `ITfCompartmentEventSink::OnChange` (register via `ITfSource`) [V]. This is also what the Win10/11 taskbar mode indicator toggles. The compartment is the single source of truth here; the lang bar button and the Ctrl+Shift chord both write to it. With `GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT` you may also expose `GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION` for the "VI / EN" label [K, not done].
- Lang bar button: `#[implement(ITfLangBarItemButton, ITfLangBarItem, ITfSource)]` with a hand-written `ITfSource` so you can call `ITfLangBarItemSink::OnUpdate(TF_LBI_ICON | TF_LBI_TOOLTIP)` when the state changes [V, chewing `lang_bar.rs`; this crate's `langbar.rs` compiles]. Add it with `ITfLangBarItemMgr::AddItem`, remove it in `Deactivate`. On Win11, tray visibility also needs `SYSTRAYSUPPORT` and `IMMERSIVESUPPORT`.
- Icons: this crate draws a 16x16 colored square with "V" or "E" through GDI; real .ico files for the profile and dark/light variants are open work [V: ajemi picks via `SystemUsesLightTheme`].

## 9. Shortcuts and modifier handling summary
- Ctrl/Alt chords: pass through. Win: pass through. Shift+Backspace etc.: treat per engine.
- Toggle hotkeys (Ctrl+Shift, Alt+Z, Ctrl+Space): either `ITfKeystrokeMgr::PreserveKey` with `TF_PRESERVEDKEY` and `OnPreservedKey`, or track Shift/Ctrl presses in the key sink and toggle on release if no other key came between (the Unikey behavior; this is `keys::ToggleChordTracker`) [K]. Windows also has its own Ctrl+Shift (switch layout) and Win+Space shortcuts. Tell users these conflict.
- Caps Lock: if on, decide the policy once (ajemi disables the IME when Caps Lock is on) [V]. Here Caps Lock is left to the layout.

## 10. Architectures and bitness
- Ship three DLLs: x64, ARM64 (native ARM64 apps), and x86 (32-bit apps load the 32-bit TIP; WoW64 processes cannot load a 64-bit DLL) [V, chewing's WiX has `chewing_tip.dll`, `_arm64`, `_x64` and an x86 build]. Start with x64 + x86; add ARM64 later (targets `aarch64-pc-windows-msvc`, `x86_64-pc-windows-msvc`, `i686-pc-windows-msvc`).
- The same CLSID is registered under the 64-bit registry view and under `WOW6432Node` pointing to the 32-bit DLL [V, WiX registers two `Class` entries for the same Id with different servers]. TSF profile data is shared.
- Never put the engine in a separate process by default. If you want one later, chewing uses `interprocess`, but it adds latency and failure modes.
- Make the DLL self-contained: static CRT (`.cargo/config.toml`: `rustflags = ["-C", "target-feature=+crt-static"]` for the msvc target) so there is no VC runtime dependency in every app [V, dumpbin /dependents shows none]. Avoid heavy dependencies and global threads in `DllMain`. Keep the binary small, since it loads into every GUI process.
- Updating an in-use DLL: loaded DLLs stay locked, so ship versioned filenames (chewing does: `chewing_tip_$(VersionSuffix).dll`) [V] and let the installer register the new one and schedule removal of the old at reboot.

## 11. Installer / registration modes
- Machine-wide (recommended): install to `%ProgramFiles%\OhKey\`, write HKLM COM keys, then run a registrar per user at first run or via a logon script, because the profile list is per user. chewing's WiX runs `tsfreg.exe -r` as a deferred, non-impersonated custom action (so effectively for the installing admin) and then enables [V]. Other users must also register the profile: run a small registrar on first launch or via an `HKLM\...\Run` entry [K].
- Per-user: write COM keys under HKCU\Software\Classes and put the DLL in `%LOCALAPPDATA%`. Works in most desktop apps, but elevated and Store apps may not load it [K]. Good for dev builds.
- Dev loop: `oh-key register` (elevated; runs `regsvr32 /s ohkey_tip.dll`) and `oh-key unregister` to undo [V, tsf-example]. Unregister removes the profile (`UnregisterProfile`) and categories (`UnregisterCategory`) [V].
- Use a pair of stable, project-owned GUIDs: CLSID and profile GUID (generated once, never change). Add a Debug build with different GUIDs so it can be installed beside the release [not done].

## 12. Debugging
- Attach a debugger to a target app (Notepad is a good first target; the TSF DLL is loaded into the app's process, and `ctfmon.exe` hosts the language bar). Rust `log` to `OutputDebugStringW`, viewed in DebugView or WinDbg [V: chewing has a `WinDbg` appender].
- Because the DLL is locked while loaded, close all apps that loaded it (or sign out) before overwriting, or use versioned filenames.
- Crash dumps: enable WER LocalDumps via `HKLM\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps` [V, chewing does this for nightly builds].
- A panic must never cross the FFI boundary. The release profile uses `panic = "abort"`, so keep COM method bodies panic-free; a crash in a TIP kills the host app [K].
- Test with Notepad, Word, Chrome, Edge, VS Code, Windows Terminal, a UWP app (Calculator, Settings), the Start menu search box, and elevated cmd.
- Test matrix to automate: engine unit tests (typing sequences to expected text, for Telex and VNI, including tone placement e.g. `hoà` vs `hòa`, `uơ`, `qu`, `gi`, restoring English words), plus a small harness using a TSF-based test app.

## 13. Pitfalls that apply to a Vietnamese IME
1. The "backspace over composition" problem Unikey has: keep the preedit inside the TSF composition (not as sent keystrokes) so apps never see partial text. This is the main reliability win over Unikey, which injects backspaces.
2. Return TRUE from `OnTestKeyDown` only for keys you will really handle; a mismatch causes lost or doubled keys in some apps. Here only letters start a word; once composing, any printable key is claimed.
3. `OnCompositionTerminated` while the user is typing (some web editors): reset engine state and do not re-insert.
4. Undo (Ctrl+Z): one composition = one undo unit, which is the TSF default.
5. Do not allocate or log on the hot path; formatting log strings in `OnTestKeyDown` adds latency.

## 14. Suggested milestones
1. [done, untested at runtime] cdylib with `DllGetClassObject`, class factory, `TextService`, registration with profile and categories.
2. [done, untested at runtime] Composition: type "a", see underlined "a", Space commits.
3. [done, untested at runtime] Engine plugged in (Telex, VNI, VIQR), display attributes, lang bar toggle plus open/close compartment.
4. Next: test in Notepad, then the app matrix above; x86 + ARM64 builds, WiX installer, per-app quirk table, real icons.

Sources checked on 2026-11-12: https://github.com/chewing/windows-chewing-tsf (files tip/src/lib.rs, com.rs, text_service/mod.rs, edit_session.rs, lang_bar.rs, crates/tsfreg/src/main.rs, installer/windows-chewing-tsf.wxs), https://github.com/dec32/ajemi (src/register.rs, src/tsf/*), https://github.com/mominul/tsf-example (Cargo.toml, README), https://github.com/fkunn1326/azooKey-Windows, crates.io API for `windows` 0.62.2, MS Learn ITfTextInputProcessorEx page. Not verified by running code: any [K] item and everything under "Not done yet" above.
