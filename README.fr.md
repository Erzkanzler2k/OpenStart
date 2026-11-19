# OpenStart

<div align="center">

**Un menu Démarrer pour Windows 10/11 — son propre crochet de la touche Win, ses styles, sa recherche.**

[English](README.md) · [Русский](README.ru.md) · [Deutsch](README.de.md) · **Français** · [Español](README.es.md) · [中文](README.zh-CN.md) · [日本語](README.ja.md)

[![Version](https://img.shields.io/github/v/release/Erzkanzler2k/OpenStart?label=version&color=e0af68)](https://github.com/Erzkanzler2k/OpenStart/releases)
[![Plateforme](https://img.shields.io/badge/Windows%2010%2F11-x64-0078d4)](#pr%C3%A9requis)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-dea584)](#compiler-depuis-les-sources)
[![Licence](https://img.shields.io/badge/licence-MIT-97ca00)](LICENSE)

</div>

![Menu OpenStart](docs/menu.png)

Un seul exe en Rust. Pas d'installateur, pas de service, pas d'écriture dans le registre,
aucun logiciel tiers. L'application intercepte la touche Win, lit les raccourcis déjà
présents dans Windows et dessine le menu elle-même.

## Fonctionnalités

- **Win ouvre, Win ferme** — comme Échap, un clic à côté ou le lancement d'un programme.
  Sans redémarrage, sans remplacer le shell.
- **Le bouton Démarrer aussi** — un clic dessus ouvre ce menu au lieu de celui du
  système. Le tout premier clic calibre la position du bouton.
- **Vos vrais programmes** — lit les dossiers du menu Démarrer (tous les utilisateurs et
  l'utilisateur courant), affiche les vraies icônes et cherche par nom à la frappe.
- **Sept styles** — Windows 7, Modern, Windows 10, Windows 11, Windows App, Windows Pro,
  Windows 10X. Chacun est dessiné de zéro, ce n'est pas un habillage.
- **Dans la zone de notification** — aucune icône dans la barre des tâches, aucune entrée
  dans Alt+Tab. Clic gauche ouvre le menu, clic droit propose *Ouvrir le menu*, *Paramètres*,
  *Quitter*. L'icône survit au redémarrage de `explorer`.
- **Une seule instance** — un second lancement ouvre seulement les paramètres de
  l'application en cours (`OpenStart.exe --settings`).
- **Paramètres** — six sections avec aperçus des styles, interrupteurs et curseur. Tout
  s'applique immédiatement.
- **Démarrage automatique** — un interrupteur dans *Gestion*, sans toucher au registre.
- **Les raccourcis Win fonctionnent** — Win+E, Win+R, Win+X et les autres passent au système.

| Styles | Paramètres |
| --- | --- |
| ![Styles](docs/settings.png) | ![Menu](docs/menu.png) |

## Installation

1. Téléchargez `OpenStart.exe` depuis les [Releases](https://github.com/Erzkanzler2k/OpenStart/releases).
2. Lancez-le. L'icône apparaît dans la zone de notification.
3. Appuyez sur **Win** — le menu s'ouvre.
4. Toujours disponible ? Activez *Lancer à l'ouverture de session* dans **Gestion**.

Si l'icône est masquée, activez-la dans **Paramètres → Personnalisation → Barre des tâches
→ Autres icônes de la zone de notification**.

## Utilisation

| Action | Comment |
| --- | --- |
| Ouvrir / fermer le menu | **Win** |
| Fermer sans rien lancer | **Échap**, clic à côté |
| Ouvrir les paramètres | Icône → *Paramètres* ou l'engrenage dans le menu |
| Quitter | Icône → *Quitter* ou **Gestion → Quitter** |
| Changer de style | Section *Menu Démarrer*, cliquez une carte |

## Prérequis

- Windows 10 ou Windows 11 (x64).
- Aucun droit administrateur, aucun installateur, aucune dépendance.

## Compiler depuis les sources

```powershell
cargo build --release
.\target\release\openstart.exe
```

Rust 1.75+ avec la chaîne MSVC.

## Fonctionnement

- Un crochet clavier de bas niveau (`WH_KEYBOARD_LL`) intercepte Win. L'appui simple est
  bloqué pour garder le menu système fermé ; quand un raccourci commence, un appui Win
  synthétique est envoyé et le système le traite normalement.
- Le menu est une fenêtre egui sans bordure en bas de l'écran. Les paramètres réutilisent
  la même fenêtre dans un autre mode.
- La liste des programmes est construite en parcourant les dossiers du menu Démarrer
  (`.lnk`), les icônes viennent du shell Windows.
- L'arrière-plan est une capture du bureau sous le menu, réduite puis étirée — un rendu
  acrylique à partir d'une image fixe, pas un flou en direct.
- L'icône de notification utilise `Shell_NotifyIcon` avec sa propre fenêtre cachée et son
  menu ; après un redémarrage de `explorer` elle se réinscrit via `TaskbarCreated`.
- La configuration est dans `%APPDATA%\OpenStart\config.json`.

## Limites

- La barre des tâches elle-même n'est pas remplacée : seuls les clics sur le bouton
  Démarrer sont interceptés, le reste reste au système.
- Windows 11 ignore les raccourcis personnalisés dans `WinX\Group3`, donc pas d'entrée Win+X.
- Pas d'applications UWP sans raccourci dans le menu Démarrer.
- Pas d'épinglage manuel ni de glisser-déposer de tuiles.
- Pas de remplacement de l'écran de verrouillage, de la recherche de fichiers ou du web.

## Conflits connus

- Les anti-triche et logiciels de sécurité qui bloquent les crochets clavier globaux
  peuvent empêcher l'interception de Win.
- Tant qu'OpenStart tourne, Win ouvre ce menu au lieu du menu système — c'est le but.

## État

Testé sous Windows 11 : ouverture et fermeture avec Win, recherche, lancement de
programmes, changement de style, sauvegarde des paramètres, Win+E, icône de notification et
son menu, redémarrage de `explorer`, instance unique avec `--settings`. Non testé : autres
dispositions de clavier, Windows 10, plusieurs écrans, changement de DPI.

## Licence

MIT — voir [LICENSE](LICENSE).
