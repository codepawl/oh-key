//! Drawing into memory instead of a window: the tray icons and the `--render-png` check
//! both use a WIC bitmap as the Direct2D target, so they run the same drawing code as the
//! windows do.

use std::path::Path;

use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE, ID2D1RenderTarget,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppPBGRA, IWICBitmap,
    IWICBitmapFrameEncode, IWICImagingFactory, WICBitmapCacheOnLoad, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::{HSTRING, Interface, Result};

use crate::graphics::Graphics;

/// A bitmap of `width` x `height` pixels and a Direct2D target that draws into it at 96 dpi.
pub struct Offscreen {
    imaging: IWICImagingFactory,
    bitmap: IWICBitmap,
    pub target: ID2D1RenderTarget,
    width: u32,
    height: u32,
}

impl Offscreen {
    /// COM must be initialised on this thread.
    pub fn new(graphics: &Graphics, width: u32, height: u32) -> Result<Self> {
        let imaging: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
        let bitmap = unsafe {
            imaging.CreateBitmap(
                width,
                height,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapCacheOnLoad,
            )?
        };
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let target = unsafe {
            graphics
                .d2d
                .CreateWicBitmapRenderTarget(&bitmap, &properties)?
        }
        .cast()?;
        Ok(Self {
            imaging,
            bitmap,
            target,
            width,
            height,
        })
    }

    /// Straight-alpha BGRA pixels, the order `CreateIcon` wants.
    pub fn straight_alpha_pixels(&self) -> Result<Vec<u8>> {
        let mut pixels = vec![0u8; (self.width * 4 * self.height) as usize];
        self.copy_premultiplied_pixels(&mut pixels)?;
        unpremultiply(&mut pixels);
        Ok(pixels)
    }

    /// Premultiplied BGRA pixels, top row first, the form `UpdateLayeredWindow` wants.
    /// `pixels` must hold `width * height * 4` bytes.
    pub fn copy_premultiplied_pixels(&self, pixels: &mut [u8]) -> Result<()> {
        let stride = self.width * 4;
        unsafe { self.bitmap.CopyPixels(std::ptr::null(), stride, pixels) }
    }

    pub fn save_png(&self, path: &Path) -> Result<()> {
        unsafe {
            let stream = self.imaging.CreateStream()?;
            stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0)?;
            let encoder = self
                .imaging
                .CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame: Option<IWICBitmapFrameEncode> = None;
            encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
            let frame = frame.ok_or_else(windows::core::Error::empty)?;
            frame.Initialize(None)?;
            frame.SetSize(self.width, self.height)?;
            let mut format = GUID_WICPixelFormat32bppPBGRA;
            frame.SetPixelFormat(&mut format)?;
            frame.WriteSource(&self.bitmap, std::ptr::null())?;
            frame.Commit()?;
            encoder.Commit()?;
        }
        Ok(())
    }
}

fn unpremultiply(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = pixel[3] as u32;
        if alpha == 0 || alpha == 255 {
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpremultiplying_restores_the_colour_of_a_half_transparent_pixel() {
        let mut pixels = [50, 100, 150, 128];
        unpremultiply(&mut pixels);
        assert_eq!(pixels, [100, 199, 255, 128]);
    }

    #[test]
    fn opaque_and_empty_pixels_are_left_alone() {
        let mut pixels = [1, 2, 3, 255, 0, 0, 0, 0];
        unpremultiply(&mut pixels);
        assert_eq!(pixels, [1, 2, 3, 255, 0, 0, 0, 0]);
    }
}
