//! Direct2D and DirectWrite factories and the text formats every window shares, as in
//! oh-snap's overlay. Labels use the bundled JetBrains Mono; a glyph it lacks falls back to
//! the system font on its own, which is how Vietnamese marks it does not carry would render.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1CreateFactory, ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_METRICS, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteFactory5,
    IDWriteFontCollection, IDWriteInMemoryFontFileLoader, IDWriteTextFormat,
};
use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
use windows::core::{BOOL, HSTRING, IUnknown, Interface, Result, w};

use crate::layout::Rect;
use crate::style;

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

impl From<Rect> for D2D_RECT_F {
    fn from(rect: Rect) -> Self {
        D2D_RECT_F {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
    }
}

/// How many Graphics are alive.
static LIVE_GRAPHICS: AtomicUsize = AtomicUsize::new(0);

/// Counts one live Graphics. Declared as its last field so it drops after the factories
/// do, and the last one to drop hands the memory back.
struct LiveGraphicsGuard;

impl LiveGraphicsGuard {
    fn new() -> Self {
        LIVE_GRAPHICS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for LiveGraphicsGuard {
    fn drop(&mut self) {
        if LIVE_GRAPHICS.fetch_sub(1, Ordering::AcqRel) == 1 {
            trim_working_set();
        }
    }
}

/// The DLL pages and caches Direct2D and DirectWrite touched stay resident after their objects
/// are gone; measured, emptying the set takes an idle tray from about 28 MB to 2 MB, and what
/// the next window needs pages back in.
fn trim_working_set() {
    let _ = unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
}

/// Device-independent factories, created for one window (or one tray icon draw) and released
/// with it, so an idle tray holds none.
pub struct Graphics {
    pub d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    mono_fonts: IDWriteFontCollection,
    /// Registered with the shared DirectWrite factory, so it is unregistered when this drops.
    font_loader: IDWriteInMemoryFontFileLoader,
    font_factory: IDWriteFactory5,
    label_format: IDWriteTextFormat,
    /// Centred in its box; draws the ✻ mark.
    spark_format: IDWriteTextFormat,
    /// Segoe Fluent Icons on Windows 11, Segoe MDL2 Assets on Windows 10.
    icon_format: IDWriteTextFormat,
    _live: LiveGraphicsGuard,
}

impl Graphics {
    pub fn new() -> Result<Self> {
        let d2d: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let factory: IDWriteFactory5 = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let (mono_fonts, font_loader) = load_private_mono_fonts(&factory)?;
        let dwrite: IDWriteFactory = factory.cast()?;

        let label_format = create_format(
            &dwrite,
            Some(&mono_fonts),
            style::MONO_FAMILY,
            style::LABEL_SIZE,
        )?;
        // Labels stay on one line; text that does not fit ends in "…".
        unsafe {
            label_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let ellipsis = dwrite.CreateEllipsisTrimmingSign(&label_format)?;
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            label_format.SetTrimming(&trimming, &ellipsis)?;
        }
        let spark_format = create_format(&dwrite, None, "Segoe UI Symbol", style::SPARK_SIZE)?;
        let icon_family = pick_system_family(&dwrite, &["Segoe Fluent Icons", "Segoe MDL2 Assets"])
            .unwrap_or("Segoe MDL2 Assets");
        let icon_format = create_format(&dwrite, None, icon_family, style::ICON_SIZE)?;
        for format in [&spark_format, &icon_format] {
            unsafe {
                format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
                format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            }
        }
        Ok(Self {
            d2d,
            dwrite,
            mono_fonts,
            font_loader,
            font_factory: factory,
            label_format,
            spark_format,
            icon_format,
            _live: LiveGraphicsGuard::new(),
        })
    }

    /// One line of text clipped to `left..right`, in the label font.
    pub fn draw_label(
        &self,
        target: &ID2D1RenderTarget,
        text: &str,
        left: f32,
        top: f32,
        right: f32,
        brush: &ID2D1SolidColorBrush,
    ) {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let layout_rect = D2D_RECT_F {
            left,
            top,
            right: right.max(left + 1.0),
            bottom: top + 30.0,
        };
        unsafe {
            target.DrawText(
                &utf16,
                &self.label_format,
                &layout_rect,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Draws one glyph of the Windows icon font, centred in `area`.
    pub fn draw_icon(
        &self,
        target: &ID2D1RenderTarget,
        icon: char,
        area: Rect,
        brush: &ID2D1SolidColorBrush,
    ) {
        let mut buffer = [0u16; 2];
        let utf16 = icon.encode_utf16(&mut buffer);
        unsafe {
            target.DrawText(
                utf16,
                &self.icon_format,
                &area.into(),
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    pub fn draw_spark(&self, target: &ID2D1RenderTarget, area: Rect, brush: &ID2D1SolidColorBrush) {
        let utf16: Vec<u16> = style::SPARK.encode_utf16().collect();
        unsafe {
            target.DrawText(
                &utf16,
                &self.spark_format,
                &area.into(),
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Draws `text` centred in `area` in the mono font at `size`; used for the tray letters.
    pub fn draw_centred_mono(
        &self,
        target: &ID2D1RenderTarget,
        text: &str,
        size: f32,
        area: Rect,
        brush: &ID2D1SolidColorBrush,
    ) {
        // Bold so a 16 px tray letter still reads; DirectWrite simulates it from the bundled regular.
        let Ok(format) = create_format_weighted(
            &self.dwrite,
            Some(&self.mono_fonts),
            style::MONO_FAMILY,
            size,
            DWRITE_FONT_WEIGHT_BOLD,
        ) else {
            return;
        };
        unsafe {
            let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        }
        let utf16: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            target.DrawText(
                &utf16,
                &format,
                &area.into(),
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Width and height of `text` in the label font.
    pub fn measure(&self, text: &str) -> (f32, f32) {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let fallback = (
            text.chars().count() as f32 * style::LABEL_SIZE * 0.6,
            style::LABEL_SIZE * 1.3,
        );
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&utf16, &self.label_format, 10_000.0, 1_000.0)
        };
        let Ok(layout) = layout else {
            return fallback;
        };
        let mut metrics = DWRITE_TEXT_METRICS::default();
        if unsafe { layout.GetMetrics(&mut metrics) }.is_err() {
            return fallback;
        }
        (metrics.widthIncludingTrailingWhitespace, metrics.height)
    }
}

/// A font collection holding only the bundled font, so nothing is installed on the machine.
fn load_private_mono_fonts(
    factory: &IDWriteFactory5,
) -> Result<(IDWriteFontCollection, IDWriteInMemoryFontFileLoader)> {
    unsafe {
        let loader = factory.CreateInMemoryFontFileLoader()?;
        factory.RegisterFontFileLoader(&loader)?;
        let file = loader.CreateInMemoryFontFileReference(
            factory,
            FONT_BYTES.as_ptr() as *const c_void,
            FONT_BYTES.len() as u32,
            None::<&IUnknown>,
        )?;
        let builder = factory.CreateFontSetBuilder()?;
        builder.AddFontFile(&file)?;
        let font_set = builder.CreateFontSet()?;
        let collection = factory.CreateFontCollectionFromFontSet(&font_set)?;
        Ok((collection.cast()?, loader))
    }
}

impl Drop for Graphics {
    fn drop(&mut self) {
        let _ = unsafe {
            self.font_factory
                .UnregisterFontFileLoader(&self.font_loader)
        };
    }
}

fn create_format(
    dwrite: &IDWriteFactory,
    collection: Option<&IDWriteFontCollection>,
    family: &str,
    size: f32,
) -> Result<IDWriteTextFormat> {
    create_format_weighted(dwrite, collection, family, size, DWRITE_FONT_WEIGHT_NORMAL)
}

fn create_format_weighted(
    dwrite: &IDWriteFactory,
    collection: Option<&IDWriteFontCollection>,
    family: &str,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
) -> Result<IDWriteTextFormat> {
    unsafe {
        dwrite.CreateTextFormat(
            &HSTRING::from(family),
            collection,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("vi-vn"),
        )
    }
}

/// The first family in `families` that is installed.
fn pick_system_family(dwrite: &IDWriteFactory, families: &[&'static str]) -> Option<&'static str> {
    let mut collection = None;
    unsafe { dwrite.GetSystemFontCollection(&mut collection, false) }.ok()?;
    let collection = collection?;
    families.iter().copied().find(|family| {
        let mut index = 0;
        let mut exists = BOOL(0);
        let found =
            unsafe { collection.FindFamilyName(&HSTRING::from(*family), &mut index, &mut exists) };
        found.is_ok() && exists.as_bool()
    })
}
