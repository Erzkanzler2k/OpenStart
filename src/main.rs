#![windows_subsystem = "windows"]

//! OpenStart11 — самостоятельное меню «Пуск» для Windows 10/11.//!
//! Меню открывается по клавише Win (низкоуровневый хук клавиатуры), показывает
//! ярлыки из пользовательского и общего меню «Пуск», ищет по ним и запускает их.
//! Пять стилей (Windows 7 / Modern / Windows 10 / Windows 11 / Windows App)
//! рисуются самим приложением. Настройки лежат в
//! %APPDATA%\OpenStart11\config.json — никакого Start11 и никакого реестра.

use std::collections::BTreeMap;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use eframe::egui;
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, FindWindowW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    SetForegroundWindow, SetWindowPos, SetWindowsHookExW, ShowWindow, UnhookWindowsHookEx,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};

const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;

const ACCENT: egui::Color32 = egui::Color32::from_rgb(0xe0, 0xaf, 0x68);
const ACCENT_DARK: egui::Color32 = egui::Color32::from_rgb(0xa5, 0x76, 0x2f);

const STYLES: &[(&str, &str)] = &[
    ("Стиль Windows 7", "Классика Windows 7"),
    ("Стиль Modern", "Обновлённая классика"),
    ("Стиль Windows 10", "Плитки Windows 10"),
    ("Стиль Windows 11", "Центрированное меню"),
    ("Стиль Windows App", "Список Windows 11"),
];

const NAV: &[&str] = &["Меню «Пуск»", "Управление", "О программе"];

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    style: usize,
    dark: bool,
    alpha: u8,
    show_search: bool,
    tiles: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            style: 3,
            dark: true,
            alpha: 242,
            show_search: true,
            tiles: true,
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
    PathBuf::from(base).join("OpenStart11").join("config.json")
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
            "OpenStart11",
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
            "OpenStart11",
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
            "OpenStart11",
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
        0 => (430.0, 560.0),
        1 => (480.0, 590.0),
        2 => (520.0, 600.0),
        3 => (660.0, 620.0),
        _ => (640.0, 600.0),
    }
}

fn monitor_size(ctx: &egui::Context) -> egui::Vec2 {
    ctx.input(|i| i.viewport().monitor_size)
        .unwrap_or(egui::vec2(1920.0, 1080.0))
}

/// Скруглить окно: DWM в Windows 11, плюс регион — на случай, если DWM отказал.
fn round_window(w: i32, h: i32) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
    let title: Vec<u16> = "OpenStart11\0".encode_utf16().collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd.is_null() {
            return;
        }
        // DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_ROUND = 2
        let round: u32 = 2;
        DwmSetWindowAttribute(hwnd, 33, &round as *const u32 as *const _, 4);

        if w > 0 && h > 0 {
            let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, 18, 18);
            if !rgn.is_null() {
                SetWindowRgn(hwnd, rgn, 1);
            }
        }
    }
}

/// Вывести окно поверх всех и забрать фокус: без этого меню закрывается сразу.
fn force_foreground() {
    let title: Vec<u16> = "OpenStart11\0".encode_utf16().collect();
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
    std::thread::spawn(|| unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), std::ptr::null_mut(), 0);
        if hook.is_null() {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        UnhookWindowsHookEx(hook);
    });
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

        if win_key && down {
            if !WIN_DOWN.swap(true, Ordering::SeqCst) {
                CHORD.store(false, Ordering::SeqCst);
                *WIN_AT.lock().unwrap() = Some(Instant::now());
            }
            // нажатие Win глушим: иначе откроется родное меню Windows
            return 1;
        }
        if down && WIN_DOWN.load(Ordering::SeqCst) {
            if !CHORD.swap(true, Ordering::SeqCst) {
                // это сочетание Win+X: возвращаем системе «зажатый» Win
                send_win_down();
            }
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        }
        if win_key && up {
            if !WIN_DOWN.swap(false, Ordering::SeqCst) {
                return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
            }
            let tap = !CHORD.load(Ordering::SeqCst)
                && WIN_AT
                    .lock()
                    .unwrap()
                    .map(|t| t.elapsed().as_millis() < 700)
                    .unwrap_or(false);
            if tap {
                TOGGLE.store(true, Ordering::SeqCst);
            }
            return 1;
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
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
    autostart: bool,
    page: usize,
    quit: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        start_hook(cc.egui_ctx.clone());

        let mut apps = scan_apps();
        attach_icons(&cc.egui_ctx, &mut apps);

        let app = App {
            cfg: load_config(),
            apps,
            query: String::new(),
            mode: Mode::Settings,
            visible: true,
            opened_at: Instant::now(),
            focus_search: false,
            was_focused: false,
            autostart: autostart_enabled(),
            page: 0,
            quit: false,
        };
        app.center_settings(&cc.egui_ctx);
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
        let size = egui::vec2(1000.0, 640.0);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            (m.x - size.x) / 2.0,
            (m.y - size.y) / 2.0 - 20.0,
        )));
    }

    fn show_menu(&mut self, ctx: &egui::Context) {
        self.mode = Mode::Menu;
        self.visible = true;
        self.opened_at = Instant::now();
        self.focus_search = self.cfg.show_search;
        self.was_focused = false;
        self.query.clear();
        let (w, h) = menu_size(self.cfg.style);
        let m = monitor_size(ctx);
        let x = if self.cfg.style >= 3 {
            (m.x - w) / 2.0
        } else {
            12.0
        };
        let y = (m.y - h - 56.0).max(8.0);
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
        // снять скругление от меню, иначе окно настроек останется обрезанным
        unsafe {
            use windows_sys::Win32::Graphics::Gdi::SetWindowRgn;
            let title: Vec<u16> = "OpenStart11\0".encode_utf16().collect();
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if !hwnd.is_null() {
                SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
            }
        }
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

    fn menu_ui(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
        if focused {
            self.was_focused = true;
        } else if !self.was_focused && self.opened_at.elapsed().as_millis() < 900 {
            force_foreground();
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if self.was_focused && !focused && self.opened_at.elapsed().as_millis() > 400 {
            self.hide(ctx);
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.hide(ctx);
            return;
        }

        let modern = self.cfg.style >= 3;
        let t = theme(self.cfg.dark, self.cfg.alpha);
        let margin = if modern { 18 } else { 10 };
        let cols = if modern { 6 } else { 4 };
        let tile_w = if modern { 96.0 } else { 88.0 };
        let tile_h = if modern { 84.0 } else { 84.0 };

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(t.bg)
                    .inner_margin(egui::Margin::same(margin)),
            )
            .show(ctx, |ui| {
                if !modern {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("OpenStart11")
                                .color(t.accent)
                                .size(15.0)
                                .strong(),
                        );
                    });
                    ui.add_space(6.0);
                }

                if self.cfg.show_search {
                    let mut resp = None;
                    egui::Frame::NONE
                        .fill(t.card)
                        .stroke(egui::Stroke::new(1.0, t.card_hi))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin {
                            left: 12,
                            right: 12,
                            top: 7,
                            bottom: 7,
                        })
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // лупа
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(14.0, 14.0),
                                    egui::Sense::hover(),
                                );
                                let c = r.center();
                                ui.painter().circle_stroke(
                                    c,
                                    5.0,
                                    egui::Stroke::new(1.4, t.muted),
                                );
                                ui.painter().line_segment(
                                    [
                                        c + egui::vec2(3.5, 3.5),
                                        c + egui::vec2(7.0, 7.0),
                                    ],
                                    egui::Stroke::new(1.4, t.muted),
                                );
                                ui.add_space(4.0);
                                resp = Some(ui.add(
                                    egui::TextEdit::singleline(&mut self.query)
                                        .hint_text("Поиск приложений")
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
                    ui.add_space(14.0);
                }

                let q = self.query.trim().to_lowercase();
                let matching: Vec<usize> = self
                    .apps
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| q.is_empty() || a.name.to_lowercase().contains(&q))
                    .map(|(i, _)| i)
                    .collect();

                let mut launch: Option<String> = None;
                let mut open_settings = false;

                if q.is_empty() && self.cfg.tiles {
                    ui.label(
                        egui::RichText::new("Закреплено")
                            .size(13.0)
                            .strong()
                            .color(t.text),
                    );
                    ui.add_space(8.0);
                    let take = (cols * 2).min(matching.len());
                    egui::Grid::new("pins")
                        .num_columns(cols)
                        .spacing([4.0, 6.0])
                        .show(ui, |ui| {
                            for (n, &idx) in matching.iter().take(take).enumerate() {
                                if tile(ui, &self.apps[idx], &t, tile_w, tile_h).clicked() {
                                    launch = Some(self.apps[idx].target.clone());
                                }
                                if (n + 1) % cols == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Приложения")
                            .size(13.0)
                            .strong()
                            .color(t.text),
                    );
                    ui.add_space(4.0);
                }

                let rest_h = (ui.available_height() - 42.0).max(80.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(rest_h)
                    .show(ui, |ui| {
                        for &idx in &matching {
                            if row(ui, &self.apps[idx], &t).clicked() {
                                launch = Some(self.apps[idx].target.clone());
                            }
                        }
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let (av, _) =
                        ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                    ui.painter().circle_filled(av.center(), 13.0, t.card_hi);
                    let user = std::env::var("USERNAME").unwrap_or_default();
                    ui.painter().text(
                        av.center(),
                        egui::Align2::CENTER_CENTER,
                        letter(&user),
                        egui::FontId::proportional(12.0),
                        t.text,
                    );
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(&user).size(12.5).color(t.text));
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Настройки").size(12.5).color(t.muted),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                open_settings = true;
                            }
                        },
                    );
                });

                if open_settings {
                    self.show_settings(ctx);
                }
                if let Some(target) = launch {
                    launch_target(&target);
                    self.hide(ctx);
                }
            });
    }
    fn settings_ui(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        egui::SidePanel::left("nav")
            .exact_width(228.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(0x15, 0x15, 0x1a)))
            .show(ctx, |ui| {
                ui.add_space(24.0);
                for (i, name) in NAV.iter().enumerate() {
                    let selected = self.page == i;
                    let resp = ui.add_sized(
                        [228.0, 40.0],
                        egui::SelectableLabel::new(
                            selected,
                            egui::RichText::new(*name).color(if selected {
                                egui::Color32::from_rgb(0xe6, 0xe6, 0xef)
                            } else {
                                egui::Color32::from_rgb(0x8a, 0x8a, 0x99)
                            }),
                        ),
                    );
                    if resp.clicked() {
                        self.page = i;
                    }
                }
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(0x1b, 0x1b, 0x1f)).inner_margin(
                egui::Margin {
                    left: 24,
                    right: 24,
                    top: 20,
                    bottom: 0,
                },
            ))
            .show(ctx, |ui| match self.page {
                0 => {
                    ui.label(
                        egui::RichText::new("Меню «Пуск»")
                            .size(24.0)
                            .strong()
                            .color(egui::Color32::from_rgb(0xe6, 0xe6, 0xef)),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Стиль меню«Пуск»")
                            .color(egui::Color32::from_rgb(0x8a, 0x8a, 0x99)),
                    );
                    ui.add_space(6.0);
                    if self.cards(ui) {
                        changed = true;
                    }
                    ui.add_space(14.0);
                    ui.label(
                        egui::RichText::new("Поведение и оформление")
                            .color(egui::Color32::from_rgb(0x8a, 0x8a, 0x99)),
                    );
                    ui.add_space(6.0);
                    let mut dark = self.cfg.dark;
                    if ui.checkbox(&mut dark, "Тёмное меню").changed() {
                        self.cfg.dark = dark;
                        changed = true;
                    }
                    let mut search = self.cfg.show_search;
                    if ui.checkbox(&mut search, "Показывать поиск").changed() {
                        self.cfg.show_search = search;
                        changed = true;
                    }
                    let mut tiles = self.cfg.tiles;
                    if ui
                        .checkbox(&mut tiles, "Плитки закреплённых сверху")
                        .changed()
                    {
                        self.cfg.tiles = tiles;
                        changed = true;
                    }
                    let mut alpha = self.cfg.alpha as i32;
                    if ui
                        .add(
                            egui::Slider::new(&mut alpha, 80..=255)
                                .text("Непрозрачность меню"),
                        )
                        .changed()
                    {
                        self.cfg.alpha = alpha as u8;
                        changed = true;
                    }
                    ui.add_space(12.0);
                    if ui.button("Показать меню").clicked() {
                        self.show_menu(ctx);
                    }
                }
                1 => {
                    ui.label(
                        egui::RichText::new("Управление")
                            .size(24.0)
                            .strong()
                            .color(egui::Color32::from_rgb(0xe6, 0xe6, 0xef)),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "Найдено программ: {}. Настройки: {}",
                            self.apps.len(),
                            config_path().display()
                        ))
                        .color(egui::Color32::from_rgb(0x8a, 0x8a, 0x99)),
                    );
                    ui.add_space(10.0);
                    let mut auto = self.autostart;
                    if ui
                        .checkbox(&mut auto, "Запускать при входе в Windows")
                        .changed()
                    {
                        self.autostart = auto;
                        set_autostart(auto);
                    }
                    if ui.button("Пересканировать программы").clicked() {
                        self.apps = scan_apps();
                        attach_icons(ctx, &mut self.apps);
                    }
                    if ui.button("Открыть папку настроек").clicked() {
                        if let Some(dir) = config_path().parent() {
                            let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
                        }
                    }
                    if ui.button("Сбросить настройки").clicked() {
                        self.cfg = Config::default();
                        changed = true;
                    }
                    ui.add_space(10.0);
                    if ui.button("Выход").clicked() {
                        self.quit(ctx);
                    }
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(
                            "Меню открывается по клавише Win. Настройки Start11 больше не нужны: \
                             всё хранится в собственном файле, реестр не трогается.",
                        )
                        .color(egui::Color32::from_rgb(0x8a, 0x8a, 0x99)),
                    );
                }
                _ => {
                    ui.label(
                        egui::RichText::new("OpenStart11")
                            .size(24.0)
                            .strong()
                            .color(egui::Color32::from_rgb(0xe6, 0xe6, 0xef)),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(
                            "Самостоятельное меню «Пуск»: свой перехват Win, свои стили, свой список \
                             программ. Start11 не нужен и не используется.\n\n\
                             Программы берутся из ярлыков меню «Пуск» Windows \
                             (общая и пользовательская папки).\n\n\
                             Пять стилей: Windows 7, Modern, Windows 10, Windows 11, Windows App. \
                             Меню открывается по Win, закрывается по Esc, клику мимо или запуску \
                             программы.\n\n\
                             Чего пока нет: замены панели задач, UWP-программ без ярлыков, \
                             закреплённых вручную и иконок из exe (вместо них буквы).",
                        )
                        .color(egui::Color32::from_rgb(0x8a, 0x8a, 0x99)),
                    );
                }
            });

        if changed {
            self.save();
        }
    }

    fn cards(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            for (i, (name, desc)) in STYLES.iter().enumerate() {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(136.0, 216.0), egui::Sense::click());
                if resp.clicked() {
                    self.cfg.style = i;
                    changed = true;
                }
                let selected = self.cfg.style == i;
                let painter = ui.painter();
                painter.rect_filled(
                    rect,
                    egui::CornerRadius::same(8),
                    if selected {
                        egui::Color32::from_rgb(0x33, 0x33, 0x3d)
                    } else {
                        egui::Color32::from_rgb(0x2a, 0x2a, 0x33)
                    },
                );
                if selected {
                    painter.rect_stroke(
                        rect,
                        egui::CornerRadius::same(8),
                        egui::Stroke::new(2.0, ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }

                let preview = egui::Rect::from_min_size(
                    rect.min + egui::vec2(14.0, 14.0),
                    egui::vec2(rect.width() - 28.0, 100.0),
                );
                painter.rect_filled(
                    preview,
                    egui::CornerRadius::same(4),
                    egui::Color32::from_rgb(0x3a, 0x3a, 0x46),
                );

                let ink = egui::Color32::from_rgb(0x5a, 0x5a, 0x6b);
                let p = preview.min;
                match i {
                    0 => {
                        painter.rect_filled(egui::Rect::from_min_size(p + egui::vec2(6.0, 14.0), egui::vec2(44.0, 20.0)), 2.0, ink);
                        painter.rect_filled(egui::Rect::from_min_size(p + egui::vec2(56.0, 14.0), egui::vec2(44.0, 72.0)), 2.0, ink);
                        painter.rect_filled(egui::Rect::from_min_size(p + egui::vec2(6.0, 40.0), egui::vec2(44.0, 46.0)), 2.0, ink);
                    }
                    1 => {
                        painter.rect_filled(egui::Rect::from_min_size(p + egui::vec2(6.0, 10.0), egui::vec2(96.0, 10.0)), 2.0, ink);
                        painter.rect_filled(egui::Rect::from_min_size(p + egui::vec2(6.0, 26.0), egui::vec2(60.0, 64.0)), 2.0, ink);
                        painter.rect_filled(egui::Rect::from_min_size(p + egui::vec2(72.0, 26.0), egui::vec2(30.0, 64.0)), 2.0, ink);
                    }
                    2 => {
                        for r in 0..2 {
                            for c in 0..4 {
                                let pos = p + egui::vec2(6.0 + c as f32 * 25.0, 14.0 + r as f32 * 26.0);
                                painter.rect_filled(egui::Rect::from_min_size(pos, egui::vec2(21.0, 21.0)), 2.0, ink);
                            }
                        }
                    }
                    3 => {
                        for r in 0..3 {
                            for c in 0..5 {
                                let pos = p + egui::vec2(8.0 + c as f32 * 19.0, 16.0 + r as f32 * 22.0);
                                painter.rect_filled(egui::Rect::from_min_size(pos, egui::vec2(15.0, 17.0)), 2.0, ink);
                            }
                        }
                    }
                    _ => {
                        for r in 0..4 {
                            let pos = p + egui::vec2(8.0, 14.0 + r as f32 * 14.0);
                            painter.rect_filled(egui::Rect::from_min_size(pos, egui::vec2(94.0, 8.0)), 2.0, ink);
                        }
                    }
                }

                painter.text(
                    rect.min + egui::vec2(12.0, 126.0),
                    egui::Align2::LEFT_TOP,
                    *name,
                    egui::FontId::proportional(13.0),
                    if selected { ACCENT } else { egui::Color32::from_rgb(0xe6, 0xe6, 0xef) },
                );
                painter.text(
                    rect.min + egui::vec2(12.0, 170.0),
                    egui::Align2::LEFT_TOP,
                    *desc,
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_rgb(0x8a, 0x8a, 0x99),
                );
            }
        });
        changed
    }
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

fn row(ui: &mut egui::Ui, app: &AppEntry, t: &Theme) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
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

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 640.0])
            .with_resizable(false)
            .with_title("OpenStart11"),
        ..Default::default()
    };
    eframe::run_native(
        "OpenStart11",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
