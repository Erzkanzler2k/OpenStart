# OpenStart

<div align="center">

**A Start menu for Windows 10/11 — its own Win key hook, its own styles, its own search.**

[English](README.md) · [Русский](README.ru.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Español](README.es.md) · [中文](README.zh-CN.md) · [日本語](README.ja.md)

[![Release](https://img.shields.io/github/v/release/Erzkanzler2k/OpenStart?label=release&color=e0af68)](https://github.com/Erzkanzler2k/OpenStart/releases)
[![CI](https://github.com/Erzkanzler2k/OpenStart/actions/workflows/ci.yml/badge.svg)](https://github.com/Erzkanzler2k/OpenStart/actions/workflows/ci.yml)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-0078d4)](#requirements)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-dea584)](#build-from-source)
[![License](https://img.shields.io/badge/license-MIT-97ca00)](LICENSE)

</div>

![OpenStart menu](docs/menu.png)

One Rust executable. No installer, no services, no admin rights, nothing else required.
It takes the Win key, reads the shortcuts Windows already has and draws the menu itself.

Built for people who want a Start menu that looks and behaves the way they choose — and
who do not want a background service or a driver installed for it.

## Highlights

- **Win to open, Win to close** — also Esc, a click outside or launching an app.
- **The taskbar Start button works too** — a click opens this menu, not the system one.
- **Tray resident** — no taskbar button, no Alt+Tab entry, survives an `explorer` restart.
- **Seven styles** — Windows 7, Modern, Windows 10, Windows 11, Windows App, Windows Pro,
  Windows 10X. Each one is drawn from scratch.
- **Real apps, real icons** — reads the Start Menu folders, searches as you type.
- **Single exe** — copy it anywhere and run it; settings live next to your user profile.

## Features

### Menu

- Opens on **Win**, closes on **Win**, **Esc**, a click outside or when an app starts.
- Pinned grid, full app list, recent apps and a search box in one window.
- Quick links on the right: Documents, Downloads, Music, Pictures, Control Panel,
  Settings, Run.
- Power menu in the footer.
- Win chords (Win+E, Win+R, Win+X, …) are forwarded to the system.

### Search

- Matches as you type: a name that starts with the query ranks first, then a word that
  starts with it, then any substring.
- **↑ / ↓** move the selection, **Enter** launches it.
- Everything is local: the Start Menu folders are scanned on startup, no index and no
  network calls.

### Look

| Windows 11 | Settings |
| --- | --- |
| ![Menu](docs/menu.png) | ![Settings](docs/settings.png) |

- Seven hand-drawn styles, light and dark, adjustable opacity.
- Backdrop is a snapshot of the desktop under the menu — an acrylic look from a still
  image, not live blur.
- Settings window with style previews, switches and a slider; changes apply instantly.

### System

- No taskbar button, no Alt+Tab entry — the icon lives in the notification area.
- Left click on the tray icon opens the menu; right click gives *Open menu*, *Settings*,
  *Exit*.
- One instance: a second launch only opens settings in the running app
  (`OpenStart.exe --settings`).
- Optional launch at sign-in; that single switch writes one per-user `Run` entry and
  nothing else.
- If a log entry is needed, `%APPDATA%\OpenStart\openstart.log` keeps the start line and
  any panic.

## Installation

1. Download `OpenStart.exe` from [Releases](https://github.com/Erzkanzler2k/OpenStart/releases).
2. Run it — the icon appears in the notification area.
3. Press **Win**.
4. Turn on *Launch at sign-in* in **Manage** if you want it always available.

If the tray icon is hidden, enable it in **Settings → Personalization → Taskbar → Other
system tray icons**.

## Usage

| Action | How |
| --- | --- |
| Open / close the menu | **Win** |
| Close without launching anything | **Esc**, click outside |
| Move through search results | **↑ / ↓** |
| Launch the selected result | **Enter** |
| Open settings | Tray icon → *Settings*, or the gear in the menu footer |
| Quit | Tray icon → *Exit*, or **Manage → Exit** |
| Change the style | *Start menu* section, click a card |

## Configuration

Settings are stored in `%APPDATA%\OpenStart\config.json`:

```json
{
  "style": 3,
  "dark": true,
  "alpha": 242,
  "show_search": true,
  "tiles": true,
  "recent": [],
  "start_x": 559,
  "start_y": 745
}
```

`style` is 0–6: Windows 7, Modern, Windows 10, Windows 11, Windows App, Windows Pro,
Windows 10X. `start_x`/`start_y` is the remembered position of the taskbar Start button;
it is filled in automatically and can be removed to re-learn the button.

## Requirements

- Windows 10 or Windows 11 (x64).
- No admin rights, no installer, no runtime.

## Build from source

```powershell
cargo build --release      # exe and tray icon resources
cargo test --release       # unit tests
.\target\release\openstart.exe
```

Rust 1.75+ with the MSVC toolchain. CI runs tests, a release build and clippy on every
push.

## How it works

- A low-level keyboard hook (`WH_KEYBOARD_LL`) catches Win. A plain tap is swallowed so
  the system menu stays closed; when a chord starts, a synthetic Win keypress is sent so
  the system handles it as usual.
- The menu is a borderless egui window. Settings reuse the same window in another mode.
- The app list is built by walking the Start Menu folders (`.lnk` files); icons come from
  the Windows shell.
- The taskbar start button is remembered by its pixel position and intercepted with a
  low-level mouse hook; if the system menu still appears, it is closed immediately.
- The tray icon uses `Shell_NotifyIcon` with its own hidden window and menu; after an
  `explorer` restart it re-adds itself on `TaskbarCreated`.
- Configuration lives in `%APPDATA%\OpenStart\config.json`.

## Limitations

- The taskbar itself is not replaced: only clicks on the Start button are intercepted,
  everything else on the taskbar stays system-owned.
- Windows 11 ignores custom shortcuts in `WinX\Group3`, so no Win+X entry is added.
- Store (UWP) apps that have no Start Menu shortcut are not listed.
- No manual tile pinning or drag and drop.
- No lock screen, file search or web search replacement.

## Known conflicts

- Anti-cheat and security software that blocks global keyboard hooks can stop the Win
  key from being intercepted.
- While OpenStart is running, Win opens this menu instead of the system one — that is
  the point.

## Status

| Area | State |
| --- | --- |
| Windows 11 24H2 | tested |
| Windows 10 | not tested |
| Multiple monitors | positioned on the monitor with the cursor, not tested on hardware |
| DPI 125–150 % | not tested |
| Other keyboard layouts | not tested |

Verified on Windows 11: opening and closing with Win, search and ranking, keyboard
selection, launching apps, switching styles, saving settings, Win+E, tray icon and its
menu, `explorer` restart, single instance with `--settings`, clicking the taskbar Start
button.

## License

MIT — see [LICENSE](LICENSE).
