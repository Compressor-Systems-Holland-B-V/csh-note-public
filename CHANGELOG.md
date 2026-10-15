# Changelog

All notable changes to znote are written down here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0]

### Added

- The assistant sees the compile errors on a page it wrote, as the editor shows
  them, and fixes them. OpenRouter and self-hosted models get them with each
  edit and with each page they read; Claude Code is told the errors already on
  the chat's pages and is sent back to a page it broke, at most twice. With
  hiding on, the text Typst quotes from the page is left out of each error and
  the line it is on is shown from the hidden page instead.
- A `.znote` file is encrypted with a password chosen when it is made, and
  holds the assistant chats with their attached files. znote asks for the
  password once per computer and keeps it in the computer's password store.
- `src-tauri/resources/policy.json` lets a company that ships its own znote
  make hiding, the name finder and amounts mandatory, set a lowest hiding
  level, block the web tools and limit the assistants. See
  `docs/privacy-policy.md`.
- Language rules (words that are never a name, endings such as "Acmes" and
  "Rotterdamse", scale words) are one JSON file per language, with a schema.
  Dutch and English are built in.
- "Show what this message would send" before a message is sent, and the
  assistant's instructions in the view of what was sent.
- OpenRouter is asked to use only providers that do not store or train on what
  is sent.
- Export to Word (`.docx`) that looks like the page: fonts, colours, spacing,
  table colours, header and footer (with page numbers) come from the laid-out
  page, while the text flows like any Word document with Word's own headings,
  lists, tables, pictures, links, footnotes and equations. Designed pages such
  as a cover are copied as they are. TrueType fonts travel inside the file.
- In PowerPoint exports, images are pictures of their own that can be replaced
  or moved, and formulas are equations that can be edited.

### Changed

- Looks and templates are separate now. A look (colours, logo, interface font)
  is a `.zth` file and templates are a `.znx` file. The Marketplace lists them
  as Looks and Templates, and Import takes either file. A `.znx` with a look in
  it is refused.
- The interface components come from `@zsuite/ui`, shared with other apps in
  the `vendor/zsuite` submodule. Clone with `--recurse-submodules`.

### Fixed

- With names hidden, the assistant no longer sees the real names of projects,
  notebooks and pages, or the add-on templates in its instructions.
- A name hidden after a chat started is hidden in that chat's copy of the pages
  too.
- Long paragraphs are searched for names to their end, not only the first 200
  words.
- Hiding no longer turns itself off when its settings have not loaded yet or the
  name finder failed to load; the assistant says so and stops instead.
- The assistant can no longer find out a hidden name by searching or editing
  with guesses at it, and text is cut only after it is hidden.
- A knowledge file with database triggers or views is refused.
- The assistant's working folder no longer holds a file with the real page
  names.
- Hiding is decided in Rust from what the user asked for. When it cannot run
  (no knowledge file, a locked one, or a name finder that is not downloaded)
  nothing is sent and the chat says why. Page levels count as strict until they
  are known.
- Names written with Typst's `~`, a soft hyphen, a zero-width space, a
  decomposed accent or a PDF ligature are hidden like any other spelling.
- An attached file whose hiding level cannot be read is hidden at the strictest
  level.
- The assistant can rename or delete only pages the chat was given.
- A custom server address uses plain `http://` only on this computer or the
  local network.
- A page or file that makes the Typst renderer or the PDF reader fail no longer
  closes znote.
- A damaged working-copy record keeps the unsaved notes instead of clearing
  them.

### Removed

- The MSIX package. znote is released only as the packages on the GitHub
  release page and the download page, not in the Microsoft Store.

## [0.9.0] - 2026-10-14

### Added

- Three layouts: Edit (as before), Assistant (a large chat with the preview and
  editor in tabs) and Redact (chat, page search and a redaction panel). Pick one
  on first start or in Settings.
- A first start that asks for the layout, what the assistant may see, the
  knowledge base and the Marketplace. Every answer can be changed in Settings.
- Each page has its own filter level, set in the editor's filter view (eye icon)
  or when a page is added to a chat. It travels inside the `.znote` file.
- Attached PDF and Office files are read as structured text with headings,
  lists and tables, and the filter view shows them formatted.
- The Marketplace can be switched off in Settings.

### Changed

- The Themes page is now called Marketplace. It offers add-ons that can change
  the look, add page templates, or both.
- Add-on files use the `.znx` extension. znote no longer reads `.znote-theme`
  files; the contents are the same, so renaming such a file to `.znx` is enough
  to import it again.
- Shorter labels and hints throughout the app.

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

[1.0.0]: https://github.com/ZAL-Analytics/znote/releases/tag/v1.0.0
