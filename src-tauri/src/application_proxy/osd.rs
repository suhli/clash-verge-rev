use super::{
    engine,
    model::{Action, Anchor, Delay, Overlay, RuleStats, Settings, Snapshot},
    process,
};
use clash_verge_logging::{Type, logging};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CString, c_void},
    mem::size_of,
    ptr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicIsize, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::RECT as WinRect,
        Graphics::{
            Direct2D::{
                Common::{D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT},
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_FACTORY_OPTIONS,
                D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
                D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
                D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory,
                ID2D1SolidColorBrush,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP,
                DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout, IDWriteTypography,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::HDC as WinHdc,
        },
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
    },
    core::{HSTRING, PCWSTR},
};
use windows_sys::Win32::{
    Foundation::{ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
    Graphics::{
        Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
        Gdi::{
            AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, ClientToScreen,
            CreateCompatibleDC, CreateDIBSection, CreateFontW, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC,
            GetMonitorInfoW, GetTextFaceW, HBITMAP, HDC, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO,
            MonitorFromPoint, MonitorFromWindow, ReleaseDC, SelectObject,
        },
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetThreadDpiAwarenessContext},
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, EnumChildWindows, EnumWindows,
            GW_HWNDPREV, GW_OWNER, GWL_EXSTYLE, GWLP_USERDATA, GetClientRect, GetForegroundWindow, GetMessageW,
            GetSystemMetrics, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, HTCAPTION,
            HTTRANSPARENT, HWND_MESSAGE, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, IDC_SIZEALL, IsIconic, IsWindow,
            IsWindowVisible, KillTimer, LoadCursorW, MA_NOACTIVATE, MSG, OBJID_WINDOW, PostMessageW, RegisterClassW,
            SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_SHOWWINDOW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
            SystemParametersInfoW, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WINEVENT_OUTOFCONTEXT,
            WINEVENT_SKIPOWNPROCESS, WM_APP, WM_ENTERSIZEMOVE, WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_MOUSEACTIVATE,
            WM_NCHITTEST, WM_TIMER, WNDCLASSW, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
            WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
        },
    },
};

const UPDATE: u32 = WM_APP + 1;
const BREATHE_TIMER_ID: usize = 1;
const BREATHE_INTERVAL_MS: u32 = 125;
const EDGE_SNAP_PX: i32 = 16;
const MAX_GLOBAL_ROWS: usize = 8;

const COLOR_OK: u32 = 0x10B981;
const COLOR_WARN: u32 = 0xF59E0B;
const COLOR_ERR: u32 = 0xEF4444;
const COLOR_DL: u32 = 0x38BDF8;
const COLOR_UL: u32 = 0xA78BFA;

// UTF-16 null-terminated window class names.
const OSD_CLASS: &[u16] = &[67, 86, 82, 65, 112, 112, 79, 83, 68, 0]; // "CVRAppOSD"
const CTRL_CLASS: &[u16] = &[67, 86, 82, 65, 112, 112, 79, 83, 68, 67, 116, 114, 108, 0]; // "CVRAppOSDCtrl"

static FRAME: Mutex<Option<(Settings, Snapshot)>> = Mutex::new(None);
static CTRL_HWND: AtomicIsize = AtomicIsize::new(0);
static QUEUED: AtomicBool = AtomicBool::new(false);
static WINDOWS_DIRTY: AtomicBool = AtomicBool::new(true);
static FOREGROUND_DIRTY: AtomicBool = AtomicBool::new(true);
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
            if let Err(error) = std::thread::Builder::new()
                .name("application-proxy-osd".into())
                .spawn(run)
            {
                logging!(warn, Type::System, "Failed to spawn OSD thread: {error}");
            }
        });
    }
    wake();
}

fn wake() {
    let hwnd = CTRL_HWND.load(Ordering::Acquire) as HWND;
    if !hwnd.is_null() && !QUEUED.swap(true, Ordering::AcqRel) && unsafe { PostMessageW(hwnd, UPDATE, 0, 0) } == 0 {
        QUEUED.store(false, Ordering::Release);
    }
}

unsafe extern "system" fn win_event_callback(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if hwnd.is_null() {
        return;
    }
    if event == EVENT_SYSTEM_FOREGROUND {
        FOREGROUND_DIRTY.store(true, Ordering::Release);
        WINDOWS_DIRTY.store(true, Ordering::Release);
        wake();
        return;
    }
    let relevant = matches!(event, EVENT_SYSTEM_MINIMIZESTART | EVENT_SYSTEM_MINIMIZEEND)
        || (event == EVENT_OBJECT_LOCATIONCHANGE && object == OBJID_WINDOW && child == 0);
    if relevant {
        WINDOWS_DIRTY.store(true, Ordering::Release);
        wake();
    }
}

const fn is_default_foreground(rgb: u32) -> bool {
    let c = rgb & 0x00FF_FFFF;
    c == 0x00F4_F4F5 || c == 0x00F3_F6FA
}

const fn is_light_rgb(rgb: u32) -> bool {
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    r * 299 + g * 587 + b * 114 > 150_000
}

const fn status_color(running: bool, endpoint_ready: bool, delay: Option<u64>) -> u32 {
    if !running || !endpoint_ready {
        return COLOR_ERR;
    }
    match delay {
        Some(ms) if ms > 150 => COLOR_ERR,
        Some(ms) if ms >= 60 => COLOR_WARN,
        _ => COLOR_OK,
    }
}

const fn rgb_to_d2d(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
        g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
        b: (rgb & 0xFF) as f32 / 255.0,
        a: alpha,
    }
}

fn format_speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec < 1024 {
        format!("{bytes_per_sec} B/s")
    } else if bytes_per_sec < 1024 * 1024 {
        let kb = bytes_per_sec as f64 / 1024.0;
        if kb >= 100.0 {
            format!("{kb:.0} KB/s")
        } else {
            format!("{kb:.1} KB/s")
        }
    } else if bytes_per_sec < 1024 * 1024 * 1024 {
        let mb = bytes_per_sec as f64 / (1024.0 * 1024.0);
        if mb >= 100.0 {
            format!("{mb:.0} MB/s")
        } else {
            format!("{mb:.1} MB/s")
        }
    } else {
        let gb = bytes_per_sec as f64 / (1024.0 * 1024.0 * 1024.0);
        format!("{gb:.2} GB/s")
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn is_font_installed(family: &str) -> bool {
    unsafe {
        let dc = GetDC(ptr::null_mut());
        if dc.is_null() {
            return false;
        }
        let wide: Vec<u16> = family.encode_utf16().chain([0]).collect();
        let font = CreateFontW(-12, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, wide.as_ptr());
        let mut found = false;
        if !font.is_null() {
            let old = SelectObject(dc, font);
            let mut actual = [0u16; 64];
            let len = GetTextFaceW(dc, actual.len() as i32, actual.as_mut_ptr());
            if len > 0 {
                let actual_name = String::from_utf16_lossy(&actual[..(len as usize - 1).min(actual.len())]);
                found = actual_name.eq_ignore_ascii_case(family);
            }
            SelectObject(dc, old);
            DeleteObject(font);
        }
        ReleaseDC(ptr::null_mut(), dc);
        found
    }
}

struct Surface {
    mem_dc: HDC,
    dib: HBITMAP,
    old_bmp: HGDIOBJ,
    width: i32,
    height: i32,
}

impl Surface {
    const fn new() -> Self {
        Self {
            mem_dc: ptr::null_mut(),
            dib: ptr::null_mut(),
            old_bmp: ptr::null_mut(),
            width: 0,
            height: 0,
        }
    }

    fn ensure_size(&mut self, width: i32, height: i32) -> bool {
        let width = width.max(1);
        let height = height.max(1);
        if !self.dib.is_null() && self.width == width && self.height == height {
            return true;
        }
        unsafe {
            if !self.mem_dc.is_null() && !self.old_bmp.is_null() {
                SelectObject(self.mem_dc, self.old_bmp);
                self.old_bmp = ptr::null_mut();
            }
            if !self.dib.is_null() {
                DeleteObject(self.dib as HGDIOBJ);
                self.dib = ptr::null_mut();
            }
            if self.mem_dc.is_null() {
                let screen = GetDC(ptr::null_mut());
                self.mem_dc = CreateCompatibleDC(screen);
                ReleaseDC(ptr::null_mut(), screen);
                if self.mem_dc.is_null() {
                    return false;
                }
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = ptr::null_mut();
            self.dib = CreateDIBSection(self.mem_dc, &info, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0);
            if self.dib.is_null() || bits.is_null() {
                return false;
            }
            self.old_bmp = SelectObject(self.mem_dc, self.dib as HGDIOBJ);
            self.width = width;
            self.height = height;
            true
        }
    }

    fn present(&self, hwnd: HWND, x: i32, y: i32) -> bool {
        unsafe {
            let screen = GetDC(ptr::null_mut());
            let dst = POINT { x, y };
            let src = POINT { x: 0, y: 0 };
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let ok = UpdateLayeredWindow(hwnd, screen, &dst, &size, self.mem_dc, &src, 0, &blend, ULW_ALPHA) != 0;
            ReleaseDC(ptr::null_mut(), screen);
            ok
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            if !self.mem_dc.is_null() && !self.old_bmp.is_null() {
                SelectObject(self.mem_dc, self.old_bmp);
            }
            if !self.dib.is_null() {
                DeleteObject(self.dib as HGDIOBJ);
            }
            if !self.mem_dc.is_null() {
                DeleteDC(self.mem_dc);
            }
        }
    }
}

struct D2dRenderer {
    d2d_factory: ID2D1Factory,
    dw_factory: IDWriteFactory,
    tabular: Option<IDWriteTypography>,
    dc_rt: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    mono_family: HSTRING,
    ui_family: HSTRING,
    formats: BTreeMap<(bool, i32, u32), IDWriteTextFormat>,
}

impl D2dRenderer {
    fn new() -> Option<Self> {
        let options = D2D1_FACTORY_OPTIONS::default();
        let d2d_factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&options)) }.ok()?;
        let dw_factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.ok()?;
        let tabular = unsafe { dw_factory.CreateTypography() }.ok().inspect(|typo| {
            let feature = DWRITE_FONT_FEATURE {
                nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                parameter: 1,
            };
            let _ = unsafe { typo.AddFontFeature(feature) };
        });
        let (dc_rt, brush) = Self::create_target(&d2d_factory)?;
        let mono_family = HSTRING::from(if is_font_installed("Cascadia Mono") {
            "Cascadia Mono"
        } else {
            "Consolas"
        });
        let ui_family = HSTRING::from(if is_font_installed("Segoe UI Variable Text") {
            "Segoe UI Variable Text"
        } else {
            "Segoe UI"
        });
        Some(Self {
            d2d_factory,
            dw_factory,
            tabular,
            dc_rt,
            brush,
            mono_family,
            ui_family,
            formats: BTreeMap::new(),
        })
    }

    fn create_target(factory: &ID2D1Factory) -> Option<(ID2D1DCRenderTarget, ID2D1SolidColorBrush)> {
        let props = D2D1_RENDER_TARGET_PROPERTIES {
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
        let rt = unsafe { factory.CreateDCRenderTarget(&props) }.ok()?;
        unsafe {
            rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        }
        let white = D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        };
        let brush = unsafe { rt.CreateSolidColorBrush(&white, None) }.ok()?;
        Some((rt, brush))
    }

    fn recreate_target(&mut self) -> bool {
        if let Some((rt, brush)) = Self::create_target(&self.d2d_factory) {
            self.dc_rt = rt;
            self.brush = brush;
            true
        } else {
            false
        }
    }

    fn text_format(&mut self, mono: bool, weight: DWRITE_FONT_WEIGHT, size_px: f32) -> Option<IDWriteTextFormat> {
        let key = (mono, weight.0, (size_px * 10.0).round() as u32);
        if let Some(cached) = self.formats.get(&key) {
            return Some(cached.clone());
        }
        if self.formats.len() > 64 {
            self.formats.clear();
        }
        let family = if mono { &self.mono_family } else { &self.ui_family };
        let locale = HSTRING::new();
        let format = unsafe {
            self.dw_factory.CreateTextFormat(
                PCWSTR(family.as_ptr()),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size_px,
                PCWSTR(locale.as_ptr()),
            )
        }
        .ok()?;
        unsafe {
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        }
        self.formats.insert(key, format.clone());
        Some(format)
    }

    #[allow(clippy::too_many_arguments)]
    fn create_layout(
        &mut self,
        text: &str,
        mono: bool,
        weight: DWRITE_FONT_WEIGHT,
        size_px: f32,
        max_w: f32,
        max_h: f32,
        align: DWRITE_TEXT_ALIGNMENT,
        tabular: bool,
    ) -> Option<IDWriteTextLayout> {
        let format = self.text_format(mono, weight, size_px)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        let layout = unsafe {
            self.dw_factory
                .CreateTextLayout(&wide, &format, max_w.max(1.0), max_h.max(1.0))
        }
        .ok()?;
        unsafe {
            let _ = layout.SetTextAlignment(align);
            let _ = layout.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            if tabular
                && let Some(typo) = &self.tabular
                && !wide.is_empty()
            {
                let _ = layout.SetTypography(
                    typo,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: wide.len() as u32,
                    },
                );
            }
        }
        Some(layout)
    }

    fn measure_text(&mut self, text: &str, mono: bool, weight: DWRITE_FONT_WEIGHT, size_px: f32, tabular: bool) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let Some(layout) = self.create_layout(
            text,
            mono,
            weight,
            size_px,
            2000.0,
            64.0,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            tabular,
        ) else {
            return text.chars().count() as f32 * size_px * 0.62;
        };
        let mut metrics = DWRITE_TEXT_METRICS::default();
        if unsafe { layout.GetMetrics(&mut metrics) }.is_ok() {
            metrics.widthIncludingTrailingWhitespace
        } else {
            text.chars().count() as f32 * size_px * 0.62
        }
    }

    fn begin(&self, surface: &Surface) -> bool {
        let rc = WinRect {
            left: 0,
            top: 0,
            right: surface.width,
            bottom: surface.height,
        };
        let hdc = WinHdc(surface.mem_dc);
        if unsafe { self.dc_rt.BindDC(hdc, &rc) }.is_err() {
            return false;
        }
        unsafe {
            self.dc_rt.BeginDraw();
            self.dc_rt.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
        }
        true
    }

    fn end(&mut self) -> bool {
        let hr = unsafe { self.dc_rt.EndDraw(None, None) };
        if hr.is_err() {
            self.recreate_target();
            return false;
        }
        true
    }

    fn set_color(&self, color: D2D1_COLOR_F) {
        unsafe { self.brush.SetColor(&color) };
    }

    fn fill_rounded_rect(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: D2D1_COLOR_F) {
        self.set_color(color);
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.dc_rt.FillRoundedRectangle(&rr, &self.brush) };
    }

    #[allow(clippy::too_many_arguments)]
    fn stroke_rounded_rect(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, stroke: f32, color: D2D1_COLOR_F) {
        self.set_color(color);
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.dc_rt.DrawRoundedRectangle(&rr, &self.brush, stroke, None) };
    }

    fn fill_circle(&self, cx: f32, cy: f32, radius: f32, color: D2D1_COLOR_F) {
        self.set_color(color);
        let mut ellipse = D2D1_ELLIPSE {
            radiusX: radius,
            radiusY: radius,
            ..Default::default()
        };
        ellipse.point.X = cx;
        ellipse.point.Y = cy;
        unsafe { self.dc_rt.FillEllipse(&ellipse, &self.brush) };
    }

    fn draw_line(&self, x0: f32, y0: f32, x1: f32, y1: f32, stroke: f32, color: D2D1_COLOR_F) {
        self.set_color(color);
        let mut p0 = D2D1_ELLIPSE::default().point;
        p0.X = x0;
        p0.Y = y0;
        let mut p1 = D2D1_ELLIPSE::default().point;
        p1.X = x1;
        p1.Y = y1;
        unsafe {
            self.dc_rt.DrawLine(p0, p1, &self.brush, stroke, None);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &mut self,
        text: &str,
        mono: bool,
        weight: DWRITE_FONT_WEIGHT,
        size_px: f32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: D2D1_COLOR_F,
        align: DWRITE_TEXT_ALIGNMENT,
        tabular: bool,
    ) {
        if text.is_empty() {
            return;
        }
        let Some(layout) = self.create_layout(text, mono, weight, size_px, w, h, align, tabular) else {
            return;
        };
        self.set_color(color);
        let mut origin = D2D1_ELLIPSE::default().point;
        origin.X = x;
        origin.Y = y;
        unsafe {
            self.dc_rt
                .DrawTextLayout(origin, &layout, &self.brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
        }
    }
}

#[derive(Clone, PartialEq)]
struct Segment {
    text: String,
    color: u32,
    alpha: f32,
    mono: bool,
    bold: bool,
    min_em: f32,
}

#[derive(Clone, PartialEq)]
struct AppRowData {
    name: String,
    down: Option<String>,
    up: Option<String>,
    total: Option<String>,
    delay: Option<String>,
}

#[derive(Clone, PartialEq)]
enum Scene {
    Pill {
        dot_color: u32,
        breathe_phase: Option<u32>,
        segments: Vec<Segment>,
        font_px: i32,
        background: u32,
        opacity: u8,
    },
    Card {
        dot_color: u32,
        breathe_phase: Option<u32>,
        header: Vec<Segment>,
        rows: Vec<AppRowData>,
        font_px: i32,
        foreground: u32,
        background: u32,
        opacity: u8,
    },
}

struct Window {
    hwnd: HWND,
    surface: Surface,
    last_scene: Option<Scene>,
    global: bool,
    topmost: bool,
    width: i32,
    height: i32,
    visible: bool,
    last_pos: Option<(i32, i32, HWND)>,
}

impl Window {
    fn new(global: bool) -> Option<Self> {
        unsafe {
            let mut ex_style = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT;
            if global {
                ex_style |= WS_EX_TOPMOST;
            }
            let hwnd = CreateWindowExW(
                ex_style,
                OSD_CLASS.as_ptr(),
                OSD_CLASS.as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                ptr::null_mut(),
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            );
            if hwnd.is_null() {
                logging!(
                    warn,
                    Type::System,
                    "Failed to create OSD window (global={global}): err={}",
                    GetLastError()
                );
                return None;
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, isize::from(global));
            Some(Self {
                hwnd,
                surface: Surface::new(),
                last_scene: None,
                global,
                topmost: global,
                width: 0,
                height: 0,
                visible: false,
                last_pos: None,
            })
        }
    }

    fn hide(&mut self) {
        if self.visible {
            unsafe { ShowWindow(self.hwnd, SW_HIDE) };
            self.visible = false;
            self.last_pos = None;
        }
    }

    fn update_interactivity(&self, style: &Overlay) {
        unsafe {
            let prev_flags = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
            let interactive_global = self.global && !style.click_through && !style.locked;
            let next_flags = if interactive_global {
                prev_flags & !WS_EX_TRANSPARENT
            } else {
                prev_flags | WS_EX_TRANSPARENT
            };
            if next_flags != prev_flags {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, next_flags as isize);
            }
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, if interactive_global { 2 } else { 0 });
        }
    }

    fn render_and_show(
        &mut self,
        renderer: &mut D2dRenderer,
        scene: Scene,
        style: &Overlay,
        target: Option<HWND>,
        own_pid: u32,
    ) {
        if self.global && DRAGGING_GLOBAL.load(Ordering::Acquire) {
            return;
        }
        self.update_interactivity(style);

        let needs_redraw = self.last_scene.as_ref() != Some(&scene) || self.width <= 0 || self.height <= 0;
        let (width, height) = if needs_redraw {
            let Some((w, h)) = render_scene(renderer, &mut self.surface, &scene) else {
                return;
            };
            self.width = w;
            self.height = h;
            (w, h)
        } else {
            (self.width, self.height)
        };

        let (x, y, after) = if let Some(target) = target {
            let Some((x, y)) = compute_target_position(target, style, width, height) else {
                self.hide();
                return;
            };
            let osd_rect = RECT {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            if is_target_occluded(target, &osd_rect, own_pid) {
                self.hide();
                return;
            }
            let after = resolve_target_zorder(target, self.hwnd, own_pid);
            (x, y, after)
        } else {
            let (x, y) = clamp_to_work_area(style.global_x, style.global_y, width, height, false);
            (x, y, HWND_TOPMOST)
        };

        let moved = self.last_pos.is_none_or(|(px, py, _)| px != x || py != y);
        if needs_redraw || moved || !self.visible {
            if !self.surface.present(self.hwnd, x, y) {
                logging!(warn, Type::System, "UpdateLayeredWindow failed: err={}", unsafe {
                    GetLastError()
                });
                return;
            }
            self.last_scene = Some(scene);
        }

        unsafe {
            let need_zorder = !self.visible
                || self.last_pos != Some((x, y, after))
                || target.is_some_and(|t| after != HWND_TOPMOST && GetWindow(t, GW_HWNDPREV) != self.hwnd);
            if !self.visible {
                ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                self.visible = true;
            }
            if need_zorder
                && SetWindowPos(
                    self.hwnd,
                    after,
                    x,
                    y,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
                ) != 0
            {
                self.last_pos = Some((x, y, after));
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.hwnd) };
    }
}

fn render_scene(renderer: &mut D2dRenderer, surface: &mut Surface, scene: &Scene) -> Option<(i32, i32)> {
    match scene {
        Scene::Pill {
            dot_color,
            breathe_phase,
            segments,
            font_px,
            background,
            opacity,
        } => render_pill(
            renderer,
            surface,
            *dot_color,
            *breathe_phase,
            segments,
            *font_px as f32,
            *background,
            *opacity,
        ),
        Scene::Card {
            dot_color,
            breathe_phase,
            header,
            rows,
            font_px,
            foreground,
            background,
            opacity,
        } => render_card(
            renderer,
            surface,
            *dot_color,
            *breathe_phase,
            header,
            rows,
            *font_px as f32,
            *foreground,
            *background,
            *opacity,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_pill(
    renderer: &mut D2dRenderer,
    surface: &mut Surface,
    dot_color: u32,
    breathe_phase: Option<u32>,
    segments: &[Segment],
    font_px: f32,
    background: u32,
    opacity: u8,
) -> Option<(i32, i32)> {
    let scale = (font_px / 11.5).clamp(0.7, 3.2);
    let height = (font_px * 1.95).clamp(22.0, 68.0).ceil();
    let pad_l = 10.0 * scale;
    let pad_r = 11.0 * scale;
    let dot_r = (3.6 * scale).clamp(3.0, 8.0);
    let dot_gap = 7.0 * scale;
    let seg_gap = 7.5 * scale;

    let mut widths = Vec::with_capacity(segments.len());
    let mut content_w = pad_l + dot_r * 2.0 + dot_gap;
    for (idx, seg) in segments.iter().enumerate() {
        let weight = if seg.bold {
            DWRITE_FONT_WEIGHT_SEMI_BOLD
        } else {
            DWRITE_FONT_WEIGHT_MEDIUM
        };
        let measured = renderer.measure_text(&seg.text, seg.mono, weight, font_px, seg.mono);
        let w = measured.max(seg.min_em * font_px);
        widths.push(w);
        content_w += w;
        if idx + 1 < segments.len() {
            content_w += seg_gap;
        }
    }
    let width = (content_w + pad_r).ceil().clamp(92.0, 900.0);
    let w_i32 = width as i32;
    let h_i32 = height as i32;

    if !surface.ensure_size(w_i32, h_i32) || !renderer.begin(surface) {
        return None;
    }

    let bg_alpha = f32::from(opacity) / 255.0;
    let light_bg = is_light_rgb(background);
    let radius = height * 0.5;
    if opacity > 0 {
        renderer.fill_rounded_rect(
            0.5,
            0.5,
            width - 1.0,
            height - 1.0,
            radius,
            rgb_to_d2d(background, bg_alpha),
        );
        let border = if light_bg {
            rgb_to_d2d(0x000000, 0.14 * bg_alpha.max(0.35))
        } else {
            rgb_to_d2d(0xFFFFFF, 0.14 * bg_alpha.max(0.35))
        };
        renderer.stroke_rounded_rect(0.5, 0.5, width - 1.0, height - 1.0, radius, 1.0, border);
    }

    let cx = pad_l + dot_r;
    let cy = height * 0.5;
    draw_status_dot(renderer, cx, cy, dot_r, dot_color, breathe_phase);

    let mut x = pad_l + dot_r * 2.0 + dot_gap;
    for (seg, w) in segments.iter().zip(&widths) {
        let weight = if seg.bold {
            DWRITE_FONT_WEIGHT_SEMI_BOLD
        } else {
            DWRITE_FONT_WEIGHT_MEDIUM
        };
        let align = if seg.text == "|" {
            DWRITE_TEXT_ALIGNMENT_CENTER
        } else {
            DWRITE_TEXT_ALIGNMENT_LEADING
        };
        renderer.draw_text(
            &seg.text,
            seg.mono,
            weight,
            font_px,
            x,
            0.0,
            *w + 2.0,
            height,
            rgb_to_d2d(seg.color, seg.alpha),
            align,
            seg.mono,
        );
        x += *w + seg_gap;
    }

    renderer.end().then_some((w_i32, h_i32))
}

#[allow(clippy::too_many_arguments)]
fn render_card(
    renderer: &mut D2dRenderer,
    surface: &mut Surface,
    dot_color: u32,
    breathe_phase: Option<u32>,
    header: &[Segment],
    rows: &[AppRowData],
    font_px: f32,
    foreground: u32,
    background: u32,
    opacity: u8,
) -> Option<(i32, i32)> {
    let scale = (font_px / 11.5).clamp(0.7, 3.2);
    let pad_x = 12.0 * scale;
    let pad_y = 8.0 * scale;
    let header_h = (font_px * 1.9).clamp(22.0, 56.0).ceil();
    let row_font = (font_px * 0.94).max(9.0);
    let row_h = (row_font * 1.65).clamp(18.0, 44.0).ceil();
    let dot_r = (3.6 * scale).clamp(3.0, 8.0);
    let seg_gap = 7.5 * scale;
    let col_gap = 10.0 * scale;

    let mut header_widths = Vec::with_capacity(header.len());
    let mut header_w = pad_x * 2.0 + dot_r * 2.0 + 7.0 * scale;
    for (idx, seg) in header.iter().enumerate() {
        let weight = if seg.bold {
            DWRITE_FONT_WEIGHT_SEMI_BOLD
        } else {
            DWRITE_FONT_WEIGHT_MEDIUM
        };
        let w = renderer
            .measure_text(&seg.text, seg.mono, weight, font_px, seg.mono)
            .max(seg.min_em * font_px);
        header_widths.push(w);
        header_w += w;
        if idx + 1 < header.len() {
            header_w += seg_gap;
        }
    }

    let mut name_col = 6.5 * row_font;
    let mut down_col = 0.0_f32;
    let mut up_col = 0.0_f32;
    let mut total_col = 0.0_f32;
    let mut delay_col = 0.0_f32;
    for row in rows {
        name_col = name_col.max(
            renderer
                .measure_text(&row.name, false, DWRITE_FONT_WEIGHT_NORMAL, row_font, false)
                .min(13.0 * row_font),
        );
        if let Some(d) = &row.down {
            down_col = down_col
                .max(renderer.measure_text(d, true, DWRITE_FONT_WEIGHT_MEDIUM, row_font, true))
                .max(6.2 * row_font);
        }
        if let Some(u) = &row.up {
            up_col = up_col
                .max(renderer.measure_text(u, true, DWRITE_FONT_WEIGHT_MEDIUM, row_font, true))
                .max(6.2 * row_font);
        }
        if let Some(tot) = &row.total {
            total_col = total_col
                .max(renderer.measure_text(tot, true, DWRITE_FONT_WEIGHT_NORMAL, row_font, true))
                .max(5.5 * row_font);
        }
        if let Some(ms) = &row.delay {
            delay_col = delay_col
                .max(renderer.measure_text(ms, true, DWRITE_FONT_WEIGHT_NORMAL, row_font, true))
                .max(3.6 * row_font);
        }
    }

    let mut row_w = pad_x * 2.0 + name_col;
    for col in [down_col, up_col, total_col, delay_col] {
        if col > 0.0 {
            row_w += col_gap + col;
        }
    }

    let width = header_w.max(row_w).ceil().clamp(210.0, 960.0);
    let height = (pad_y + header_h + 6.0 * scale + row_h * rows.len() as f32 + pad_y * 0.75).ceil();
    let w_i32 = width as i32;
    let h_i32 = height as i32;

    if !surface.ensure_size(w_i32, h_i32) || !renderer.begin(surface) {
        return None;
    }

    let bg_alpha = f32::from(opacity) / 255.0;
    let light_bg = is_light_rgb(background);
    let radius = (12.0 * scale).clamp(8.0, 18.0);
    if opacity > 0 {
        renderer.fill_rounded_rect(
            0.5,
            0.5,
            width - 1.0,
            height - 1.0,
            radius,
            rgb_to_d2d(background, bg_alpha),
        );
        let border = if light_bg {
            rgb_to_d2d(0x000000, 0.13 * bg_alpha.max(0.35))
        } else {
            rgb_to_d2d(0xFFFFFF, 0.13 * bg_alpha.max(0.35))
        };
        renderer.stroke_rounded_rect(0.5, 0.5, width - 1.0, height - 1.0, radius, 1.0, border);
    }

    let header_y = pad_y * 0.6;
    draw_status_dot(
        renderer,
        pad_x + dot_r,
        header_y + header_h * 0.5,
        dot_r,
        dot_color,
        breathe_phase,
    );

    let mut x = pad_x + dot_r * 2.0 + 7.0 * scale;
    for (seg, w) in header.iter().zip(&header_widths) {
        let weight = if seg.bold {
            DWRITE_FONT_WEIGHT_SEMI_BOLD
        } else {
            DWRITE_FONT_WEIGHT_MEDIUM
        };
        let align = if seg.text == "|" {
            DWRITE_TEXT_ALIGNMENT_CENTER
        } else {
            DWRITE_TEXT_ALIGNMENT_LEADING
        };
        renderer.draw_text(
            &seg.text,
            seg.mono,
            weight,
            font_px,
            x,
            header_y,
            *w + 2.0,
            header_h,
            rgb_to_d2d(seg.color, seg.alpha),
            align,
            seg.mono,
        );
        x += *w + seg_gap;
    }

    let divider_y = header_y + header_h + 2.5 * scale;
    let divider_color = if light_bg {
        rgb_to_d2d(0x000000, 0.10)
    } else {
        rgb_to_d2d(0xFFFFFF, 0.10)
    };
    renderer.draw_line(pad_x, divider_y, width - pad_x, divider_y, 1.0, divider_color);

    let use_semantic = is_default_foreground(foreground);
    let dl_rgb = if use_semantic { COLOR_DL } else { foreground };
    let ul_rgb = if use_semantic { COLOR_UL } else { foreground };
    let mut y = divider_y + 3.5 * scale;
    for row in rows {
        let mut rx = pad_x;
        let name_w = (width - pad_x * 2.0)
            - [down_col, up_col, total_col, delay_col]
                .into_iter()
                .filter(|c| *c > 0.0)
                .map(|c| c + col_gap)
                .sum::<f32>();
        renderer.draw_text(
            &row.name,
            false,
            DWRITE_FONT_WEIGHT_NORMAL,
            row_font,
            rx,
            y,
            name_w.max(40.0),
            row_h,
            rgb_to_d2d(foreground, 0.92),
            DWRITE_TEXT_ALIGNMENT_LEADING,
            false,
        );
        rx += name_w.max(40.0);

        if let Some(d) = &row.down
            && down_col > 0.0
        {
            rx += col_gap;
            renderer.draw_text(
                d,
                true,
                DWRITE_FONT_WEIGHT_MEDIUM,
                row_font,
                rx,
                y,
                down_col + 2.0,
                row_h,
                rgb_to_d2d(dl_rgb, 0.98),
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                true,
            );
            rx += down_col;
        }
        if let Some(u) = &row.up
            && up_col > 0.0
        {
            rx += col_gap;
            renderer.draw_text(
                u,
                true,
                DWRITE_FONT_WEIGHT_MEDIUM,
                row_font,
                rx,
                y,
                up_col + 2.0,
                row_h,
                rgb_to_d2d(ul_rgb, 0.98),
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                true,
            );
            rx += up_col;
        }
        if let Some(tot) = &row.total
            && total_col > 0.0
        {
            rx += col_gap;
            renderer.draw_text(
                tot,
                true,
                DWRITE_FONT_WEIGHT_NORMAL,
                row_font,
                rx,
                y,
                total_col + 2.0,
                row_h,
                rgb_to_d2d(foreground, 0.72),
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                true,
            );
            rx += total_col;
        }
        if let Some(ms) = &row.delay
            && delay_col > 0.0
        {
            rx += col_gap;
            renderer.draw_text(
                ms,
                true,
                DWRITE_FONT_WEIGHT_NORMAL,
                row_font,
                rx,
                y,
                delay_col + 2.0,
                row_h,
                rgb_to_d2d(foreground, 0.78),
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                true,
            );
        }
        y += row_h;
    }

    renderer.end().then_some((w_i32, h_i32))
}

fn draw_status_dot(renderer: &D2dRenderer, cx: f32, cy: f32, r: f32, color: u32, breathe_phase: Option<u32>) {
    if let Some(step) = breathe_phase {
        let angle = (step % 24) as f32 * (std::f32::consts::TAU / 24.0);
        let pulse = 0.5 + 0.5 * angle.sin();
        let halo_alpha = 0.12 + pulse * 0.28;
        renderer.fill_circle(cx, cy, r + pulse * (r * 0.65), rgb_to_d2d(color, halo_alpha));
    } else {
        renderer.fill_circle(cx, cy, r * 1.45, rgb_to_d2d(color, 0.20));
    }
    renderer.fill_circle(cx, cy, r, rgb_to_d2d(color, 1.0));
}

fn build_telemetry_segments(
    label: Option<&str>,
    style: &Overlay,
    stats: &RuleStats,
    delay: Option<u64>,
    running: bool,
) -> Vec<Segment> {
    let fg = style.foreground;
    let semantic = is_default_foreground(fg);
    let dl = if semantic { COLOR_DL } else { fg };
    let ul = if semantic { COLOR_UL } else { fg };
    let mut segs = Vec::new();

    if let Some(title) = label
        && !title.is_empty()
    {
        segs.push(Segment {
            text: truncate_chars(title, 22),
            color: fg,
            alpha: 0.96,
            mono: false,
            bold: true,
            min_em: 0.0,
        });
    }

    if style.show_delay {
        let delay_text = if !running {
            "OFF".to_owned()
        } else {
            delay.map_or_else(|| "--ms".to_owned(), |ms| format!("{ms}ms"))
        };
        segs.push(Segment {
            text: delay_text,
            color: fg,
            alpha: 0.94,
            mono: true,
            bold: false,
            min_em: 3.1,
        });
    }

    let push_sep = |list: &mut Vec<Segment>| {
        if !list.is_empty() {
            list.push(Segment {
                text: "|".into(),
                color: fg,
                alpha: 0.34,
                mono: true,
                bold: false,
                min_em: 0.5,
            });
        }
    };

    if style.show_rates {
        push_sep(&mut segs);
        segs.push(Segment {
            text: format!("↓ {}", format_speed(stats.download_rate)),
            color: dl,
            alpha: 1.0,
            mono: true,
            bold: false,
            min_em: 6.4,
        });
        segs.push(Segment {
            text: format!("↑ {}", format_speed(stats.upload_rate)),
            color: ul,
            alpha: 1.0,
            mono: true,
            bold: false,
            min_em: 6.4,
        });
    }

    if style.show_totals {
        push_sep(&mut segs);
        segs.push(Segment {
            text: format!("Σ {}", format_bytes(stats.download_total + stats.upload_total)),
            color: fg,
            alpha: 0.88,
            mono: true,
            bold: false,
            min_em: 5.5,
        });
    }

    if segs.is_empty() {
        segs.push(Segment {
            text: if running { "ON".into() } else { "OFF".into() },
            color: fg,
            alpha: 0.95,
            mono: true,
            bold: true,
            min_em: 2.0,
        });
    }

    segs
}

fn scaled_font_px(style: &Overlay, hwnd: HWND) -> i32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    (i32::from(style.font_size) * i32::from(style.scale_percent) * dpi as i32 / 9600).clamp(8, 72)
}

fn work_area_for_point(x: i32, y: i32, w: i32, h: i32) -> RECT {
    unsafe {
        let pt = POINT {
            x: x + w / 2,
            y: y + h / 2,
        };
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !mon.is_null() && GetMonitorInfoW(mon, &mut info) != 0 {
            return info.rcWork;
        }
        let mut wa = RECT::default();
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut wa as *mut RECT).cast(), 0) != 0 {
            return wa;
        }
        RECT {
            left: 0,
            top: 0,
            right: GetSystemMetrics(SM_CXSCREEN),
            bottom: GetSystemMetrics(SM_CYSCREEN),
        }
    }
}

fn clamp_to_work_area(x: i32, y: i32, w: i32, h: i32, snap_edges: bool) -> (i32, i32) {
    let wa = work_area_for_point(x, y, w, h);
    let mut nx = x;
    let mut ny = y;
    if wa.right - wa.left >= w {
        nx = nx.clamp(wa.left, wa.right - w);
        if snap_edges {
            if nx - wa.left < EDGE_SNAP_PX {
                nx = wa.left;
            } else if wa.right - (nx + w) < EDGE_SNAP_PX {
                nx = wa.right - w;
            }
        }
    }
    if wa.bottom - wa.top >= h {
        ny = ny.clamp(wa.top, wa.bottom - h);
    }
    (nx, ny)
}

fn visible_frame_rect(hwnd: HWND) -> Option<RECT> {
    unsafe {
        let mut rect = RECT::default();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut rect as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        ) == 0
            && rect.right > rect.left
            && rect.bottom > rect.top
        {
            return Some(rect);
        }
        (GetWindowRect(hwnd, &mut rect) != 0 && rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
    }
}

fn compute_target_position(target: HWND, style: &Overlay, width: i32, height: i32) -> Option<(i32, i32)> {
    unsafe {
        if IsWindow(target) == 0 || IsWindowVisible(target) == 0 || IsIconic(target) != 0 {
            return None;
        }
        let mut area = RECT::default();
        let mut origin = POINT::default();
        if GetClientRect(target, &mut area) == 0 || ClientToScreen(target, &mut origin) == 0 {
            return None;
        }
        let client_w = area.right - area.left;
        let client_h = area.bottom - area.top;
        if client_w < 40 || client_h < 24 {
            return None;
        }
        let dpi = GetDpiForWindow(target).max(96) as i32;
        let off_x = style.offset_x * dpi / 96;
        let off_y = style.offset_y * dpi / 96;
        let right = matches!(style.anchor, Anchor::TopRight | Anchor::BottomRight);
        let bottom = matches!(style.anchor, Anchor::BottomLeft | Anchor::BottomRight);
        let mut x = origin.x + if right { client_w - width - off_x } else { off_x };
        let mut y = origin.y + if bottom { client_h - height - off_y } else { off_y };

        let mon = MonitorFromWindow(target, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !mon.is_null() && GetMonitorInfoW(mon, &mut info) != 0 {
            let mr = info.rcMonitor;
            if mr.right - mr.left > width + 4 {
                x = x.clamp(mr.left + 2, mr.right - width - 2);
            }
            if mr.bottom - mr.top > height + 4 {
                y = y.clamp(mr.top + 2, mr.bottom - height - 2);
            }
        }
        Some((x, y))
    }
}

const fn rects_intersect(a: &RECT, b: &RECT) -> bool {
    a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
}

fn is_window_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            (&mut cloaked as *mut u32).cast(),
            size_of::<u32>() as u32,
        ) == 0
            && cloaked != 0
    }
}

fn is_target_occluded(target: HWND, osd_rect: &RECT, own_pid: u32) -> bool {
    unsafe {
        if IsIconic(target) != 0 || IsWindowVisible(target) == 0 || is_window_cloaked(target) {
            return true;
        }
        let mut cur = GetWindow(target, GW_HWNDPREV);
        let mut steps = 0u32;
        while !cur.is_null() && steps < 256 {
            steps += 1;
            if IsWindowVisible(cur) != 0 && IsIconic(cur) == 0 && !is_window_cloaked(cur) {
                let mut pid = 0;
                GetWindowThreadProcessId(cur, &mut pid);
                if pid != own_pid {
                    let ex = GetWindowLongPtrW(cur, GWL_EXSTYLE) as u32;
                    let click_through_overlay = (ex & (WS_EX_LAYERED | WS_EX_TRANSPARENT))
                        == (WS_EX_LAYERED | WS_EX_TRANSPARENT);
                    if !click_through_overlay
                        && let Some(frame) = visible_frame_rect(cur)
                        && rects_intersect(&frame, osd_rect)
                    {
                        return true;
                    }
                }
            }
            cur = GetWindow(cur, GW_HWNDPREV);
        }
        false
    }
}

fn resolve_target_zorder(target: HWND, osd_hwnd: HWND, own_pid: u32) -> HWND {
    unsafe {
        let target_ex = GetWindowLongPtrW(target, GWL_EXSTYLE) as u32;
        if GetForegroundWindow() == target || (target_ex & WS_EX_TOPMOST) != 0 {
            return HWND_TOPMOST;
        }
        let mut after = GetWindow(target, GW_HWNDPREV);
        let mut steps = 0u32;
        while !after.is_null() && steps < 64 {
            steps += 1;
            let mut pid = 0;
            GetWindowThreadProcessId(after, &mut pid);
            if after != osd_hwnd && pid != own_pid {
                break;
            }
            after = GetWindow(after, GW_HWNDPREV);
        }
        if after.is_null() {
            HWND_TOP
        } else if (GetWindowLongPtrW(after, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST) != 0 {
            HWND_NOTOPMOST
        } else {
            after
        }
    }
}

struct UwpProbe {
    host_pid: u32,
    real_pid: u32,
}

unsafe extern "system" fn unwrap_uwp_child(child: HWND, arg: LPARAM) -> i32 {
    let probe = unsafe { &mut *(arg as *mut UwpProbe) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(child, &mut pid) };
    if pid != 0 && pid != probe.host_pid {
        probe.real_pid = pid;
        return 0;
    }
    1
}

fn resolve_window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return 0;
    }
    if let Some(path) = process::image(pid)
        && path
            .rsplit(['\\', '/'])
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("ApplicationFrameHost.exe"))
    {
        let mut probe = UwpProbe {
            host_pid: pid,
            real_pid: 0,
        };
        unsafe { EnumChildWindows(hwnd, Some(unwrap_uwp_child), (&mut probe as *mut UwpProbe) as LPARAM) };
        if probe.real_pid != 0 {
            return probe.real_pid;
        }
    }
    pid
}

fn wildcard_matches(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().map(|c| c.to_ascii_lowercase()).collect();
    let t: Vec<char> = text.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut backtrack) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            backtrack = ti;
        } else if let Some(s) = star {
            pi = s + 1;
            backtrack += 1;
            ti = backtrack;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn fallback_matches(pattern: &str, image: &str) -> bool {
    let normalized = image.replace('/', "\\");
    let basename = normalized.rsplit('\\').next().unwrap_or(&normalized);
    pattern.split(';').map(str::trim).filter(|s| !s.is_empty()).any(|part| {
        let part_norm = part.replace('/', "\\");
        if part_norm.contains('\\') {
            wildcard_matches(&part_norm, &normalized)
        } else {
            wildcard_matches(&part_norm, basename)
        }
    })
}

fn rule_matches(pattern_str: &str, pattern_c: &CString, image_str: &str, image_c: &CString) -> bool {
    engine::matches(pattern_c, image_c) || fallback_matches(pattern_str, image_str)
}

unsafe extern "system" fn osd_wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_ERASEBKGND => 1,
        WM_NCHITTEST => {
            if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 2 {
                HTCAPTION as LRESULT
            } else {
                HTTRANSPARENT as LRESULT
            }
        }
        WM_ENTERSIZEMOVE if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 2 => {
            DRAGGING_GLOBAL.store(true, Ordering::Release);
            0
        }
        WM_EXITSIZEMOVE if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 2 => {
            DRAGGING_GLOBAL.store(false, Ordering::Release);
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(hwnd, &mut rect) } != 0 {
                let w_px = (rect.right - rect.left).max(1);
                let h_px = (rect.bottom - rect.top).max(1);
                let (x, y) = clamp_to_work_area(rect.left, rect.top, w_px, h_px, true);
                if x != rect.left || y != rect.top {
                    unsafe {
                        SetWindowPos(
                            hwnd,
                            HWND_TOPMOST,
                            x,
                            y,
                            w_px,
                            h_px,
                            SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                        );
                    }
                }
                if let Ok(mut frame) = FRAME.lock()
                    && let Some((settings, _)) = frame.as_mut()
                {
                    settings.overlay.global_x = x;
                    settings.overlay.global_y = y;
                }
                tauri::async_runtime::spawn(super::save_overlay_position(x, y));
            }
            wake();
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

unsafe extern "system" fn ctrl_wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == UPDATE {
        // Fallback when UPDATE arrives during a nested modal loop (e.g. window drag).
        QUEUED.store(false, Ordering::Release);
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

unsafe extern "system" fn list_top_window(hwnd: HWND, arg: LPARAM) -> i32 {
    let windows = unsafe { &mut *(arg as *mut Vec<HWND>) };
    if unsafe { IsWindowVisible(hwnd) } != 0
        && unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOOLWINDOW == 0
        && unsafe { GetWindow(hwnd, GW_OWNER) }.is_null()
    {
        windows.push(hwnd);
    }
    1
}

fn register_classes() -> bool {
    unsafe {
        let hinst = GetModuleHandleW(ptr::null());
        let osd_class = WNDCLASSW {
            lpfnWndProc: Some(osd_wnd_proc),
            hInstance: hinst,
            hCursor: LoadCursorW(ptr::null_mut(), IDC_SIZEALL),
            lpszClassName: OSD_CLASS.as_ptr(),
            ..Default::default()
        };
        if RegisterClassW(&osd_class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            logging!(warn, Type::System, "RegisterClassW(OSD) failed: {}", GetLastError());
            return false;
        }
        let ctrl_class = WNDCLASSW {
            lpfnWndProc: Some(ctrl_wnd_proc),
            hInstance: hinst,
            lpszClassName: CTRL_CLASS.as_ptr(),
            ..Default::default()
        };
        if RegisterClassW(&ctrl_class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            logging!(warn, Type::System, "RegisterClassW(Ctrl) failed: {}", GetLastError());
            return false;
        }
        true
    }
}

fn build_global_scene(
    settings: &Settings,
    snapshot: &Snapshot,
    global_hwnd: HWND,
    breathe_step: u32,
) -> Scene {
    let style = &settings.overlay;
    let rules: Vec<_> = settings
        .rules
        .iter()
        .filter(|rule| rule.enabled && rule.action == Action::Proxy)
        .collect();

    let mut total = RuleStats::default();
    let mut rows = Vec::new();
    for rule in &rules {
        let stats = snapshot.rules.get(&rule.id).cloned().unwrap_or_default();
        total.upload_rate += stats.upload_rate;
        total.download_rate += stats.download_rate;
        total.upload_total += stats.upload_total;
        total.download_total += stats.download_total;
        let active = stats.upload_rate != 0 || stats.download_rate != 0 || stats.active != 0;
        if (!style.hide_idle || active) && rows.len() < MAX_GLOBAL_ROWS {
            let row_delay = match style.delay {
                Delay::Node => None,
                Delay::Connection => stats.connection_delay,
            };
            rows.push(AppRowData {
                name: truncate_chars(&rule.name, 18),
                down: style.show_rates.then(|| format!("↓ {}", format_speed(stats.download_rate))),
                up: style.show_rates.then(|| format!("↑ {}", format_speed(stats.upload_rate))),
                total: style
                    .show_totals
                    .then(|| format!("Σ {}", format_bytes(stats.download_total + stats.upload_total))),
                delay: (style.show_delay && style.delay == Delay::Connection)
                    .then(|| row_delay.map_or_else(|| "--ms".into(), |ms| format!("{ms}ms"))),
            });
        }
    }

    let global_delay = match style.delay {
        Delay::Node => snapshot.node_delay.map(u64::from),
        Delay::Connection => rules
            .iter()
            .filter_map(|r| snapshot.rules.get(&r.id).and_then(|s| s.connection_delay))
            .next(),
    };
    let dot_color = status_color(snapshot.running, snapshot.endpoint_ready, global_delay);
    let breathe = (snapshot.running && snapshot.endpoint_ready).then_some(breathe_step);
    let header = build_telemetry_segments(None, style, &total, global_delay, snapshot.running);
    let font_px = scaled_font_px(style, global_hwnd);

    if rows.is_empty() {
        Scene::Pill {
            dot_color,
            breathe_phase: breathe,
            segments: header,
            font_px,
            background: style.background,
            opacity: style.opacity,
        }
    } else {
        Scene::Card {
            dot_color,
            breathe_phase: breathe,
            header,
            rows,
            font_px,
            foreground: style.foreground,
            background: style.background,
            opacity: style.opacity,
        }
    }
}

fn build_window_scene(
    rule_name: &str,
    style: &Overlay,
    stats: &RuleStats,
    snapshot: &Snapshot,
    target: HWND,
) -> Scene {
    let delay = match style.delay {
        Delay::Node => snapshot.node_delay.map(u64::from),
        Delay::Connection => stats.connection_delay,
    };
    let label = snapshot
        .node
        .as_ref()
        .map_or_else(|| rule_name.to_owned(), |node| format!("{rule_name} · {node}"));
    let segments = build_telemetry_segments(Some(&label), style, stats, delay, snapshot.running);
    Scene::Pill {
        dot_color: status_color(snapshot.running, snapshot.endpoint_ready, delay),
        breathe_phase: None,
        segments,
        font_px: scaled_font_px(style, target),
        background: style.background,
        opacity: style.opacity,
    }
}

fn discover_target_windows(
    settings: &Settings,
    windows: &mut BTreeMap<usize, (Window, u32, u32)>,
    own_pid: u32,
) {
    let filters: Vec<_> = settings
        .rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter_map(|rule| CString::new(rule.pattern.as_bytes()).ok().map(|c| (rule, c)))
        .collect();

    let mut candidates: Vec<HWND> = Vec::new();
    unsafe { EnumWindows(Some(list_top_window), (&mut candidates as *mut Vec<HWND>) as LPARAM) };

    let mut seen = BTreeSet::new();
    let mut images: BTreeMap<u32, Option<(String, CString)>> = BTreeMap::new();

    for hwnd in candidates {
        let pid = resolve_window_pid(hwnd);
        if pid == 0 || pid == own_pid {
            continue;
        }
        let entry = images.entry(pid).or_insert_with(|| {
            let image = process::image(pid)?;
            let cstr = CString::new(image.as_bytes()).ok()?;
            Some((image, cstr))
        });
        let Some((image_str, image_c)) = entry else {
            continue;
        };
        let Some((rule, _)) = filters
            .iter()
            .find(|(rule, filter_c)| rule_matches(&rule.pattern, filter_c, image_str, image_c))
        else {
            continue;
        };
        if rule.action != Action::Proxy || !rule.overlay {
            continue;
        }
        let key = hwnd as usize;
        seen.insert(key);
        if windows.get(&key).is_some_and(|(_, old_pid, _)| *old_pid != pid) {
            windows.remove(&key);
        }
        if let Some(existing) = windows.get_mut(&key) {
            existing.2 = rule.id;
        } else if let Some(window) = Window::new(false) {
            windows.insert(key, (window, pid, rule.id));
        }
    }
    windows.retain(|key, _| seen.contains(key));
}

fn run() {
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let com_ok = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        if !register_classes() {
            if com_ok {
                CoUninitialize();
            }
            return;
        }

        let Some(mut renderer) = D2dRenderer::new() else {
            logging!(warn, Type::System, "Failed to initialize Direct2D/DirectWrite OSD renderer");
            if com_ok {
                CoUninitialize();
            }
            return;
        };

        let ctrl_hwnd = CreateWindowExW(
            0,
            CTRL_CLASS.as_ptr(),
            ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            ptr::null_mut(),
            GetModuleHandleW(ptr::null()),
            ptr::null(),
        );
        if ctrl_hwnd.is_null() {
            logging!(warn, Type::System, "Failed to create OSD controller window: {}", GetLastError());
            if com_ok {
                CoUninitialize();
            }
            return;
        }
        CTRL_HWND.store(ctrl_hwnd as isize, Ordering::Release);

        let hook_fg = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            ptr::null_mut(),
            Some(win_event_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        let hook_min = SetWinEventHook(
            EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MINIMIZEEND,
            ptr::null_mut(),
            Some(win_event_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        let hook_loc = SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            ptr::null_mut(),
            Some(win_event_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );

        wake();

        let own_pid = std::process::id();
        let mut windows: BTreeMap<usize, (Window, u32, u32)> = BTreeMap::new();
        let mut global: Option<Window> = None;
        let mut last_discovery: Option<Instant> = None;
        let mut last_rules = Vec::new();
        let mut breathe_step = 0u32;
        let mut timer_active = false;
        let mut message = MSG::default();

        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            let is_ctrl_update = message.hwnd == ctrl_hwnd && message.message == UPDATE;
            let is_breathe = message.hwnd == ctrl_hwnd && message.message == WM_TIMER && message.wParam == BREATHE_TIMER_ID;
            if !is_ctrl_update && !is_breathe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
                continue;
            }
            if is_ctrl_update {
                QUEUED.store(false, Ordering::Release);
            }
            if is_breathe {
                breathe_step = breathe_step.wrapping_add(1);
            }

            let Some((settings, snapshot)) = FRAME.lock().ok().and_then(|frame| frame.clone()) else {
                continue;
            };
            let style = &settings.overlay;

            if style.global {
                if global.is_none() {
                    global = Window::new(true);
                }
                let want_timer = snapshot.running && snapshot.endpoint_ready;
                if want_timer && !timer_active {
                    SetTimer(ctrl_hwnd, BREATHE_TIMER_ID, BREATHE_INTERVAL_MS, None);
                    timer_active = true;
                } else if !want_timer && timer_active {
                    KillTimer(ctrl_hwnd, BREATHE_TIMER_ID);
                    timer_active = false;
                }
                if let Some(window) = &mut global {
                    let scene = build_global_scene(&settings, &snapshot, window.hwnd, breathe_step);
                    window.render_and_show(&mut renderer, scene, style, None, own_pid);
                }
            } else {
                if timer_active {
                    KillTimer(ctrl_hwnd, BREATHE_TIMER_ID);
                    timer_active = false;
                }
                global = None;
            }

            if is_breathe && !is_ctrl_update {
                continue;
            }

            if !style.windows || !snapshot.running {
                windows.clear();
                last_discovery = None;
                continue;
            }

            let dirty = WINDOWS_DIRTY.swap(false, Ordering::AcqRel);
            if last_rules != settings.rules
                || last_discovery
                    .is_none_or(|instant| instant.elapsed() >= Duration::from_millis(if dirty { 250 } else { 2000 }))
            {
                last_rules.clone_from(&settings.rules);
                last_discovery = Some(Instant::now());
                discover_target_windows(&settings, &mut windows, own_pid);
            }

            windows.retain(|key, (window, pid, rule_id)| {
                let hwnd = *key as HWND;
                if IsWindow(hwnd) == 0 || resolve_window_pid(hwnd) != *pid {
                    return false;
                }
                let Some(rule) = settings
                    .rules
                    .iter()
                    .find(|rule| rule.id == *rule_id && rule.enabled && rule.action == Action::Proxy && rule.overlay)
                else {
                    return false;
                };
                if is_window_cloaked(hwnd) || IsIconic(hwnd) != 0 || IsWindowVisible(hwnd) == 0 {
                    window.hide();
                    return true;
                }
                let stats = snapshot.rules.get(rule_id).cloned().unwrap_or_default();
                let scene = build_window_scene(&rule.name, style, &stats, &snapshot, hwnd);
                window.render_and_show(&mut renderer, scene, style, Some(hwnd), own_pid);
                true
            });
        }

        if timer_active {
            KillTimer(ctrl_hwnd, BREATHE_TIMER_ID);
        }
        if !hook_loc.is_null() {
            UnhookWinEvent(hook_loc);
        }
        if !hook_min.is_null() {
            UnhookWinEvent(hook_min);
        }
        if !hook_fg.is_null() {
            UnhookWinEvent(hook_fg);
        }
        CTRL_HWND.store(0, Ordering::Release);
        DestroyWindow(ctrl_hwnd);
        if com_ok {
            CoUninitialize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_d2d_pill_and_card_and_presents_layered_window() {
        let com_ok = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        assert!(register_classes());
        let mut renderer = D2dRenderer::new().expect("Direct2D/DirectWrite renderer should initialize");
        let style = Overlay::default();
        let stats = RuleStats {
            upload_rate: 24 * 1024,
            download_rate: 1_250_000,
            upload_total: 18 * 1024 * 1024,
            download_total: 342 * 1024 * 1024,
            ..RuleStats::default()
        };

        let pill = Scene::Pill {
            dot_color: COLOR_OK,
            breathe_phase: Some(6),
            segments: build_telemetry_segments(Some("Firefox"), &style, &stats, Some(42), true),
            font_px: 12,
            background: style.background,
            opacity: style.opacity,
        };
        let mut surface = Surface::new();
        let (pw, ph) = render_scene(&mut renderer, &mut surface, &pill).expect("Pill scene should render");
        assert!(pw > 100 && ph >= 22);

        let window = Window::new(true).expect("Layered OSD window should be created");
        assert!(surface.present(window.hwnd, 32, 64));

        let card = Scene::Card {
            dot_color: COLOR_OK,
            breathe_phase: Some(0),
            header: build_telemetry_segments(None, &style, &stats, Some(42), true),
            rows: vec![AppRowData {
                name: "Firefox".into(),
                down: Some("↓ 1.2 MB/s".into()),
                up: Some("↑ 24.0 KB/s".into()),
                total: Some("Σ 360.0 MB".into()),
                delay: None,
            }],
            font_px: 12,
            foreground: style.foreground,
            background: style.background,
            opacity: style.opacity,
        };
        let (cw, ch) = render_scene(&mut renderer, &mut surface, &card).expect("Card scene should render");
        assert!(cw >= 210 && ch > ph);
        assert!(surface.present(window.hwnd, 32, 64));
        drop(window);
        if com_ok {
            unsafe { CoUninitialize() };
        }
    }

    #[test]
    fn fallback_matcher_handles_basenames_paths_and_semicolons() {
        assert!(fallback_matches("firefox.exe;chrome.exe", r"C:\Program Files\Mozilla Firefox\firefox.exe"));
        assert!(fallback_matches(r"C:\Games\*\client?.exe", r"C:\Games\Test\client1.exe"));
        assert!(!fallback_matches("chrome.exe", r"C:\Program Files\Mozilla Firefox\firefox.exe"));
    }
}
