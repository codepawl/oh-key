//! The macro editor (gõ tắt): a list of key → text rows and two text inputs under it, in the
//! control panel's look. Every change is written to the Unikey macro file at once and the
//! settings generation is bumped, so the hook reloads it.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Once;

use oh_key_ipc::{load_settings, macros_path};
use oh_key_macro::{MAX_ITEMS, MAX_KEY_LENGTH, MAX_TEXT_LENGTH, MacroError, MacroTable};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, HBRUSH, HDC, HFONT, HGDIOBJ, SetBkColor, SetTextColor,
    ValidateRect,
};
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_OVERWRITEPROMPT,
    OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, GWLP_USERDATA, IDC_ARROW, IDC_HAND, LoadCursorW, SetCursor,
    SetWindowLongPtrW, WM_CTLCOLOREDIT, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY,
    WM_PAINT, WM_SETCURSOR,
};
use windows::core::{PWSTR, Result, w};

use crate::graphics::Graphics;
use crate::layout::{self, Rect};
use crate::tool_window::{
    self, Brushes, MESSAGE_EDIT_ENTER, MESSAGE_EDIT_TAB, OpenWindow, Outcome,
};
use crate::{icons, persistence, style};

const WIDTH: f32 = 760.0;
const RIGHT: f32 = WIDTH - layout::PADDING;
const LIST_ROWS: usize = 6;
const LIST_TOP: f32 = layout::TITLE_HEIGHT;
const LIST_ROWS_TOP: f32 = LIST_TOP + layout::SECTION_HEIGHT;
const EDITOR_TOP: f32 = LIST_ROWS_TOP + LIST_ROWS as f32 * layout::ROW_HEIGHT + layout::SECTION_GAP;
const KEY_ROW_TOP: f32 = EDITOR_TOP + layout::SECTION_HEIGHT;
const TEXT_ROW_TOP: f32 = KEY_ROW_TOP + layout::ROW_HEIGHT;
const ACTIONS_TOP: f32 = TEXT_ROW_TOP + layout::ROW_HEIGHT;
const STATUS_TOP: f32 = ACTIONS_TOP + layout::ROW_HEIGHT;
const HEIGHT: f32 = STATUS_TOP + layout::ROW_HEIGHT + layout::PADDING;

const INPUT_LEFT: f32 = layout::PADDING + 150.0;
const INPUT_INSET: f32 = 10.0;
const KEY_COLUMN_WIDTH: f32 = 140.0;
const ARROW_OFFSET: f32 = 156.0;
const TEXT_COLUMN_OFFSET: f32 = 190.0;

const KEY_INPUT_ID: isize = 101;
const TEXT_INPUT_ID: isize = 102;

const LABEL_NEW: &str = "mới";
const LABEL_SAVE: &str = "lưu";
const LABEL_DELETE: &str = "xóa";
const LABEL_IMPORT: &str = "nhập…";
const LABEL_EXPORT: &str = "xuất…";
const ACTION_LABELS: [&str; 5] = [
    LABEL_NEW,
    LABEL_SAVE,
    LABEL_DELETE,
    LABEL_IMPORT,
    LABEL_EXPORT,
];

static OPEN_WINDOW: OpenWindow = OpenWindow::new();
static CLASS_REGISTERED: Once = Once::new();

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Row(usize),
    New,
    Save,
    Delete,
    Import,
    Export,
}

const ACTION_HITS: [Hit; 5] = [Hit::New, Hit::Save, Hit::Delete, Hit::Import, Hit::Export];

/// Why an edit could not be stored.
#[derive(Clone, Debug, PartialEq, Eq)]
enum EditFailure {
    EmptyKey,
    KeyHasColon,
    TableFull,
}

impl EditFailure {
    fn message(&self) -> String {
        match self {
            EditFailure::EmptyKey => "chưa nhập từ viết tắt".to_owned(),
            EditFailure::KeyHasColon => "từ viết tắt không được có dấu hai chấm".to_owned(),
            EditFailure::TableFull => format!("đã đủ {MAX_ITEMS} mục"),
        }
    }
}

/// Stores `text` under `key`. When `previous_key` names the row being edited, that row is
/// replaced, so renaming a key does not leave the old one behind. Returns the stored key.
fn apply_edit(
    table: &mut MacroTable,
    previous_key: Option<&str>,
    key: &str,
    text: &str,
) -> std::result::Result<String, EditFailure> {
    let key = key.trim();
    if key.is_empty() {
        return Err(EditFailure::EmptyKey);
    }
    if key.contains(':') {
        return Err(EditFailure::KeyHasColon);
    }
    if let Some(previous_key) = previous_key {
        table.remove(previous_key);
    }
    match table.insert(key, text) {
        Ok(()) => Ok(key.chars().take(MAX_KEY_LENGTH).collect()),
        Err(MacroError::EmptyKey) => Err(EditFailure::EmptyKey),
        Err(MacroError::TableFull) => Err(EditFailure::TableFull),
    }
}

/// Adds every macro of `source` to `target`; returns how many were stored.
fn merge_tables(target: &mut MacroTable, source: &MacroTable) -> usize {
    source
        .items()
        .into_iter()
        .filter(|(key, text)| target.insert(key, text).is_ok())
        .count()
}

/// The first visible row such that `selected` is on screen, keeping `scroll` when it already is.
fn scroll_to_show(scroll: usize, selected: usize) -> usize {
    if selected < scroll {
        selected
    } else if selected >= scroll + LIST_ROWS {
        selected + 1 - LIST_ROWS
    } else {
        scroll
    }
}

fn clamp_scroll(scroll: usize, item_count: usize) -> usize {
    scroll.min(item_count.saturating_sub(LIST_ROWS))
}

fn sorted_items(table: &MacroTable) -> Vec<(String, String)> {
    table
        .items()
        .into_iter()
        .map(|(key, text)| (key.to_owned(), text.to_owned()))
        .collect()
}

fn position_of_key(items: &[(String, String)], key: &str) -> Option<usize> {
    let wanted = key.to_lowercase();
    items
        .iter()
        .position(|(item_key, _)| item_key.to_lowercase() == wanted)
}

fn load_table() -> MacroTable {
    macros_path()
        .and_then(|path| MacroTable::load(&path).ok())
        .map(|loaded| loaded.table)
        .unwrap_or_default()
}

/// Writes the macro file and tells the hook to reload it.
fn persist(table: &MacroTable) -> io::Result<()> {
    let path = macros_path().ok_or(io::ErrorKind::NotFound)?;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    table.save(&path)?;
    persistence::publish(&load_settings());
    Ok(())
}

struct MacrosWindow {
    graphics: Graphics,
    hwnd: HWND,
    target: ID2D1RenderTarget,
    key_input: HWND,
    text_input: HWND,
    font: HFONT,
    input_brush: HBRUSH,
    table: MacroTable,
    items: Vec<(String, String)>,
    selected: Option<usize>,
    scroll: usize,
    outcome: Option<Outcome>,
    pointer: (f32, f32),
    hovered: Option<Hit>,
    hits: Vec<(Hit, Rect)>,
}

impl Drop for MacrosWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.font.0));
            let _ = DeleteObject(HGDIOBJ(self.input_brush.0));
        }
    }
}

/// Opens the editor, or brings the open one forward.
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
        w!("oh-key.macros"),
        Some(window_proc),
        &CLASS_REGISTERED,
    );
    let frame = tool_window::create_frame(
        w!("oh-key.macros"),
        w!("oh-key gõ tắt"),
        (WIDTH, HEIGHT),
        owner,
    )?;
    let built = build_window(graphics, &frame);
    match built {
        Ok(window) => {
            unsafe { SetWindowLongPtrW(frame.hwnd, GWLP_USERDATA, Box::into_raw(window) as _) };
            OPEN_WINDOW.set(frame.hwnd);
            tool_window::show(frame.hwnd);
            Ok(())
        }
        Err(error) => {
            unsafe {
                let _ = DestroyWindow(frame.hwnd);
            }
            Err(error)
        }
    }
}

fn build_window(graphics: Graphics, frame: &tool_window::Frame) -> Result<Box<MacrosWindow>> {
    let target = tool_window::create_target(&graphics, frame.hwnd, frame.scale)?;
    let font = tool_window::create_input_font(frame.scale);
    let key_input = tool_window::create_input(
        frame.hwnd,
        KEY_INPUT_ID,
        input_area(&graphics, KEY_ROW_TOP),
        frame.scale,
        font,
        MAX_KEY_LENGTH,
    )?;
    let text_input = tool_window::create_input(
        frame.hwnd,
        TEXT_INPUT_ID,
        input_area(&graphics, TEXT_ROW_TOP),
        frame.scale,
        font,
        MAX_TEXT_LENGTH,
    )?;
    let table = load_table();
    let items = sorted_items(&table);
    Ok(Box::new(MacrosWindow {
        graphics,
        hwnd: frame.hwnd,
        target,
        key_input,
        text_input,
        font,
        input_brush: unsafe { CreateSolidBrush(tool_window::panel_colorref()) },
        table,
        items,
        selected: None,
        scroll: 0,
        outcome: None,
        pointer: (-1.0, -1.0),
        hovered: None,
        hits: Vec::new(),
    }))
}

/// The rounded fill behind the input of the row that starts at `row_top`.
fn panel_rect(row_top: f32) -> Rect {
    let top = row_top + (layout::ROW_HEIGHT - layout::BUTTON_HEIGHT) / 2.0;
    Rect {
        left: INPUT_LEFT,
        top,
        right: RIGHT,
        bottom: top + layout::BUTTON_HEIGHT,
    }
}

/// The EDIT control inside that panel: one text line tall, centred.
fn input_area(graphics: &Graphics, row_top: f32) -> Rect {
    let panel = panel_rect(row_top);
    let line_height = graphics.measure("Ag").1;
    let top = panel.top + (layout::BUTTON_HEIGHT - line_height) / 2.0;
    Rect {
        left: panel.left + INPUT_INSET,
        top,
        right: panel.right - INPUT_INSET,
        bottom: top + line_height,
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state = tool_window::state_of::<MacrosWindow>(hwnd);
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
        WM_MOUSEWHEEL => {
            let notches = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 / 120;
            window.scroll_by(-(notches as isize));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            window.pointer = tool_window::pointer_in_dips(&window.target, lparam);
            if let Some(hit) = window.hit_at(window.pointer) {
                window.choose(hit);
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let device = HDC(wparam.0 as *mut _);
            unsafe {
                SetTextColor(device, tool_window::text_colorref());
                SetBkColor(device, tool_window::panel_colorref());
            }
            LRESULT(window.input_brush.0 as isize)
        }
        MESSAGE_EDIT_ENTER => {
            window.save();
            LRESULT(0)
        }
        MESSAGE_EDIT_TAB => {
            let from_key = HWND(wparam.0 as *mut _) == window.key_input;
            tool_window::focus(if from_key {
                window.text_input
            } else {
                window.key_input
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// Frees the state when the window goes, whichever way it was closed.
fn release_window(hwnd: HWND) {
    let pointer =
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) }
            as *mut MacrosWindow;
    if pointer.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(pointer) });
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
    OPEN_WINDOW.clear();
}

impl MacrosWindow {
    fn hit_at(&self, point: (f32, f32)) -> Option<Hit> {
        layout::hit_test(&self.hits, point)
    }

    fn paint(&mut self) {
        let view = View {
            items: &self.items,
            selected: self.selected,
            scroll: self.scroll,
            outcome: self.outcome.as_ref(),
            sample_inputs: None,
        };
        let hits = draw(&self.target, &self.graphics, &view, self.hovered);
        self.hits = hits;
        // The window's Direct2D surface may cover the inputs; have each one draw itself again.
        for input in [self.key_input, self.text_input] {
            tool_window::redraw_now(input);
        }
    }

    fn choose(&mut self, hit: Hit) {
        match hit {
            Hit::Row(index) => self.select(Some(index)),
            Hit::New => {
                self.select(None);
                tool_window::focus(self.key_input);
            }
            Hit::Save => self.save(),
            Hit::Delete => self.delete(),
            Hit::Import => self.import(),
            Hit::Export => self.export(),
        }
        self.hovered = self.hit_at(self.pointer);
        self.paint();
    }

    fn select(&mut self, index: Option<usize>) {
        self.selected = index.filter(|index| *index < self.items.len());
        let (key, text) = self
            .selected
            .map(|index| self.items[index].clone())
            .unwrap_or_default();
        tool_window::set_input_text(self.key_input, &key);
        tool_window::set_input_text(self.text_input, &text);
        if let Some(index) = self.selected {
            self.scroll = scroll_to_show(self.scroll, index);
        }
    }

    fn scroll_by(&mut self, rows: isize) {
        let moved = self.scroll.saturating_add_signed(rows);
        self.scroll = clamp_scroll(moved, self.items.len());
        self.hovered = None;
        self.paint();
    }

    fn selected_key(&self) -> Option<String> {
        self.selected.map(|index| self.items[index].0.clone())
    }

    fn save(&mut self) {
        let key = tool_window::input_text(self.key_input);
        let text = tool_window::input_text(self.text_input);
        let previous_key = self.selected_key();
        let mut edited = self.table.clone();
        let outcome = match apply_edit(&mut edited, previous_key.as_deref(), &key, &text) {
            Ok(stored_key) => self.store(edited, &format!("đã lưu “{stored_key}”"), &stored_key),
            Err(failure) => Outcome::Failure(failure.message()),
        };
        self.outcome = Some(outcome);
        self.paint();
    }

    /// Writes `table`; on success it becomes the table and `focus_key` is selected.
    fn store(&mut self, table: MacroTable, success_message: &str, focus_key: &str) -> Outcome {
        if persist(&table).is_err() {
            return Outcome::Failure("không ghi được tệp gõ tắt".to_owned());
        }
        self.table = table;
        self.items = sorted_items(&self.table);
        self.scroll = clamp_scroll(self.scroll, self.items.len());
        let index = position_of_key(&self.items, focus_key);
        self.select(index);
        Outcome::Success(success_message.to_owned())
    }

    fn delete(&mut self) {
        let Some(key) = self.selected_key() else {
            self.outcome = Some(Outcome::Failure("chưa chọn mục nào để xóa".to_owned()));
            return;
        };
        let mut remaining = self.table.clone();
        remaining.remove(&key);
        let outcome = self.store(remaining, &format!("đã xóa “{key}”"), "");
        self.outcome = Some(outcome);
    }

    fn import(&mut self) {
        let Some(path) = pick_file(self.hwnd, FileDialog::Open) else {
            return;
        };
        let outcome = match MacroTable::load(&path) {
            Ok(loaded) => {
                let mut merged = self.table.clone();
                let stored = merge_tables(&mut merged, &loaded.table);
                self.store(merged, &format!("đã nhập {stored} mục"), "")
            }
            Err(_) => Outcome::Failure("không đọc được tệp".to_owned()),
        };
        self.outcome = Some(outcome);
    }

    fn export(&mut self) {
        let Some(path) = pick_file(self.hwnd, FileDialog::Save) else {
            return;
        };
        let outcome = match self.table.save(&path) {
            Ok(()) => Outcome::Success(format!("đã xuất {} mục", self.table.len())),
            Err(_) => Outcome::Failure("không ghi được tệp".to_owned()),
        };
        self.outcome = Some(outcome);
    }
}

#[derive(Clone, Copy)]
enum FileDialog {
    Open,
    Save,
}

/// The Windows file dialog for a Unikey macro file; `None` when cancelled.
fn pick_file(owner: HWND, kind: FileDialog) -> Option<PathBuf> {
    let mut buffer = [0u16; 1024];
    if matches!(kind, FileDialog::Save) {
        for (slot, unit) in buffer.iter_mut().zip("oh-key-gotat.txt".encode_utf16()) {
            *slot = unit;
        }
    }
    let flags = match kind {
        FileDialog::Open => OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        FileDialog::Save => OFN_EXPLORER | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
    };
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: w!("Tệp gõ tắt (*.txt)\0*.txt\0Mọi tệp (*.*)\0*.*\0"),
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrDefExt: w!("txt"),
        Flags: flags,
        ..Default::default()
    };
    let chosen = unsafe {
        match kind {
            FileDialog::Open => GetOpenFileNameW(&mut dialog),
            FileDialog::Save => GetSaveFileNameW(&mut dialog),
        }
    };
    chosen.as_bool().then(|| path_from_buffer(&buffer))
}

fn path_from_buffer(buffer: &[u16]) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    Path::new(&std::ffi::OsString::from_wide(&buffer[..length])).to_path_buf()
}

/// Everything the drawing needs, so the window and `--render-png` draw the same thing.
struct View<'a> {
    items: &'a [(String, String)],
    selected: Option<usize>,
    scroll: usize,
    outcome: Option<&'a Outcome>,
    /// Only for `--render-png`, which has no EDIT controls: the key and text to paint in the panels.
    sample_inputs: Option<(&'a str, &'a str)>,
}

fn row_rect(visible_index: usize) -> Rect {
    let top = LIST_ROWS_TOP
        + visible_index as f32 * layout::ROW_HEIGHT
        + (layout::ROW_HEIGHT - layout::BUTTON_HEIGHT) / 2.0;
    Rect {
        left: layout::PADDING,
        top,
        right: RIGHT,
        bottom: top + layout::BUTTON_HEIGHT,
    }
}

fn draw(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    view: &View,
    hovered: Option<Hit>,
) -> Vec<(Hit, Rect)> {
    let Some(brushes) = tool_window::create_brushes(target) else {
        return Vec::new();
    };
    tool_window::begin_window(target, graphics, &brushes, "gõ tắt");
    let mut hits = Vec::new();
    draw_list(target, graphics, &brushes, view, hovered, &mut hits);
    draw_editor(target, graphics, &brushes, view, hovered, &mut hits);
    if let Some(outcome) = view.outcome {
        tool_window::draw_outcome(target, graphics, &brushes, outcome, STATUS_TOP, RIGHT);
    }
    tool_window::end_window(target);
    hits
}

fn draw_list(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    view: &View,
    hovered: Option<Hit>,
    hits: &mut Vec<(Hit, Rect)>,
) {
    let title = format!("danh sách ({})", view.items.len());
    tool_window::draw_section_header(
        target,
        graphics,
        brushes,
        icons::TEXT,
        &title,
        LIST_TOP,
        RIGHT,
    );
    if view.items.len() > LIST_ROWS {
        let last = (view.scroll + LIST_ROWS).min(view.items.len());
        let range = format!("{}–{} / {}", view.scroll + 1, last, view.items.len());
        let width = graphics.measure(&range).0;
        graphics.draw_label(
            target,
            &range,
            RIGHT - width - 12.0,
            LIST_TOP + 14.0,
            RIGHT,
            &brushes.dim,
        );
    }
    if view.items.is_empty() {
        let empty = row_rect(0);
        tool_window::draw_label_centred(
            target,
            graphics,
            "chưa có gõ tắt nào",
            empty,
            0.0,
            &brushes.dim,
        );
        return;
    }
    let visible = view
        .items
        .iter()
        .enumerate()
        .skip(view.scroll)
        .take(LIST_ROWS);
    for (visible_index, (item_index, (key, text))) in visible.enumerate() {
        let rect = row_rect(visible_index);
        let hit = Hit::Row(item_index);
        let selected = view.selected == Some(item_index);
        draw_list_row(
            target,
            graphics,
            brushes,
            (key, text),
            rect,
            selected,
            hovered == Some(hit),
        );
        hits.push((hit, rect));
    }
}

fn draw_list_row(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    (key, text): (&str, &str),
    rect: Rect,
    selected: bool,
    hovered: bool,
) {
    let fill = if selected {
        Some(&brushes.accent_soft)
    } else if hovered {
        Some(&brushes.hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        let shape = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
            rect: rect.into(),
            radiusX: style::BUTTON_RADIUS,
            radiusY: style::BUTTON_RADIUS,
        };
        unsafe { target.FillRoundedRectangle(&shape, fill) };
    }
    let (_, text_height) = graphics.measure(key);
    let text_top = rect.top + (layout::BUTTON_HEIGHT - text_height) / 2.0;
    let key_brush = if selected {
        &brushes.accent
    } else {
        &brushes.text
    };
    let left = rect.left + INPUT_INSET;
    graphics.draw_label(
        target,
        key,
        left,
        text_top,
        left + KEY_COLUMN_WIDTH,
        key_brush,
    );
    graphics.draw_label(
        target,
        "→",
        rect.left + ARROW_OFFSET,
        text_top,
        rect.left + TEXT_COLUMN_OFFSET,
        &brushes.dim,
    );
    graphics.draw_label(
        target,
        text,
        rect.left + TEXT_COLUMN_OFFSET,
        text_top,
        rect.right - INPUT_INSET,
        &brushes.dim,
    );
}

fn draw_editor(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    view: &View,
    hovered: Option<Hit>,
    hits: &mut Vec<(Hit, Rect)>,
) {
    let title = if view.selected.is_some() {
        "sửa gõ tắt"
    } else {
        "thêm gõ tắt"
    };
    tool_window::draw_section_header(
        target,
        graphics,
        brushes,
        icons::EDIT,
        title,
        EDITOR_TOP,
        RIGHT,
    );
    let rows = [
        (KEY_ROW_TOP, icons::KEYBOARD, "viết tắt"),
        (TEXT_ROW_TOP, icons::TEXT, "thay bằng"),
    ];
    for (row_top, icon, label) in rows {
        tool_window::draw_row_label(target, graphics, brushes, icon, label, row_top, INPUT_LEFT);
        tool_window::draw_input_panel(target, brushes, panel_rect(row_top));
    }
    if let Some((key, text)) = view.sample_inputs {
        draw_sample_input(target, graphics, brushes, KEY_ROW_TOP, key);
        draw_sample_input(target, graphics, brushes, TEXT_ROW_TOP, text);
    }
    let rects = tool_window::button_row(graphics, &ACTION_LABELS, ACTIONS_TOP, RIGHT);
    for ((label, hit), rect) in ACTION_LABELS.iter().zip(ACTION_HITS).zip(rects) {
        tool_window::draw_text_button(
            target,
            graphics,
            brushes,
            label,
            rect,
            hovered == Some(hit),
            false,
        );
        hits.push((hit, rect));
    }
}

fn draw_sample_input(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    row_top: f32,
    text: &str,
) {
    let area = input_area(graphics, row_top);
    graphics.draw_label(target, text, area.left, area.top, area.right, &brushes.text);
}

/// `oh-key.exe --render-png macros out.png`: the window with a few sample macros and a
/// success message, since the real list lives in the user's own file.
pub fn render_png(graphics: &Graphics, path: &Path) -> Result<()> {
    let samples = [
        ("hn", "Hà Nội"),
        ("tphcm", "Thành phố Hồ Chí Minh"),
        ("vn", "Việt Nam"),
        ("kg", "Kính gửi"),
        ("cmt", "Cảm ơn bạn đã quan tâm"),
        ("dc", "được"),
        ("sg", "Sài Gòn"),
        ("ob", "Oh Key, bộ gõ tiếng Việt"),
    ];
    let items: Vec<(String, String)> = samples
        .iter()
        .map(|(key, text)| (key.to_string(), text.to_string()))
        .collect();
    let outcome = Outcome::Success("đã lưu “vn”".to_owned());
    let view = View {
        items: &items,
        selected: Some(2),
        scroll: 0,
        outcome: Some(&outcome),
        sample_inputs: Some(("vn", "Việt Nam")),
    };
    let offscreen = crate::offscreen::Offscreen::new(graphics, WIDTH as u32, HEIGHT as u32)?;
    draw(&offscreen.target, graphics, &view, Some(Hit::Save));
    offscreen.save_png(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_a_new_key_adds_a_row() {
        let mut table = MacroTable::new();
        let stored = apply_edit(&mut table, None, "  hn ", "Hà Nội").unwrap();
        assert_eq!(stored, "hn");
        assert_eq!(table.lookup("hn"), Some("Hà Nội"));
    }

    #[test]
    fn renaming_the_edited_row_drops_the_old_key() {
        let mut table = MacroTable::new();
        table.insert("hn", "Hà Nội").unwrap();
        apply_edit(&mut table, Some("hn"), "hanoi", "Hà Nội").unwrap();
        assert_eq!(table.lookup("hn"), None);
        assert_eq!(table.lookup("hanoi"), Some("Hà Nội"));
    }

    #[test]
    fn editing_the_text_keeps_the_key() {
        let mut table = MacroTable::new();
        table.insert("hn", "Hà Nội").unwrap();
        apply_edit(&mut table, Some("hn"), "hn", "Hanoi").unwrap();
        assert_eq!(table.len(), 1);
        assert_eq!(table.lookup("hn"), Some("Hanoi"));
    }

    #[test]
    fn an_empty_key_or_one_with_a_colon_is_refused_and_changes_nothing() {
        let mut table = MacroTable::new();
        table.insert("hn", "Hà Nội").unwrap();
        assert_eq!(
            apply_edit(&mut table, Some("hn"), "  ", "x"),
            Err(EditFailure::EmptyKey)
        );
        assert_eq!(
            apply_edit(&mut table, Some("hn"), "a:b", "x"),
            Err(EditFailure::KeyHasColon)
        );
        assert_eq!(table.lookup("hn"), Some("Hà Nội"));
    }

    #[test]
    fn importing_merges_and_the_imported_text_wins() {
        let mut table = MacroTable::new();
        table.insert("hn", "cũ").unwrap();
        let imported = MacroTable::parse(
            "\u{FEFF};DO NOT DELETE THIS LINE*** version=1 ***\nhn:mới\nvn:Việt Nam".as_bytes(),
        );
        assert_eq!(merge_tables(&mut table, &imported.table), 2);
        assert_eq!(table.lookup("hn"), Some("mới"));
        assert_eq!(table.lookup("vn"), Some("Việt Nam"));
    }

    #[test]
    fn the_list_scrolls_just_enough_to_show_the_selected_row() {
        assert_eq!(scroll_to_show(0, 3), 0);
        assert_eq!(scroll_to_show(0, LIST_ROWS), 1);
        assert_eq!(scroll_to_show(5, 2), 2);
    }

    #[test]
    fn scrolling_stops_at_the_last_full_page() {
        assert_eq!(clamp_scroll(10, 8), 8 - LIST_ROWS);
        assert_eq!(clamp_scroll(3, 4), 0);
    }

    #[test]
    fn a_key_is_found_in_the_sorted_list_ignoring_case() {
        let items = vec![("HN".to_owned(), "Hà Nội".to_owned())];
        assert_eq!(position_of_key(&items, "hn"), Some(0));
        assert_eq!(position_of_key(&items, "vn"), None);
    }
}
