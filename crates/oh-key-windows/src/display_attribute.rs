//! The underline style of the composing word.

use crate::registry::DISPLAY_ATTRIBUTE_GUID;
use std::cell::Cell;
use windows::Win32::Foundation::{E_INVALIDARG, S_FALSE};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, IEnumTfDisplayAttributeInfo_Impl, ITfDisplayAttributeInfo,
    ITfDisplayAttributeInfo_Impl, TF_ATTR_INPUT, TF_CT_NONE, TF_DISPLAYATTRIBUTE, TF_LS_DOT,
};
use windows::core::{BOOL, BSTR, GUID, Result, implement};

fn composing_word_attribute() -> TF_DISPLAYATTRIBUTE {
    let mut attribute = TF_DISPLAYATTRIBUTE::default();
    attribute.crText.r#type = TF_CT_NONE;
    attribute.crBk.r#type = TF_CT_NONE;
    attribute.crLine.r#type = TF_CT_NONE;
    attribute.lsStyle = TF_LS_DOT;
    attribute.fBoldLine = BOOL(0);
    attribute.bAttr = TF_ATTR_INPUT;
    attribute
}

#[implement(ITfDisplayAttributeInfo)]
pub struct ComposingAttributeInfo;

impl ITfDisplayAttributeInfo_Impl for ComposingAttributeInfo_Impl {
    fn GetGUID(&self) -> Result<GUID> {
        Ok(DISPLAY_ATTRIBUTE_GUID)
    }

    fn GetDescription(&self) -> Result<BSTR> {
        Ok(BSTR::from("oh-key composing word"))
    }

    fn GetAttributeInfo(&self, attribute: *mut TF_DISPLAYATTRIBUTE) -> Result<()> {
        if attribute.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe { *attribute = composing_word_attribute() };
        Ok(())
    }

    fn SetAttributeInfo(&self, _attribute: *const TF_DISPLAYATTRIBUTE) -> Result<()> {
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        Ok(())
    }
}

/// Enumerates the single attribute this input method owns.
#[implement(IEnumTfDisplayAttributeInfo)]
#[derive(Default)]
pub struct ComposingAttributeEnumerator {
    delivered: Cell<bool>,
}

impl IEnumTfDisplayAttributeInfo_Impl for ComposingAttributeEnumerator_Impl {
    fn Clone(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        let copy = ComposingAttributeEnumerator {
            delivered: Cell::new(self.delivered.get()),
        };
        Ok(copy.into())
    }

    fn Next(
        &self,
        count: u32,
        infos: *mut Option<ITfDisplayAttributeInfo>,
        fetched: *mut u32,
    ) -> Result<()> {
        let mut delivered_now = 0;
        if count > 0 && !self.delivered.get() && !infos.is_null() {
            unsafe { infos.write(Some(ComposingAttributeInfo.into())) };
            self.delivered.set(true);
            delivered_now = 1;
        }
        if !fetched.is_null() {
            unsafe { *fetched = delivered_now };
        }
        if delivered_now == count {
            Ok(())
        } else {
            Err(S_FALSE.into())
        }
    }

    fn Reset(&self) -> Result<()> {
        self.delivered.set(false);
        Ok(())
    }

    fn Skip(&self, count: u32) -> Result<()> {
        if count > 0 {
            self.delivered.set(true);
        }
        Ok(())
    }
}
