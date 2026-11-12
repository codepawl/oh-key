//! The VI / EN button in the language bar and the system tray.

use crate::registry::{LANG_BAR_ITEM_GUID, OHKEY_CLSID};
use std::cell::{Cell, RefCell};
use windows::Win32::Foundation::{E_INVALIDARG, E_UNEXPECTED, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FillRect, GetDC, HGDIOBJ,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::TextServices::{
    ITfLangBarItem, ITfLangBarItem_Impl, ITfLangBarItemButton, ITfLangBarItemButton_Impl,
    ITfLangBarItemSink, ITfMenu, ITfSource, ITfSource_Impl, TF_LANGBARITEMINFO, TF_LBI_CLK_LEFT,
    TF_LBI_ICON, TF_LBI_STYLE_BTN_BUTTON, TF_LBI_STYLE_SHOWNINTRAY, TF_LBI_TEXT, TF_LBI_TOOLTIP,
    TfLBIClick,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO};
use windows::core::{BOOL, BSTR, GUID, IUnknown, Interface, Ref, Result, implement};

const ICON_SIZE: i32 = 16;
const VIETNAMESE_COLOR: u32 = 0x0000_00C8; // COLORREF 0x00BBGGRR: red
const ENGLISH_COLOR: u32 = 0x0080_6A3C; // slate blue-grey
const WHITE: u32 = 0x00FF_FFFF;

#[implement(ITfLangBarItemButton, ITfLangBarItem, ITfSource)]
pub struct LangBarButton {
    vietnamese: Cell<bool>,
    on_click: Box<dyn Fn()>,
    sink: RefCell<Option<ITfLangBarItemSink>>,
    cookie: Cell<u32>,
    vietnamese_icon: Cell<HICON>,
    english_icon: Cell<HICON>,
}

impl LangBarButton {
    pub fn new(vietnamese: bool, on_click: Box<dyn Fn()>) -> Self {
        LangBarButton {
            vietnamese: Cell::new(vietnamese),
            on_click,
            sink: RefCell::new(None),
            cookie: Cell::new(0),
            vietnamese_icon: Cell::new(HICON::default()),
            english_icon: Cell::new(HICON::default()),
        }
    }

    pub fn set_vietnamese(&self, vietnamese: bool) {
        if self.vietnamese.replace(vietnamese) == vietnamese {
            return;
        }
        // Clone first: the sink may call back into this button.
        let sink = self.sink.borrow().clone();
        if let Some(sink) = sink {
            let _ = unsafe { sink.OnUpdate(TF_LBI_ICON | TF_LBI_TEXT | TF_LBI_TOOLTIP) };
        }
    }

    fn icon_for_current_mode(&self) -> HICON {
        let (slot, letters, color) = if self.vietnamese.get() {
            (&self.vietnamese_icon, "V", VIETNAMESE_COLOR)
        } else {
            (&self.english_icon, "E", ENGLISH_COLOR)
        };
        if slot.get().is_invalid() {
            slot.set(draw_letter_icon(letters, color));
        }
        slot.get()
    }
}

impl Drop for LangBarButton {
    fn drop(&mut self) {
        for icon in [self.vietnamese_icon.get(), self.english_icon.get()] {
            if !icon.is_invalid() {
                let _ = unsafe { DestroyIcon(icon) };
            }
        }
    }
}

/// A 16x16 colored square with one white letter.
fn draw_letter_icon(letters: &str, background: u32) -> HICON {
    unsafe {
        let screen = GetDC(None);
        let device = CreateCompatibleDC(Some(screen));
        let color_bitmap = CreateCompatibleBitmap(screen, ICON_SIZE, ICON_SIZE);
        let opaque_mask = vec![0u8; (ICON_SIZE * ICON_SIZE / 8) as usize];
        let mask_bitmap = CreateBitmap(
            ICON_SIZE,
            ICON_SIZE,
            1,
            1,
            Some(opaque_mask.as_ptr().cast()),
        );
        let previous = SelectObject(device, HGDIOBJ(color_bitmap.0));

        let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(background));
        let mut area = RECT {
            left: 0,
            top: 0,
            right: ICON_SIZE,
            bottom: ICON_SIZE,
        };
        FillRect(device, &area, brush);
        SetBkMode(device, TRANSPARENT);
        SetTextColor(device, windows::Win32::Foundation::COLORREF(WHITE));
        let mut text: Vec<u16> = letters.encode_utf16().collect();
        DrawTextW(
            device,
            &mut text,
            &mut area,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        SelectObject(device, previous);
        let info = ICONINFO {
            fIcon: BOOL(1),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bitmap,
            hbmColor: color_bitmap,
        };
        let icon = CreateIconIndirect(&info).unwrap_or_default();

        let _ = DeleteObject(HGDIOBJ(brush.0));
        let _ = DeleteObject(HGDIOBJ(color_bitmap.0));
        let _ = DeleteObject(HGDIOBJ(mask_bitmap.0));
        let _ = DeleteDC(device);
        ReleaseDC(None, screen);
        icon
    }
}

impl ITfLangBarItem_Impl for LangBarButton_Impl {
    fn GetInfo(&self, info: *mut TF_LANGBARITEMINFO) -> Result<()> {
        if info.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let mut description = [0u16; 32];
        for (slot, unit) in description.iter_mut().zip("oh-key".encode_utf16()) {
            *slot = unit;
        }
        unsafe {
            *info = TF_LANGBARITEMINFO {
                clsidService: OHKEY_CLSID,
                guidItem: LANG_BAR_ITEM_GUID,
                dwStyle: TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_SHOWNINTRAY,
                ulSort: 0,
                szDescription: description,
            };
        }
        Ok(())
    }

    fn GetStatus(&self) -> Result<u32> {
        Ok(0)
    }

    fn Show(&self, _show: BOOL) -> Result<()> {
        Ok(())
    }

    fn GetTooltipString(&self) -> Result<BSTR> {
        let text = if self.vietnamese.get() {
            "oh-key: tiếng Việt (bấm để chuyển sang English)"
        } else {
            "oh-key: English (bấm để chuyển sang tiếng Việt)"
        };
        Ok(BSTR::from(text))
    }
}

impl ITfLangBarItemButton_Impl for LangBarButton_Impl {
    fn OnClick(&self, click: TfLBIClick, _point: &POINT, _area: *const RECT) -> Result<()> {
        if click == TF_LBI_CLK_LEFT {
            (self.on_click)();
        }
        Ok(())
    }

    fn InitMenu(&self, _menu: Ref<ITfMenu>) -> Result<()> {
        Ok(())
    }

    fn OnMenuSelect(&self, _item: u32) -> Result<()> {
        Ok(())
    }

    fn GetIcon(&self) -> Result<HICON> {
        // The caller owns the returned icon, so hand out a copy.
        let icon = self.icon_for_current_mode();
        let copy = unsafe { windows::Win32::UI::WindowsAndMessaging::CopyIcon(icon)? };
        Ok(copy)
    }

    fn GetText(&self) -> Result<BSTR> {
        Ok(BSTR::from(if self.vietnamese.get() { "VI" } else { "EN" }))
    }
}

impl ITfSource_Impl for LangBarButton_Impl {
    fn AdviseSink(&self, interface_id: *const GUID, sink: Ref<IUnknown>) -> Result<u32> {
        if interface_id.is_null() || unsafe { *interface_id } != ITfLangBarItemSink::IID {
            return Err(E_INVALIDARG.into());
        }
        let sink: ITfLangBarItemSink = sink.ok()?.cast()?;
        *self.sink.borrow_mut() = Some(sink);
        self.cookie.set(1);
        Ok(1)
    }

    fn UnadviseSink(&self, cookie: u32) -> Result<()> {
        if cookie != self.cookie.get() || cookie == 0 {
            return Err(E_UNEXPECTED.into());
        }
        *self.sink.borrow_mut() = None;
        self.cookie.set(0);
        Ok(())
    }
}

pub fn as_lang_bar_item(button: &windows::core::ComObject<LangBarButton>) -> ITfLangBarItem {
    button.to_interface()
}
