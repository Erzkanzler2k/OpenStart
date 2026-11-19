#![windows_subsystem = "windows"]

//! OpenStart — самостоятельное меню «Пуск» для Windows 10/11.//!
//! Меню открывается по клавише Win (низкоуровневый хук клавиатуры), показывает
//! ярлыки из пользовательского и общего меню «Пуск», ищет по ним и запускает их.
//! Семь стилей (Windows 7 / Modern / Windows 10 / Windows 11 / Windows App / Windows Pro / Windows 10X)
//! рисуются самим приложением. Настройки лежат в
//! %APPDATA%\OpenStart\config.json. Реестр не используется.

use std::collections::BTreeMap;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use eframe::egui;
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, FindWindowExW, FindWindowW, GetClassNameW, GetForegroundWindow, GetMessageW,
    GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, PostMessageW, SetForegroundWindow, SetWindowPos,
    WM_CLOSE, SW_HIDE,
    SetWindowsHookExW, ShowWindow, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, MSG,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, BOOL, ERROR_ALREADY_EXISTS, HANDLE, HWND, POINT, RECT,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Graphics::Gdi::DeleteObject;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, CreateEventW, CreateMutexW, GetCurrentThreadId, OpenEventW, SetEvent,
    WaitForSingleObject, EVENT_MODIFY_STATE,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, GetCursorPos, ICONINFO, MENU_ITEM_FLAGS, MF_SEPARATOR, MF_STRING,
    RegisterClassW, RegisterWindowMessageW, TrackPopupMenu, TPM_NONOTIFY, TPM_RETURNCMD,
    WNDCLASSW, WM_LBUTTONDBLCLK, WM_RBUTTONUP, HICON, IMAGE_ICON, LR_LOADFROMFILE,
    LoadImageW,
};

const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;

const ACCENT: egui::Color32 = egui::Color32::from_rgb(0xe0, 0xaf, 0x68);
const ACCENT_DARK: egui::Color32 = egui::Color32::from_rgb(0xa5, 0x76, 0x2f);

const STYLES: &[(&str, &str)] = &[
    ("Стиль Windows 7", "Классика Windows 7"),
    ("Стиль Modern", "Обновлённая классика"),
    ("Стиль Windows 10", "Плитки Windows 10"),
    ("Стиль Windows 11", "Центрированное меню"),
    ("Стиль Windows App", "Полный список программ"),
    ("Стиль Windows Pro", "Плоские списки без плиток"),
    ("Стиль Windows 10X", "Центрированные значки"),
];

const NAV: &[&str] = &["Меню «Пуск»", "Управление", "О программе"];

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    style: usize,
    dark: bool,
    alpha: u8,
    show_search: bool,
    tiles: bool,
    #[serde(default)]
    recent: Vec<String>,
    #[serde(default)]
    start_x: i32,
    #[serde(default)]
    start_y: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            style: 3,
            dark: true,
            alpha: 242,
            show_search: true,
            tiles: true,
            recent: Vec::new(),
            start_x: 0,
            start_y: 0,
        }
    }
}

struct AppEntry {
    name: String,
    target: String,
    icon: Option<egui::TextureHandle>,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Menu,
    Settings,
}

struct Theme {
    bg: egui::Color32,
    text: egui::Color32,
    muted: egui::Color32,
    card: egui::Color32,
    card_hi: egui::Color32,
    accent: egui::Color32,
}

fn theme(dark: bool, alpha: u8) -> Theme {
    // окно непрозрачное: прозрачность меню имитируем затемнением фона
    let mix = |r: u8, g: u8, b: u8| {
        let a = alpha as u32;
        egui::Color32::from_rgb(
            (r as u32 * a / 255) as u8,
            (g as u32 * a / 255) as u8,
            (b as u32 * a / 255) as u8,
        )
    };
    if dark {
        Theme {
            bg: mix(0x20, 0x20, 0x20),
            text: egui::Color32::from_rgb(0xf3, 0xf3, 0xf3),
            muted: egui::Color32::from_rgb(0x9d, 0x9d, 0x9d),
            card: egui::Color32::from_rgb(0x2c, 0x2c, 0x2c),
            card_hi: egui::Color32::from_rgb(0x38, 0x38, 0x38),
            accent: ACCENT,
        }
    } else {
        Theme {
            bg: mix(0xf3, 0xf3, 0xf3),
            text: egui::Color32::from_rgb(0x1a, 0x1a, 0x1a),
            muted: egui::Color32::from_rgb(0x5f, 0x5f, 0x5f),
            card: egui::Color32::from_rgb(0xe6, 0xe6, 0xe6),
            card_hi: egui::Color32::from_rgb(0xd8, 0xd8, 0xd8),
            accent: ACCENT_DARK,
        }
    }
}

fn config_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("OpenStart").join("config.json")
}

fn load_config() -> Config {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn scan_apps() -> Vec<AppEntry> {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    let roots = [
        std::env::var("APPDATA")
            .ok()
            .map(|p| PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs")),
        Some(PathBuf::from(
            r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs",
        )),
    ];
    for root in roots.into_iter().flatten() {
        walk(&root, &mut map);
    }
    map.entry("Проводник".into())
        .or_insert("explorer.exe".into());
    map.entry("Параметры".into()).or_insert("ms-settings:".into());
    map.into_iter()
        .map(|(name, target)| AppEntry {
            name,
            target,
            icon: None,
        })
        .collect()
}

fn attach_icons(ctx: &egui::Context, apps: &mut [AppEntry]) {
    for app in apps.iter_mut() {
        if let Some(img) = load_icon(&app.target) {
            app.icon = Some(ctx.load_texture(
                format!("icon-{}", app.name),
                img,
                egui::TextureOptions::LINEAR,
            ));
        }
    }
}

/// Иконка ярлыка или exe в виде RGBA 32×32.
fn load_icon(target: &str) -> Option<egui::ColorImage> {
    use std::ffi::c_void;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetObjectW, SelectObject,
        BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows_sys::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, DrawIconEx, GetIconInfo, DI_NORMAL, HICON, ICONINFO,
    };

    // у «Параметров» нет файла — берём иконку SystemSettings.exe
    let path = if target.starts_with("ms-settings:") {
        r"C:\Windows\ImmersiveControlPanel\SystemSettings.exe".to_string()
    } else {
        target.to_string()
    };
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let ok = SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || info.hIcon.is_null() {
            return None;
        }
        let hicon: HICON = info.hIcon;
        let mut ii: ICONINFO = std::mem::zeroed();
        if GetIconInfo(hicon, &mut ii) == 0 {
            DestroyIcon(hicon);
            return None;
        }
        let mut bmp: BITMAP = std::mem::zeroed();
        GetObjectW(
            ii.hbmColor,
            std::mem::size_of::<BITMAP>() as i32,
            &mut bmp as *mut _ as *mut c_void,
        );
        let (w, h) = (bmp.bmWidth.max(1), bmp.bmHeight.max(1));

        let hdc = CreateCompatibleDC(std::ptr::null_mut());
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(
            hdc,
            &bi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        let old = SelectObject(hdc, hbmp as HGDIOBJ);
        DrawIconEx(hdc, 0, 0, hicon, w, h, 0, std::ptr::null_mut(), DI_NORMAL);

        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        if !bits.is_null() {
            let raw = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
            for px in raw.chunks_exact(4) {
                // DIB: BGRA
                rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }

        SelectObject(hdc, old);
        DeleteObject(hbmp as HGDIOBJ);
        DeleteDC(hdc);
        if !ii.hbmColor.is_null() {
            DeleteObject(ii.hbmColor);
        }
        if !ii.hbmMask.is_null() {
            DeleteObject(ii.hbmMask);
        }
        DestroyIcon(hicon);

        if rgba.len() == (w * h * 4) as usize {
            Some(egui::ColorImage::from_rgba_unmultiplied(
                [w as usize, h as usize],
                &rgba,
            ))
        } else {
            None
        }
    }
}

fn walk(dir: &Path, out: &mut BTreeMap<String, String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
            continue;
        }
        if !p
            .extension()
            .map(|x| x.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false)
        {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let low = stem.to_lowercase();
        if ["uninstall", "удал", "readme", "справка", "help"]
            .iter()
            .any(|w| low.contains(w))
        {
            continue;
        }
        out.entry(stem.to_string())
            .or_insert(p.display().to_string());
    }
}

fn launch_target(target: &str) {    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let op = wide("open");
    let file = wide(target);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        );
    }
}

fn autostart_enabled() -> bool {
    std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "OpenStart",
        ])
        .creation_flags(0x08000000)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn set_autostart(on: bool) {
    let mut cmd = std::process::Command::new("reg");
    if on {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        cmd.args([
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "OpenStart",
            "/t",
            "REG_SZ",
            "/d",
            &exe,
            "/f",
        ]);
    } else {
        cmd.args([
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "OpenStart",
            "/f",
        ]);
    }
    let _ = cmd.creation_flags(0x08000000).output();
}

fn avatar_color(name: &str) -> egui::Color32 {
    const PALETTE: &[egui::Color32] = &[
        egui::Color32::from_rgb(0x5a, 0x6a, 0x9a),
        egui::Color32::from_rgb(0x7a, 0x5a, 0x8a),
        egui::Color32::from_rgb(0x8a, 0x6a, 0x4a),
        egui::Color32::from_rgb(0x4a, 0x7a, 0x6a),
        egui::Color32::from_rgb(0x8a, 0x5a, 0x5a),
        egui::Color32::from_rgb(0x4f, 0x6f, 0x8f),
    ];
    let sum: u32 = name.bytes().map(|b| b as u32).sum();
    PALETTE[sum as usize % PALETTE.len()]
}

fn letter(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
}

fn short(name: &str, limit: usize) -> String {
    if name.chars().count() > limit {
        format!("{}…", name.chars().take(limit - 1).collect::<String>())
    } else {
        name.to_string()
    }
}

fn menu_size(style: usize) -> (f32, f32) {
    match style {
        0 => (520.0, 600.0),
        1 => (480.0, 590.0),
        2 => (620.0, 620.0),
        3 => (1000.0, 700.0),
        4 => (1000.0, 680.0),
        5 => (640.0, 640.0),
        _ => (860.0, 700.0),
    }
}

fn monitor_size(ctx: &egui::Context) -> egui::Vec2 {
    ctx.input(|i| i.viewport().monitor_size)
        .unwrap_or(egui::vec2(1920.0, 1080.0))
}

/// Снимок экрана под меню — из него делаем размытый фон в духе акрила Windows.
fn capture_backdrop(x: i32, y: i32, w: i32, h: i32) -> Option<egui::ColorImage> {
    use std::ffi::c_void;
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
        HGDIOBJ, SRCCOPY,
    };
    if w <= 0 || h <= 0 {
        return None;
    }
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return None;
        }
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp as HGDIOBJ);
        let ok = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY);

        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        GetDIBits(
            mem,
            bmp,
            0,
            h as u32,
            buf.as_mut_ptr() as *mut c_void,
            &mut bi,
            DIB_RGB_COLORS,
        );

        SelectObject(mem, old);
        DeleteObject(bmp as HGDIOBJ);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);

        if ok == 0 {
            return None;
        }

        // уменьшаем в 20 раз: при растягивании обратно получается мягкое размытие
        let step = 20usize;
        let ow = (w as usize / step).max(1);
        let oh = (h as usize / step).max(1);
        let mut rgba = Vec::with_capacity(ow * oh * 4);
        for yy in 0..oh {
            for xx in 0..ow {
                let i = ((yy * step) * w as usize + xx * step) * 4;
                if i + 3 < buf.len() {
                    rgba.extend_from_slice(&[buf[i + 2], buf[i + 1], buf[i], 255]);
                }
            }
        }
        if rgba.len() != ow * oh * 4 {
            return None;
        }
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [ow, oh],
            &rgba,
        ))
    }
}

/// Скруглить окно средствами DWM (Windows 11).
fn round_window(_w: i32, _h: i32) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    let title: Vec<u16> = "OpenStart\0".encode_utf16().collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd.is_null() {
            return;
        }
        // DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_ROUND = 2
        let round: u32 = 2;
        DwmSetWindowAttribute(hwnd, 33, &round as *const u32 as *const _, 4);
    }
}

fn is_minimized_now() -> bool {
    let title: Vec<u16> = "OpenStart\0".encode_utf16().collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        !hwnd.is_null() && windows_sys::Win32::UI::WindowsAndMessaging::IsIconic(hwnd) != 0
    }
}

/// Вывести окно поверх всех и забрать фокус: без этого меню закрывается сразу.
fn force_foreground() {
    let title: Vec<u16> = "OpenStart\0".encode_utf16().collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd.is_null() {
            return;
        }
        ShowWindow(
            hwnd,
            windows_sys::Win32::UI::WindowsAndMessaging::SW_RESTORE,
        );
        SetWindowPos(
            hwnd,
            windows_sys::Win32::UI::WindowsAndMessaging::HWND_TOPMOST,
            0,
            0,
            0,
            0,
            windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                | windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                | windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
        );
        let fg = GetForegroundWindow();
        let tid_fg = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let tid_me = GetCurrentThreadId();
        let attached = AttachThreadInput(tid_me, tid_fg, 1);
        SetForegroundWindow(hwnd);
        if attached != 0 {
            AttachThreadInput(tid_me, tid_fg, 0);
        }
    }
}

// ---------- перехват клавиши Win ----------

static TOGGLE: AtomicBool = AtomicBool::new(false);
static WIN_DOWN: AtomicBool = AtomicBool::new(false);
static CHORD: AtomicBool = AtomicBool::new(false);
static WIN_AT: Mutex<Option<Instant>> = Mutex::new(None);
/// Прямоугольник кнопки «Пуск»: центр, выученный по клику.
static START_ZONE: Mutex<Option<(i32, i32)>> = Mutex::new(None);
static ZONE_DIRTY: AtomicBool = AtomicBool::new(false);
static MOUSE_SWALLOW: AtomicBool = AtomicBool::new(false);
static LAST_CLICK: Mutex<Option<(i32, i32, Instant)>> = Mutex::new(None);
static START_FOUND: AtomicIsize = AtomicIsize::new(0);
/// Наше меню сейчас на экране.
static MENU_VISIBLE: AtomicBool = AtomicBool::new(false);
/// Системное меню закрыто — пора показать наше.
static SHOW_PENDING: AtomicBool = AtomicBool::new(false);

fn start_hook(ctx: egui::Context) {
    // Хук не должен трогать egui: если обработчик задержится, Windows снимет хук.
    // Отдельный поток замечает флаг и будит окно.
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if TOGGLE.load(Ordering::SeqCst) {
                ctx.request_repaint();
            }
        }
    });
    // системное меню «Пуск» не должно открываться, пока работаем мы:
    // закрываем его сразу, как только оно появилось
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(10));
            let fg = unsafe { foreground_class_and_title() };
            let start_open = fg
                .as_ref()
                .map(|(class, title)| {
                    class == "Windows.UI.Core.CoreWindow" && is_start_title(title)
                })
                .unwrap_or(false);
            if start_open {
                unsafe { close_start_menu() };
                let click = *LAST_CLICK.lock().unwrap();
                if let Some((x, y, at)) = click {
                    if at.elapsed().as_millis() < 1200 && taskbar_contains(x, y) {
                        *START_ZONE.lock().unwrap() = Some((x, y));
                        ZONE_DIRTY.store(true, Ordering::SeqCst);
                    }
                }
                if !MENU_VISIBLE.load(Ordering::SeqCst) {
                    SHOW_PENDING.store(true, Ordering::SeqCst);
                }
                continue;
            }
            // системное меню закрылось — показываем своё, ровно один раз
            if SHOW_PENDING.swap(false, Ordering::SeqCst) {
                TOGGLE.store(true, Ordering::SeqCst);
                wake();
            }
        }
    });
    std::thread::spawn(|| unsafe {
        // прошлый экземпляр мог оставить Win «зажатым» синтетическим нажатием
        send_win_up();
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), std::ptr::null_mut(), 0);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), std::ptr::null_mut(), 0);
        if hook.is_null() {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        UnhookWindowsHookEx(hook);
        if !mouse.is_null() {
            UnhookWindowsHookEx(mouse);
        }
    });
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && lparam != 0 {
        let ms = &*(lparam as *const MSLLHOOKSTRUCT);
        let msg = wparam as u32;
        if msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP {
            let (x, y) = (ms.pt.x, ms.pt.y);
            if msg == WM_LBUTTONDOWN {
                *LAST_CLICK.lock().unwrap() = Some((x, y, Instant::now()));
                if in_start_zone(x, y) {
                    MOUSE_SWALLOW.store(true, Ordering::SeqCst);
                    return 1;
                }
                return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
            }
            // отпускание: продлеваем отметку клика, чтобы сторож успел увидеть меню
            let prev = *LAST_CLICK.lock().unwrap();
            if let Some((lx, ly, _)) = prev {
                *LAST_CLICK.lock().unwrap() = Some((lx, ly, Instant::now()));
            }
            if MOUSE_SWALLOW.swap(false, Ordering::SeqCst) {
                TOGGLE.store(true, Ordering::SeqCst);
                wake();
                return 1;
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// Клик по выученной кнопке «Пуск» внутри панели задач.
fn in_start_zone(x: i32, y: i32) -> bool {
    let zone = *START_ZONE.lock().unwrap();
    let Some((cx, cy)) = zone else {
        return false;
    };
    if (y - cy).abs() > 24 || !taskbar_contains(x, y) {
        return false;
    }
    // справа сразу за кнопкой «Пуск» начинаются кнопки программ — их не трогаем
    let right = tasklist_left().map(|l| l - 2).unwrap_or(i32::MAX);
    x >= cx - 24 && x <= (cx + 24).min(right)
}

/// Левый край области кнопок запущенных программ на панели задач.
fn tasklist_left() -> Option<i32> {
    unsafe {
        let tray = FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
        if tray.is_null() {
            return None;
        }
        let list = FindWindowExW(
            tray,
            std::ptr::null_mut(),
            wide("MSTaskListWClass").as_ptr(),
            std::ptr::null(),
        );
        if list.is_null() {
            return None;
        }
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(list, &mut r) == 0 {
            return None;
        }
        Some(r.left)
    }
}

fn taskbar_contains(x: i32, y: i32) -> bool {
    unsafe {
        let hwnd = FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
        if hwnd.is_null() {
            return false;
        }
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return false;
        }
        x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
    }
}

/// Заголовки системного меню «Пуск» в разных языках.
fn is_start_title(title: &str) -> bool {
    matches!(
        title,
        "Пуск" | "Start" | "Démarrer" | "Startmenü" | "Inicio" | "Avvio" | "Początek" | "开始"
            | "スタート" | "시작" | "Začínáme"
    )
}

/// Класс окна на переднем плане.
unsafe fn foreground_class() -> String {
    let hwnd = GetForegroundWindow();
    if hwnd.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 128];
    let n = GetClassNameW(hwnd, buf.as_mut_ptr(), 128) as usize;
    String::from_utf16_lossy(&buf[..n.min(128)])
}

/// Класс и заголовок окна на переднем плане.
unsafe fn foreground_class_and_title() -> Option<(String, String)> {
    let hwnd = GetForegroundWindow();
    if hwnd.is_null() {
        return None;
    }
    let mut cbuf = [0u16; 128];
    let cn = GetClassNameW(hwnd, cbuf.as_mut_ptr(), 128) as usize;
    let mut tbuf = [0u16; 128];
    let tn = GetWindowTextW(hwnd, tbuf.as_mut_ptr(), 128) as usize;
    Some((
        String::from_utf16_lossy(&cbuf[..cn.min(128)]),
        String::from_utf16_lossy(&tbuf[..tn.min(128)]),
    ))
}

/// Закрывает системное меню «Пуск»: Esc активному окну и WM_CLOSE на всякий случай.
unsafe fn close_start_menu() {
    let hwnd = GetForegroundWindow();
    if hwnd.is_null() {
        return;
    }
    // сначала мгновенно прячем окно, потом закрываем по-настоящему
    ShowWindow(hwnd, SW_HIDE);
    PostMessageW(hwnd, WM_CLOSE, 0, 0);
    send_escape();
}

unsafe fn send_escape() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };
    let mut input: INPUT = std::mem::zeroed();
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: 0x1b,
        wScan: 0,
        dwFlags: 0,
        time: 0,
        dwExtraInfo: 1,
    };
    SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    input.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
    SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
}

unsafe extern "system" fn kb_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && lparam != 0 {
        let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
        // своё синтетическое нажатие (Win для сочетаний) пропускаем без обработки
        if kb.dwExtraInfo == 1 {
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        }
        let down = wparam as u32 == WM_KEYDOWN || wparam as u32 == WM_SYSKEYDOWN;
        let up = wparam as u32 == WM_KEYUP || wparam as u32 == WM_SYSKEYUP;
        let win_key = kb.vkCode == VK_LWIN || kb.vkCode == VK_RWIN;

        // Win глушим всегда, и нажатие и отпускание: одиночный Win-up без
        // проглоченного Win-down система считает тапом и открывает родное меню.
        if win_key {
            if down {
                if !WIN_DOWN.swap(true, Ordering::SeqCst) {
                    CHORD.store(false, Ordering::SeqCst);
                    *WIN_AT.lock().unwrap() = Some(Instant::now());
                }
            } else if up && WIN_DOWN.swap(false, Ordering::SeqCst) {
                if CHORD.swap(false, Ordering::SeqCst) {
                    // парный up к синтетическому down, иначе Win остаётся зажатым
                    send_win_up();
                } else {
                    let tap = WIN_AT
                        .lock()
                        .unwrap()
                        .map(|t| t.elapsed().as_millis() < 700)
                        .unwrap_or(false);
                    if tap {
                        TOGGLE.store(true, Ordering::SeqCst);
                    }
                }
            }
            return 1;
        }
        if down && WIN_DOWN.load(Ordering::SeqCst) {
            if !CHORD.swap(true, Ordering::SeqCst) {
                // это сочетание Win+X: возвращаем системе «зажатый» Win
                send_win_down();
            }
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// Синтетическое отпускание Win — закрывает состояние, открытое send_win_down.
unsafe fn send_win_up() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };
    let mut input: INPUT = std::mem::zeroed();
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: VK_LWIN as u16,
        wScan: 0,
        dwFlags: KEYEVENTF_KEYUP,
        time: 0,
        dwExtraInfo: 1,
    };
    SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
}

/// Синтетическое нажатие Win, чтобы сочетания вроде Win+E продолжали работать.
unsafe fn send_win_down() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT,
    };
    let mut input: INPUT = std::mem::zeroed();
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: VK_LWIN as u16,
        wScan: 0,
        dwFlags: 0,
        time: 0,
        dwExtraInfo: 1,
    };
    SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
}

// ---------- приложение ----------

struct App {
    cfg: Config,
    apps: Vec<AppEntry>,
    query: String,
    mode: Mode,
    visible: bool,
    opened_at: Instant,
    focus_search: bool,
    was_focused: bool,
    focused_at: Instant,
    autostart: bool,
    all_apps: bool,
    power_open: bool,
    was_minimized: bool,
    setting_filter: String,
    backdrop: Option<egui::TextureHandle>,
    page: usize,
    quit: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, settings: bool) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
            fonts.font_data.insert(
                "segoe".into(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                if let Some(list) = fonts.families.get_mut(&family) {
                    list.insert(0, "segoe".into());
                }
            }
        }
        // иконки-глифы Windows (лупа, шеврон, питание, шестерёнка)
        let icons = std::fs::read(r"C:\Windows\Fonts\segmdl2.ttf")
            .or_else(|_| std::fs::read(r"C:\Windows\Fonts\SegoeIcons.ttf"));
        if let Ok(bytes) = icons {
            fonts.font_data.insert(
                "mdl2".into(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                list.push("mdl2".into());
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        start_hook(cc.egui_ctx.clone());

        let mut apps = scan_apps();
        attach_icons(&cc.egui_ctx, &mut apps);

        let cfg = load_config();
        if cfg.start_x != 0 || cfg.start_y != 0 {
            *START_ZONE.lock().unwrap() = Some((cfg.start_x, cfg.start_y));
        }
        let app = App {
            cfg,
            apps,
            query: String::new(),
            mode: Mode::Menu,
            visible: settings,
            opened_at: Instant::now(),
            focus_search: false,
            was_focused: false,
            focused_at: Instant::now(),
            autostart: autostart_enabled(),
            all_apps: false,
            power_open: false,
            was_minimized: false,
            setting_filter: String::new(),
            backdrop: None,
            page: 0,
            quit: false,
        };
        *CTX.lock().unwrap() = Some(cc.egui_ctx.clone());
        tray_init();
        if settings {
            app.center_settings(&cc.egui_ctx);
        } else {
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                egui::pos2(-10000.0, -10000.0),
            ));
        }
        round_window(0, 0);
        app
    }

    fn save(&self) {
        if let Some(dir) = config_path().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.cfg) {
            let _ = std::fs::write(config_path(), text);
        }
    }

    fn center_settings(&self, ctx: &egui::Context) {
        let m = monitor_size(ctx);
        let size = egui::vec2(1330.0, 680.0);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            (m.x - size.x) / 2.0,
            (m.y - size.y) / 2.0 - 20.0,
        )));
    }

    fn show_menu(&mut self, ctx: &egui::Context) {
        self.mode = Mode::Menu;
        self.visible = true;
        MENU_VISIBLE.store(true, Ordering::SeqCst);
        self.opened_at = Instant::now();
        self.focus_search = self.cfg.show_search;
        self.was_focused = false;
        self.focused_at = Instant::now();
        self.all_apps = false;
        self.power_open = false;
        self.query.clear();
        let (w, h) = menu_size(self.cfg.style);
        let m = monitor_size(ctx);
        let x = if self.cfg.style >= 3 {
            (m.x - w) / 2.0
        } else {
            12.0
        };
        let y = (m.y - h - 56.0).max(8.0);
        self.backdrop = capture_backdrop(x as i32, y as i32, w as i32, h as i32).map(|img| {
            ctx.load_texture("backdrop", img, egui::TextureOptions::LINEAR)
        });
        use egui::ViewportCommand as V;
        for cmd in [
            V::Decorations(false),
            V::WindowLevel(egui::WindowLevel::AlwaysOnTop),
            V::Minimized(false),
            V::InnerSize(egui::vec2(w, h)),
            V::OuterPosition(egui::pos2(x, y)),
            V::Visible(true),
            V::Focus,
        ] {
            ctx.send_viewport_cmd(cmd);
        }
        round_window(w as i32, h as i32);
        ctx.request_repaint();
    }

    fn show_settings(&mut self, ctx: &egui::Context) {
        self.mode = Mode::Settings;
        self.visible = true;
        self.center_settings(ctx);
        use egui::ViewportCommand as V;
        for cmd in [
            V::Decorations(true),
            V::WindowLevel(egui::WindowLevel::Normal),
            V::Visible(true),
            V::Focus,
        ] {
            ctx.send_viewport_cmd(cmd);
        }
        ctx.request_repaint();
    }

    fn hide(&mut self, ctx: &egui::Context) {
        self.visible = false;
        MENU_VISIBLE.store(false, Ordering::SeqCst);
        // не Visible(false): скрытое окно нельзя вывести на передний план по Win.
        // Уводим за край экрана и оставляем показанным.
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            -10000.0, -10000.0,
        )));
    }

    fn quit(&mut self, ctx: &egui::Context) {
        self.quit = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn remember(&mut self, target: &str) {
        self.cfg.recent.retain(|t| t != target);
        self.cfg.recent.insert(0, target.to_string());
        self.cfg.recent.truncate(6);
        self.save();
    }

    fn recent_indices(&self) -> Vec<usize> {
        self.cfg
            .recent
            .iter()
            .filter_map(|t| self.apps.iter().position(|a| &a.target == t))
            .take(6)
            .collect()
    }

    fn left_pane(&mut self, ui: &mut egui::Ui, t: &Theme, modern: bool) -> Option<String> {
        let cols = if modern { 6 } else { 4 };
        let tile_w = if modern { 96.0 } else { 88.0 };
        let tile_h = 84.0;
        let mut launch: Option<String> = None;

        if self.cfg.show_search {
            let hint = if modern {
                "Поиск приложений и файлов"
            } else {
                "Поиск приложений"
            };
            self.search_box(ui, t, modern, hint);
            ui.add_space(16.0);
        }

        let q = self.query.trim().to_lowercase();
        let matching: Vec<usize> = self
            .apps
            .iter()
            .enumerate()
            .filter(|(_, a)| q.is_empty() || a.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();

        if !q.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt("results")
                .auto_shrink([false, false])
                .max_height((ui.available_height() - 8.0).max(80.0))
                .show(ui, |ui| {
                    for &idx in &matching {
                        if row(ui, &self.apps[idx], t).clicked() {
                            launch = Some(self.apps[idx].target.clone());
                        }
                    }
                });
            return launch;
        }

        if self.all_apps {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("\u{E72B}")
                                .font(egui::FontId::proportional(13.0))
                                .color(t.text),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.all_apps = false;
                }
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Все приложения")
                        .size(13.0)
                        .strong()
                        .color(t.text),
                );
            });
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .id_salt("all")
                .auto_shrink([false, false])
                .max_height((ui.available_height() - 8.0).max(80.0))
                .show(ui, |ui| {
                    for &idx in &matching {
                        if row(ui, &self.apps[idx], t).clicked() {
                            launch = Some(self.apps[idx].target.clone());
                        }
                    }
                });
            return launch;
        }

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Закреплено")
                    .size(13.0)
                    .strong()
                    .color(t.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn = egui::Button::new(
                    egui::RichText::new("Все приложения  \u{E76C}")
                        .size(12.0)
                        .color(t.text),
                )
                .fill(t.card)
                .corner_radius(egui::CornerRadius::same(6))
                .min_size(egui::vec2(0.0, 26.0));
                if ui.add(btn).clicked() {
                    self.all_apps = true;
                }
            });
        });
        ui.add_space(10.0);

        let take = (cols * 3).min(matching.len());
        egui::Grid::new("pins")
            .num_columns(cols)
            .spacing([4.0, 4.0])
            .show(ui, |ui| {
                for (n, &idx) in matching.iter().take(take).enumerate() {
                    if tile(ui, &self.apps[idx], t, tile_w, tile_h).clicked() {
                        launch = Some(self.apps[idx].target.clone());
                    }
                    if (n + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });

        ui.add_space(14.0);
        ui.label(
            egui::RichText::new("Рекомендуем")
                .size(13.0)
                .strong()
                .color(t.text),
        );
        ui.add_space(6.0);
        let recent = self.recent_indices();
        if recent.is_empty() {
            ui.label(
                egui::RichText::new("Недавние программы появятся здесь")
                    .size(11.5)
                    .color(t.muted),
            );
        } else {
            egui::Grid::new("recent")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    for (n, idx) in recent.iter().enumerate() {
                        if rec_row(ui, &self.apps[*idx], t).clicked() {
                            launch = Some(self.apps[*idx].target.clone());
                        }
                        if n % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        }
        launch
    }

    fn search_box(&mut self, ui: &mut egui::Ui, t: &Theme, round: bool, hint: &str) {
        let mut resp = None;
        egui::Frame::NONE
            .fill(t.card)
            .stroke(egui::Stroke::new(1.0, t.card_hi))
            .corner_radius(egui::CornerRadius::same(if round { 20 } else { 6 }))
            .inner_margin(egui::Margin {
                left: 14,
                right: 14,
                top: 9,
                bottom: 9,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("\u{E721}")
                            .font(egui::FontId::proportional(14.0))
                            .color(t.muted),
                    );
                    ui.add_space(8.0);
                    resp = Some(ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text(hint)
                            .desired_width(f32::INFINITY)
                            .frame(false),
                    ));
                });
            });
        if let Some(r) = resp {
            if self.focus_search {
                r.request_focus();
                self.focus_search = false;
            }
        }
    }

    fn matching(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        self.apps
            .iter()
            .enumerate()
            .filter(|(_, a)| q.is_empty() || a.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    fn footer_bar(&mut self, ui: &mut egui::Ui, t: &Theme, user: &str, small: bool) -> bool {
        let mut open_settings = false;
        let d = if small { 26.0 } else { 28.0 };
        ui.horizontal(|ui| {
            let (av, _) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::hover());
            ui.painter().circle_filled(av.center(), d / 2.0, t.card_hi);
            ui.painter().text(
                av.center(),
                egui::Align2::CENTER_CENTER,
                letter(user),
                egui::FontId::proportional(if small { 11.0 } else { 12.0 }),
                t.text,
            );
            ui.add_space(10.0);
            ui.label(egui::RichText::new(user).size(13.0).color(t.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let power = ui.add(
                    egui::Button::new(
                        egui::RichText::new("\u{E7E8}")
                            .font(egui::FontId::proportional(15.0))
                            .color(t.text),
                    )
                    .frame(false),
                );
                if power.clicked() {
                    self.power_open = !self.power_open;
                }
                ui.add_space(10.0);
                let gear = ui.add(
                    egui::Button::new(
                        egui::RichText::new("\u{E713}")
                            .font(egui::FontId::proportional(15.0))
                            .color(t.muted),
                    )
                    .frame(false),
                );
                if gear.clicked() {
                    open_settings = true;
                }
            });
        });
        open_settings
    }

    fn app_list_pane(&mut self, ui: &mut egui::Ui, t: &Theme, back: bool) -> Option<String> {
        let mut launch = None;
        if self.cfg.show_search {
            self.search_box(ui, t, true, "Поиск приложений и файлов");
            ui.add_space(12.0);
        }
        if back {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("\u{E72B}")
                                .font(egui::FontId::proportional(13.0))
                                .color(t.text),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.all_apps = false;
                }
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Все приложения")
                        .size(13.0)
                        .strong()
                        .color(t.text),
                );
            });
            ui.add_space(8.0);
        } else {
            ui.label(
                egui::RichText::new("Все приложения")
                    .size(13.0)
                    .strong()
                    .color(t.text),
            );
            ui.add_space(6.0);
        }
        let list = self.matching();
        egui::ScrollArea::vertical()
            .id_salt("app-list")
            .auto_shrink([false, false])
            .max_height((ui.available_height() - 56.0).max(80.0))
            .show(ui, |ui| {
                for &idx in &list {
                    if row(ui, &self.apps[idx], t).clicked() {
                        launch = Some(self.apps[idx].target.clone());
                    }
                }
            });
        launch
    }

    fn pro_pane(&mut self, ui: &mut egui::Ui, t: &Theme) -> Option<String> {
        let mut launch = None;
        if self.cfg.show_search {
            self.search_box(ui, t, false, "Поиск приложений");
            ui.add_space(12.0);
        }
        ui.label(
            egui::RichText::new("Закреплено")
                .size(13.0)
                .strong()
                .color(t.text),
        );
        ui.add_space(6.0);
        let list = self.matching();
        let w = ((ui.available_width() - 10.0) / 2.0).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("pro")
            .auto_shrink([false, false])
            .max_height((ui.available_height() - 56.0).max(80.0))
            .show(ui, |ui| {
                egui::Grid::new("pro-grid")
                    .num_columns(2)
                    .spacing([10.0, 2.0])
                    .show(ui, |ui| {
                        for (n, &idx) in list.iter().enumerate() {
                            if row_w(ui, &self.apps[idx], t, w).clicked() {
                                launch = Some(self.apps[idx].target.clone());
                            }
                            if n % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
            });
        launch
    }

    fn tenx_pane(&mut self, ui: &mut egui::Ui, t: &Theme) -> Option<String> {
        if self.all_apps {
            return self.app_list_pane(ui, t, true);
        }
        let mut launch = None;
        if self.cfg.show_search {
            ui.vertical_centered(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(420.0, 34.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.search_box(ui, t, true, "Поиск"),
                );
            });
            ui.add_space(14.0);
        }
        let list = self.matching();
        let cols = 5;
        let take = (cols * 3).min(list.len());
        let pad = ((ui.available_width() - cols as f32 * 102.0) / 2.0).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(pad);
            egui::Grid::new("tenx")
                .num_columns(cols)
                .spacing([6.0, 6.0])
                .show(ui, |ui| {
                    for (n, &idx) in list.iter().take(take).enumerate() {
                        if tile(ui, &self.apps[idx], t, 96.0, 92.0).clicked() {
                            launch = Some(self.apps[idx].target.clone());
                        }
                        if (n + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                });
        });
        if !self.query.trim().is_empty() {
            return launch;
        }
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            let btn = egui::Button::new(
                egui::RichText::new("Все приложения  \u{E76C}")
                    .size(12.0)
                    .color(t.text),
            )
            .fill(t.card)
            .corner_radius(egui::CornerRadius::same(16))
            .min_size(egui::vec2(0.0, 30.0));
            if ui.add(btn).clicked() {
                self.all_apps = true;
            }
        });
        launch
    }

    fn win7_pane(
        &mut self,
        ui: &mut egui::Ui,
        t: &Theme,
        user: &str,
    ) -> (Option<String>, bool) {
        let h = ui.available_height();
        let mut launch = None;
        let mut open_settings = false;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2((ui.available_width() - 180.0).max(180.0), h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if self.cfg.show_search {
                        self.search_box(ui, t, false, "Поиск программ");
                        ui.add_space(8.0);
                    }
                    let list = self.matching();
                    egui::ScrollArea::vertical()
                        .id_salt("win7-list")
                        .auto_shrink([false, false])
                        .max_height((ui.available_height() - 46.0).max(80.0))
                        .show(ui, |ui| {
                            for &idx in &list {
                                if row(ui, &self.apps[idx], t).clicked() {
                                    launch = Some(self.apps[idx].target.clone());
                                }
                            }
                        });
                    ui.add_space(6.0);
                    if self.footer_bar(ui, t, user, true) {
                        open_settings = true;
                    }
                },
            );
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let links = [
                        ("\u{E8B7}", "Документы"),
                        ("\u{E896}", "Загрузки"),
                        ("\u{E8D6}", "Музыка"),
                        ("\u{EB9F}", "Изображения"),
                        ("\u{E713}", "Параметры"),
                        ("\u{E7F4}", "Панель управления"),
                    ];
                    for (i, (icon, label)) in links.iter().enumerate() {
                        if link_row(ui, icon, label, t).clicked() {
                            match i {
                                0 => open_folder("Documents"),
                                1 => open_folder("Downloads"),
                                2 => open_folder("Music"),
                                3 => open_folder("Pictures"),
                                4 => launch_target("ms-settings:"),
                                _ => run_detached("control.exe", &[]),
                            }
                        }
                    }
                },
            );
        });
        (launch, open_settings)
    }

    fn win10_pane(
        &mut self,
        ui: &mut egui::Ui,
        t: &Theme,
        user: &str,
    ) -> (Option<String>, bool) {
        let h = ui.available_height();
        let mut launch = None;
        let mut open_settings = false;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(196.0, h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if self.cfg.show_search {
                        self.search_box(ui, t, false, "Поиск в Windows");
                        ui.add_space(8.0);
                    }
                    let list = self.matching();
                    egui::ScrollArea::vertical()
                        .id_salt("win10-list")
                        .auto_shrink([false, false])
                        .max_height((ui.available_height() - 46.0).max(80.0))
                        .show(ui, |ui| {
                            for &idx in &list {
                                if row(ui, &self.apps[idx], t).clicked() {
                                    launch = Some(self.apps[idx].target.clone());
                                }
                            }
                        });
                    ui.add_space(6.0);
                    if self.footer_bar(ui, t, user, true) {
                        open_settings = true;
                    }
                },
            );
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.label(
                        egui::RichText::new("Закреплено")
                            .size(13.0)
                            .strong()
                            .color(t.text),
                    );
                    ui.add_space(8.0);
                    let list = self.matching();
                    egui::Grid::new("win10-tiles")
                        .num_columns(3)
                        .spacing([6.0, 6.0])
                        .show(ui, |ui| {
                            for (n, &idx) in list.iter().take(12).enumerate() {
                                if tile(ui, &self.apps[idx], t, 104.0, 86.0).clicked() {
                                    launch = Some(self.apps[idx].target.clone());
                                }
                                if (n + 1) % 3 == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                },
            );
        });
        (launch, open_settings)
    }

    fn menu_ui(&mut self, ctx: &egui::Context) {
        let minimized = ctx.input(|i| i.viewport().minimized).unwrap_or(false);
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
        if !minimized {
            if focused {
                if !self.was_focused {
                    self.focused_at = Instant::now();
                }
                self.was_focused = true;
            } else if !self.was_focused && self.opened_at.elapsed().as_millis() < 900 {
                force_foreground();
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            if self.was_focused
                && !focused
                && self.opened_at.elapsed().as_millis() > 400
                && self.focused_at.elapsed().as_millis() > 250
                && !is_minimized_now()
            {
                self.hide(ctx);
                return;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.hide(ctx);
            return;
        }

        let modern = self.cfg.style == 3;
        let t = theme(self.cfg.dark, self.cfg.alpha);
        let margin = if self.cfg.style >= 3 { 22 } else { 10 };
        let user = std::env::var("USERNAME").unwrap_or_default();

        let mut launch: Option<String> = None;
        let mut open_settings = false;

        if let Some(tex) = &self.backdrop {
            let full = ctx.screen_rect();
            ctx.layer_painter(egui::LayerId::background())
                .image(
                    tex.id(),
                    full,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
        }
        let bg = if self.backdrop.is_some() {
            if self.cfg.dark {
                egui::Color32::from_rgba_unmultiplied(0x1c, 0x1c, 0x1c, 228)
            } else {
                egui::Color32::from_rgba_unmultiplied(0xf3, 0xf3, 0xf3, 232)
            }
        } else {
            t.bg
        };
        let overlay_bg = bg;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(overlay_bg)
                    .inner_margin(egui::Margin {
                        left: margin,
                        right: margin,
                        top: 18,
                        bottom: 14,
                    }),
            )
            .show(ctx, |ui| {
                if self.cfg.style >= 4 {
                    let small = self.cfg.style == 6;
                    launch = match self.cfg.style {
                        4 => self.app_list_pane(ui, &t, false),
                        5 => self.pro_pane(ui, &t),
                        _ => self.tenx_pane(ui, &t),
                    };
                    ui.add_space((ui.available_height() - 34.0).max(6.0));
                    if self.footer_bar(ui, &t, &user, small) {
                        open_settings = true;
                    }
                } else if self.cfg.style == 0 {
                    let (l, s) = self.win7_pane(ui, &t, &user);
                    launch = l;
                    open_settings |= s;
                } else if self.cfg.style == 2 {
                    let (l, s) = self.win10_pane(ui, &t, &user);
                    launch = l;
                    open_settings |= s;
                } else if !modern {
                    ui.label(
                        egui::RichText::new("OpenStart")
                            .color(t.accent)
                            .size(15.0)
                            .strong(),
                    );
                    ui.add_space(6.0);
                    launch = self.left_pane(ui, &t, false);

                    ui.add_space((ui.available_height() - 34.0).max(6.0));
                    if self.footer_bar(ui, &t, &user, false) {
                        open_settings = true;
                    }
                } else {
                    let full_w = ui.available_width();
                    let full_h = ui.available_height();
                    let left_w = (full_w * 0.66).floor();

                    ui.horizontal_top(|ui| {
                        let mut left_launch = None;
                        ui.allocate_ui_with_layout(
                            egui::vec2(left_w, full_h),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                left_launch = self.left_pane(ui, &t, true);
                            },
                        );
                        launch = left_launch;

                        let (line, _) = ui.allocate_exact_size(
                            egui::vec2(1.0, full_h),
                            egui::Sense::hover(),
                        );
                        ui.painter().vline(
                            line.center().x,
                            line.y_range(),
                            egui::Stroke::new(1.0, t.card_hi),
                        );
                        ui.add_space(18.0);

                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), full_h),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.vertical_centered(|ui| {
                                    let (av, _) = ui.allocate_exact_size(
                                        egui::vec2(88.0, 88.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().circle_filled(av.center(), 44.0, t.card_hi);
                                    ui.painter().text(
                                        av.center(),
                                        egui::Align2::CENTER_CENTER,
                                        letter(&user),
                                        egui::FontId::proportional(34.0),
                                        t.text,
                                    );
                                    ui.add_space(10.0);
                                    ui.label(
                                        egui::RichText::new(&user).size(21.0).color(t.text),
                                    );
                                });
                                ui.add_space(22.0);

                                let links: [(&str, String); 8] = [
                                    ("\u{E8B7}", "Документы".into()),
                                    ("\u{E896}", "Загрузки".into()),
                                    ("\u{E8D6}", "Музыка".into()),
                                    ("\u{EB9F}", "Изображения".into()),
                                    ("\u{E77B}", user.clone()),
                                    ("\u{E8B7}", "Панель управления".into()),
                                    ("\u{E713}", "Параметры".into()),
                                    ("\u{E756}", "Выполнить...".into()),
                                ];
                                let mut action: Option<usize> = None;
                                for (i, (icon, label)) in links.iter().enumerate() {
                                    if link_row(ui, icon, label, &t).clicked() {
                                        action = Some(i);
                                    }
                                }
                                if let Some(i) = action {
                                    match i {
                                        0 => open_folder("Documents"),
                                        1 => open_folder("Downloads"),
                                        2 => open_folder("Music"),
                                        3 => open_folder("Pictures"),
                                        4 => open_folder(""),
                                        5 => run_detached("control.exe", &[]),
                                        6 => launch_target("ms-settings:"),
                                        _ => {
                                            run_detached("rundll32.exe", &["shell32.dll,#61"])
                                        }
                                    }
                                    self.hide(ctx);
                                }

                                ui.add_space((ui.available_height() - 34.0).max(6.0));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let power = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new("\u{E7E8}")
                                                    .font(egui::FontId::proportional(16.0))
                                                    .color(t.text),
                                            )
                                            .frame(false),
                                        );
                                        if power.clicked() {
                                            self.power_open = !self.power_open;
                                        }
                                        ui.add_space(8.0);
                                        let gear = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new("\u{E713}")
                                                    .font(egui::FontId::proportional(16.0))
                                                    .color(t.muted),
                                            )
                                            .frame(false),
                                        );
                                        if gear.clicked() {
                                            open_settings = true;
                                        }
                                    },
                                );
                            },
                        );
                    });
                }

                if open_settings {
                    self.show_settings(ctx);
                }
                if let Some(target) = launch {
                    self.remember(&target);
                    launch_target(&target);
                    self.hide(ctx);
                }
            });

        if self.power_open {
            egui::Area::new(egui::Id::new("power-menu"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(12.0, 12.0))
                .show(ctx, |ui| {
                    egui::Frame::NONE
                        .fill(t.card)
                        .stroke(egui::Stroke::new(1.0, t.card_hi))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                if ui.button("Спящий режим").clicked() {
                                    run_detached(
                                        "rundll32.exe",
                                        &["powrprof.dll,SetSuspendState", "0,1,0"],
                                    );
                                    self.power_open = false;
                                }
                                if ui.button("Завершение работы").clicked() {
                                    run_detached("shutdown", &["/s", "/t", "0"]);
                                    self.power_open = false;
                                }
                                if ui.button("Перезагрузка").clicked() {
                                    run_detached("shutdown", &["/r", "/t", "0"]);
                                    self.power_open = false;
                                }
                            });
                        });
                });
        }
    }

    fn settings_ui(&mut self, ctx: &egui::Context) {
        let mut changed = false;

        egui::SidePanel::left("nav")
            .exact_width(SIDEBAR_W)
            .resizable(false)
            .frame(
                egui::Frame::NONE.fill(SIDE_BG).inner_margin(egui::Margin {
                    left: 22,
                    right: 18,
                    top: 16,
                    bottom: 16,
                }),
            )
            .show(ctx, |ui| {
                let back = ui.add(
                    egui::Button::new(egui::RichText::new("\u{E72B}").size(18.0).color(INK_DIM))
                        .frame(false),
                );
                if back.clicked() {
                    if self.page >= 3 {
                        self.page = 0;
                    } else {
                        self.show_menu(ctx);
                    }
                }
                ui.add_space(10.0);

                egui::Frame::NONE
                    .stroke(egui::Stroke::new(1.0, PANEL_LINE))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 10,
                        top: 7,
                        bottom: 7,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.setting_filter)
                                    .hint_text("Найти параметр")
                                    .desired_width((ui.available_width() - 22.0).max(40.0))
                                    .frame(false),
                            );
                            ui.label(
                                egui::RichText::new("\u{E721}")
                                    .font(egui::FontId::proportional(13.0))
                                    .color(INK_DIM),
                            );
                        });
                    });
                ui.add_space(18.0);

                ui.spacing_mut().item_spacing.y = 4.0;
                for (page, icon, name) in [
                    (0, "\u{E80F}", "Меню «Пуск»"),
                    (3, "\u{E70F}", "Внешний вид"),
                    (4, "\u{E790}", "Цвета"),
                    (5, "\u{E721}", "Поиск"),
                    (1, "\u{E713}", "Управление"),
                    (2, "\u{E946}", "О программе"),
                ] {
                    if nav_item(ui, icon, name, self.page == page).clicked() {
                        self.page = page;
                    }
                }
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE.fill(BG).inner_margin(egui::Margin {
                    left: 38,
                    right: 34,
                    top: 26,
                    bottom: 24,
                }),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        0 => {
                            changed = self.page_styles(ui);
                        }
                        1 => {
                            changed = self.page_manage(ui, ctx);
                        }
                        2 => self.page_about(ui),
                        3 => {
                            changed = self.page_appearance(ui);
                        }
                        4 => {
                            changed = self.page_colors(ui);
                        }
                        _ => {
                            changed = self.page_search(ui);
                        }
                    });
            });

        if changed {
            self.save();
        }
    }

    fn page_styles(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;

        page_title(ui, "Меню «Пуск»");

        let card_w = ((ui.available_width() - 30.0) / 4.0).max(180.0);
        let filter = self.setting_filter.trim().to_lowercase();
        egui::Grid::new("style-cards")
            .num_columns(4)
            .spacing([10.0, 12.0])
            .show(ui, |ui| {
                let mut shown = 0;
                for (i, (name, desc)) in STYLES.iter().enumerate() {
                    if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    if style_card(ui, i, name, desc, self.cfg.style == i, card_w).clicked() {
                        self.cfg.style = i;
                        changed = true;
                    }
                    shown += 1;
                    if shown % 4 == 0 {
                        ui.end_row();
                    }
                }
            });

        ui.add_space(14.0);
        let mut goto = None;
        setting_group(ui, |ui| {
            if nav_row(
                ui,
                "\u{E70F}",
                "Внешний вид",
                "Цвета, прозрачность, поиск и плитки",
            )
            .clicked()
            {
                goto = Some(3);
            }
            if nav_row(ui, "\u{E790}", "Цвета", "Тёмная или светлая схема меню").clicked() {
                goto = Some(4);
            }
            if nav_row(ui, "\u{E713}", "Управление", "Автозапуск, настройки и выход").clicked() {
                goto = Some(1);
            }
        });
        if let Some(page) = goto {
            self.page = page;
        }
        changed
    }

    fn page_appearance(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;

        page_title(ui, "Внешний вид");

        section_title(ui, "Меню «Пуск»");
        let mut dark = self.cfg.dark;
        let mut search = self.cfg.show_search;
        let mut tiles = self.cfg.tiles;
        setting_group(ui, |ui| {
            changed |= switch_row(
                ui,
                "\u{E706}",
                "Тёмное меню",
                "Тёмная или светлая схема меню «Пуск»",
                &mut dark,
            );
            changed |= switch_row(
                ui,
                "\u{E721}",
                "Строка поиска",
                "Искать программы прямо в меню",
                &mut search,
            );
            changed |= switch_row(
                ui,
                "\u{E80F}",
                "Плитки закреплённых",
                "Плитки сверху в стилях с сеткой",
                &mut tiles,
            );
        });
        self.cfg.dark = dark;
        self.cfg.show_search = search;
        self.cfg.tiles = tiles;

        ui.add_space(18.0);
        section_title(ui, "Прозрачность");
        let mut alpha = self.cfg.alpha as i32;
        setting_group(ui, |ui| {
            changed |= slider_row(
                ui,
                "\u{E790}",
                "Непрозрачность меню",
                "Насколько меню пропускает фон рабочего стола",
                &mut alpha,
                80,
                255,
            );
        });
        self.cfg.alpha = alpha as u8;

        ui.add_space(14.0);
        ui.label(
            egui::RichText::new("Изменения применяются при следующем открытии меню.")
                .size(12.0)
                .color(INK_DIM),
        );
        changed
    }

    fn page_colors(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;

        page_title(ui, "Цвета");

        section_title(ui, "Схема меню");
        let mut dark = self.cfg.dark;
        setting_group(ui, |ui| {
            changed |= switch_row(
                ui,
                "\u{E706}",
                "Тёмная схема",
                "Выключите для светлого меню",
                &mut dark,
            );
        });
        self.cfg.dark = dark;

        ui.add_space(18.0);
        section_title(ui, "Фон");
        let mut alpha = self.cfg.alpha as i32;
        setting_group(ui, |ui| {
            changed |= slider_row(
                ui,
                "\u{E790}",
                "Непрозрачность фона",
                "Плотность фона меню",
                &mut alpha,
                80,
                255,
            );
        });
        self.cfg.alpha = alpha as u8;

        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(
                "Цвет акцента — золотой (#e0af68). Свои цветовые схемы пока не поддерживаются.",
            )
            .size(12.0)
            .color(INK_DIM),
        );
        changed
    }

    fn page_search(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;

        page_title(ui, "Поиск");

        section_title(ui, "В меню «Пуск»");
        let mut search = self.cfg.show_search;
        setting_group(ui, |ui| {
            changed |= switch_row(
                ui,
                "\u{E721}",
                "Строка поиска",
                "Поле поиска в верхней части меню",
                &mut search,
            );
        });
        self.cfg.show_search = search;

        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(
                "Поиск идёт по ярлыкам меню «Пуск» Windows и именам программ. Всё локально, индекс не создаётся и сеть не используется.",
            )
            .size(12.0)
            .color(INK_DIM),
        );
        changed
    }

    fn page_manage(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> bool {
        page_title(ui, "Управление");

        section_title(ui, "Запуск");
        let mut auto = self.autostart;
        let found = format!("Найдено программ: {}", self.apps.len());
        let mut rescan = false;
        setting_group(ui, |ui| {
            if switch_row(
                ui,
                "\u{E768}",
                "Запускать при входе в Windows",
                "Меню будет готово сразу после входа",
                &mut auto,
            ) {
                set_autostart(auto);
            }
            rescan = nav_row(ui, "\u{E72C}", "Пересканировать программы", &found).clicked();
        });
        self.autostart = auto;
        if rescan {
            self.apps = scan_apps();
            attach_icons(ctx, &mut self.apps);
        }

        ui.add_space(18.0);
        section_title(ui, "Настройки");
        let mut open_dir = false;
        let mut reset = false;
        let mut quit = false;
        let dir = config_path().display().to_string();
        setting_group(ui, |ui| {
            open_dir = nav_row(ui, "\u{E838}", "Открыть папку настроек", &dir).clicked();
            reset = nav_row(
                ui,
                "\u{E777}",
                "Сбросить настройки",
                "Вернуть стиль Windows 11 и параметры по умолчанию",
            )
            .clicked();
            quit = nav_row(ui, "\u{E7E8}", "Выход", "Закрыть OpenStart").clicked();
        });
        if open_dir {
            if let Some(dir) = config_path().parent() {
                let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
            }
        }
        if reset {
            self.cfg = Config::default();
            self.save();
        }
        if quit {
            self.quit(ctx);
        }

        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(
                "Меню открывается по клавише Win. Настройки хранятся в собственном файле, \
                 реестр не трогается.",
            )
            .size(12.0)
            .color(INK_DIM),
        );
        false
    }

    fn page_about(&mut self, ui: &mut egui::Ui) {
        page_title(ui, "О программе");

        ui.horizontal(|ui| {
            let (logo, _) =
                ui.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(logo, egui::CornerRadius::same(14), ACCENT);
            painter.text(
                logo.center(),
                egui::Align2::CENTER_CENTER,
                "\u{E74A}",
                egui::FontId::proportional(28.0),
                egui::Color32::BLACK,
            );
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("OpenStart")
                        .size(22.0)
                        .strong()
                        .color(INK),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Меню «Пуск» для Windows")
                        .size(12.5)
                        .color(INK_DIM),
                );
            });
        });

        ui.add_space(20.0);
        section_title(ui, "Сборка");
        let mut github = false;
        setting_group(ui, |ui| {
            info_row(
                ui,
                "\u{E946}",
                &format!("Версия {}", env!("CARGO_PKG_VERSION")),
                "Собственная реализация на Rust, без реестра",
            );
            github =
                nav_row(ui, "\u{E774}", "GitHub и релизы", "github.com/Erzkanzler2k/OpenStart")
                    .clicked();
            info_row(
                ui,
                "\u{E73E}",
                "Лицензия",
                "MIT",
            );
        });
        if github {
            launch_target("https://github.com/Erzkanzler2k/OpenStart");
        }

        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(
                "Не делает: замену панели задач, UWP-программы без ярлыков, ручное \
                 закрепление плиток.",
            )
            .size(12.0)
            .color(INK_DIM),
        );
    }

}

/// Общие цвета окна настроек.
const SIDEBAR_W: f32 = 300.0;
const SIDE_BG: egui::Color32 = egui::Color32::from_rgb(0x10, 0x10, 0x13);
const BG: egui::Color32 = egui::Color32::from_rgb(0x14, 0x14, 0x17);
const PANEL_BG: egui::Color32 = egui::Color32::from_rgb(0x1a, 0x1a, 0x1e);
const PANEL_LINE: egui::Color32 = egui::Color32::from_rgb(0x26, 0x26, 0x2b);
const INK: egui::Color32 = egui::Color32::from_rgb(0xe9, 0xe9, 0xed);
const INK_DIM: egui::Color32 = egui::Color32::from_rgb(0x9a, 0x9a, 0xa4);
const ROW_HOVER: egui::Color32 = egui::Color32::from_rgb(0x1f, 0x1f, 0x24);

fn page_title(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(30.0).color(INK));
    ui.add_space(20.0);
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(13.0).strong().color(INK));
    ui.add_space(8.0);
}

/// Рамка-группа: строки рисуются без отступов, фон один на всю группу.
fn setting_group(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(PANEL_BG)
        .stroke(egui::Stroke::new(1.0_f32, PANEL_LINE))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(1))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
}

fn row_base(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    desc: &str,
    sense: egui::Sense,
) -> (egui::Rect, egui::Response) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), sense);
    let painter = ui.painter();
    if resp.hovered() && sense.senses_click() {
        painter.rect_filled(rect, egui::CornerRadius::ZERO, ROW_HOVER);
    }
    let text_x = if icon.is_empty() {
        rect.min.x + 24.0
    } else {
        painter.text(
            egui::pos2(rect.min.x + 26.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::proportional(18.0),
            INK_DIM,
        );
        rect.min.x + 60.0
    };
    painter.text(
        egui::pos2(text_x, rect.min.y + 13.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::proportional(13.5),
        INK,
    );
    painter.text(
        egui::pos2(text_x, rect.min.y + 31.0),
        egui::Align2::LEFT_TOP,
        desc,
        egui::FontId::proportional(11.5),
        INK_DIM,
    );
    painter.hline(
        rect.x_range(),
        rect.max.y,
        egui::Stroke::new(1.0_f32, PANEL_LINE),
    );
    (rect, resp)
}

fn nav_row(ui: &mut egui::Ui, icon: &str, title: &str, desc: &str) -> egui::Response {
    let (rect, resp) = row_base(ui, icon, title, desc, egui::Sense::click());
    ui.painter().text(
        egui::pos2(rect.max.x - 26.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        "\u{E76C}",
        egui::FontId::proportional(13.0),
        egui::Color32::from_rgb(0x88, 0x88, 0x92),
    );
    resp
}

fn info_row(ui: &mut egui::Ui, icon: &str, title: &str, desc: &str) {
    row_base(ui, icon, title, desc, egui::Sense::hover());
}

fn switch_row(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    desc: &str,
    on: &mut bool,
) -> bool {
    let (rect, resp) = row_base(ui, icon, title, desc, egui::Sense::click());
    let pill = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - 66.0, rect.center().y - 11.0),
        egui::vec2(40.0, 22.0),
    );
    let painter = ui.painter();
    painter.text(
        egui::pos2(pill.min.x - 12.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        if *on { "Вкл" } else { "Выкл" },
        egui::FontId::proportional(12.0),
        INK_DIM,
    );
    painter.rect_filled(
        pill,
        egui::CornerRadius::same(11),
        if *on {
            ACCENT
        } else {
            egui::Color32::from_rgb(0x3a, 0x3a, 0x42)
        },
    );
    let knob_x = if *on {
        pill.max.x - 11.0
    } else {
        pill.min.x + 11.0
    };
    painter.circle_filled(
        egui::pos2(knob_x, pill.center().y),
        8.0,
        egui::Color32::WHITE,
    );
    if resp.clicked() {
        *on = !*on;
        return true;
    }
    false
}

fn slider_row(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    desc: &str,
    value: &mut i32,
    min: i32,
    max: i32,
) -> bool {
    let (rect, resp) = row_base(ui, icon, title, desc, egui::Sense::click_and_drag());
    let track = egui::Rect::from_min_max(
        egui::pos2(rect.max.x - 300.0, rect.center().y - 2.0),
        egui::pos2(rect.max.x - 26.0, rect.center().y + 2.0),
    );
    let mut changed = false;
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.clicked() || resp.dragged() {
            let f = ((pos.x - track.min.x) / track.width()).clamp(0.0, 1.0);
            let v = min + (f * (max - min) as f32).round() as i32;
            if v != *value {
                *value = v;
                changed = true;
            }
        }
    }
    let frac = ((*value - min) as f32 / (max - min) as f32).clamp(0.0, 1.0);
    let painter = ui.painter();
    painter.rect_filled(
        track,
        egui::CornerRadius::same(2),
        egui::Color32::from_rgb(0x3a, 0x3a, 0x42),
    );
    painter.rect_filled(
        egui::Rect::from_min_max(
            track.min,
            egui::pos2(track.min.x + track.width() * frac, track.max.y),
        ),
        egui::CornerRadius::same(2),
        ACCENT,
    );
    painter.circle_filled(
        egui::pos2(track.min.x + track.width() * frac, track.center().y),
        8.0,
        egui::Color32::WHITE,
    );
    changed
}

fn nav_item(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 46.0),
        egui::Sense::click(),
    );
    let painter = ui.painter();
    if selected {
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(rect.min.x - 12.0, rect.center().y - 11.0),
                egui::vec2(3.0, 22.0),
            ),
            egui::CornerRadius::same(2),
            ACCENT,
        );
    } else if resp.hovered() {
        painter.rect_filled(rect, egui::CornerRadius::same(6), ROW_HOVER);
    }
    painter.text(
        egui::pos2(rect.min.x + 12.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(18.0),
        if selected { INK } else { INK_DIM },
    );
    painter.text(
        egui::pos2(rect.min.x + 40.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        if selected {
            INK
        } else {
            egui::Color32::from_rgb(0xc2, 0xc2, 0xca)
        },
    );
    resp
}

fn style_card(
    ui: &mut egui::Ui,
    style: usize,
    name: &str,
    desc: &str,
    selected: bool,
    width: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 166.0), egui::Sense::click());
    let painter = ui.painter();
    let bg = if selected {
        egui::Color32::from_rgb(0x1d, 0x1d, 0x22)
    } else if resp.hovered() {
        egui::Color32::from_rgb(0x1c, 0x1c, 0x21)
    } else {
        egui::Color32::from_rgb(0x19, 0x19, 0x1d)
    };
    painter.rect_filled(rect, egui::CornerRadius::same(10), bg);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(
            1.0_f32,
            if selected { ACCENT } else { PANEL_LINE },
        ),
        egui::StrokeKind::Inside,
    );
    let preview = egui::Rect::from_min_size(
        rect.min + egui::vec2(10.0, 10.0),
        egui::vec2(width - 20.0, 96.0),
    );
    painter.rect_filled(
        preview,
        egui::CornerRadius::same(7),
        egui::Color32::from_rgb(0x2b, 0x32, 0x3e),
    );
    draw_preview(painter, preview.shrink(7.0), style);
    painter.text(
        rect.min + egui::vec2(13.0, 114.0),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::proportional(14.0),
        if selected { ACCENT } else { INK },
    );
    let galley = painter.layout(
        desc.to_string(),
        egui::FontId::proportional(11.5),
        INK_DIM,
        width - 26.0,
    );
    painter.galley(rect.min + egui::vec2(13.0, 134.0), galley, INK_DIM);
    resp
}

fn draw_preview(painter: &egui::Painter, rect: egui::Rect, style: usize) {
    let win = egui::Color32::from_rgb(0x32, 0x3a, 0x47);
    let panel = egui::Color32::from_rgb(0x3d, 0x46, 0x55);
    let panel_hi = egui::Color32::from_rgb(0x4a, 0x54, 0x65);
    let ink = egui::Color32::from_rgb(0xc6, 0xce, 0xda);
    let line = egui::Color32::from_rgb(0x97, 0xa1, 0xb0);
    let accent = ACCENT;

    let x = |f: f32| rect.min.x + rect.width() * f;
    let y = |f: f32| rect.min.y + rect.height() * f;
    let bx = |x0: f32, y0: f32, x1: f32, y1: f32, c: egui::Color32, r: u8| {
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x(x0), y(y0)), egui::pos2(x(x1), y(y1))),
            egui::CornerRadius::same(r),
            c,
        );
    };
    let dot = |cx: f32, cy: f32, d: f32, c: egui::Color32| {
        painter.circle_filled(egui::pos2(x(cx), y(cy)), d * rect.height() / 2.0, c);
    };
    // значок приложения + подпись под ним
    let icon = |cx: f32, cy: f32, s: f32, label: bool| {
        let half = s * rect.height() / 2.0;
        painter.rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(x(cx), y(cy)),
                egui::vec2(half * 2.0, half * 2.0),
            ),
            egui::CornerRadius::same(2),
            ink,
        );
        if label {
            painter.rect_filled(
                egui::Rect::from_center_size(
                    egui::pos2(x(cx), y(cy) + half + rect.height() * 0.035),
                    egui::vec2(half * 1.8, 1.5),
                ),
                egui::CornerRadius::same(1),
                line,
            );
        }
    };
    let footer = |top: f32| {
        dot(0.07, top + 0.045, 0.075, panel_hi);
        bx(0.12, top + 0.032, 0.30, top + 0.056, line, 1);
        dot(0.91, top + 0.045, 0.032, line);
        dot(0.97, top + 0.045, 0.032, line);
    };

    bx(0.0, 0.0, 1.0, 1.0, win, 4);

    match style {
        0 => {
            let mid = 0.59;
            bx(0.04, 0.06, mid, 0.94, panel, 2);
            bx(0.06, 0.09, 0.28, 0.115, accent, 1);
            bx(0.06, 0.15, mid - 0.02, 0.24, panel_hi, 3);
            for i in 0..6 {
                let cy = 0.31 + i as f32 * 0.095;
                icon(0.10, cy, 0.065, false);
                bx(0.15, cy - 0.011, mid - 0.04, cy + 0.011, line, 1);
            }
            bx(mid + 0.02, 0.06, 0.96, 0.94, panel, 2);
            for i in 0..5 {
                let cy = 0.15 + i as f32 * 0.13;
                icon(mid + 0.07, cy, 0.05, false);
                bx(mid + 0.12, cy - 0.011, 0.92, cy + 0.011, line, 1);
            }
        }
        1 => {
            bx(0.05, 0.07, 0.30, 0.10, accent, 1);
            bx(0.05, 0.14, 0.95, 0.23, panel_hi, 4);
            bx(0.05, 0.28, 0.20, 0.30, line, 1);
            for r in 0..2 {
                for c in 0..4 {
                    icon(0.14 + c as f32 * 0.22, 0.40 + r as f32 * 0.21, 0.075, true);
                }
            }
            bx(0.05, 0.67, 0.22, 0.69, line, 1);
            bx(0.05, 0.74, 0.46, 0.78, panel, 2);
            bx(0.52, 0.74, 0.93, 0.78, panel, 2);
            footer(0.85);
        }
        2 => {
            bx(0.04, 0.06, 0.34, 0.94, panel, 2);
            bx(0.05, 0.10, 0.33, 0.18, panel_hi, 3);
            for i in 0..5 {
                let cy = 0.24 + i as f32 * 0.105;
                icon(0.09, cy, 0.06, false);
                bx(0.14, cy - 0.010, 0.32, cy + 0.010, line, 1);
            }
            dot(0.09, 0.88, 0.07, panel_hi);
            bx(0.14, 0.868, 0.26, 0.892, line, 1);
            bx(0.37, 0.08, 0.55, 0.10, line, 1);
            for r in 0..2 {
                for c in 0..3 {
                    icon(0.46 + c as f32 * 0.17, 0.24 + r as f32 * 0.25, 0.085, true);
                }
            }
        }
        3 => {
            bx(0.03, 0.05, 0.68, 0.95, panel, 2);
            bx(0.05, 0.10, 0.66, 0.19, panel_hi, 5);
            bx(0.05, 0.24, 0.18, 0.265, line, 1);
            for r in 0..2 {
                for c in 0..5 {
                    icon(0.10 + c as f32 * 0.12, 0.35 + r as f32 * 0.16, 0.06, true);
                }
            }
            bx(0.05, 0.62, 0.20, 0.645, line, 1);
            for r in 0..2 {
                for c in 0..2 {
                    let x0 = 0.05 + c as f32 * 0.31;
                    let y0 = 0.69 + r as f32 * 0.10;
                    bx(x0, y0, x0 + 0.29, y0 + 0.08, panel_hi, 2);
                }
            }
            bx(0.70, 0.05, 0.97, 0.95, panel, 2);
            dot(0.835, 0.18, 0.15, panel_hi);
            bx(0.78, 0.28, 0.89, 0.305, line, 1);
            for i in 0..5 {
                let cy = 0.38 + i as f32 * 0.075;
                icon(0.735, cy, 0.045, false);
                bx(0.755, cy - 0.010, 0.93, cy + 0.010, line, 1);
            }
            dot(0.86, 0.90, 0.036, line);
            dot(0.93, 0.90, 0.036, line);
        }
        4 => {
            bx(0.03, 0.06, 0.97, 0.94, panel, 2);
            bx(0.05, 0.10, 0.95, 0.19, panel_hi, 5);
            bx(0.05, 0.24, 0.22, 0.265, line, 1);
            for i in 0..7 {
                let cy = 0.33 + i as f32 * 0.078;
                icon(0.08, cy, 0.05, false);
                bx(0.12, cy - 0.010, 0.55, cy + 0.010, line, 1);
            }
            footer(0.86);
        }
        5 => {
            bx(0.03, 0.05, 0.97, 0.95, panel, 2);
            bx(0.04, 0.09, 0.96, 0.18, panel_hi, 3);
            bx(0.04, 0.23, 0.22, 0.255, line, 1);
            for c in 0..2 {
                for r in 0..6 {
                    let cx = 0.05 + c as f32 * 0.47;
                    let cy = 0.31 + r as f32 * 0.095;
                    icon(cx + 0.02, cy, 0.05, false);
                    bx(cx + 0.05, cy - 0.010, cx + 0.42, cy + 0.010, line, 1);
                }
            }
            footer(0.90);
        }
        _ => {
            bx(0.03, 0.05, 0.97, 0.95, panel, 2);
            bx(0.30, 0.10, 0.70, 0.19, panel_hi, 5);
            for r in 0..2 {
                for c in 0..5 {
                    icon(0.20 + c as f32 * 0.15, 0.34 + r as f32 * 0.21, 0.065, true);
                }
            }
            bx(0.43, 0.79, 0.57, 0.84, panel_hi, 4);
        }
    }
}

fn open_folder(name: &str) {
    if let Ok(home) = std::env::var("USERPROFILE") {
        let path = if name.is_empty() {
            home
        } else {
            format!("{home}\\{name}")
        };
        let _ = std::process::Command::new("explorer.exe")
            .arg(path)
            .spawn();
    }
}

fn run_detached(prog: &str, args: &[&str]) {
    let _ = std::process::Command::new(prog)
        .args(args)
        .creation_flags(0x08000000)
        .spawn();
}

fn link_row(ui: &mut egui::Ui, icon: &str, label: &str, t: &Theme) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
    let painter = ui.painter().with_clip_rect(rect);
    if resp.hovered() {
        painter.rect_filled(rect, egui::CornerRadius::same(5), t.card_hi);
    }
    painter.text(
        egui::pos2(rect.min.x + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(17.0),
        t.text,
    );
    painter.text(
        egui::pos2(rect.min.x + 38.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        t.text,
    );
    resp
}

fn tile(
    ui: &mut egui::Ui,
    app: &AppEntry,
    t: &Theme,
    w: f32,
    h: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let painter = ui.painter().with_clip_rect(rect);
    if resp.hovered() {
        painter.rect_filled(rect, egui::CornerRadius::same(6), t.card_hi);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.min.y + 26.0),
        egui::vec2(34.0, 34.0),
    );
    if let Some(tex) = &app.icon {
        painter.image(
            tex.id(),
            icon_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        painter.rect_filled(icon_rect, egui::CornerRadius::same(7), avatar_color(&app.name));
        painter.text(
            icon_rect.center(),
            egui::Align2::CENTER_CENTER,
            letter(&app.name),
            egui::FontId::proportional(16.0),
            egui::Color32::WHITE,
        );
    }
    let galley = painter.layout(
        app.name.clone(),
        egui::FontId::proportional(11.5),
        t.text,
        w - 6.0,
    );
    painter.galley(
        egui::pos2(rect.min.x + 3.0, rect.min.y + 48.0),
        galley,
        t.text,
    );
    resp
}

fn rec_row(ui: &mut egui::Ui, app: &AppEntry, t: &Theme) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
    let painter = ui.painter().with_clip_rect(rect);
    if resp.hovered() {
        painter.rect_filled(rect, egui::CornerRadius::same(5), t.card_hi);
    }
    let icon = egui::Rect::from_min_size(
        rect.min + egui::vec2(6.0, 5.0),
        egui::vec2(28.0, 28.0),
    );
    if let Some(tex) = &app.icon {
        painter.image(
            tex.id(),
            icon,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        painter.rect_filled(icon, egui::CornerRadius::same(6), avatar_color(&app.name));
    }
    painter.text(
        egui::pos2(rect.min.x + 42.0, rect.min.y + 8.0),
        egui::Align2::LEFT_TOP,
        short(&app.name, 24),
        egui::FontId::proportional(12.5),
        t.text,
    );
    painter.text(
        egui::pos2(rect.min.x + 42.0, rect.min.y + 24.0),
        egui::Align2::LEFT_TOP,
        "Недавно",
        egui::FontId::proportional(10.5),
        t.muted,
    );
    resp
}

fn row(ui: &mut egui::Ui, app: &AppEntry, t: &Theme) -> egui::Response {
    row_w(ui, app, t, ui.available_width())
}

fn row_w(ui: &mut egui::Ui, app: &AppEntry, t: &Theme, w: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 34.0), egui::Sense::click());
    let painter = ui.painter().with_clip_rect(rect);
    if resp.hovered() {
        painter.rect_filled(rect, egui::CornerRadius::same(5), t.card_hi);
    }
    let icon = egui::Rect::from_min_size(
        rect.min + egui::vec2(6.0, 5.0),
        egui::vec2(24.0, 24.0),
    );
    if let Some(tex) = &app.icon {
        painter.image(
            tex.id(),
            icon,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        painter.rect_filled(icon, egui::CornerRadius::same(5), avatar_color(&app.name));
        painter.text(
            icon.center(),
            egui::Align2::CENTER_CENTER,
            letter(&app.name),
            egui::FontId::proportional(11.0),
            egui::Color32::WHITE,
        );
    }
    painter.text(
        egui::pos2(rect.min.x + 38.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        short(&app.name, 32),
        egui::FontId::proportional(13.0),
        t.text,
    );
    resp
}

impl Drop for App {
    fn drop(&mut self) {
        tray_remove();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // после сворачивания/разворачивания текстура фона недействительна,
        // а окно может остаться с чёрным кадром — заставляем перерисовать
        let minimized = ctx.input(|i| i.viewport().minimized).unwrap_or(false);
        if minimized != self.was_minimized {
            self.was_minimized = minimized;
            if !minimized {
                ctx.request_repaint();
            }
        }
        if self.visible {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        if ZONE_DIRTY.swap(false, Ordering::SeqCst) {
            if let Some((x, y)) = *START_ZONE.lock().unwrap() {
                self.cfg.start_x = x;
                self.cfg.start_y = y;
                App::save(self);
            }
        }

        let ev = SETTINGS_EVENT.load(Ordering::SeqCst);
        if ev != 0 && unsafe { WaitForSingleObject(ev as HANDLE, 0) } == WAIT_OBJECT_0 {
            self.show_settings(ctx);
        }

        match TRAY_ACTION.swap(0, Ordering::SeqCst) {
            1 => self.show_menu(ctx),
            2 => self.show_settings(ctx),
            3 => {
                self.quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }

        if TOGGLE.swap(false, Ordering::SeqCst) {
            if self.visible && self.mode == Mode::Menu {
                self.hide(ctx);
            } else {
                self.show_menu(ctx);
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.quit {
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide(ctx);
            return;
        }

        if self.visible {
            match self.mode {
                Mode::Menu => self.menu_ui(ctx),
                Mode::Settings => self.settings_ui(ctx),
            }
        }
    }
}


// ---- значок в трее ----

const WM_TRAY: u32 = 0x8000 + 1;
const TRAY_UID: u32 = 1;
static TRAY_ACTION: AtomicU8 = AtomicU8::new(0);
static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
static SETTINGS_EVENT: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static CTX: Mutex<Option<egui::Context>> = Mutex::new(None);

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Будит окно egui из оконной процедуры трея.
fn wake() {
    if let Ok(guard) = CTX.lock() {
        if let Some(ctx) = guard.as_ref() {
            ctx.request_repaint();
        }
    }
}

const ICON_ICO: &[u8] = include_bytes!("../assets/openstart.ico");

/// Иконка трея: берём готовый .ico, GDI-иконки выходят прозрачными.
fn make_icon() -> HICON {
    let path = std::env::temp_dir().join("openstart.ico");
    if std::fs::write(&path, ICON_ICO).is_err() {
        return std::ptr::null_mut();
    }
    let name = wide(&path.to_string_lossy());
    unsafe {
        LoadImageW(
            std::ptr::null_mut(),
            name.as_ptr(),
            IMAGE_ICON,
            32,
            32,
            LR_LOADFROMFILE,
        ) as HICON
    }
}

unsafe extern "system" fn tray_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    if msg == TASKBAR_CREATED.load(Ordering::SeqCst) && msg != 0 {
        tray_add(hwnd);
        return 0;
    }
    if msg == WM_TRAY {
        match (lp as u32) & 0xffff {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                TRAY_ACTION.store(1, Ordering::SeqCst);
                wake();
            }
            WM_RBUTTONUP => {
                let menu = CreatePopupMenu();
                AppendMenuW(menu, MF_STRING, 1, wide("Открыть меню").as_ptr());
                AppendMenuW(menu, MF_STRING, 2, wide("Настройки OpenStart").as_ptr());
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                AppendMenuW(menu, MF_STRING, 3, wide("Выход").as_ptr());
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                // без этого меню не закрывается по клику мимо
                SetForegroundWindow(hwnd);
                let cmd = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_NONOTIFY as MENU_ITEM_FLAGS,
                    pt.x,
                    pt.y,
                    0,
                    hwnd,
                    std::ptr::null(),
                );
                DestroyMenu(menu);
                if cmd != 0 {
                    TRAY_ACTION.store(cmd as u8, Ordering::SeqCst);
                    wake();
                }
            }
            _ => {}
        }
        return 0;
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

fn tray_init() {
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide("OpenStartTray");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(tray_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("OpenStartTray").as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            return;
        }
        TRAY_HWND.store(hwnd as isize, Ordering::SeqCst);
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            Ordering::SeqCst,
        );
        tray_add(hwnd);
    }
}

fn tray_add(hwnd: HWND) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_UID;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = make_icon();
        let tip = wide("OpenStart");
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(NIM_ADD, &mut nid);
    }
}

fn tray_remove() {
    let hwnd = TRAY_HWND.swap(0, Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd as HWND;
        nid.uID = TRAY_UID;
        Shell_NotifyIconW(NIM_DELETE, &mut nid);
        DestroyWindow(hwnd as HWND);
    }
}

/// false — уже запущен другой экземпляр, он получит сигнал открыть настройки.
fn single_instance() -> bool {
    unsafe {
        let mutex = CreateMutexW(std::ptr::null(), 0, wide("OpenStart.Menu.Single").as_ptr());
        if mutex.is_null() {
            return true;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let ev = OpenEventW(
                EVENT_MODIFY_STATE,
                0,
                wide("OpenStart.ShowSettings").as_ptr(),
            );
            if !ev.is_null() {
                SetEvent(ev);
                CloseHandle(ev);
            }
            return false;
        }
        let ev = CreateEventW(
            std::ptr::null(),
            0,
            0,
            wide("OpenStart.ShowSettings").as_ptr(),
        );
        SETTINGS_EVENT.store(ev as isize, Ordering::SeqCst);
        true
    }
}

fn main() -> eframe::Result<()> {
    let settings = std::env::args().any(|a| a == "--settings");
    if !single_instance() {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 640.0])
            .with_resizable(false)
            .with_taskbar(false)
            .with_title("OpenStart"),
        ..Default::default()
    };
    eframe::run_native(
        "OpenStart",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, settings)))),
    )
}
