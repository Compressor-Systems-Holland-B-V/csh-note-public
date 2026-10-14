# Changelog

All notable changes to znote are written down here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.0] - 2026-10-14

### Added

- A release pipeline: every version tag builds a Windows installer (`.exe`), a
  macOS disk image (`.dmg`), and Debian (`.deb`), Fedora (`.rpm`) and Arch Linux
  (`.pkg.tar.zst`) packages, each signed with OpenPGP and published with its
  sha256 checksum and a build provenance attestation.
- An MSIX package for the Microsoft Store, built on request.
- A download page that picks out the package for your system.

### Changed

- The editor toolbar shows icons only, with a tooltip on each.
- Error and warning counts moved to a status bar under the editor that is always visible.
- Buttons to make the editor font larger or smaller.
- Ctrl and the scroll wheel zoom the preview.
- Assistant presets are shown as icons, and the assistant panel is narrower.
- The export preview opens at 100%.
- Chat attachments show an icon for their file type.
- Archive all and delete all in the chat history have labels.
- A back button leads out of the chat history.
- The workspace name next to the logo is easier to read.
- A new app icon in the ZAL Analytics colours: a flat rounded tile on a transparent
  background, generated as a PNG and vectorized with `scripts/generate_icon`.

## [0.7.0] - 2026-10-14

### Added

- The assistant can use Claude Code, OpenRouter, or your own OpenAI-compatible
  server such as Ollama. Keys for OpenRouter and your own server are kept in the
  operating system's password store.
- The assistant reads and edits pages through its own tools, can read attached
  PDF and Office documents, and can read web pages when a chat allows it.
- Projects in the sidebar, and quality of life improvements to the assistant chat.
- The interface is translated into several languages.

### Fixed

- Several interface bugs.

## [0.2.1] - 2026-10-13

### Added

- A CodeMirror Typst editor with a live preview rendered by the built-in Typst compiler.
- Notebooks and pages in the sidebar, saved as plain files in a workspace.
- Themes for the editor and the interface, imported from `.znote-theme` files,
  and a theme store with several themes.
- Export to PDF and PowerPoint.
- The assistant panel.
- Saving and opening a whole workspace as one `.znote` file, with the themes and fonts it uses.

## [0.2.0] - 2026-10-12

### Added

- The first version: a Tauri and SvelteKit window with the basic interface and a
  Windows build in CI.

[0.8.0]: https://github.com/ZAL-Analytics/znote/releases/tag/v0.8.0
