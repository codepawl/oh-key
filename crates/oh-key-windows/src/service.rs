//! The text service: one COM object that TSF activates per thread.
//!
//! TSF calls it on the application's UI thread, so plain `Cell` and
//! `RefCell` are enough. No borrow is held across a call that can re-enter
//! the service (edit sessions can end the composition and call us back).

use crate::display_attribute::{ComposingAttributeEnumerator, ComposingAttributeInfo};
use crate::edit::{CompositionEdit, SharedComposition, run_edit};
use crate::keys::{
    KeyDecision, KeyEvent, ToggleChordTracker, VK_CONTROL, VK_MENU, VK_RMENU, decide_key,
    will_consume,
};
use crate::langbar::{LangBarButton, as_lang_bar_item};
use crate::registry::DISPLAY_ATTRIBUTE_GUID;
use crate::settings::{Settings, ToggleHotkey, load_settings};
use oh_key_engine::{Engine, KeyResult};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::{E_INVALIDARG, E_UNEXPECTED, LPARAM, WPARAM};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardLayout, GetKeyboardState, ToUnicodeEx,
};
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_LangBarItemMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
    IEnumTfDisplayAttributeInfo, ITfCategoryMgr, ITfCompartment, ITfCompartmentEventSink,
    ITfCompartmentEventSink_Impl, ITfCompartmentMgr, ITfComposition, ITfCompositionSink,
    ITfCompositionSink_Impl, ITfContext, ITfDisplayAttributeInfo, ITfDisplayAttributeProvider,
    ITfDisplayAttributeProvider_Impl, ITfDocumentMgr, ITfKeyEventSink, ITfKeyEventSink_Impl,
    ITfKeystrokeMgr, ITfLangBarItemMgr, ITfSource, ITfTextInputProcessor_Impl,
    ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl, ITfThreadMgr, ITfThreadMgrEventSink,
    ITfThreadMgrEventSink_Impl,
};
use windows::core::{BOOL, GUID, IUnknownImpl, Interface, Ref, Result, implement};

const KEY_DOWN_MASK: u16 = 0x8000;
/// Tells `ToUnicodeEx` to leave the keyboard's dead-key buffer alone.
const KEEP_KEYBOARD_STATE: u32 = 0x4;

#[implement(
    ITfTextInputProcessorEx,
    ITfKeyEventSink,
    ITfThreadMgrEventSink,
    ITfCompositionSink,
    ITfDisplayAttributeProvider,
    ITfCompartmentEventSink
)]
pub struct TextService {
    settings: Cell<Settings>,
    engine: RefCell<Engine>,
    vietnamese: Cell<bool>,
    key_busy: Cell<bool>,
    client_id: Cell<u32>,
    attribute_atom: Cell<u32>,
    thread_manager: RefCell<Option<ITfThreadMgr>>,
    composition: SharedComposition,
    /// The context holding the open composition, kept to commit it on focus loss.
    composing_context: RefCell<Option<ITfContext>>,
    toggle_tracker: RefCell<ToggleChordTracker>,
    open_close_compartment: RefCell<Option<ITfCompartment>>,
    thread_sink_cookie: Cell<u32>,
    compartment_sink_cookie: Cell<u32>,
    lang_bar_button: RefCell<Option<windows::core::ComObject<LangBarButton>>>,
}

impl TextService {
    pub fn new() -> Self {
        crate::OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);
        let settings = Settings::default();
        TextService {
            settings: Cell::new(settings),
            engine: RefCell::new(Engine::new(settings.engine)),
            vietnamese: Cell::new(settings.start_vietnamese),
            key_busy: Cell::new(false),
            client_id: Cell::new(0),
            attribute_atom: Cell::new(0),
            thread_manager: RefCell::new(None),
            composition: Rc::new(RefCell::new(None)),
            composing_context: RefCell::new(None),
            toggle_tracker: RefCell::new(ToggleChordTracker::default()),
            open_close_compartment: RefCell::new(None),
            thread_sink_cookie: Cell::new(0),
            compartment_sink_cookie: Cell::new(0),
            lang_bar_button: RefCell::new(None),
        }
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        crate::OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------- activation

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, thread_manager: Ref<ITfThreadMgr>, client_id: u32) -> Result<()> {
        let thread_manager = thread_manager.ok()?.clone();
        self.client_id.set(client_id);
        self.load_configuration();
        let outcome = self.attach(&thread_manager, client_id);
        if outcome.is_err() {
            self.detach();
        }
        outcome
    }

    fn Deactivate(&self) -> Result<()> {
        self.detach();
        Ok(())
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(
        &self,
        thread_manager: Ref<ITfThreadMgr>,
        client_id: u32,
        _flags: u32,
    ) -> Result<()> {
        ITfTextInputProcessor_Impl::Activate(self, thread_manager, client_id)
    }
}

impl TextService_Impl {
    fn load_configuration(&self) {
        let settings = load_settings();
        self.settings.set(settings);
        self.engine.borrow_mut().set_config(settings.engine);
        self.vietnamese.set(settings.start_vietnamese);
    }

    fn attach(&self, thread_manager: &ITfThreadMgr, client_id: u32) -> Result<()> {
        *self.thread_manager.borrow_mut() = Some(thread_manager.clone());
        self.register_display_attribute()?;

        let key_sink: ITfKeyEventSink = self.to_interface();
        let keystroke_manager: ITfKeystrokeMgr = thread_manager.cast()?;
        unsafe { keystroke_manager.AdviseKeyEventSink(client_id, &key_sink, true)? };

        let thread_sink: ITfThreadMgrEventSink = self.to_interface();
        let thread_source: ITfSource = thread_manager.cast()?;
        let cookie =
            unsafe { thread_source.AdviseSink(&ITfThreadMgrEventSink::IID, &thread_sink)? };
        self.thread_sink_cookie.set(cookie);

        self.attach_open_close_compartment(thread_manager, client_id)?;
        self.add_lang_bar_button(thread_manager);
        Ok(())
    }

    fn register_display_attribute(&self) -> Result<()> {
        let categories: ITfCategoryMgr =
            unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)? };
        let atom = unsafe { categories.RegisterGUID(&DISPLAY_ATTRIBUTE_GUID)? };
        self.attribute_atom.set(atom);
        Ok(())
    }

    fn attach_open_close_compartment(
        &self,
        thread_manager: &ITfThreadMgr,
        client_id: u32,
    ) -> Result<()> {
        let compartment_manager: ITfCompartmentMgr = thread_manager.cast()?;
        let compartment =
            unsafe { compartment_manager.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE)? };
        let sink: ITfCompartmentEventSink = self.to_interface();
        let source: ITfSource = compartment.cast()?;
        let cookie = unsafe { source.AdviseSink(&ITfCompartmentEventSink::IID, &sink)? };
        self.compartment_sink_cookie.set(cookie);
        *self.open_close_compartment.borrow_mut() = Some(compartment.clone());
        // Last, because setting the value calls OnChange.
        write_open_close(&compartment, client_id, self.vietnamese.get());
        Ok(())
    }

    /// The language bar is optional: typing works without it.
    fn add_lang_bar_button(&self, thread_manager: &ITfThreadMgr) {
        let Some(compartment) = self.open_close_compartment.borrow().clone() else {
            return;
        };
        let client_id = self.client_id.get();
        let toggle = Box::new(move || {
            let currently_open = read_open_close(&compartment);
            write_open_close(&compartment, client_id, !currently_open);
        });
        let button =
            windows::core::ComObject::new(LangBarButton::new(self.vietnamese.get(), toggle));
        let item = as_lang_bar_item(&button);
        let Some(item_manager) = lang_bar_item_manager(thread_manager) else {
            return;
        };
        if unsafe { item_manager.AddItem(&item) }.is_ok() {
            *self.lang_bar_button.borrow_mut() = Some(button);
        }
    }

    /// Undoes everything `attach` did. Leaks here crash the host on exit.
    fn detach(&self) {
        self.abandon_composition();
        let thread_manager = self.thread_manager.borrow_mut().take();
        if let Some(thread_manager) = thread_manager {
            self.remove_lang_bar_button(&thread_manager);
            self.remove_sinks(&thread_manager);
        }
        *self.open_close_compartment.borrow_mut() = None;
        self.client_id.set(0);
    }

    fn remove_lang_bar_button(&self, thread_manager: &ITfThreadMgr) {
        let button = self.lang_bar_button.borrow_mut().take();
        let Some(button) = button else {
            return;
        };
        if let Some(item_manager) = lang_bar_item_manager(thread_manager) {
            let _ = unsafe { item_manager.RemoveItem(&as_lang_bar_item(&button)) };
        }
    }

    fn remove_sinks(&self, thread_manager: &ITfThreadMgr) {
        if let Ok(keystroke_manager) = thread_manager.cast::<ITfKeystrokeMgr>() {
            let _ = unsafe { keystroke_manager.UnadviseKeyEventSink(self.client_id.get()) };
        }
        let thread_cookie = self.thread_sink_cookie.replace(0);
        if thread_cookie != 0
            && let Ok(source) = thread_manager.cast::<ITfSource>()
        {
            let _ = unsafe { source.UnadviseSink(thread_cookie) };
        }
        let compartment_cookie = self.compartment_sink_cookie.replace(0);
        let compartment = self.open_close_compartment.borrow().clone();
        if compartment_cookie != 0
            && let Some(compartment) = compartment
            && let Ok(source) = compartment.cast::<ITfSource>()
        {
            let _ = unsafe { source.UnadviseSink(compartment_cookie) };
        }
    }
}

fn lang_bar_item_manager(thread_manager: &ITfThreadMgr) -> Option<ITfLangBarItemMgr> {
    thread_manager
        .cast::<ITfLangBarItemMgr>()
        .ok()
        .or_else(|| unsafe {
            CoCreateInstance(&CLSID_TF_LangBarItemMgr, None, CLSCTX_INPROC_SERVER).ok()
        })
}

fn read_open_close(compartment: &ITfCompartment) -> bool {
    let value = unsafe { compartment.GetValue() };
    match value {
        Ok(variant) => i32::try_from(&variant) != Ok(0),
        Err(_) => true,
    }
}

fn write_open_close(compartment: &ITfCompartment, client_id: u32, open: bool) {
    let variant = VARIANT::from(i32::from(open));
    let _ = unsafe { compartment.SetValue(client_id, &variant) };
}

// ------------------------------------------------------------- mode and text

impl TextService_Impl {
    fn is_composing(&self) -> bool {
        self.engine.borrow().is_composing()
    }

    /// Applies the English / Vietnamese switch, wherever it came from.
    fn apply_mode(&self, vietnamese: bool) {
        if self.vietnamese.get() == vietnamese {
            return;
        }
        self.commit_composition(None);
        self.vietnamese.set(vietnamese);
        let button = self.lang_bar_button.borrow().clone();
        if let Some(button) = button {
            button.set_vietnamese(vietnamese);
        }
    }

    fn toggle_mode(&self) {
        let target = !self.vietnamese.get();
        let compartment = self.open_close_compartment.borrow().clone();
        // The change comes back through OnChange; apply it now as well so the
        // next key already sees the new mode.
        if let Some(compartment) = compartment {
            write_open_close(&compartment, self.client_id.get(), target);
        }
        self.apply_mode(target);
    }

    fn show_preedit(&self, context: &ITfContext) {
        let preedit = self.engine.borrow().preedit().to_owned();
        *self.composing_context.borrow_mut() = Some(context.clone());
        self.run_edit_in(context, CompositionEdit::update(&preedit));
    }

    fn insert_committed(&self, context: &ITfContext, text: &str) {
        self.run_edit_in(context, CompositionEdit::commit(text));
        *self.composing_context.borrow_mut() = None;
    }

    fn run_edit_in(&self, context: &ITfContext, edit: CompositionEdit) {
        let sink: ITfCompositionSink = self.to_interface();
        let outcome = run_edit(
            context,
            self.client_id.get(),
            &self.composition,
            &sink,
            self.attribute_atom.get(),
            edit,
        );
        if outcome.is_err() {
            // The host refused the edit; do not keep state it never saw.
            self.abandon_composition();
        }
    }

    /// Writes the composing word into the document and ends the composition.
    fn commit_composition(&self, context: Option<&ITfContext>) {
        let text = self.engine.borrow_mut().commit();
        let context = context
            .cloned()
            .or_else(|| self.composing_context.borrow().clone());
        let has_open_composition = self.composition.borrow().is_some();
        if let (true, Some(context)) = (has_open_composition || !text.is_empty(), context) {
            self.insert_committed(&context, &text);
        }
        self.abandon_composition();
    }

    /// Drops composition state without touching the document.
    fn abandon_composition(&self) {
        self.engine.borrow_mut().reset();
        *self.composition.borrow_mut() = None;
        *self.composing_context.borrow_mut() = None;
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    fn OnCompositionTerminated(
        &self,
        _edit_cookie: u32,
        _composition: Ref<ITfComposition>,
    ) -> Result<()> {
        // The application ended it (click, focus change). Do not re-insert.
        self.abandon_composition();
        Ok(())
    }
}

// --------------------------------------------------------------------- keys

impl TextService_Impl {
    fn read_key_event(&self, wparam: WPARAM, lparam: LPARAM) -> KeyEvent {
        let virtual_key = wparam.0 as u32;
        let ctrl = key_is_down(VK_CONTROL);
        let alt = key_is_down(VK_MENU);
        let alt_gr = ctrl && alt && key_is_down(VK_RMENU);
        let windows_key = key_is_down(0x5B) || key_is_down(0x5C);
        let shortcut_held = (ctrl || alt) && !alt_gr;
        let character = if shortcut_held {
            None
        } else {
            translate_character(virtual_key, lparam)
        };
        KeyEvent {
            virtual_key,
            character,
            ctrl,
            alt,
            alt_gr,
            windows_key,
        }
    }

    fn track_key_down(&self, virtual_key: u32) {
        self.toggle_tracker.borrow_mut().key_down(virtual_key);
    }

    fn track_key_up(&self, virtual_key: u32) {
        let completed = self.toggle_tracker.borrow_mut().key_up(virtual_key);
        if completed && self.settings.get().toggle_hotkey == ToggleHotkey::CtrlShift {
            self.toggle_mode();
        }
    }

    fn handle_key_down(&self, context: &ITfContext, wparam: WPARAM, lparam: LPARAM) -> bool {
        let event = self.read_key_event(wparam, lparam);
        self.track_key_down(event.virtual_key);
        if !self.vietnamese.get() {
            return false;
        }
        match decide_key(&event, self.is_composing()) {
            KeyDecision::PassThrough => false,
            KeyDecision::CommitThenPassThrough => {
                self.commit_composition(Some(context));
                false
            }
            KeyDecision::Process(character) => {
                let result = self.engine.borrow_mut().process_key(character);
                self.apply_engine_result(context, result)
            }
            KeyDecision::Backspace => {
                let result = self.engine.borrow_mut().backspace();
                self.apply_engine_result(context, result)
            }
        }
    }

    /// Returns whether the key was consumed.
    fn apply_engine_result(&self, context: &ITfContext, result: KeyResult) -> bool {
        match result {
            KeyResult::Composing => {
                if self.is_composing() {
                    self.show_preedit(context);
                } else {
                    // The last letter was deleted: close the empty composition.
                    self.insert_committed(context, "");
                    self.abandon_composition();
                }
                true
            }
            KeyResult::Commit(text) => {
                self.insert_committed(context, &text);
                self.abandon_composition();
                true
            }
            KeyResult::PassThrough => false,
        }
    }
}

fn key_is_down(virtual_key: u32) -> bool {
    (unsafe { GetKeyState(virtual_key as i32) } as u16 & KEY_DOWN_MASK) != 0
}

/// The character this key press types on the current layout, if any.
fn translate_character(virtual_key: u32, lparam: LPARAM) -> Option<char> {
    let mut keyboard_state = [0u8; 256];
    unsafe { GetKeyboardState(&mut keyboard_state).ok()? };
    let scan_code = ((lparam.0 >> 16) & 0xFF) as u32;
    let mut buffer = [0u16; 8];
    let layout = unsafe { GetKeyboardLayout(0) };
    let written = unsafe {
        ToUnicodeEx(
            virtual_key,
            scan_code,
            &keyboard_state,
            &mut buffer,
            KEEP_KEYBOARD_STATE,
            Some(layout),
        )
    };
    if written == 1 {
        char::from_u32(u32::from(buffer[0]))
    } else {
        None
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, _foreground: BOOL) -> Result<()> {
        Ok(())
    }

    fn OnTestKeyDown(
        &self,
        _context: Ref<ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Result<BOOL> {
        let event = self.read_key_event(wparam, lparam);
        self.track_key_down(event.virtual_key);
        if !self.vietnamese.get() {
            return Ok(BOOL(0));
        }
        let composing = self.is_composing();
        let decision = decide_key(&event, composing);
        Ok(BOOL::from(will_consume(decision, composing)))
    }

    fn OnTestKeyUp(
        &self,
        _context: Ref<ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        self.track_key_up(wparam.0 as u32);
        Ok(BOOL(0))
    }

    fn OnKeyDown(&self, context: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        if self.key_busy.replace(true) {
            return Ok(BOOL(0));
        }
        let consumed = match context.ok() {
            Ok(context) => self.handle_key_down(context, wparam, lparam),
            Err(_) => false,
        };
        self.key_busy.set(false);
        Ok(BOOL::from(consumed))
    }

    fn OnKeyUp(&self, _context: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        self.track_key_up(wparam.0 as u32);
        Ok(BOOL(0))
    }

    fn OnPreservedKey(&self, _context: Ref<ITfContext>, _guid: *const GUID) -> Result<BOOL> {
        Ok(BOOL(0))
    }
}

// ----------------------------------------------------------- focus and mode

impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(&self, _document: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnUninitDocumentMgr(&self, _document: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnSetFocus(
        &self,
        _focused: Ref<ITfDocumentMgr>,
        _previous: Ref<ITfDocumentMgr>,
    ) -> Result<()> {
        self.commit_composition(None);
        Ok(())
    }

    fn OnPushContext(&self, _context: Ref<ITfContext>) -> Result<()> {
        Ok(())
    }

    fn OnPopContext(&self, _context: Ref<ITfContext>) -> Result<()> {
        Ok(())
    }
}

impl ITfCompartmentEventSink_Impl for TextService_Impl {
    fn OnChange(&self, compartment_guid: *const GUID) -> Result<()> {
        if compartment_guid.is_null() {
            return Err(E_INVALIDARG.into());
        }
        if unsafe { *compartment_guid } != GUID_COMPARTMENT_KEYBOARD_OPENCLOSE {
            return Ok(());
        }
        let compartment = self.open_close_compartment.borrow().clone();
        let compartment = compartment.ok_or(E_UNEXPECTED)?;
        self.apply_mode(read_open_close(&compartment));
        Ok(())
    }
}

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(ComposingAttributeEnumerator::default().into())
    }

    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
        if guid.is_null() || unsafe { *guid } != DISPLAY_ATTRIBUTE_GUID {
            return Err(E_INVALIDARG.into());
        }
        Ok(ComposingAttributeInfo.into())
    }
}
