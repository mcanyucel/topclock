#![windows_subsystem = "windows"]

use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::OnceLock;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, VK_ESCAPE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

const TIMER_ID: usize = 1;

// Which minute is currently on screen; -1 until the first render.
static LAST_MINUTE: AtomicI32 = AtomicI32::new(-1);
static LAYOUT: OnceLock<Layout> = OnceLock::new();
static CONFIG: OnceLock<Config> = OnceLock::new();

/// Written next to the exe on first run, so the knobs document themselves.
const DEFAULT_INI: &str = "\
# topclock settings. Middle-click the clock to reopen this file.
# Restart topclock after editing. Delete this file to regenerate the defaults.
# Comments must be on their own line.

[clock]
# Colors are #RRGGBB, the way you'd write them anywhere else.
fg = #00E6AA
# Only visible if bg_alpha > 0, but it is also what the antialiased glyph
# edges blend toward, so keep it near your intended backdrop.
bg = #0C0C10

# Backdrop opacity, 0-255. 0 means fully transparent AND click-through: the
# digits are then the only thing you can grab or right-click. Anything above 0
# makes the whole rectangle solid to the mouse (try 40 for smoked glass).
bg_alpha = 0

font = Consolas
# Cell height in pixels. The digits come out roughly half of this, and the
# window resizes itself to fit them.
font_size = 26
# 400 = normal, 700 = bold.
weight = 700

# Smooth the glyph edges. Off by default: at this size every pixel then lands
# fully on or fully off, which is crisper and avoids a soft half-covered row on
# round digits. Turn it on for smoother diagonals and curves, which pays off as
# font_size grows.
antialias = off

# Transparent border around the digits, in pixels. 0 = the window is exactly
# the glyphs, so it can sit flush against a screen edge.
pad = 0

# Which monitor to start on. 0 is the primary display; 1, 2, 3 ... match the
# numbers in Settings > System > Display. An unattached number falls back to the
# primary display. Run one copy per monitor with TOPCLOCK_CONFIG pointing each
# at its own ini.
monitor = 0

# Distance from the top-right corner of that monitor's work area, at startup
# only.
margin_x = 0
margin_y = 0
";

/// Everything the .ini can override.
struct Config {
    font: Vec<u16>, // null-terminated, for PCWSTR
    font_size: i32,
    weight: i32,
    fg: COLORREF,
    bg: COLORREF,
    bg_alpha: u32,
    pad: i32,
    antialias: bool,
    margin_x: i32,
    margin_y: i32,
    monitor: i32,
    /// The file the settings above came from, so a middle-click can open the
    /// one actually in use rather than guessing at the location again.
    path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: wide("Consolas"),
            font_size: 26,
            weight: 700,
            fg: rgb(0x00, 0xE6, 0xAA), // mint green
            bg: rgb(0x0C, 0x0C, 0x10), // near-black
            bg_alpha: 0,
            antialias: false,
            pad: 0,
            margin_x: 0,
            margin_y: 0,
            monitor: 0,
            path: None,
        }
    }
}

/// Window size, and where the text has to be drawn inside it to land on the ink
/// bounds we measured. Both come out of `measure`.
struct Layout {
    w: i32,
    h: i32,
    text_x: i32,
    text_y: i32,
}

fn main() -> Result<()> {
    let cfg = config();
    let lay = layout();
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class_name = w!("TopClockWindow");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class_name,
            // hbrBackground left null: we paint every pixel ourselves.
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(Error::from_win32());
        }

        // Park it in the top-right corner of the chosen monitor.
        let area = work_area(cfg.monitor);
        let x = area.right - lay.w - cfg.margin_x;
        let y = area.top + cfg.margin_y;

        let hwnd = CreateWindowExW(
            // Layered: the backdrop is drawn with per-pixel alpha (see render).
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name,
            w!("topclock"),
            WS_POPUP | WS_VISIBLE, // no title bar, no border
            x,
            y,
            lay.w,
            lay.h,
            None,
            None,
            instance,
            None,
        )?;

        render(hwnd);
        SetTimer(hwnd, TIMER_ID, 1000, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER => {
                // Ticking every second, but the display only changes once a
                // minute -- skip the other 59 redraws.
                let minute = GetLocalTime().wMinute as i32;
                if LAST_MINUTE.swap(minute, Ordering::Relaxed) != minute {
                    render(hwnd);
                }
                LRESULT(0)
            }
            // A layered window's pixels come from UpdateLayeredWindow, not from
            // a paint cycle, so there is nothing to do here but validate.
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_LBUTTONDOWN => {
                // Hand the drag off to the window manager's move loop.
                let _ = ReleaseCapture();
                SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), LPARAM(0));
                LRESULT(0)
            }
            // Finding the ini by hand is the tedious part of configuring this,
            // and the clock is the one thing on screen that knows where it is.
            WM_MBUTTONUP => {
                open_config();
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_DESTROY => {
                KillTimer(hwnd, TIMER_ID).ok();
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn config() -> &'static Config {
    CONFIG.get_or_init(load)
}

/// Hands the ini to whatever the shell opens .ini files with, usually Notepad.
fn open_config() {
    let Some(path) = config().path.as_ref() else {
        return;
    };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// Where the exe would keep a portable config: right next to itself.
fn portable_ini() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("topclock.ini"))
}

/// Where a per-user config lives: %APPDATA%\topclock\topclock.ini.
fn user_ini() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("topclock").join("topclock.ini"))
}

/// Picks the config file, creating one with the documented defaults if there
/// isn't one yet.
///
/// Order matters for installed copies. An exe under Program Files cannot write
/// beside itself without elevation, so the fallback is the per-user location,
/// which always works and is where Windows expects user settings to live. A
/// portable copy unzipped into a writable folder keeps its ini alongside it.
fn resolve_ini() -> Option<PathBuf> {
    // An explicit override wins, for installers and for testing.
    if let Some(p) = std::env::var_os("TOPCLOCK_CONFIG") {
        return Some(PathBuf::from(p));
    }
    // Then whichever file already exists, portable first.
    if let Some(p) = portable_ini().filter(|p| p.exists()) {
        return Some(p);
    }
    if let Some(p) = user_ini().filter(|p| p.exists()) {
        return Some(p);
    }
    // Nothing yet: try to create one beside the exe, and fall back to the
    // per-user location when that folder is not writable.
    if let Some(p) = portable_ini() {
        if std::fs::write(&p, DEFAULT_INI).is_ok() {
            return Some(p);
        }
    }
    let p = user_ini()?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    std::fs::write(&p, DEFAULT_INI).ok()?;
    Some(p)
}

/// Reads the ini and applies it over the defaults. Anything unparseable keeps
/// its default rather than killing the clock -- a typo in a color should not
/// cost you the app.
fn load() -> Config {
    let mut c = Config::default();

    let Some(path) = resolve_ini() else {
        return c;
    };
    c.path = Some(path.clone());
    let Ok(text) = std::fs::read_to_string(&path) else {
        return c;
    };

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';', '[']) {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let (key, val) = (key.trim().to_ascii_lowercase(), val.trim());
        match key.as_str() {
            "font" => c.font = wide(val),
            "font_size" => c.font_size = val.parse().unwrap_or(c.font_size).max(1),
            "weight" => c.weight = val.parse().unwrap_or(c.weight).clamp(1, 1000),
            "fg" => c.fg = color(val).unwrap_or(c.fg),
            "bg" => c.bg = color(val).unwrap_or(c.bg),
            "bg_alpha" => c.bg_alpha = val.parse().unwrap_or(c.bg_alpha).min(255),
            "antialias" => c.antialias = flag(val).unwrap_or(c.antialias),
            "pad" => c.pad = val.parse().unwrap_or(c.pad).max(0),
            "margin_x" => c.margin_x = val.parse().unwrap_or(c.margin_x),
            "margin_y" => c.margin_y = val.parse().unwrap_or(c.margin_y),
            "monitor" => c.monitor = val.parse().unwrap_or(c.monitor).max(0),
            _ => {}
        }
    }
    c
}

struct MonitorSearch {
    want: i32,
    rect: Option<RECT>,
}

/// Matches monitors by the number Windows shows in Settings > Display.
///
/// That number is the tail of the device name, `\\.\DISPLAY2` and friends, which
/// is not the same thing as enumeration order, so this compares names rather
/// than counting callbacks. Returning FALSE stops the enumeration once matched.
unsafe extern "system" fn find_monitor(
    monitor: HMONITOR,
    _dc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> BOOL {
    unsafe {
        let search = &mut *(data.0 as *mut MonitorSearch);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool() {
            return TRUE;
        }
        // szDevice is a fixed 32-wide buffer padded with NULs; stop at the
        // first one, or they end up inside the string and break the parse.
        let end = info
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDevice.len());
        let device = String::from_utf16_lossy(&info.szDevice[..end]);
        let number: i32 = device
            .rsplit("DISPLAY")
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(-1);
        if number == search.want {
            search.rect = Some(info.monitorInfo.rcWork);
            return FALSE;
        }
        TRUE
    }
}

/// The work area to place the clock in: the chosen monitor, or the primary one
/// when `want` is 0 or names a monitor that is not currently attached.
///
/// Work area rather than full bounds, so the clock does not land underneath a
/// taskbar docked to the top or the side. With the usual bottom taskbar the two
/// are the same.
fn work_area(want: i32) -> RECT {
    unsafe {
        if want > 0 {
            let mut search = MonitorSearch { want, rect: None };
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(find_monitor),
                LPARAM(&mut search as *mut _ as isize),
            );
            if let Some(rect) = search.rect {
                return rect;
            }
        }

        let primary = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(primary, &mut info).as_bool() {
            return info.rcWork;
        }
        RECT {
            left: 0,
            top: 0,
            right: GetSystemMetrics(SM_CXSCREEN),
            bottom: GetSystemMetrics(SM_CYSCREEN),
        }
    }
}

fn layout() -> &'static Layout {
    LAYOUT.get_or_init(measure)
}

/// Finds the exact pixel bounds of the digits.
///
/// Font metrics would only give us the *cell* box -- ascent, internal leading
/// and descent all reserve room for accents and descenders that "12:34" never
/// uses, and that reserved space is exactly the padding we are trying to get
/// rid of. So draw a sample onto a scratch canvas and see where the ink landed.
fn measure() -> Layout {
    let cfg = config();
    let pad = cfg.pad;
    // Generous canvas, sample drawn well inside it so no glyph can spill off an
    // edge before we find it.
    let (cw, ch) = (cfg.font_size * 8, cfg.font_size * 4);
    let origin = cfg.font_size;
    // left, top, right, bottom -- inverted, so any ink at all narrows them.
    let (mut l, mut t, mut r, mut b) = (cw, ch, -1, -1);

    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(screen);
        if let Some((bmp, bits)) = dib(screen, cw, ch) {
            let old_bmp = SelectObject(mem, bmp);
            let font = make_font();
            let old_font = SelectObject(mem, font);

            let pixels = std::slice::from_raw_parts_mut(bits, (cw * ch) as usize);
            pixels.fill(0);

            // White on black: we only care where the ink is, not what color the
            // user picked for it.
            SetBkMode(mem, TRANSPARENT);
            SetTextColor(mem, rgb(0xFF, 0xFF, 0xFF));
            // Widest digits: every time we ever draw fits inside this box.
            let sample: Vec<u16> = "88:88".encode_utf16().collect();
            let _ = TextOutW(mem, origin, origin, &sample);
            let _ = GdiFlush();

            for (i, px) in pixels.iter().enumerate() {
                if *px & 0x00FF_FFFF != 0 {
                    let (x, y) = (i as i32 % cw, i as i32 / cw);
                    l = l.min(x);
                    t = t.min(y);
                    r = r.max(x);
                    b = b.max(y);
                }
            }

            SelectObject(mem, old_font);
            let _ = DeleteObject(font);
            SelectObject(mem, old_bmp);
            let _ = DeleteObject(bmp);
        }
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }

    if r < l || b < t {
        // Nothing drew; fall back to a cell-sized guess rather than a 0x0 window.
        return Layout {
            w: cfg.font_size * 3,
            h: cfg.font_size,
            text_x: pad,
            text_y: pad,
        };
    }

    Layout {
        w: r - l + 1 + 2 * pad,
        h: b - t + 1 + 2 * pad,
        // Shift the draw origin by however far the ink sat from where we asked
        // for it, so the glyphs start exactly at pad.
        text_x: pad - (l - origin),
        text_y: pad - (t - origin),
    }
}

/// Renders the clock into a 32bpp DIB and hands it to the compositor.
///
/// GDI text drawing ignores the alpha channel, so we draw the digits white on
/// black to get a coverage mask, then tint that mask with the configured
/// colors: background pixels become transparent, glyph pixels opaque, and the
/// antialiased edges in between land on a smooth ramp instead of a dark fringe.
/// Deriving coverage from a fixed white-on-black pass rather than from the real
/// colors is what keeps the clock visible whatever the .ini asks for.
fn render(hwnd: HWND) {
    let cfg = config();
    let lay = layout();
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(screen);
        let Some((bmp, bits)) = dib(screen, lay.w, lay.h) else {
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            return;
        };
        let old_bmp = SelectObject(mem, bmp);

        let pixels = std::slice::from_raw_parts_mut(bits, (lay.w * lay.h) as usize);
        pixels.fill(0);

        let font = make_font();
        let old_font = SelectObject(mem, font);

        SetBkMode(mem, TRANSPARENT);
        SetTextColor(mem, rgb(0xFF, 0xFF, 0xFF));

        let st = GetLocalTime();
        let text = format!("{:02}:{:02}", st.wHour, st.wMinute);
        let wide: Vec<u16> = text.encode_utf16().collect();
        let _ = TextOutW(mem, lay.text_x, lay.text_y, &wide);

        // GDI is done scribbling; make sure it lands before we read the bits back.
        let _ = GdiFlush();

        let (bg_r, bg_g, bg_b) = split(cfg.bg);
        let (fg_r, fg_g, fg_b) = split(cfg.fg);
        let a0 = cfg.bg_alpha;
        for px in pixels.iter_mut() {
            // Grayscale mask, so any channel is the coverage; green is as good
            // as either neighbor.
            let cov = (*px >> 8) & 0xFF;
            // Two layers composited source-over, written straight into the
            // premultiplied form AC_SRC_ALPHA expects: the glyph at `cov`, and
            // the backdrop at `a0` showing through only where the glyph does
            // not cover.
            //
            // The glyph keeps its own color at every coverage level. Blending
            // it toward `bg` first and then scaling by alpha would count the
            // backdrop twice, which tints half-covered edge pixels with the
            // backdrop color even when the backdrop is fully transparent -- a
            // dark fringe around every glyph, glaring over a light desktop.
            let alpha = cov + a0 * (255 - cov) / 255;
            let chan = |bg: u32, fg: u32| (fg * cov * 255 + bg * a0 * (255 - cov)) / (255 * 255);
            *px = (alpha << 24)
                | (chan(bg_r, fg_r) << 16)
                | (chan(bg_g, fg_g) << 8)
                | chan(bg_b, fg_b);
        }

        let size = SIZE { cx: lay.w, cy: lay.h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            screen,
            None, // leave the window where it is
            Some(&size),
            mem,
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );

        SelectObject(mem, old_font);
        let _ = DeleteObject(font);
        SelectObject(mem, old_bmp);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

/// A top-down 32bpp DIB, plus a pointer to its pixels.
unsafe fn dib(hdc: HDC, w: i32, h: i32) -> Option<(HBITMAP, *mut u32)> {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // negative: top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0, // BI_RGB
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bmp = unsafe { CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    Some((bmp, bits as *mut u32))
}

/// Numeric literals instead of the named constants: their Rust types move
/// around between windows-rs releases, the values never do.
unsafe fn make_font() -> HFONT {
    let cfg = config();
    unsafe {
        CreateFontW(
            cfg.font_size,
            0,
            0,
            0, // height, width(auto), escapement, orientation
            cfg.weight,
            0,
            0,
            0, // italic, underline, strikeout
            1, // DEFAULT_CHARSET
            0, // OUT_DEFAULT_PRECIS
            0, // CLIP_DEFAULT_PRECIS
            // ANTIALIASED_QUALITY (4), never CLEARTYPE_QUALITY: subpixel
            // antialiasing is tuned for a known background, and ours is
            // whatever happens to be behind the window. Grayscale keeps the
            // edges neutral.
            //
            // NONANTIALIASED_QUALITY (3) makes every pixel fully on or fully
            // off, so with a transparent backdrop the glyph edges are hard
            // rather than ramped -- crisper, at the cost of jagged diagonals.
            if cfg.antialias { 4 } else { 3 },
            0, // DEFAULT_PITCH | FF_DONTCARE
            PCWSTR(cfg.font.as_ptr()),
        )
    }
}

/// COLORREF is 0x00BBGGRR, not RGB.
fn rgb(r: u32, g: u32, b: u32) -> COLORREF {
    COLORREF((b << 16) | (g << 8) | r)
}

/// COLORREF back into (r, g, b).
fn split(c: COLORREF) -> (u32, u32, u32) {
    (c.0 & 0xFF, (c.0 >> 8) & 0xFF, (c.0 >> 16) & 0xFF)
}

/// The usual spellings of yes and no.
fn flag(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// "#RRGGBB" (or bare "RRGGBB") into a COLORREF.
fn color(s: &str) -> Option<COLORREF> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some(rgb((n >> 16) & 0xFF, (n >> 8) & 0xFF, n & 0xFF))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
