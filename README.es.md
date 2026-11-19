# OpenStart

<div align="center">

**Un menú Inicio para Windows 10/11 — su propio gancho de la tecla Win, sus estilos, su búsqueda.**

[English](README.md) · [Русский](README.ru.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · **Español** · [中文](README.zh-CN.md) · [日本語](README.ja.md)

[![Versión](https://img.shields.io/github/v/release/Erzkanzler2k/OpenStart?label=versi%C3%B3n&color=e0af68)](https://github.com/Erzkanzler2k/OpenStart/releases)
[![CI](https://github.com/Erzkanzler2k/OpenStart/actions/workflows/ci.yml/badge.svg)](https://github.com/Erzkanzler2k/OpenStart/actions/workflows/ci.yml)
[![Plataforma](https://img.shields.io/badge/Windows%2010%2F11-x64-0078d4)](#requisitos)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-dea584)](#compilar-desde-el-c%C3%B3digo)
[![Licencia](https://img.shields.io/badge/licencia-MIT-97ca00)](LICENSE)

</div>

![Menú de OpenStart](docs/menu.png)

Un solo exe en Rust. Sin instalador, sin servicios, sin permisos de administrador y sin nada
de terceros. La aplicación intercepta la tecla Win, lee los accesos directos que Windows ya
tiene y dibuja el menú por su cuenta.

## Funciones

- **Win abre, Win cierra** — también Esc, un clic fuera o al lanzar un programa. Sin
  reiniciar y sin reemplazar el shell.
- **También el botón Inicio** — al pulsarlo se abre este menú en lugar del del sistema.
  El primer clic calibra la posición del botón.
- **Tus programas reales** — lee las carpetas del menú Inicio (todos los usuarios y el
  actual), muestra iconos reales y busca por nombre mientras escribes.
- **Siete estilos** — Windows 7, Modern, Windows 10, Windows 11, Windows App, Windows Pro,
  Windows 10X. Cada uno dibujado desde cero, no es un tema.
- **En la bandeja** — sin botón en la barra de tareas y sin entrada en Alt+Tab. Clic
  izquierdo abre el menú, clic derecho ofrece *Abrir menú*, *Configuración*, *Salir*. El
  icono sobrevive al reinicio de `explorer`.
- **Una sola instancia** — un segundo inicio solo abre la configuración de la aplicación en
  marcha (`OpenStart.exe --settings`).
- **Configuración** — seis secciones con vistas previas de estilos, interruptores y un
  control deslizante. Todo se aplica al instante.
- **Mosaicos fijados** — el clic derecho sobre una aplicación la fija o la suelta;
  el orden es tuyo.
- **Inicio automático** — un interruptor en *Administrar*; escribe una sola entrada `Run`
  del usuario.
- **Los atajos Win funcionan** — Win+E, Win+R, Win+X y compañía pasan al sistema.

| Estilos | Configuración |
| --- | --- |
| ![Estilos](docs/settings.png) | ![Menú](docs/menu.png) |

## Instalación

1. Descarga `OpenStart.exe` desde [Releases](https://github.com/Erzkanzler2k/OpenStart/releases).
2. Ejecútalo. El icono aparece en el área de notificación.
3. Pulsa **Win** — el menú se abre.
4. ¿Siempre disponible? Activa *Iniciar al iniciar sesión* en **Administrar**.

Si el icono está oculto, actívalo en **Configuración → Personalización → Barra de tareas →
Otros iconos del área de notificación**.

## Uso

| Acción | Cómo |
| --- | --- |
| Moverse por los resultados | **↑ / ↓** |
| Abrir la selección | **Intro** |
| Abrir / cerrar el menú | **Win** |
| Cerrar sin abrir nada | **Esc**, clic fuera |
| Abrir la configuración | Icono de la bandeja → *Configuración* o el engranaje del menú |
| Salir | Icono → *Salir* o **Administrar → Salir** |
| Cambiar de estilo | Sección *Menú Inicio*, pulsa una tarjeta |

## Configuración

Los ajustes están en `%APPDATA%\OpenStart\config.json`: `style` (0–6), `dark`, `alpha`,
`show_search`, `tiles`, `recent`, `pinned`, `start_x`/`start_y` (posición recordada del botón Inicio;
bórrela para recalibrarla).

## Requisitos

- Windows 10 o Windows 11 (x64).
- Sin permisos de administrador, sin instalador, sin dependencias.

## Compilar desde el código

```powershell
cargo build --release
cargo test --release
.\target\release\openstart.exe
```

Rust 1.75+ con la cadena MSVC.

## Cómo funciona

- Un gancho de teclado de bajo nivel (`WH_KEYBOARD_LL`) captura Win. La pulsación simple se
  bloquea para que no se abra el menú del sistema; cuando empieza un atajo se envía una
  pulsación Win sintética y el sistema la trata como siempre.
- El menú es una ventana egui sin bordes en la parte inferior de la pantalla. La
  configuración reutiliza la misma ventana en otro modo.
- La lista de programas se construye recorriendo las carpetas del menú Inicio (`.lnk`); los
  iconos vienen del shell de Windows.
- El fondo es una captura del escritorio bajo el menú, reducida y estirada de vuelta: un
  aspecto acrílico a partir de una imagen fija, no un desenfoque en vivo.
- El icono de la bandeja usa `Shell_NotifyIcon` con su propia ventana oculta y su menú;
  tras reiniciar `explorer` se vuelve a registrar con `TaskbarCreated`.
- La configuración está en `%APPDATA%\OpenStart\config.json`.

## Límites

- La barra de tareas no se reemplaza: solo se interceptan los clics en el botón Inicio,
  el resto sigue siendo del sistema.
- Windows 11 ignora los accesos directos propios en `WinX\Group3`, así que no hay entrada
  en Win+X.
- Sin aplicaciones UWP que no tengan acceso directo en el menú Inicio.
- Sin anclaje manual ni arrastre de mosaicos.
- Sin reemplazo de la pantalla de bloqueo, la búsqueda de archivos o la web.

## Conflictos conocidos

- Los antitrampas y el software de seguridad que bloquean ganchos de teclado globales
  pueden impedir la captura de Win.
- Mientras OpenStart se ejecuta, Win abre este menú en lugar del del sistema — es su trabajo.

## Estado

Probado en Windows 11: abrir y cerrar con Win, búsqueda, lanzar programas, cambiar de estilo,
guardar la configuración, Win+E, icono de la bandeja y su menú, reinicio de `explorer`,
instancia única con `--settings`, clic en el botón Inicio. No probado: otras distribuciones de teclado, Windows 10,
varios monitores, cambios de DPI.

## Licencia

MIT — ver [LICENSE](LICENSE).
