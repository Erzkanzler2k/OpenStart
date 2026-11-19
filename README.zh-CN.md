# OpenStart

<div align="center">

**Windows 10/11 的开始菜单 —— 自己的 Win 键钩子、自己的样式、自己的搜索。**

[English](README.md) · [Русский](README.ru.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Español](README.es.md) · **中文** · [日本語](README.ja.md)

[![版本](https://img.shields.io/github/v/release/Erzkanzler2k/OpenStart?label=%E7%89%88%E6%9C%AC&color=e0af68)](https://github.com/Erzkanzler2k/OpenStart/releases)
[![平台](https://img.shields.io/badge/Windows%2010%2F11-x64-0078d4)](#系统要求)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-dea584)](#从源码构建)
[![许可证](https://img.shields.io/badge/%E8%AE%B8%E5%8F%AF%E8%AF%81-MIT-97ca00)](LICENSE)

</div>

![OpenStart 菜单](docs/menu.png)

一个 Rust 可执行文件。无需安装、无需服务、不写注册表，也不依赖任何第三方软件。程序
自己拦截 Win 键、读取 Windows 已有的快捷方式，并自己绘制菜单。

## 功能

- **Win 打开，Win 关闭** —— 也可以用 Esc、点击菜单外或启动程序关闭。无需重启，也不替换
  系统外壳。
- **真实的程序** —— 读取开始菜单文件夹（所有用户和当前用户），显示真实图标，边输入边
  按名称搜索。
- **七种样式** —— Windows 7、Modern、Windows 10、Windows 11、Windows App、Windows Pro、
  Windows 10X。每种都是从头绘制，而不是套皮肤。
- **常驻托盘** —— 没有任务栏按钮，也不出现在 Alt+Tab 中。左键打开菜单，右键提供
  *打开菜单*、*设置*、*退出*。`explorer` 重启后图标仍会恢复。
- **单实例** —— 再次启动只会在已运行的程序中打开设置（`OpenStart.exe --settings`）。
- **设置** —— 六个分区，带样式预览、开关和滑块，改动立即生效。
- **开机自启** —— 在 *管理* 中一键开启，不需要改注册表。
- **Win 组合键照常工作** —— Win+E、Win+R、Win+X 等会交给系统处理。

| 样式 | 设置 |
| --- | --- |
| ![样式](docs/settings.png) | ![菜单](docs/menu.png) |

## 安装

1. 从 [Releases](https://github.com/Erzkanzler2k/OpenStart/releases) 下载 `OpenStart.exe`。
2. 运行，图标会出现在通知区域。
3. 按下 **Win** —— 菜单打开。
4. 想一直可用？在 **管理** 中开启「登录时启动」。

如果看不到图标，请在 **设置 → 个性化 → 任务栏 → 其他系统托盘图标** 中启用它。

## 使用

| 操作 | 方法 |
| --- | --- |
| 打开 / 关闭菜单 | **Win** |
| 关闭且不启动任何程序 | **Esc**、点击菜单外 |
| 打开设置 | 托盘图标 → *设置*，或菜单底部的齿轮 |
| 退出 | 托盘图标 → *退出*，或 **管理 → 退出** |
| 切换样式 | *开始菜单* 分区，点击卡片 |

## 系统要求

- Windows 10 或 Windows 11（x64）。
- 不需要管理员权限、安装程序或运行时。

## 从源码构建

```powershell
cargo build --release
.\target\release\openstart.exe
```

需要 Rust 1.75+ 和 MSVC 工具链。

## 工作原理

- 低级键盘钩子（`WH_KEYBOARD_LL`）截获 Win。单击会被吞掉，以免系统菜单弹出；当组合键
  开始时，程序会发送一次合成的 Win 按键，系统便照常处理。
- 菜单是屏幕底部的无边框 egui 窗口。设置复用了同一个窗口，只是模式不同。
- 程序列表通过遍历开始菜单文件夹（`.lnk`）生成，图标来自 Windows 外壳。
- 背景是菜单下方桌面的截图，缩小后再拉伸 —— 静态图片做出的亚克力效果，而不是实时模糊。
- 托盘图标使用 `Shell_NotifyIcon`，配有独立的隐藏窗口和菜单；`explorer` 重启后会通过
  `TaskbarCreated` 消息重新添加。
- 配置保存在 `%APPDATA%\OpenStart\config.json`。

## 暂不支持

- 不替换任务栏：「开始」按钮仍归系统，菜单由 Win 键或托盘图标打开。
- 不添加 Win+X 条目：Windows 11 会忽略 `WinX\Group3` 中的自定义快捷方式。
- 没有开始菜单快捷方式的 UWP 应用。
- 手动固定或拖动磁贴。
- 锁屏、文件搜索和网络搜索的替代。

## 已知冲突

- 反作弊和安全软件如果阻止全局键盘钩子，就可能无法拦截 Win 键。
- OpenStart 运行时，Win 打开的是本菜单而不是系统菜单 —— 这正是它的用途。

## 状态

已在 Windows 11 上测试：用 Win 打开和关闭、搜索、启动程序、切换样式、保存设置、Win+E、
托盘图标及其菜单、`explorer` 重启、`--settings` 单实例。未测试：其他键盘布局、Windows 10、
多显示器、DPI 变化。

## 许可证

MIT —— 见 [LICENSE](LICENSE)。
