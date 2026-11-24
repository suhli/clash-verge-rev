use super::{
    engine,
    model::{Action, Anchor, Delay, Overlay, RuleStats, Settings, Snapshot},
    process,
};
use std::{
    collections::BTreeMap,
    ffi::{CString, c_void},
    mem::size_of,
    ptr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
    Graphics::{
        Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        Gdi::{
            AC_SRC_ALPHA, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection,
            CreateFontW, DIB_RGB_COLORS, DT_NOPREFIX, DT_SINGLELINE, DeleteDC, DeleteObject, DrawTextW, GdiFlush, HDC,
            SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
        },
    },
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetThreadDpiAwarenessContext},
        WindowsAndMessaging::*,
    },
};

const UPDATE: u32 = WM_APP + 1;
const CLASS: &[u16] = &[67, 86, 82, 65, 112, 112, 79, 83, 68, 0];
static FRAME: Mutex<Option<(Settings, Snapshot)>> = Mutex::new(None);
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
static QUEUED: AtomicBool = AtomicBool::new(false);
static WINDOWS_DIRTY: AtomicBool = AtomicBool::new(true);
static DRAGGING_GLOBAL: AtomicBool = AtomicBool::new(false);
static STARTED: OnceLock<()> = OnceLock::new();

pub fn publish(settings: &Settings, snapshot: &Snapshot) {
    let enabled = settings.overlay.global || settings.overlay.windows;
    if !enabled && STARTED.get().is_none() {
        return;
    }
    if let Ok(mut frame) = FRAME.lock() {
        if !enabled
            && frame
                .as_ref()
                .is_some_and(|(old, _)| !old.overlay.global && !old.overlay.windows)
        {
            return;
        }
        let mut view = settings.clone();
        view.rules.iter_mut().for_each(|rule| rule.icon = None);
        *frame = Some((view, snapshot.clone()));
    }
    if enabled {
        STARTED.get_or_init(|| {
            let _ = std::thread::Builder::new()
                .name("application-proxy-osd".into())
                .spawn(run);
        });
    }
    wake();
}

fn wake() {
    let id = THREAD_ID.load(Ordering::Acquire);
    if id != 0 && !QUEUED.swap(true, Ordering::AcqRel) && unsafe { PostThreadMessageW(id, UPDATE, 0, 0) } == 0 {
        QUEUED.store(false, Ordering::Release);
    }
}

unsafe extern "system" fn changed(_: HWINEVENTHOOK, event: u32, hwnd: HWND, object: i32, child: i32, _: u32, _: u32) {
    if !hwnd.is_null() && (event == EVENT_SYSTEM_FOREGROUND || (object == OBJID_WINDOW && child == 0)) {
        WINDOWS_DIRTY.store(true, Ordering::Release);
        wake();
    }
}

#[derive(Clone, PartialEq)]
struct Drawing {
    lines: Vec<String>,
    font: i32,
    foreground: u32,
    background: u32,
    opacity: u8,
    width: i32,
    height: i32,
}

struct Window {
    hwnd: HWND,
    last: Option<Drawing>,
    global: bool,
    position: Option<(i32, i32, i32, i32, HWND)>,
}

impl Window {
    fn new(global: bool) -> Option<Self> {
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                CLASS.as_ptr(),
                CLASS.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            );
            if hwnd.is_null() {
                return None;
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, isize::from(global));
            Some(Self {
                hwnd,
                last: None,
                global,
                position: None,
            })
        }
    }

    fn show(&mut self, dc: HDC, lines: Vec<String>, style: &Overlay, target: Option<HWND>) {
        if self.global && DRAGGING_GLOBAL.load(Ordering::Acquire) {
            return;
        }
        let dpi = unsafe { GetDpiForWindow(target.unwrap_or(self.hwnd)) }.max(96);
        let font = (i32::from(style.font_size) * i32::from(style.scale_percent) * dpi as i32 / 9600).max(7);
        let width = font * if self.global { 32 } else { 26 };
        let height = (font + 5) * lines.len() as i32 + 14;
        let drawing = Drawing {
            lines,
            font,
            foreground: style.foreground,
            background: style.background,
            opacity: style.opacity,
            width,
            height,
        };

        unsafe {
            let previous_flags = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
            let flags = if !self.global || style.click_through {
                previous_flags | WS_EX_TRANSPARENT
            } else {
                previous_flags & !WS_EX_TRANSPARENT
            };
            if flags != previous_flags {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, flags as isize);
            }
            SetWindowLongPtrW(
                self.hwnd,
                GWLP_USERDATA,
                if self.global && !style.locked && !style.click_through {
                    2
                } else {
                    isize::from(self.global)
                },
            );
            if self.last.as_ref() != Some(&drawing) {
                if !draw(self.hwnd, dc, &drawing) {
                    return;
                }
                self.last = Some(drawing);
            }

            let (x, y, after) = if let Some(target) = target {
                let mut area = RECT::default();
                let mut origin = POINT::default();
                if GetClientRect(target, &mut area) == 0
                    || windows_sys::Win32::Graphics::Gdi::ClientToScreen(target, &mut origin) == 0
                {
                    ShowWindow(self.hwnd, SW_HIDE);
                    return;
                }
                let right = matches!(style.anchor, Anchor::TopRight | Anchor::BottomRight);
                let bottom = matches!(style.anchor, Anchor::BottomLeft | Anchor::BottomRight);
                let x = origin.x
                    + if right {
                        area.right - width - style.offset_x
                    } else {
                        style.offset_x
                    };
                let y = origin.y
                    + if bottom {
                        area.bottom - height - style.offset_y
                    } else {
                        style.offset_y
                    };
                let point = POINT {
                    x: x + width.min(32) / 2,
                    y: y + height.min(20) / 2,
                };
                let covering = windows_sys::Win32::UI::WindowsAndMessaging::WindowFromPoint(point);
                if covering.is_null() || GetAncestor(covering, GA_ROOT) != target {
                    ShowWindow(self.hwnd, SW_HIDE);
                    return;
                }
                let mut after = GetWindow(target, GW_HWNDPREV);
                if after == self.hwnd {
                    after = GetWindow(self.hwnd, GW_HWNDPREV);
                }
                if after.is_null() {
                    after = HWND_TOP;
                } else if GetWindowLongPtrW(target, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST == 0
                    && GetWindowLongPtrW(after, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0
                {
                    after = HWND_NOTOPMOST;
                }
                (x, y, after)
            } else {
                (style.global_x, style.global_y, HWND_TOPMOST)
            };
            let position = (x, y, width, height, after);
            if self.position != Some(position)
                || IsWindowVisible(self.hwnd) == 0
                || target.is_some_and(|target| GetWindow(target, GW_HWNDPREV) != self.hwnd)
            {
                if SetWindowPos(self.hwnd, after, x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW) != 0 {
                    self.position = Some(position);
                }
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.hwnd) };
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_NCHITTEST if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 2 => HTCAPTION as LRESULT,
        WM_ENTERSIZEMOVE if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 2 => {
            DRAGGING_GLOBAL.store(true, Ordering::Release);
            0
        }
        WM_EXITSIZEMOVE if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 2 => {
            DRAGGING_GLOBAL.store(false, Ordering::Release);
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(hwnd, &mut rect) } != 0 {
                let (x, y) = (rect.left, rect.top);
                if let Ok(mut frame) = FRAME.lock()
                    && let Some((settings, _)) = frame.as_mut()
                {
                    settings.overlay.global_x = x;
                    settings.overlay.global_y = y;
                }
                tauri::async_runtime::spawn(super::save_overlay_position(x, y));
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn draw(hwnd: HWND, dc: HDC, data: &Drawing) -> bool {
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: data.width,
                biHeight: -data.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut memory: *mut c_void = ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut memory, ptr::null_mut(), 0);
        if bitmap.is_null() || memory.is_null() {
            return false;
        }
        let old_bitmap = SelectObject(dc, bitmap);
        let background = data.background & 0xffffff;
        let count = (data.width * data.height) as usize;
        std::slice::from_raw_parts_mut(memory.cast::<u32>(), count).fill(background);
        let font = CreateFontW(-data.font, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 4, 0, ptr::null());
        let old_font = SelectObject(dc, font);
        let rgb = data.foreground;
        SetTextColor(dc, ((rgb >> 16) & 0xff) | (rgb & 0xff00) | ((rgb & 0xff) << 16));
        SetBkMode(dc, TRANSPARENT as i32);
        for (i, line) in data.lines.iter().enumerate() {
            let mut text: Vec<u16> = line.encode_utf16().collect();
            let y = 7 + (data.font + 5) * i as i32;
            let mut rect = RECT {
                left: 8,
                top: y,
                right: data.width - 8,
                bottom: y + data.font + 5,
            };
            DrawTextW(
                dc,
                text.as_mut_ptr(),
                text.len() as i32,
                &mut rect,
                DT_SINGLELINE | DT_NOPREFIX,
            );
        }
        GdiFlush();
        for pixel in std::slice::from_raw_parts_mut(memory.cast::<u32>(), count) {
            let rgb = *pixel & 0xffffff;
            let alpha = if rgb == background {
                u32::from(data.opacity)
            } else {
                255
            };
            let red = ((rgb >> 16) & 255) * alpha / 255;
            let green = ((rgb >> 8) & 255) * alpha / 255;
            let blue = (rgb & 255) * alpha / 255;
            *pixel = (alpha << 24) | (red << 16) | (green << 8) | blue;
        }
        let size = SIZE {
            cx: data.width,
            cy: data.height,
        };
        let src = POINT::default();
        let blend = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let ok = UpdateLayeredWindow(
            hwnd,
            ptr::null_mut(),
            ptr::null(),
            &size,
            dc,
            &src,
            0,
            &blend,
            ULW_ALPHA,
        ) != 0;
        SelectObject(dc, old_font);
        SelectObject(dc, old_bitmap);
        DeleteObject(font);
        DeleteObject(bitmap);
        ok
    }
}

fn format_bytes(bytes: u64) -> String {
    for (div, name) in [(1u64 << 30, "GiB"), (1 << 20, "MiB"), (1 << 10, "KiB")] {
        if bytes >= div {
            return format!("{:.1} {name}", bytes as f64 / div as f64);
        }
    }
    format!("{bytes} B")
}

fn lines(name: &str, style: &Overlay, stats: &RuleStats, snapshot: &Snapshot) -> Vec<String> {
    let delay = match style.delay {
        Delay::Node => snapshot.node_delay.map(u64::from),
        Delay::Connection => stats.connection_delay,
    };
    let mut lines = vec![if style.show_delay {
        format!(
            "{name}    {}",
            delay.map_or_else(|| "N/A".into(), |value| format!("{value} ms"))
        )
    } else {
        name.into()
    }];
    if style.show_rates {
        lines.push(format!(
            "↑ {}/s    ↓ {}/s",
            format_bytes(stats.upload_rate),
            format_bytes(stats.download_rate)
        ));
    }
    if style.show_totals {
        lines.push(format!(
            "↑ {}     ↓ {}",
            format_bytes(stats.upload_total),
            format_bytes(stats.download_total)
        ));
    }
    lines
}

unsafe extern "system" fn list_window(hwnd: HWND, arg: LPARAM) -> i32 {
    let windows = unsafe { &mut *(arg as *mut Vec<HWND>) };
    if unsafe { IsWindowVisible(hwnd) } != 0
        && unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOOLWINDOW == 0
        && unsafe { GetWindow(hwnd, GW_OWNER) }.is_null()
    {
        windows.push(hwnd);
    }
    1
}

fn run() {
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: GetModuleHandleW(ptr::null()),
            lpszClassName: CLASS.as_ptr(),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return;
        }
        let dc = CreateCompatibleDC(ptr::null_mut());
        if dc.is_null() {
            return;
        }
        let mut message = MSG::default();
        PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_NOREMOVE);
        THREAD_ID.store(GetCurrentThreadId(), Ordering::Release);
        let objects = SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_LOCATIONCHANGE,
            ptr::null_mut(),
            Some(changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        let foreground = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            ptr::null_mut(),
            Some(changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        wake();
        let mut windows: BTreeMap<usize, (Window, u32, u32)> = BTreeMap::new();
        let mut global: Option<Window> = None;
        let mut last_discovery: Option<Instant> = None;
        let mut last_rules = Vec::new();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            if message.message != UPDATE {
                TranslateMessage(&message);
                DispatchMessageW(&message);
                continue;
            }
            QUEUED.store(false, Ordering::Release);
            let Some((settings, snapshot)) = FRAME.lock().ok().and_then(|frame| frame.clone()) else {
                continue;
            };
            let style = &settings.overlay;
            let rules: Vec<_> = settings
                .rules
                .iter()
                .filter(|rule| rule.enabled && rule.action == Action::Proxy)
                .collect();
            if style.global {
                let mut panel = vec!["Application Network Monitor".into()];
                let mut total = RuleStats::default();
                for rule in &rules {
                    let stats = snapshot.rules.get(&rule.id).cloned().unwrap_or_default();
                    total.upload_rate += stats.upload_rate;
                    total.download_rate += stats.download_rate;
                    total.upload_total += stats.upload_total;
                    total.download_total += stats.download_total;
                    if !style.hide_idle || stats.upload_rate != 0 || stats.download_rate != 0 || stats.active != 0 {
                        panel.extend(lines(&rule.name, style, &stats, &snapshot));
                    }
                }
                panel.extend(lines("Total", style, &total, &snapshot));
                if global.is_none() {
                    global = Window::new(true);
                }
                if let Some(window) = &mut global {
                    window.show(dc, panel, style, None);
                }
            } else {
                global = None;
            }

            if !style.windows || !snapshot.running {
                windows.clear();
                last_discovery = None;
                continue;
            }
            let changed = WINDOWS_DIRTY.swap(false, Ordering::AcqRel);
            if last_rules != settings.rules
                || last_discovery
                    .is_none_or(|instant| instant.elapsed() >= Duration::from_secs(if changed { 1 } else { 4 }))
            {
                last_rules.clone_from(&settings.rules);
                last_discovery = Some(Instant::now());
                let filters: Vec<_> = settings
                    .rules
                    .iter()
                    .filter(|rule| rule.enabled)
                    .filter_map(|rule| {
                        CString::new(rule.pattern.as_bytes())
                            .ok()
                            .map(|pattern| (rule, pattern))
                    })
                    .collect();
                let mut candidates: Vec<HWND> = Vec::new();
                EnumWindows(Some(list_window), (&mut candidates as *mut Vec<HWND>) as LPARAM);
                let mut seen = std::collections::BTreeSet::new();
                let mut images = BTreeMap::new();
                for hwnd in candidates {
                    let mut pid = 0;
                    GetWindowThreadProcessId(hwnd, &mut pid);
                    if pid == std::process::id() {
                        continue;
                    }
                    let image = images
                        .entry(pid)
                        .or_insert_with(|| process::image(pid).and_then(|image| CString::new(image).ok()));
                    let Some(image) = image else {
                        continue;
                    };
                    let Some((rule, _)) = filters.iter().find(|(_, filter)| engine::matches(filter, image)) else {
                        continue;
                    };
                    if rule.action != Action::Proxy || !rule.overlay {
                        continue;
                    }
                    let key = hwnd as usize;
                    seen.insert(key);
                    if windows.get(&key).is_some_and(|(_, old, _)| *old != pid) {
                        windows.remove(&key);
                    }
                    if let Some(window) = windows.get_mut(&key) {
                        window.2 = rule.id;
                    } else if let Some(window) = Window::new(false) {
                        windows.insert(key, (window, pid, rule.id));
                    }
                }
                windows.retain(|key, _| seen.contains(key));
            }
            windows.retain(|key, (window, pid, id)| {
                let hwnd = *key as HWND;
                let mut actual_pid = 0;
                GetWindowThreadProcessId(hwnd, &mut actual_pid);
                if actual_pid != *pid {
                    return false;
                }
                let mut cloaked = 0u32;
                DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, (&mut cloaked as *mut u32).cast(), 4);
                if cloaked != 0 || IsIconic(hwnd) != 0 || IsWindowVisible(hwnd) == 0 {
                    ShowWindow(window.hwnd, SW_HIDE);
                    return true;
                }
                let Some(rule) = rules.iter().find(|rule| rule.id == *id) else {
                    return false;
                };
                let stats = snapshot.rules.get(id).cloned().unwrap_or_default();
                let name = snapshot
                    .node
                    .as_ref()
                    .map_or_else(|| rule.name.clone(), |node| format!("{} / {node}", rule.name));
                window.show(dc, lines(&name, style, &stats, &snapshot), style, Some(hwnd));
                true
            });
        }
        if !objects.is_null() {
            UnhookWinEvent(objects);
        }
        if !foreground.is_null() {
            UnhookWinEvent(foreground);
        }
        DeleteDC(dc);
        THREAD_ID.store(0, Ordering::Release);
    }
}
