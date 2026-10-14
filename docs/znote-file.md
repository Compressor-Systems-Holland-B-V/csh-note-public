# The `.znote` format

A `.znote` file holds a whole workspace in one file: every project, notebook and page, the look
that was in use, and the list of themes the pages need. It is the file you work in: znote opens
the last one at start, and everything you write is saved back into it. Copy the file to another
computer and it opens there with all its pages; no other files are needed.

## How znote works with the file

- **New and open.** With no file to open, znote asks at start: make a new znote file (you choose
  where it is stored), open one, or bring in a notes folder from an earlier version.
- **Double-click.** The Windows installer registers `.znote` files. Opening one starts znote with
  that file; if znote is already running, its window comes to the front and switches to the file.
- **Working copy.** An opened file is unpacked into `<app data>/projects/<id>/`. Pages are edited
  there, by you and by the assistant, and every change is on disk at once.
- **Writing the file.** The `.znote` is written again when znote closes, when you switch to another
  file, when the window loses focus (so the file is current by the time you copy it), and after 30
  seconds without a change. Only what changed is compressed again; everything else is copied as
  it is from the previous file, so a large file with pictures stays quick to save.
- **Safe replace.** The new file is written next to the old one, read back, and only then put in
  its place. The file before the last save is kept as `Name.znote.bak`.
- **After a crash.** Changes that were not written yet stay in the working copy and are written
  the next time the file is opened. If the file itself was also replaced in the meantime, those
  changes are saved next to it as `Name (recovered 1).znote` and the file is opened as it is.
- **A file replaced from outside** (copied over, synced) is unpacked again the next time it is opened.
- "Save a copy" (next to Settings) writes a second `.znote` without switching to it. It proposes
  `Name copy.znote`, so the copy does not land on the open file by accident.
- A copy or a new file written over an existing `.znote` keeps that file as `Name.znote.bak` too.

## Layout

A `.znote` is a zip archive with a different extension:

```
znote.json                        what the file needs and how it was last used
pages/Project/Notebook/Page.typ   the pages
pages/Project/images/logo.svg     other files inside a project travel too
themes/<id>/<version>/...         every theme the pages or the look use
fonts/<file>                      fonts the pages use that are not part of a theme
fonts.json                        which page uses which font
```

Names that start with a dot, other `.znote` files and unfinished `.tmp` files are left out.
Pictures, PDFs and other files that are already compressed are stored as they are.

## `znote.json`

```json
{
  "format": 1,
  "look": "acme",
  "themes": ["acme:0.1.0"],
  "settings": { "theme": "dark", "lastPage": "Work/Notes/Monday", "hiddenTemplates": [], "hiddenLooks": [] }
}
```

| Field | Meaning |
|---|---|
| `format` | Always `1`. A higher number is refused as made by a newer znote. |
| `look` | Id of the theme used for the app's look, or `null` for the default look. |
| `themes` | Every `@themes/<id>:<version>` a page imports. |
| `settings` | What belongs to the notes and not to the computer: light or dark, the page that was open, hidden templates and looks. Text size, language, the assistant and its keys never travel. |

## Themes

Themes travel in the file. When a file needs a theme that is not installed in that version, znote
asks once, then checks the theme exactly like an imported `.znx` and installs it next to
any other version. A theme that fails the check is not installed and the file is not opened. A
file made by an older znote that only lists a theme is refused with the names of what is missing.

## Fonts

A font a page uses is copied into the file when it is first shown, unless the font says it may
not be embedded or is larger than 64 MB. Fonts of a theme travel with the theme. znote tells the
writer which fonts were left out, and tells the reader which of them are missing on their
computer: there another font is used, so lines can break elsewhere and the page count can change.
Only pages that were shown in znote are known; a page that was never opened contributes no fonts.

## Opening

The file is untrusted: entries that would land outside the working copy are refused, as with a
`.znx`.

## Growing the format

Entries znote does not know, and fields it does not know in `znote.json`, are kept and written
back, so an older version does not drop what a newer one added. A change older versions must not
open raises `format`.
