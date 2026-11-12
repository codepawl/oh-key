//! The charset converter (chuyển mã): pick a source and a target charset, then convert the
//! text on the clipboard in place, as Unikey's converter does. Legacy byte charsets (TCVN3,
//! VNI-Windows and the rest) travel on the clipboard as raw CF_TEXT bytes; the text-like ones
//! as Unicode text.

use std::path::Path;
use std::sync::Once;

use oh_key_charset::{Charset, convert, decode};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, IDC_ARROW, IDC_HAND,
    LoadCursorW, SetCursor, SetWindowLongPtrW, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT,
    WM_SETCURSOR,
};
use windows::core::{Result, w};

use crate::graphics::Graphics;
use crate::icons;
use crate::layout::{self, Rect};
use crate::tool_window::{self, OpenWindow, Outcome};

const WIDTH: f32 = 760.0;
const RIGHT: f32 = WIDTH - layout::PADDING;
/// Charset buttons start where the group title's text does.
const CHOICES_LEFT: f32 = layout::PADDING + layout::ICON_SLOT - layout::BUTTON_PADDING / 2.0;

const CF_TEXT: u32 = 1;
const CF_UNICODE_TEXT: u32 = 13;

const LABEL_CONVERT: &str = "chuyển clipboard";
const LABEL_SWAP: &str = "đổi chiều";
const ACTION_LABELS: [&str; 2] = [LABEL_SWAP, LABEL_CONVERT];

static OPEN_WINDOW: OpenWindow = OpenWindow::new();
static CLASS_REGISTERED: Once = Once::new();

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Source(Charset),
    Target(Charset),
    Swap,
    Convert,
}

/// How a charset's text sits on the clipboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClipboardForm {
    /// CF_UNICODETEXT: the charset's text is readable Unicode text (UTF-8 bytes, NCR, VIQR and so on).
    UnicodeText,
    /// CF_TEXT read and written as the raw bytes of a legacy font charset.
    RawBytes,
}

fn clipboard_form(charset: Charset) -> ClipboardForm {
    match charset {
        Charset::Unicode
        | Charset::UnicodeDecomposed
        | Charset::Utf8
        | Charset::XUtf8
        | Charset::NcrDecimal
        | Charset::NcrHexadecimal
        | Charset::CString
        | Charset::Viqr
        | Charset::Utf8Viqr => ClipboardForm::UnicodeText,
        _ => ClipboardForm::RawBytes,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Payload {
    Text(String),
    Bytes(Vec<u8>),
}

fn is_utf16(charset: Charset) -> bool {
    matches!(charset, Charset::Unicode | Charset::UnicodeDecomposed)
}

/// The payload as bytes in `charset`'s encoding, what `oh_key_charset::convert` takes.
fn payload_bytes(payload: &Payload, charset: Charset) -> Vec<u8> {
    match payload {
        Payload::Bytes(bytes) => bytes.clone(),
        Payload::Text(text) if is_utf16(charset) => text
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect(),
        Payload::Text(text) => text.as_bytes().to_vec(),
    }
}

/// Bytes in `charset`'s encoding as what goes back on the clipboard.
fn payload_from_bytes(bytes: Vec<u8>, charset: Charset) -> Payload {
    match clipboard_form(charset) {
        ClipboardForm::RawBytes => Payload::Bytes(bytes),
        ClipboardForm::UnicodeText if is_utf16(charset) => {
            let units: Vec<u16> = utf16_units(&bytes).collect();
            Payload::Text(String::from_utf16_lossy(&units))
        }
        ClipboardForm::UnicodeText => Payload::Text(String::from_utf8_lossy(&bytes).into_owned()),
    }
}

/// The converted clipboard content and how many characters the source held.
fn convert_payload(payload: &Payload, from: Charset, to: Charset) -> (Payload, usize) {
    let source_bytes = payload_bytes(payload, from);
    let character_count = decode(&source_bytes, from).chars().count();
    let converted = convert(&source_bytes, from, to);
    (payload_from_bytes(converted, to), character_count)
}

fn is_empty(payload: &Payload) -> bool {
    match payload {
        Payload::Text(text) => text.is_empty(),
        Payload::Bytes(bytes) => bytes.is_empty(),
    }
}

fn success_message(character_count: usize, from: Charset, to: Charset) -> String {
    format!(
        "đã chuyển {character_count} ký tự, {} → {}",
        from.name(),
        to.name()
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipboardFailure {
    Unavailable,
    Empty,
    NotWritten,
}

impl ClipboardFailure {
    fn message(self) -> &'static str {
        match self {
            ClipboardFailure::Unavailable => "không mở được clipboard",
            ClipboardFailure::Empty => "clipboard không có văn bản để chuyển",
            ClipboardFailure::NotWritten => "không ghi lại được vào clipboard",
        }
    }
}

/// Reads the clipboard, converts it and writes the result back.
fn convert_clipboard(
    owner: HWND,
    from: Charset,
    to: Charset,
) -> std::result::Result<usize, ClipboardFailure> {
    let _clipboard = OpenedClipboard::open(owner)?;
    let payload = read_clipboard(clipboard_form(from))?;
    if is_empty(&payload) {
        return Err(ClipboardFailure::Empty);
    }
    let (converted, character_count) = convert_payload(&payload, from, to);
    write_clipboard(&converted)?;
    Ok(character_count)
}

struct OpenedClipboard;

impl OpenedClipboard {
    fn open(owner: HWND) -> std::result::Result<Self, ClipboardFailure> {
        unsafe { OpenClipboard(Some(owner)) }
            .map(|()| OpenedClipboard)
            .map_err(|_| ClipboardFailure::Unavailable)
    }
}

impl Drop for OpenedClipboard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn read_clipboard(form: ClipboardForm) -> std::result::Result<Payload, ClipboardFailure> {
    let format = match form {
        ClipboardForm::UnicodeText => CF_UNICODE_TEXT,
        ClipboardForm::RawBytes => CF_TEXT,
    };
    let handle = unsafe { GetClipboardData(format) }.map_err(|_| ClipboardFailure::Empty)?;
    let memory = HGLOBAL(handle.0);
    let pointer = unsafe { GlobalLock(memory) } as *const u8;
    if pointer.is_null() {
        return Err(ClipboardFailure::Empty);
    }
    let size = unsafe { GlobalSize(memory) };
    let bytes = unsafe { std::slice::from_raw_parts(pointer, size) }.to_vec();
    unsafe {
        let _ = GlobalUnlock(memory);
    }
    Ok(match form {
        ClipboardForm::UnicodeText => Payload::Text(text_until_nul(&bytes)),
        ClipboardForm::RawBytes => Payload::Bytes(bytes_until_nul(&bytes)),
    })
}

fn bytes_until_nul(bytes: &[u8]) -> Vec<u8> {
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    bytes[..length].to_vec()
}

/// UTF-16 code units from little-endian bytes; a trailing odd byte is dropped.
fn utf16_units(bytes: &[u8]) -> impl Iterator<Item = u16> + '_ {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
}

fn text_until_nul(bytes: &[u8]) -> String {
    let units: Vec<u16> = utf16_units(bytes).take_while(|unit| *unit != 0).collect();
    String::from_utf16_lossy(&units)
}

/// The bytes the clipboard stores for a payload, with their format and the terminating nul.
fn clipboard_bytes(payload: &Payload) -> (u32, Vec<u8>) {
    match payload {
        Payload::Text(text) => {
            let mut bytes: Vec<u8> = text
                .encode_utf16()
                .flat_map(|unit| unit.to_le_bytes())
                .collect();
            bytes.extend([0, 0]);
            (CF_UNICODE_TEXT, bytes)
        }
        Payload::Bytes(raw) => {
            let mut bytes = raw.clone();
            bytes.push(0);
            (CF_TEXT, bytes)
        }
    }
}

fn write_clipboard(payload: &Payload) -> std::result::Result<(), ClipboardFailure> {
    let (format, bytes) = clipboard_bytes(payload);
    unsafe {
        let memory =
            GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|_| ClipboardFailure::NotWritten)?;
        let pointer = GlobalLock(memory) as *mut u8;
        if pointer.is_null() {
            return Err(ClipboardFailure::NotWritten);
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
        let _ = GlobalUnlock(memory);
        EmptyClipboard().map_err(|_| ClipboardFailure::NotWritten)?;
        // The clipboard owns the memory once this succeeds.
        SetClipboardData(format, Some(HANDLE(memory.0)))
            .map_err(|_| ClipboardFailure::NotWritten)?;
    }
    Ok(())
}

/// Where everything sits. The two charset lists wrap, so the height depends on the text widths.
struct Geometry {
    source_top: f32,
    source_choices: Vec<Rect>,
    target_top: f32,
    target_choices: Vec<Rect>,
    actions_top: f32,
    status_top: f32,
    height: f32,
}

/// Buttons laid left to right in rows of `ROW_HEIGHT`, wrapping at `right`. Returns the
/// rectangles in order and the number of rows used.
fn flow_rects(text_widths: &[f32], left: f32, right: f32, top: f32) -> (Vec<Rect>, usize) {
    let mut rects = Vec::with_capacity(text_widths.len());
    let mut x = left;
    let mut row = 0;
    for text_width in text_widths {
        let width = text_width + layout::BUTTON_PADDING;
        if x > left && x + width > right {
            row += 1;
            x = left;
        }
        let row_top = top + row as f32 * layout::ROW_HEIGHT;
        let button_top = row_top + (layout::ROW_HEIGHT - layout::BUTTON_HEIGHT) / 2.0;
        rects.push(Rect {
            left: x,
            top: button_top,
            right: x + width,
            bottom: button_top + layout::BUTTON_HEIGHT,
        });
        x += width + layout::BUTTON_GAP;
    }
    let rows = if text_widths.is_empty() { 0 } else { row + 1 };
    (rects, rows)
}

impl Geometry {
    fn new(graphics: &Graphics) -> Self {
        let widths: Vec<f32> = Charset::ALL
            .iter()
            .map(|charset| graphics.measure(charset.name()).0)
            .collect();
        let source_top = layout::TITLE_HEIGHT;
        let source_rows_top = source_top + layout::SECTION_HEIGHT;
        let (source_choices, rows) = flow_rects(&widths, CHOICES_LEFT, RIGHT, source_rows_top);
        let rows_height = rows as f32 * layout::ROW_HEIGHT;
        let target_top = source_rows_top + rows_height + layout::SECTION_GAP;
        let target_rows_top = target_top + layout::SECTION_HEIGHT;
        let (target_choices, _) = flow_rects(&widths, CHOICES_LEFT, RIGHT, target_rows_top);
        let actions_top = target_rows_top + rows_height + layout::SECTION_GAP;
        let status_top = actions_top + layout::ROW_HEIGHT;
        Self {
            source_top,
            source_choices,
            target_top,
            target_choices,
            actions_top,
            status_top,
            height: status_top + layout::ROW_HEIGHT + layout::PADDING,
        }
    }
}

struct ConverterWindow {
    graphics: Graphics,
    hwnd: HWND,
    target: ID2D1RenderTarget,
    geometry: Geometry,
    source: Charset,
    destination: Charset,
    outcome: Option<Outcome>,
    pointer: (f32, f32),
    hovered: Option<Hit>,
    hits: Vec<(Hit, Rect)>,
}

/// Opens the converter, or brings the open one forward.
pub fn open(owner: Option<HWND>) {
    if OPEN_WINDOW.bring_forward() {
        return;
    }
    let _ = try_open(owner);
}

fn try_open(owner: Option<HWND>) -> Result<()> {
    let graphics = Graphics::new()?;
    tool_window::register_class(
        &graphics,
        w!("oh-key.converter"),
        Some(window_proc),
        &CLASS_REGISTERED,
    );
    let geometry = Geometry::new(&graphics);
    let frame = tool_window::create_frame(
        w!("oh-key.converter"),
        w!("oh-key chuyển mã"),
        (WIDTH, geometry.height),
        owner,
    )?;
    let target = match tool_window::create_target(&graphics, frame.hwnd, frame.scale) {
        Ok(target) => target,
        Err(error) => {
            unsafe {
                let _ = DestroyWindow(frame.hwnd);
            }
            return Err(error);
        }
    };
    let window = Box::new(ConverterWindow {
        graphics,
        hwnd: frame.hwnd,
        target,
        geometry,
        source: Charset::Unicode,
        destination: Charset::Tcvn3,
        outcome: None,
        pointer: (-1.0, -1.0),
        hovered: None,
        hits: Vec::new(),
    });
    unsafe { SetWindowLongPtrW(frame.hwnd, GWLP_USERDATA, Box::into_raw(window) as _) };
    OPEN_WINDOW.set(frame.hwnd);
    tool_window::show(frame.hwnd);
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state = tool_window::state_of::<ConverterWindow>(hwnd);
    if let Some(result) =
        tool_window::default_while_stateless(hwnd, message, wparam, lparam, state.is_some())
    {
        if message == WM_NCDESTROY {
            release_window(hwnd);
        }
        return result;
    }
    let Some(window) = state else {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    match message {
        WM_PAINT => {
            window.paint();
            unsafe {
                let _ = ValidateRect(Some(hwnd), None);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let shape = if window.hit_at(window.pointer).is_some() {
                IDC_HAND
            } else {
                IDC_ARROW
            };
            if let Ok(cursor) = unsafe { LoadCursorW(None, shape) } {
                unsafe { SetCursor(Some(cursor)) };
                return LRESULT(1);
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_MOUSEMOVE => {
            window.pointer = tool_window::pointer_in_dips(&window.target, lparam);
            let hovered = window.hit_at(window.pointer);
            if hovered != window.hovered {
                window.hovered = hovered;
                window.paint();
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            window.pointer = tool_window::pointer_in_dips(&window.target, lparam);
            if let Some(hit) = window.hit_at(window.pointer) {
                window.choose(hit);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn release_window(hwnd: HWND) {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut ConverterWindow;
    if pointer.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(pointer) });
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
    OPEN_WINDOW.clear();
}

impl ConverterWindow {
    fn hit_at(&self, point: (f32, f32)) -> Option<Hit> {
        layout::hit_test(&self.hits, point)
    }

    fn paint(&mut self) {
        let view = View {
            source: self.source,
            destination: self.destination,
            outcome: self.outcome.as_ref(),
        };
        let hits = draw(
            &self.target,
            &self.graphics,
            &self.geometry,
            &view,
            self.hovered,
        );
        self.hits = hits;
    }

    fn choose(&mut self, hit: Hit) {
        match hit {
            Hit::Source(charset) => self.source = charset,
            Hit::Target(charset) => self.destination = charset,
            Hit::Swap => std::mem::swap(&mut self.source, &mut self.destination),
            Hit::Convert => self.convert_clipboard(),
        }
        self.hovered = self.hit_at(self.pointer);
        self.paint();
    }

    fn convert_clipboard(&mut self) {
        let outcome = match convert_clipboard(self.hwnd, self.source, self.destination) {
            Ok(character_count) => Outcome::Success(success_message(
                character_count,
                self.source,
                self.destination,
            )),
            Err(failure) => Outcome::Failure(failure.message().to_owned()),
        };
        self.outcome = Some(outcome);
    }
}

struct View<'a> {
    source: Charset,
    destination: Charset,
    outcome: Option<&'a Outcome>,
}

fn draw(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    geometry: &Geometry,
    view: &View,
    hovered: Option<Hit>,
) -> Vec<(Hit, Rect)> {
    let Some(brushes) = tool_window::create_brushes(target) else {
        return Vec::new();
    };
    tool_window::begin_window(target, graphics, &brushes, "chuyển mã");
    let mut hits = Vec::new();
    let groups = [
        (
            geometry.source_top,
            icons::TEXT,
            "từ bảng mã",
            &geometry.source_choices,
            view.source,
        ),
        (
            geometry.target_top,
            icons::SYNC,
            "sang bảng mã",
            &geometry.target_choices,
            view.destination,
        ),
    ];
    for (index, (top, icon, title, choices, chosen)) in groups.into_iter().enumerate() {
        tool_window::draw_section_header(target, graphics, &brushes, icon, title, top, RIGHT);
        for (charset, rect) in Charset::ALL.iter().zip(choices) {
            let hit = if index == 0 {
                Hit::Source(*charset)
            } else {
                Hit::Target(*charset)
            };
            tool_window::draw_text_button(
                target,
                graphics,
                &brushes,
                charset.name(),
                *rect,
                hovered == Some(hit),
                *charset == chosen,
            );
            hits.push((hit, *rect));
        }
    }
    let rects = tool_window::button_row(graphics, &ACTION_LABELS, geometry.actions_top, RIGHT);
    for ((label, hit), rect) in ACTION_LABELS
        .iter()
        .zip([Hit::Swap, Hit::Convert])
        .zip(rects)
    {
        tool_window::draw_text_button(
            target,
            graphics,
            &brushes,
            label,
            rect,
            hovered == Some(hit),
            false,
        );
        hits.push((hit, rect));
    }
    if let Some(outcome) = view.outcome {
        tool_window::draw_outcome(
            target,
            graphics,
            &brushes,
            outcome,
            geometry.status_top,
            RIGHT,
        );
    }
    tool_window::end_window(target);
    hits
}

/// `oh-key.exe --render-png converter out.png`: the window with a finished conversion shown.
pub fn render_png(graphics: &Graphics, path: &Path) -> Result<()> {
    let geometry = Geometry::new(graphics);
    let outcome = Outcome::Success(success_message(26, Charset::Unicode, Charset::Tcvn3));
    let view = View {
        source: Charset::Unicode,
        destination: Charset::Tcvn3,
        outcome: Some(&outcome),
    };
    let offscreen =
        crate::offscreen::Offscreen::new(graphics, WIDTH as u32, geometry.height.round() as u32)?;
    draw(
        &offscreen.target,
        graphics,
        &geometry,
        &view,
        Some(Hit::Target(Charset::Viscii)),
    );
    offscreen.save_png(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Payload {
        Payload::Text(value.to_owned())
    }

    #[test]
    fn unicode_text_becomes_raw_tcvn3_bytes() {
        let (converted, count) =
            convert_payload(&text("Việt Nam"), Charset::Unicode, Charset::Tcvn3);
        let expected = oh_key_charset::encode("Việt Nam", Charset::Tcvn3);
        assert_eq!(converted, Payload::Bytes(expected));
        assert_eq!(count, 8);
    }

    #[test]
    fn raw_tcvn3_bytes_come_back_as_unicode_text() {
        let bytes = oh_key_charset::encode("Hà Nội", Charset::Tcvn3);
        let (converted, _) =
            convert_payload(&Payload::Bytes(bytes), Charset::Tcvn3, Charset::Unicode);
        assert_eq!(converted, text("Hà Nội"));
    }

    #[test]
    fn text_like_targets_stay_text() {
        let (decimal, _) = convert_payload(&text("ệ"), Charset::Unicode, Charset::NcrDecimal);
        assert_eq!(decimal, text("&#7879;"));
        let (viqr, _) = convert_payload(&text("Việt"), Charset::Utf8, Charset::Viqr);
        assert_eq!(viqr, text("Vie^.t"));
    }

    #[test]
    fn the_decomposed_form_survives_a_round_trip_through_text() {
        let (decomposed, _) =
            convert_payload(&text("ệ"), Charset::Unicode, Charset::UnicodeDecomposed);
        assert_eq!(decomposed, text("ê\u{0323}"));
        let (composed, _) =
            convert_payload(&decomposed, Charset::UnicodeDecomposed, Charset::Unicode);
        assert_eq!(composed, text("ệ"));
    }

    #[test]
    fn only_legacy_font_charsets_use_raw_clipboard_bytes() {
        assert_eq!(clipboard_form(Charset::Tcvn3), ClipboardForm::RawBytes);
        assert_eq!(clipboard_form(Charset::VniWindows), ClipboardForm::RawBytes);
        assert_eq!(
            clipboard_form(Charset::WindowsCp1258),
            ClipboardForm::RawBytes
        );
        assert_eq!(clipboard_form(Charset::Unicode), ClipboardForm::UnicodeText);
        assert_eq!(
            clipboard_form(Charset::Utf8Viqr),
            ClipboardForm::UnicodeText
        );
    }

    #[test]
    fn clipboard_buffers_end_in_a_nul_of_the_right_width() {
        assert_eq!(
            clipboard_bytes(&text("a")),
            (CF_UNICODE_TEXT, vec![b'a', 0, 0, 0])
        );
        assert_eq!(
            clipboard_bytes(&Payload::Bytes(vec![0xB5])),
            (CF_TEXT, vec![0xB5, 0])
        );
    }

    #[test]
    fn reading_stops_at_the_nul() {
        assert_eq!(text_until_nul(&[b'h', 0, b'i', 0, 0, 0, b'x', 0]), "hi");
        assert_eq!(bytes_until_nul(&[0xB5, 0x61, 0, 0x62]), vec![0xB5, 0x61]);
    }

    #[test]
    fn the_message_names_both_charsets_and_the_count() {
        let message = success_message(12, Charset::Unicode, Charset::Tcvn3);
        assert_eq!(message, "đã chuyển 12 ký tự, UNICODE → TCVN3");
    }

    #[test]
    fn buttons_wrap_into_a_new_row_at_the_right_edge() {
        let widths = [50.0, 50.0, 50.0];
        let (rects, rows) = flow_rects(&widths, 0.0, 150.0, 100.0);
        assert_eq!(rows, 2);
        assert_eq!(rects[0].left, 0.0);
        assert!(rects[1].left > rects[0].right);
        assert_eq!(rects[2].left, 0.0);
        assert_eq!(rects[2].top - rects[0].top, layout::ROW_HEIGHT);
    }

    #[test]
    fn a_button_wider_than_the_row_still_gets_its_own_row() {
        let (rects, rows) = flow_rects(&[500.0, 20.0], 0.0, 100.0, 0.0);
        assert_eq!(rows, 2);
        assert_eq!(rects[0].left, 0.0);
        assert_eq!(rects[1].left, 0.0);
    }
}
