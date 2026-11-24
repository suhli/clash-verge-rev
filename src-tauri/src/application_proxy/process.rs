use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GdiFlush, SelectObject,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
        },
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
    },
    UI::{
        Shell::ExtractIconExW,
        WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx},
    },
};

#[derive(Serialize)]
pub struct Process {
    name: String,
    path: String,
    icon: Option<String>,
}

pub fn image(pid: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut buffer = [0; 32768];
    let mut length = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle.as_raw_handle(), 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

fn icon(path: &str) -> Option<String> {
    let path: Vec<_> = path.encode_utf16().chain([0]).collect();
    unsafe {
        let mut icon = ptr::null_mut();
        ExtractIconExW(path.as_ptr(), 0, ptr::null_mut(), &mut icon, 1);
        if icon.is_null() {
            return None;
        }
        let dc = CreateCompatibleDC(ptr::null_mut());
        let header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: 16,
            biHeight: 16,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        };
        let info = BITMAPINFO {
            bmiHeader: header,
            ..Default::default()
        };
        let mut buffer = ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut buffer, ptr::null_mut(), 0);
        let result = if bitmap.is_null() || buffer.is_null() {
            None
        } else {
            let previous = SelectObject(dc, bitmap);
            ptr::write_bytes(buffer.cast::<u8>(), 0, 16 * 16 * 4);
            let drawn = DrawIconEx(dc, 0, 0, icon, 16, 16, 0, ptr::null_mut(), DI_NORMAL) != 0;
            GdiFlush();
            SelectObject(dc, previous);
            drawn.then(|| {
                // ICO directory, DIB header, bottom-up BGRA pixels, and the DWORD-aligned mask.
                let mut data = vec![0, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 32, 0];
                data.extend((40u32 + 1024 + 64).to_le_bytes());
                data.extend(22u32.to_le_bytes());
                data.extend(40u32.to_le_bytes());
                data.extend(16u32.to_le_bytes());
                data.extend(32u32.to_le_bytes());
                data.extend([1, 0, 32, 0]);
                data.extend([0; 24]);
                data.extend_from_slice(std::slice::from_raw_parts(buffer.cast::<u8>(), 16 * 16 * 4));
                data.extend([0; 64]);
                format!("data:image/x-icon;base64,{}", STANDARD.encode(data))
            })
        };
        if !bitmap.is_null() {
            DeleteObject(bitmap);
        }
        if !dc.is_null() {
            DeleteDC(dc);
        }
        DestroyIcon(icon);
        result
    }
}

pub fn running() -> Result<Vec<Process>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut matches = BTreeMap::new();
    let mut more = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) };
    while more != 0 {
        if let Some(path) = image(entry.th32ProcessID) {
            let name = Path::new(&path)
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(&path)
                .to_owned();
            matches
                .entry((name.to_lowercase(), path.to_lowercase()))
                .or_insert_with(|| Process {
                    icon: icon(&path),
                    name,
                    path,
                });
        }
        more = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) };
    }
    Ok(matches.into_values().collect())
}
