# The `.znote` format

A `.znote` file holds a whole workspace in one file: every project, notebook and page, the
assistant chats with their attached files, the look in use and the template add-ons the pages
import. The whole file is encrypted with a password chosen when the file is made. It is the file
you work in: znote opens the last one at start, and everything you write is saved back into it.
Copy the file to another computer and it opens there with all its pages; no other files are
needed.

## How znote works with the file

- **New and open.** With no file to open, znote asks at start: make a new znote file, open one,
  or import an existing notes folder (`Project/Notebook/Page.typ`, at most 2 GB and 50 000 files)
  into a new file. A new file asks for a password and where to store it.
- **Password.** At least 8 characters. znote asks for a file's password the first time it opens
  it on a computer and then keeps it in the computer's password store (Windows Credential
  Manager, the macOS keychain or the Secret Service on Linux), under a hash of the file's path. A
  moved file asks again once. "Save a copy" gives the copy the same password. A file whose
  password is lost cannot be opened.
- **Double-click.** The Windows installer registers `.znote` files. Opening one starts znote with
  that file; if znote is already running, its window comes to the front and switches to the file.
- **Working copy.** An opened file is unpacked into `<app data>/projects/<id>/`. Pages are edited
  there, by you and by the assistant, and every change is on disk at once. The working copy is not
  encrypted, because Typst reads the pages from it; it is removed when znote closes, unless it
  holds changes that could not be written into the file.
- **Writing the file.** The `.znote` is written again when znote closes, when you switch to another
  file, when the window loses focus (so the file is current by the time you copy it), and after 30
  seconds without a change. Only what changed is written again; everything else stays as it was
  in the previous file, so a large file with pictures stays quick to save.
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

A `.znote` is a SQLCipher 4 database with its default settings: AES-256 in CBC mode, an
HMAC-SHA512 on every page, and the key derived from the password with PBKDF2-HMAC-SHA512 (256 000
rounds, a random salt per file). Everything in the file is encrypted, names included. It has one
table:

```sql
CREATE TABLE entries (name TEXT PRIMARY KEY, data BLOB NOT NULL);
```

Each row is one file of the workspace. The names are:

```
znote.json                        what the file needs and how it was last used
pages/Project/Notebook/Page.typ   the pages
pages/Project/images/logo.svg     other files inside a project travel too
themes/<id>/<version>/...         every template add-on (.znx) the pages import
looks/<id>/<version>/...          the look (.zth) in use
fonts/<file>                      fonts the pages use that are not part of an add-on
fonts.json                        which page uses which font
chats/<chat>/chat.json            an assistant chat as the user saw it
chats/<chat>/...                  its history, attached files and hiding levels
```

Names that start with a dot, other `.znote` files and unfinished `.tmp` files are left out, and so
are files directly in `pages/` that belong to no project.

## `znote.json`

```json
{
  "format": 2,
  "look": "acme",
  "themes": ["acme:0.1.0"],
  "settings": {
    "theme": "dark",
    "lastPage": "Work/Notes/Monday",
    "hiddenTemplates": [],
    "hiddenLooks": [],
    "pageLevels": { "Work/Notes/Monday": 2 }
  }
}
```

| Field | Meaning |
|---|---|
| `format` | Always `2`. Any other number is refused. |
| `look` | Id of the look in use, or `null` for the default look. |
| `themes` | Every `@themes/<id>:<version>` a page imports. |
| `settings` | What belongs to the notes and not to the computer: light or dark, the page that was open, hidden templates and looks, and each page's hiding level (0 to 3). Text size, language, the assistant and its keys never travel. |

## Add-ons and looks

Add-ons and the look travel in the file. When a file needs one that is not installed in that
version, znote asks once, then checks it exactly like an imported `.znx` or `.zth` and installs it
next to any other version. One that fails the check is not installed and the file is not opened.
A file that needs an add-on or look it does not carry and that is not installed is refused with
the names of what is missing.

## Fonts

A font a page uses is copied into the file when it is first shown, unless the font says it may
not be embedded or is larger than 64 MB. Fonts of an add-on travel with the add-on. znote tells
the writer which fonts were left out, and tells the reader which of them are missing on their
computer: there another font is used, so lines can break elsewhere and the page count can change.
Only pages that were shown in znote are known; a page that was never opened contributes no fonts.

## Opening

The file is untrusted. A database with triggers or views is not opened. A file with more than
50 000 entries, more than 2 GB of data or a `znote.json` over 1 MB is refused. Entries that would
land outside the working copy are refused, as with a `.znx`.

## Growing the format

Top-level entries znote does not know, and fields it does not know in `znote.json`, are kept and
written back, so a version that does not know them does not drop them. A change a version must
not open raises `format`.
