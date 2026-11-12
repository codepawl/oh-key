//! Edit sessions: the only way TSF lets a text service change the document.
//!
//! The composing word lives inside an `ITfComposition`, so the application
//! sees one underlined range that changes in place. No backspaces or
//! synthesized keystrokes are ever sent.

use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::{
    GUID_PROP_ATTRIBUTE, ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition,
    ITfEditSession, ITfEditSession_Impl, ITfInsertAtSelection, ITfRange, TF_AE_NONE, TF_ANCHOR_END,
    TF_E_SYNCHRONOUS, TF_ES_ASYNC, TF_ES_READWRITE, TF_ES_SYNC, TF_IAS_QUERYONLY, TF_SELECTION,
    TF_SELECTIONSTYLE,
};
use windows::core::{BOOL, Result, implement};

pub type SharedComposition = Rc<RefCell<Option<ITfComposition>>>;

/// What an edit session does to the composition.
pub enum CompositionEdit {
    /// Show this text as the underlined composing word.
    Update(Vec<u16>),
    /// Write this text, drop the underline and end the composition.
    Commit(Vec<u16>),
}

impl CompositionEdit {
    pub fn update(text: &str) -> Self {
        CompositionEdit::Update(text.encode_utf16().collect())
    }

    pub fn commit(text: &str) -> Self {
        CompositionEdit::Commit(text.encode_utf16().collect())
    }
}

#[implement(ITfEditSession)]
struct EditSession {
    context: ITfContext,
    composition: SharedComposition,
    sink: ITfCompositionSink,
    edit: CompositionEdit,
    attribute_atom: u32,
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, edit_cookie: u32) -> Result<()> {
        let outcome = self.apply_edit(edit_cookie);
        crate::diagnostics::trace!("do edit session: {outcome:?}");
        outcome
    }
}

impl EditSession_Impl {
    fn apply_edit(&self, edit_cookie: u32) -> Result<()> {
        let range = self.composition_range(edit_cookie)?;
        match &self.edit {
            CompositionEdit::Update(text) => {
                unsafe { range.SetText(edit_cookie, 0, text)? };
                self.mark_as_composing(edit_cookie, &range)?;
                self.place_caret_at_end(edit_cookie, &range)
            }
            CompositionEdit::Commit(text) => {
                unsafe { range.SetText(edit_cookie, 0, text)? };
                self.place_caret_at_end(edit_cookie, &range)?;
                self.clear_composing_mark(edit_cookie, &range);
                self.end_composition(edit_cookie)
            }
        }
    }
}

impl EditSession_Impl {
    /// The range of the open composition, starting one at the caret if needed.
    fn composition_range(&self, edit_cookie: u32) -> Result<ITfRange> {
        if let Some(composition) = self.composition.borrow().as_ref() {
            return unsafe { composition.GetRange() };
        }
        let insert: ITfInsertAtSelection = windows::core::Interface::cast(&self.context)?;
        let caret = unsafe { insert.InsertTextAtSelection(edit_cookie, TF_IAS_QUERYONLY, &[])? };
        let owner: ITfContextComposition = windows::core::Interface::cast(&self.context)?;
        let composition = unsafe { owner.StartComposition(edit_cookie, &caret, &self.sink)? };
        *self.composition.borrow_mut() = Some(composition);
        Ok(caret)
    }

    fn mark_as_composing(&self, edit_cookie: u32, range: &ITfRange) -> Result<()> {
        let property = unsafe { self.context.GetProperty(&GUID_PROP_ATTRIBUTE)? };
        let atom = VARIANT::from(self.attribute_atom as i32);
        unsafe { property.SetValue(edit_cookie, range, &atom) }
    }

    /// Best effort: a failure here only leaves an underline on committed text.
    fn clear_composing_mark(&self, edit_cookie: u32, range: &ITfRange) {
        if let Ok(property) = unsafe { self.context.GetProperty(&GUID_PROP_ATTRIBUTE) } {
            let _ = unsafe { property.Clear(edit_cookie, range) };
        }
    }

    fn place_caret_at_end(&self, edit_cookie: u32, range: &ITfRange) -> Result<()> {
        unsafe { range.Collapse(edit_cookie, TF_ANCHOR_END)? };
        let selection = [TF_SELECTION {
            range: ManuallyDrop::new(Some(range.clone())),
            style: TF_SELECTIONSTYLE {
                ase: TF_AE_NONE,
                fInterimChar: BOOL(0),
            },
        }];
        let result = unsafe { self.context.SetSelection(edit_cookie, &selection) };
        // The array holds a clone we must release ourselves.
        for entry in selection {
            drop(ManuallyDrop::into_inner(entry.range));
        }
        result
    }

    fn end_composition(&self, edit_cookie: u32) -> Result<()> {
        let composition = self.composition.borrow_mut().take();
        match composition {
            Some(composition) => unsafe { composition.EndComposition(edit_cookie) },
            None => Ok(()),
        }
    }
}

/// Runs one edit session, synchronously when the host allows it.
pub fn run_edit(
    context: &ITfContext,
    client_id: u32,
    composition: &SharedComposition,
    sink: &ITfCompositionSink,
    attribute_atom: u32,
    edit: CompositionEdit,
) -> Result<()> {
    let session: ITfEditSession = EditSession {
        context: context.clone(),
        composition: composition.clone(),
        sink: sink.clone(),
        edit,
        attribute_atom,
    }
    .into();
    let synchronous =
        unsafe { context.RequestEditSession(client_id, &session, TF_ES_SYNC | TF_ES_READWRITE)? };
    if synchronous == TF_E_SYNCHRONOUS {
        unsafe { context.RequestEditSession(client_id, &session, TF_ES_ASYNC | TF_ES_READWRITE)? }
            .ok()?;
        return Ok(());
    }
    synchronous.ok()?;
    // The session's own result is delivered through the outer HRESULT above.
    Ok(())
}
