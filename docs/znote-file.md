# The `.znote` format

A `.znote` file holds a whole workspace in one file: every project, notebook and page, the look
that was in use, and the list of themes the pages need. Save or open one with the two buttons
next to Settings.

## Layout

A `.znote` is a zip archive with a different extension:

```
znote.json                        what the file needs
pages/Project/Notebook/Page.typ   the workspace folder, as it is on disk
pages/Project/images/logo.svg     other files inside a project travel too
```

Files directly in the workspace folder, names that start with a dot and other `.znote` files are
left out.

## `znote.json`

```json
{
  "format": 1,
  "look": "acme",
  "themes": ["acme:0.1.0"]
}
```

| Field | Meaning |
|---|---|
| `format` | Always `1`. A higher number is refused as made by a newer znote. |
| `look` | Id of the theme used for the app's look, or `null` for the default look. |
| `themes` | Every `@themes/<id>:<version>` a page imports. |

Themes are not stored in the file. Opening is refused, and nothing is unpacked, unless the `look`
theme is installed (any version) and every entry in `themes` is installed in exactly that version.
The message lists what is missing.

## Opening

Opening a `.znote` replaces the workspace: after a warning, every project in the workspace folder
is moved to the Recycle Bin and the projects from the file take their place. Files directly in the
workspace folder and names that start with a dot are left alone. Nothing is touched when the file
is refused. The file is untrusted: entries that would land outside the workspace folder are
refused, as with a `.znote-theme`.
