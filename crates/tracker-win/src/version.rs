//! Strings from an exe's version resource (what Explorer's Details tab shows).

use std::path::Path;

use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::core::HSTRING;

/// A `StringFileInfo` value such as `FileDescription` or `ProductName`, from the first string
/// table the file lists. `None` if it is missing or blank.
pub(crate) fn string(path: &Path, name: &str) -> Option<String> {
    let path = HSTRING::from(path);
    // SAFETY: plain FFI call with a valid path string.
    let size = unsafe { GetFileVersionInfoSizeW(&path, None) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    // SAFETY: `data` is exactly `size` bytes, as the API was told.
    unsafe { GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()) }.ok()?;
    // The first language/codepage pair names the string table to read.
    let translation = query_value(&data, &HSTRING::from(r"\VarFileInfo\Translation"))?;
    let [lang, codepage, ..] = translation[..] else {
        return None;
    };
    let key = format!(r"\StringFileInfo\{lang:04x}{codepage:04x}\{name}");
    let text = query_value(&data, &HSTRING::from(key))?;
    let text = String::from_utf16_lossy(&text);
    let text = text.trim_end_matches('\0').trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// A version-resource value as UTF-16 units (for `Translation`, pairs of u16 lang/codepage).
fn query_value(data: &[u8], sub_block: &HSTRING) -> Option<Vec<u16>> {
    let mut value = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: `data` is a complete version-info block from GetFileVersionInfoW; on success the
    // API points `value` into it, with `len` in u16 units for strings and bytes for binary data.
    let found = unsafe { VerQueryValueW(data.as_ptr().cast(), sub_block, &mut value, &mut len) };
    if !found.as_bool() || value.is_null() || len == 0 {
        return None;
    }
    let is_binary = sub_block.to_string_lossy().starts_with(r"\VarFileInfo");
    let units = if is_binary {
        len as usize / 2
    } else {
        len as usize
    };
    // SAFETY: `value` points to `units` u16s inside `data`, per the API contract above; the
    // version block is 2-byte aligned.
    Some(unsafe { std::slice::from_raw_parts(value.cast::<u16>(), units) }.to_vec())
}
