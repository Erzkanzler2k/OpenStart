# OpenStart

<div align="center">

**Windows 10/11 用のスタートメニュー — 独自の Win キーフック、独自のスタイル、独自の検索。**

[English](README.md) · [Русский](README.ru.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Español](README.es.md) · [中文](README.zh-CN.md) · **日本語**

[![リリース](https://img.shields.io/github/v/release/Erzkanzler2k/OpenStart?label=release&color=e0af68)](https://github.com/Erzkanzler2k/OpenStart/releases)
[![CI](https://github.com/Erzkanzler2k/OpenStart/actions/workflows/ci.yml/badge.svg)](https://github.com/Erzkanzler2k/OpenStart/actions/workflows/ci.yml)
[![プラットフォーム](https://img.shields.io/badge/Windows%2010%2F11-x64-0078d4)](#動作環境)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-dea584)](#ソースからビルド)
[![ライセンス](https://img.shields.io/badge/license-MIT-97ca00)](LICENSE)

</div>

![OpenStart のメニュー](docs/menu.png)

Rust 製の実行ファイル 1 つ。インストーラーもサービスも管理者権限もなく、
他のソフトも必要ありません。Win キーをフックし、Windows に既にあるショートカットを読み、
メニューを自前で描画します。

## 機能

- **Win で開き、Win で閉じる** — Esc、外側のクリック、プログラムの起動でも閉じます。
  再起動もシェルの置き換えも不要です。
- **タスクバーのスタート ボタンも** — クリックするとシステムのメニューではなくこの
  メニューが開きます。最初の 1 回のクリックでボタン位置を学習します。
- **本物のアプリ** — スタートメニューのフォルダー（全ユーザーと現在のユーザー）を読み、
  実際のアイコンを表示し、入力しながら名前で検索します。
- **7 つのスタイル** — Windows 7、Modern、Windows 10、Windows 11、Windows App、Windows Pro、
  Windows 10X。どれもスキンではなく一から描画しています。
- **常駐トレイ** — タスクバーのボタンも Alt+Tab の項目もありません。左クリックでメニュー、
  右クリックで *メニューを開く*、*設定*、*終了*。`explorer` の再起動後もアイコンは復帰します。
- **単一インスタンス** — 二重起動は実行中のアプリで設定を開くだけです
  （`OpenStart.exe --settings`）。
- **設定** — スタイルのプレビュー、スイッチ、スライダーを備えた 6 つのセクション。
  変更はすぐに反映されます。
- **自動起動** — *管理* のスイッチで有効化。ユーザーの `Run` 項目を 1 つ書くだけです。
- **Win の組み合わせはそのまま** — Win+E、Win+R、Win+X などはシステムに渡します。

| スタイル | 設定 |
| --- | --- |
| ![スタイル](docs/settings.png) | ![メニュー](docs/menu.png) |

## インストール

1. [Releases](https://github.com/Erzkanzler2k/OpenStart/releases) から `OpenStart.exe` を
   ダウンロードします。
2. 実行すると、通知領域にアイコンが現れます。
3. **Win** を押すとメニューが開きます。
4. 常に使うなら、**管理** で「サインイン時に起動」をオンにします。

アイコンが見えない場合は、**設定 → 個人用設定 → タスクバー → その他のシステム トレイ
アイコン** で有効にしてください。

## 使い方

| 操作 | 方法 |
| --- | --- |
| 候補を移動 | **↑ / ↓** |
| 選択を起動 | **Enter** |
| メニューを開く / 閉じる | **Win** |
| 何も起動せず閉じる | **Esc**、外側をクリック |
| 設定を開く | トレイのアイコン → *設定*、またはメニュー右下の歯車 |
| 終了 | トレイのアイコン → *終了*、または **管理 → 終了** |
| スタイル変更 | *スタートメニュー* のセクションでカードをクリック |

## 設定

設定は `%APPDATA%\OpenStart\config.json` に保存されます: `style`（0〜6）、`dark`、`alpha`、
`show_search`、`tiles`、`recent`、`start_x`/`start_y`（スタート ボタンの記憶位置。削除すると
再学習します）。

## 動作環境

- Windows 10 または Windows 11（x64）。
- 管理者権限、インストーラー、ランタイムは不要です。

## ソースからビルド

```powershell
cargo build --release
cargo test --release
.\target\release\openstart.exe
```

Rust 1.75+ と MSVC ツールチェーンが必要です。

## 仕組み

- 低レベル キーボード フック（`WH_KEYBOARD_LL`）で Win を捕捉します。単独タップは飲み込み、
  システムのメニューが出ないようにします。組み合わせが始まると合成の Win キーを送り、
  システムは通常どおり処理します。
- メニューは画面下部の枠なし egui ウィンドウです。設定は同じウィンドウを別モードで
  使い回します。
- プログラム一覧はスタートメニューのフォルダー（`.lnk`）を走査して作り、アイコンは
  Windows シェルから取得します。
- 背景はメニューの下のデスクトップのスクリーンショットを縮小して引き伸ばしたもの。
  静止画によるアクリル風で、リアルタイムのぼかしではありません。
- トレイ アイコンは `Shell_NotifyIcon` と独自の隠しウィンドウ、メニューを使用します。
  `explorer` の再起動後は `TaskbarCreated` で再登録します。
- 設定は `%APPDATA%\OpenStart\config.json` に保存されます。

## 未対応

- タスクバー自体の置き換えはしません。スタート ボタンのクリックだけを横取りし、それ以外は
  システムのままです。
- Win+X の項目は追加できません。Windows 11 は `WinX\Group3` の独自ショートカットを無視します。
- スタートメニューにショートカットがない UWP アプリ。
- タイルの手動ピン留めやドラッグ アンド ドロップ。
- ロック画面、ファイル検索、ウェブ検索の置き換え。

## 既知の競合

- グローバル キーボード フックを遮断するアンチチートやセキュリティ ソフトは、Win の捕捉を
  妨げることがあります。
- OpenStart の実行中は、Win がシステム メニューではなくこのメニューを開きます。それが目的です。

## 状態

Windows 11 で確認済み: Win での開閉、検索、プログラムの起動、スタイル変更、設定の保存、
Win+E、トレイ アイコンとそのメニュー、`explorer` の再起動、`--settings` による単一
インスタンス。未確認: 他のキーボード配列、Windows 10、複数モニター、DPI の変更。

## ライセンス

MIT — [LICENSE](LICENSE) を参照してください。
