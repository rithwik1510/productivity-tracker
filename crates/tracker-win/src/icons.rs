//! App icons for the catalog: a 64×64 shell image, un-premultiplied, saved as PNG at
//! `tracker_api::icon_path`. Extracted once, after the listing that first needs it returns;
//! later listings reuse the file on disk.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use tracker_api::{AppIdentity, icon_path};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDC, GetDIBits, GetObjectW,
    HBITMAP, HGDIOBJ, ReleaseDC,
};
use windows::Win32::UI::Shell::{IShellItem, IShellItemImageFactory, SIIGBF_ICONONLY};
use windows::core::{Interface, Owned};

use crate::catalog::CatalogError;

const ICON_SIZE: i32 = 64;

/// The icon file for `identity`, if it has been extracted. Until then the UI shows a monogram.
pub(crate) fn cached(data_dir: &Path, identity: &AppIdentity) -> Option<String> {
    let path = icon_path(data_dir, identity);
    path.is_file().then(|| path.to_string_lossy().into_owned())
}

/// Extracts `source`'s icon into `identity`'s file. False if that fails, which leaves the
/// monogram; the next listing tries again.
pub(crate) fn extract_to(data_dir: &Path, identity: &AppIdentity, source: &IShellItem) -> bool {
    let path = icon_path(data_dir, identity);
    let written = extract(source).and_then(|(w, h, rgba)| save(&path, w, h, &rgba));
    if let Err(err) = &written {
        tracing::debug!(?identity, "icon extraction failed: {err}");
    }
    written.is_ok()
}

/// Width, height and straight-alpha RGBA pixels.
fn extract(source: &IShellItem) -> Result<(u32, u32, Vec<u8>), CatalogError> {
    let factory: IShellItemImageFactory = source.cast()?;
    let size = SIZE {
        cx: ICON_SIZE,
        cy: ICON_SIZE,
    };
    // SAFETY: COM call on this thread's STA; the returned bitmap is owned (deleted on drop).
    let bitmap = unsafe { Owned::new(factory.GetImage(size, SIIGBF_ICONONLY)?) };
    let (width, height, mut pixels) = bgra_pixels(*bitmap)?;
    unpremultiply_to_rgba(&mut pixels);
    Ok((width, height, pixels))
}

/// Top-down 32-bit BGRA pixels of a bitmap.
fn bgra_pixels(bitmap: HBITMAP) -> Result<(u32, u32, Vec<u8>), CatalogError> {
    let mut info = BITMAP::default();
    // SAFETY: `info` is a BITMAP-sized out buffer, as the size argument says.
    let got = unsafe {
        GetObjectW(
            HGDIOBJ(bitmap.0),
            size_of::<BITMAP>() as i32,
            Some((&raw mut info).cast()),
        )
    };
    let (width, height) = (info.bmWidth, info.bmHeight.abs());
    if got == 0 || width <= 0 || height <= 0 {
        return Err(CatalogError::Icon("unreadable bitmap"));
    }
    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // negative: top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let (w, h) = (width as u32, height as u32);
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    // SAFETY: `pixels` holds exactly `height` rows of `width` 32-bit pixels, matching `header`;
    // the screen DC is released right after.
    let lines = unsafe {
        let dc = GetDC(None);
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            h,
            Some(pixels.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, dc);
        lines
    };
    if lines != height {
        return Err(CatalogError::Icon("GetDIBits failed"));
    }
    Ok((w, h, pixels))
}

/// Shell bitmaps are premultiplied BGRA; PNG wants straight RGBA. An all-zero alpha channel
/// means the bitmap has no alpha at all, so it is opaque.
fn unpremultiply_to_rgba(pixels: &mut [u8]) {
    let opaque = pixels.chunks_exact(4).all(|px| px[3] == 0);
    for px in pixels.chunks_exact_mut(4) {
        let [b, g, r, a] = [px[0], px[1], px[2], px[3]];
        let a = if opaque { 255 } else { a };
        let straight = |c: u8| match a {
            0 => 0,
            255 => c,
            a => ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8,
        };
        px.copy_from_slice(&[straight(r), straight(g), straight(b), a]);
    }
}

/// Writes via a temp file and rename, so a reader never sees a half-written PNG.
fn save(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), CatalogError> {
    let tmp = path.with_extension("png.tmp");
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(&tmp)?), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    fs::rename(&tmp, path)?;
    Ok(())
}
