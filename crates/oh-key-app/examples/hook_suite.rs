//! End-to-end suite for the keyboard hook, against this process's own windows only.
//!
//! One short-lived process installs the real hook, opens a plain EDIT, a RICHEDIT50W and an
//! EDIT wired to a custom inline-autocomplete source (the browser address bar bug), types each
//! case through `SendInput` with virtual keys, and compares the text read back. It types only
//! while its own window is the foreground window and exits within 60 seconds whatever happens.
//!
//! Run: `cargo run -p oh-key-app --example hook_suite --target x86_64-pc-windows-msvc`
//! Exit code: 0 all passed, 1 some cases failed, 2 no foreground (nothing typed), 3 timed out,
//! 4 the hook could not start, 5 Caps Lock was already on.
//!
//! An optional first argument limits the run to cases whose name contains one of its
//! comma-separated parts (the whole suite can outlast the 58 second watchdog).
//!
//! Key script: plain characters are typed; `{BS}` `{ENTER}` `{TAB}` `{ESC}` `{CAPS}` press that
//! key, `{CTRLSHIFT}` and `{ALTZ}` press the toggle chords, `{WAIT}` pauses 350 ms so the
//! autocomplete suggestion lands.

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../src/hook.rs"]
mod hook;

#[cfg(windows)]
mod suite {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use oh_key_engine::Config;
    use oh_key_ipc::{Settings, SharedState, ToggleHotkey, load_settings};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemAlloc, IEnumString, IEnumString_Impl,
    };
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        SetFocus, VIRTUAL_KEY, VkKeyScanW,
    };
    use windows::Win32::UI::Shell::{
        ACO_AUTOAPPEND, ACO_AUTOSUGGEST, CLSID_AutoComplete, IAutoComplete2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL,
        ES_MULTILINE, GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
        MSG, PM_REMOVE, PeekMessageW, RegisterClassExW, SW_SHOW, SendMessageW, SetForegroundWindow,
        SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_STYLE, WNDCLASSEXW, WS_BORDER,
        WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };
    use windows::core::{HRESULT, PCWSTR, PWSTR, implement, w};

    use crate::hook::{self, HookInputs, HookOptions};

    const WATCHDOG: Duration = Duration::from_secs(58);
    const SUGGESTION_WAIT: Duration = Duration::from_millis(350);
    const EM_GETSEL: u32 = 0x00B0;
    const VK_BACK: u16 = 0x08;
    const VK_TAB: u16 = 0x09;
    const VK_RETURN: u16 = 0x0D;
    const VK_SHIFT: u16 = 0x10;
    const VK_CONTROL: u16 = 0x11;
    const VK_MENU: u16 = 0x12;
    const VK_CAPITAL: u16 = 0x14;
    const VK_ESCAPE: u16 = 0x1B;
    const VK_Z: u16 = 0x5A;

    static HOTKEY: AtomicU8 = AtomicU8::new(0);
    static VIETNAMESE: AtomicBool = AtomicBool::new(true);
    static TOGGLES: AtomicUsize = AtomicUsize::new(0);

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Control {
        Plain,
        Rich,
        AddressBar,
    }

    impl Control {
        fn label(self) -> &'static str {
            match self {
                Control::Plain => "edit",
                Control::Rich => "rich",
                Control::AddressBar => "addr",
            }
        }
    }

    struct Case {
        name: &'static str,
        control: Control,
        keys: &'static str,
        expected: &'static str,
        key_delay_ms: u64,
        hotkey: u8,
    }

    const fn case(
        name: &'static str,
        control: Control,
        keys: &'static str,
        expected: &'static str,
    ) -> Case {
        Case {
            name,
            control,
            keys,
            expected,
            key_delay_ms: 25,
            hotkey: 0,
        }
    }

    const fn fast(mut base: Case) -> Case {
        base.key_delay_ms = 5;
        base
    }

    const fn with_hotkey(mut base: Case, hotkey: u8) -> Case {
        base.hotkey = hotkey;
        base
    }

    const HARD_RHYMES_KEYS: &str = "nguwowif khuyru thuowr quoocs giuwx nguyeenx dduowngf ";
    const HARD_RHYMES_TEXT: &str = "người khuỷu thuở quốc giữ nguyễn đường ";

    fn all_cases() -> Vec<Case> {
        use Control::{AddressBar, Plain, Rich};
        vec![
            case("basic-sentence", Plain, "Vieetj Nam ", "Việt Nam "),
            case(
                "hard-rhymes-edit",
                Plain,
                HARD_RHYMES_KEYS,
                HARD_RHYMES_TEXT,
            ),
            case("hard-rhymes-rich", Rich, HARD_RHYMES_KEYS, HARD_RHYMES_TEXT),
            case(
                "hard-rhymes-addr",
                AddressBar,
                HARD_RHYMES_KEYS,
                HARD_RHYMES_TEXT,
            ),
            fast(case(
                "fast-5ms-edit",
                Plain,
                HARD_RHYMES_KEYS,
                HARD_RHYMES_TEXT,
            )),
            fast(case(
                "fast-5ms-rich",
                Rich,
                HARD_RHYMES_KEYS,
                HARD_RHYMES_TEXT,
            )),
            fast(case(
                "fast-5ms-addr",
                AddressBar,
                HARD_RHYMES_KEYS,
                HARD_RHYMES_TEXT,
            )),
            case("uppercase-shift", Plain, "VIEETJ ", "VIỆT "),
            case("capitalised", Plain, "Nguwowif ", "Người "),
            case(
                "caps-lock-edit",
                Plain,
                "{CAPS}vieetj {CAPS}vieetj",
                "VIỆT việt",
            ),
            case("caps-lock-rich", Rich, "{CAPS}vieetj {CAPS}", "VIỆT "),
            case("backspace-then-retype", Plain, "vieet{BS}tj ", "việt "),
            case("backspace-to-empty", Plain, "toois{BS}{BS}{BS}toi ", "toi "),
            case("backspace-mid-word-rich", Rich, "chaof{BS}o ", "chào "),
            case(
                "backspace-then-retype-addr",
                AddressBar,
                "vieet{BS}tj ",
                "việt ",
            ),
            case("backspace-plain-letters", Plain, "abc{BS}d ", "abd "),
            case(
                "english-words",
                Plain,
                "text window coffee ",
                "text window coffee ",
            ),
            case(
                "english-words-rich",
                Rich,
                "text window coffee ",
                "text window coffee ",
            ),
            case("toggle-ctrl-shift", Plain, "{CTRLSHIFT}", "mode:english"),
            case(
                "toggle-ctrl-shift-twice",
                Plain,
                "{CTRLSHIFT}{CTRLSHIFT}",
                "mode:vietnamese",
            ),
            case(
                "toggle-ctrl-shift-after-typing",
                Plain,
                "vieetj {CTRLSHIFT}",
                "mode:english",
            ),
            with_hotkey(case("toggle-alt-z", Plain, "{ALTZ}", "mode:english"), 1),
            with_hotkey(
                case(
                    "toggle-alt-z-after-typing",
                    Plain,
                    "vieetj {ALTZ}",
                    "mode:english",
                ),
                1,
            ),
            with_hotkey(
                case(
                    "toggle-alt-z-keeps-typing-working",
                    Plain,
                    "{ALTZ}{ALTZ}vieetj ",
                    "mode:vietnamese",
                ),
                1,
            ),
            case(
                "digits-punctuation",
                Plain,
                "vieetj, nam. 2026 vieetj1",
                "việt, nam. 2026 việt1",
            ),
            case(
                "enter-commits-rich",
                Rich,
                "vieetj{ENTER}nam{ENTER}",
                "việt\r\nnam\r\n",
            ),
            case("tab-commits-edit", Plain, "vieetj{TAB}", "việt"),
            case("english-then-enter-rich", Rich, "text{ENTER}", "text\r\n"),
            case(
                "addr-suggestion-then-circumflex",
                AddressBar,
                "vie{WAIT}e",
                "viê",
            ),
            case(
                "addr-suggestion-then-tone",
                AddressBar,
                "vie{WAIT}ej",
                "việ",
            ),
            case("addr-suggestion-d-bar", AddressBar, "d{WAIT}di", "đi"),
            case(
                "addr-suggestion-backspace",
                AddressBar,
                "vie{WAIT}{BS}",
                "vi",
            ),
            case(
                "addr-word-then-suggestion",
                AddressBar,
                "coffee{WAIT}",
                "coffee",
            ),
            case("addr-circumflex-word", AddressBar, "vie{WAIT}etj ", "việt "),
        ]
    }

    // ---- the owner's tray app ---------------------------------------------------------------

    /// The oh-key running in the owner's tray also hooks the keyboard and converts the keys this
    /// suite injects, and its hotkey toggles on the suite's chords. While the suite runs, the
    /// tray is told, through the shared state it already reads, to leave this program alone;
    /// the settings and the mode the owner had are put back at the end.
    struct OwnerShield {
        original_settings: Settings,
        original_mode: bool,
    }

    impl OwnerShield {
        fn raise() -> Option<Self> {
            let shared = SharedState::open().ok()?;
            let original_settings = shared.read_settings().unwrap_or_else(load_settings);
            let original_mode = shared.vietnamese();
            let mut shielded = original_settings.clone();
            shielded.excluded_apps.push(own_exe_name()?);
            shared.publish_settings(&shielded);
            Some(OwnerShield {
                original_settings,
                original_mode,
            })
        }

        fn lower(&self) {
            if let Ok(shared) = SharedState::open() {
                shared.publish_settings(&self.original_settings);
                shared.set_vietnamese(self.original_mode);
            }
        }
    }

    fn own_exe_name() -> Option<String> {
        let path = std::env::current_exe().ok()?;
        Some(path.file_name()?.to_string_lossy().to_lowercase())
    }

    // ---- autocomplete source ----------------------------------------------------------------

    #[implement(IEnumString)]
    struct SuggestionSource {
        suggestions: Vec<Vec<u16>>,
        position: Cell<usize>,
    }

    impl SuggestionSource {
        fn new() -> Self {
            let words = [
                "vietnam.example.com",
                "viet-nam.example.com",
                "text.example.com",
                "coffee.example.com",
                "dien.example.com",
                "vid.example.com",
                "view.example.com",
            ];
            let suggestions = words
                .iter()
                .map(|word| word.encode_utf16().chain(std::iter::once(0)).collect())
                .collect();
            SuggestionSource {
                suggestions,
                position: Cell::new(0),
            }
        }
    }

    impl IEnumString_Impl for SuggestionSource_Impl {
        fn Next(&self, count: u32, items: *mut PWSTR, fetched: *mut u32) -> HRESULT {
            let mut delivered = 0u32;
            while delivered < count && self.position.get() < self.suggestions.len() {
                let suggestion = &self.suggestions[self.position.get()];
                let bytes = suggestion.len() * 2;
                let memory = unsafe { CoTaskMemAlloc(bytes) } as *mut u16;
                unsafe {
                    std::ptr::copy_nonoverlapping(suggestion.as_ptr(), memory, suggestion.len());
                    *items.add(delivered as usize) = PWSTR(memory);
                }
                self.position.set(self.position.get() + 1);
                delivered += 1;
            }
            if !fetched.is_null() {
                unsafe { *fetched = delivered };
            }
            if delivered == count {
                HRESULT(0)
            } else {
                HRESULT(1)
            }
        }

        fn Skip(&self, count: u32) -> HRESULT {
            self.position.set(self.position.get() + count as usize);
            HRESULT(0)
        }

        fn Reset(&self) -> windows::core::Result<()> {
            self.position.set(0);
            Ok(())
        }

        fn Clone(&self) -> windows::core::Result<IEnumString> {
            let copy = SuggestionSource {
                suggestions: self.suggestions.clone(),
                position: Cell::new(self.position.get()),
            };
            Ok(copy.into())
        }
    }

    fn attach_autocomplete(edit: HWND) -> windows::core::Result<IAutoComplete2> {
        let auto_complete: IAutoComplete2 =
            unsafe { CoCreateInstance(&CLSID_AutoComplete, None, CLSCTX_INPROC_SERVER)? };
        let source: IEnumString = SuggestionSource::new().into();
        unsafe {
            auto_complete.Init(edit, &source, PCWSTR::null(), PCWSTR::null())?;
            auto_complete.SetOptions((ACO_AUTOSUGGEST.0 | ACO_AUTOAPPEND.0) as u32)?;
        }
        Ok(auto_complete)
    }

    // ---- windows ----------------------------------------------------------------------------

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    fn pump_messages(duration: Duration) {
        let deadline = Instant::now() + duration;
        let mut message = MSG::default();
        loop {
            while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            if Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(Duration::from_micros(500));
        }
    }

    struct Windows {
        top: HWND,
        plain: HWND,
        rich: HWND,
        address_bar: HWND,
    }

    impl Windows {
        fn control(&self, control: Control) -> HWND {
            match control {
                Control::Plain => self.plain,
                Control::Rich => self.rich,
                Control::AddressBar => self.address_bar,
            }
        }
    }

    fn create_child(
        class: PCWSTR,
        extra_style: u32,
        top: HWND,
        row: i32,
    ) -> windows::core::Result<HWND> {
        let instance = unsafe { GetModuleHandleW(None)? };
        unsafe {
            CreateWindowExW(
                Default::default(),
                class,
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(extra_style),
                10,
                10 + row * 80,
                560,
                70,
                Some(top),
                None,
                Some(instance.into()),
                None,
            )
        }
    }

    fn create_windows() -> windows::core::Result<Windows> {
        let instance = unsafe { GetModuleHandleW(None)? };
        unsafe { LoadLibraryW(w!("Msftedit.dll"))? };
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("oh-key.hook-suite"),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
        let top = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("oh-key.hook-suite"),
                w!("oh-key hook suite"),
                WS_OVERLAPPEDWINDOW,
                200,
                200,
                620,
                340,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };
        let single_line = ES_AUTOHSCROLL as u32;
        let plain = create_child(w!("EDIT"), single_line, top, 0)?;
        let rich = create_child(w!("RICHEDIT50W"), ES_MULTILINE as u32, top, 1)?;
        let address_bar = create_child(w!("EDIT"), single_line, top, 2)?;
        Ok(Windows {
            top,
            plain,
            rich,
            address_bar,
        })
    }

    fn take_foreground(window: HWND) -> bool {
        let current_thread = unsafe { GetCurrentThreadId() };
        let front = unsafe { GetForegroundWindow() };
        let front_thread = unsafe { GetWindowThreadProcessId(front, None) };
        unsafe {
            let _ = AttachThreadInput(current_thread, front_thread, true);
            let _ = ShowWindow(window, SW_SHOW);
            let _ = SetForegroundWindow(window);
            let _ = AttachThreadInput(current_thread, front_thread, false);
        }
        for _ in 0..40 {
            if unsafe { GetForegroundWindow() } == window {
                return true;
            }
            pump_messages(Duration::from_millis(50));
        }
        false
    }

    fn read_text(control: HWND) -> String {
        let mut buffer = [0u16; 1024];
        let length = unsafe { GetWindowTextW(control, &mut buffer) }.max(0) as usize;
        String::from_utf16_lossy(&buffer[..length])
    }

    /// The text before the selection: for the address bar that is what the user typed, since an
    /// inline suggestion arrives selected after it.
    fn read_typed_text(control: HWND) -> String {
        let full: Vec<u16> = read_text(control).encode_utf16().collect();
        let mut start = 0u32;
        let mut end = 0u32;
        unsafe {
            SendMessageW(
                control,
                EM_GETSEL,
                Some(WPARAM(&mut start as *mut u32 as usize)),
                Some(LPARAM(&mut end as *mut u32 as isize)),
            );
        }
        let keep = if end > start {
            start as usize
        } else {
            full.len()
        };
        String::from_utf16_lossy(&full[..keep.min(full.len())])
    }

    fn caps_lock_is_on() -> bool {
        let state = unsafe { GetKeyState(i32::from(VK_CAPITAL)) };
        state & 1 != 0
    }

    // ---- typing -----------------------------------------------------------------------------

    fn key_event(virtual_key: u16, release: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(virtual_key),
                    wScan: 0,
                    dwFlags: if release {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send_events(events: &[INPUT]) {
        unsafe { SendInput(events, size_of::<INPUT>() as i32) };
    }

    fn press_chord(keys: &[u16]) {
        let mut events: Vec<INPUT> = keys.iter().map(|key| key_event(*key, false)).collect();
        events.extend(keys.iter().rev().map(|key| key_event(*key, true)));
        send_events(&events);
    }

    fn type_character(character: char) {
        let scan = unsafe { VkKeyScanW(character as u16) };
        let virtual_key = (scan & 0xFF) as u16;
        let needs_shift = (scan >> 8) & 1 != 0;
        if needs_shift {
            press_chord(&[VK_SHIFT, virtual_key]);
        } else {
            press_chord(&[virtual_key]);
        }
    }

    enum Action {
        Character(char),
        Chord(Vec<u16>),
        Wait,
    }

    fn parse_script(script: &str) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut characters = script.chars();
        while let Some(character) = characters.next() {
            if character != '{' {
                actions.push(Action::Character(character));
                continue;
            }
            let name: String = characters
                .by_ref()
                .take_while(|next| *next != '}')
                .collect();
            actions.push(match name.as_str() {
                "BS" => Action::Chord(vec![VK_BACK]),
                "ENTER" => Action::Chord(vec![VK_RETURN]),
                "TAB" => Action::Chord(vec![VK_TAB]),
                "ESC" => Action::Chord(vec![VK_ESCAPE]),
                "CAPS" => Action::Chord(vec![VK_CAPITAL]),
                "CTRLSHIFT" => Action::Chord(vec![VK_CONTROL, VK_SHIFT]),
                "ALTZ" => Action::Chord(vec![VK_MENU, VK_Z]),
                "WAIT" => Action::Wait,
                other => panic!("unknown key script token {other}"),
            });
        }
        actions
    }

    fn run_script(script: &str, key_delay: Duration, top: HWND) -> bool {
        for action in parse_script(script) {
            let front = unsafe { GetForegroundWindow() };
            if front != top {
                let mut class = [0u16; 64];
                let length = unsafe { GetClassNameW(front, &mut class) }.max(0) as usize;
                eprintln!(
                    "hook_suite: lost the foreground to {:?}",
                    String::from_utf16_lossy(&class[..length])
                );
                return false;
            }
            match action {
                Action::Character(character) => type_character(character),
                Action::Chord(keys) => press_chord(&keys),
                Action::Wait => {
                    pump_messages(SUGGESTION_WAIT);
                    continue;
                }
            }
            pump_messages(key_delay);
        }
        true
    }

    // ---- running cases ----------------------------------------------------------------------

    struct Outcome {
        passed: bool,
        actual: String,
    }

    fn prepare_case(windows: &Windows, case: &Case) -> HWND {
        let control = windows.control(case.control);
        HOTKEY.store(case.hotkey, Ordering::Relaxed);
        VIETNAMESE.store(true, Ordering::Relaxed);
        unsafe {
            let _ = SetWindowTextW(control, w!(""));
            let _ = SetFocus(Some(control));
        }
        // The hook thread notices mode and focus changes on its 50 ms timer.
        pump_messages(Duration::from_millis(130));
        control
    }

    const ATTEMPTS: usize = 3;

    /// Another program (the owner's browser, say) may take the foreground between keys. The case
    /// then stops typing at once and runs again from a clean field, up to three times.
    fn run_case(windows: &Windows, case: &Case) -> Outcome {
        let delay = Duration::from_millis(case.key_delay_ms);
        let mut outcome = Outcome {
            passed: false,
            actual: String::from("(foreground lost, nothing typed)"),
        };
        for _ in 0..ATTEMPTS {
            if !take_foreground(windows.top) {
                continue;
            }
            let control = prepare_case(windows, case);
            let toggles_before = TOGGLES.load(Ordering::Relaxed);
            let resets_before = hook::outside_interference();
            let mut completed = run_script(case.keys, delay, windows.top);
            pump_messages(Duration::from_millis(200));
            let actual = if case.expected.starts_with("mode:") {
                let on = VIETNAMESE.load(Ordering::Relaxed);
                String::from(if on {
                    "mode:vietnamese"
                } else {
                    "mode:english"
                })
            } else if case.control == Control::AddressBar {
                read_typed_text(control)
            } else {
                read_text(control)
            };
            // Escape ends any word the case left open, so the next case starts clean.
            press_chord(&[VK_ESCAPE]);
            pump_messages(Duration::from_millis(40));
            // A flicker of the foreground or a key from the person at the keyboard makes a disturbed
            // run, which is not a result.
            completed &= hook::outside_interference() == resets_before;
            if completed {
                let toggles = TOGGLES.load(Ordering::Relaxed) - toggles_before;
                return Outcome {
                    passed: actual == case.expected,
                    actual: format!("{actual:?} (mode toggled {toggles} times)"),
                };
            }
            outcome.actual =
                format!("{actual:?} (disturbed by the foreground or the keyboard, no result)");
        }
        outcome
    }

    /// Another program on the machine (UniKey, for one) may convert keys the way oh-key does and
    /// toggles on the same chords. With the hook off, a clean field means no such converter is on.
    fn foreign_converter_is_on(windows: &Windows) -> bool {
        HOTKEY.store(2, Ordering::Relaxed);
        VIETNAMESE.store(false, Ordering::Relaxed);
        let probe = Case {
            name: "probe",
            control: Control::Plain,
            keys: "",
            expected: "",
            key_delay_ms: 25,
            hotkey: 2,
        };
        let control = prepare_case(windows, &probe);
        VIETNAMESE.store(false, Ordering::Relaxed);
        pump_messages(Duration::from_millis(130));
        run_script("vieetj", Duration::from_millis(25), windows.top);
        pump_messages(Duration::from_millis(200));
        let converted = read_text(control) != "vieetj";
        press_chord(&[VK_ESCAPE]);
        pump_messages(Duration::from_millis(40));
        converted
    }

    /// Flips the foreign converter with its own chord, with the hook's hotkey off.
    fn set_foreign_converter(windows: &Windows, wanted: bool) {
        for _ in 0..2 {
            if foreign_converter_is_on(windows) == wanted {
                return;
            }
            press_chord(&[VK_CONTROL, VK_SHIFT]);
            pump_messages(Duration::from_millis(200));
        }
    }

    fn percentile(sorted: &[u32], fraction: f64) -> u32 {
        if sorted.is_empty() {
            return 0;
        }
        let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
        sorted[index]
    }

    fn print_timings() {
        let mut timings = hook::callback_microseconds();
        timings.sort_unstable();
        println!(
            "hook callback over {} keys: p50 {} us, p99 {} us, max {} us",
            timings.len(),
            percentile(&timings, 0.5),
            percentile(&timings, 0.99),
            timings.last().copied().unwrap_or(0)
        );
    }

    fn print_row(case: &Case, outcome: &Outcome) {
        let verdict = if outcome.passed { "PASS" } else { "FAIL" };
        println!(
            "{:<34} {:<5} {:<5} {}",
            case.name,
            case.control.label(),
            case.key_delay_ms,
            verdict
        );
        if !outcome.passed {
            println!("    expected {:?}", case.expected);
            println!("    actual   {}", outcome.actual);
        }
    }

    fn hotkey_from_code(code: u8) -> ToggleHotkey {
        match code {
            0 => ToggleHotkey::CtrlShift,
            1 => ToggleHotkey::AltZ,
            _ => ToggleHotkey::Disabled,
        }
    }

    pub fn run() -> i32 {
        let shield = std::sync::Arc::new(OwnerShield::raise());
        let watchdog_shield = std::sync::Arc::clone(&shield);
        std::thread::spawn(move || {
            std::thread::sleep(WATCHDOG);
            eprintln!("hook_suite: timed out");
            if let Some(shield) = watchdog_shield.as_ref() {
                shield.lower();
            }
            std::process::exit(3);
        });
        // The tray reads the shared state on its 50 ms timer.
        std::thread::sleep(Duration::from_millis(250));
        let exit_code = run_with_windows();
        if let Some(shield) = shield.as_ref() {
            shield.lower();
        }
        exit_code
    }

    fn run_with_windows() -> i32 {
        if caps_lock_is_on() {
            eprintln!("hook_suite: Caps Lock is on, turn it off first; nothing was typed");
            return 5;
        }
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let Ok(windows) = create_windows() else {
            return 2;
        };
        let _auto_complete = attach_autocomplete(windows.address_bar);
        if _auto_complete.is_err() {
            eprintln!("hook_suite: the autocomplete source could not be attached");
        }
        let provider = Box::new(|| HookInputs {
            excluded_apps: Vec::new(),
            vietnamese: VIETNAMESE.load(Ordering::Relaxed),
            engine: Config::default(),
            toggle_hotkey: hotkey_from_code(HOTKEY.load(Ordering::Relaxed)),
            macros: None,
        });
        let toggle = Box::new(|| {
            VIETNAMESE.fetch_xor(true, Ordering::Relaxed);
            TOGGLES.fetch_add(1, Ordering::Relaxed);
        });
        let options = HookOptions {
            accept_foreign_injected_keys: true,
        };
        let Some(hook_handle) = hook::start(provider, toggle, options) else {
            eprintln!("hook_suite: the hook could not start");
            return 4;
        };
        let exit_code = run_all(&windows);
        if caps_lock_is_on() {
            press_chord(&[VK_CAPITAL]);
        }
        drop(hook_handle);
        unsafe {
            let _ = DestroyWindow(windows.top);
        }
        exit_code
    }

    fn run_all(windows: &Windows) -> i32 {
        if !take_foreground(windows.top) {
            eprintln!("hook_suite: could not get the foreground, nothing was typed");
            return 2;
        }
        pump_messages(Duration::from_millis(400));
        let filter = std::env::args().nth(1).unwrap_or_default();
        let converter_was_on = foreign_converter_is_on(windows);
        if converter_was_on {
            println!("note: another keyboard converter is on; switching it off for the run");
            set_foreign_converter(windows, false);
        }
        let cases: Vec<Case> = all_cases()
            .into_iter()
            .filter(|case| filter.split(',').any(|part| case.name.contains(part)))
            .collect();
        println!("{:<34} {:<5} {:<5} result", "case", "ctrl", "ms");
        let mut results: Vec<(&Case, Outcome)> = Vec::new();
        for case in &cases {
            let outcome = run_case(windows, case);
            print_row(case, &outcome);
            results.push((case, outcome));
        }
        let failures = results
            .iter()
            .filter(|(_, outcome)| !outcome.passed)
            .count();
        println!("{} passed, {} failed", results.len() - failures, failures);
        // The toggle cases press chords that the other converter sees too: leave it as found.
        set_foreign_converter(windows, converter_was_on);
        print_timings();
        i32::from(failures > 0)
    }
}

#[cfg(windows)]
fn main() {
    std::process::exit(suite::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("hook_suite is Windows only");
}
