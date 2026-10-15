# The `.znx` format

A `.znx` file is a znote add-on with Typst document templates and the Typst package they import.
Users get one from the Marketplace or import it there. This page is for people who make one.

The look of the app (colours, logo, interface font) is not part of a `.znx`. It comes in a `.zth`
file, a format shared with other apps; see
[`vendor/zsuite/docs/zth.md`](../vendor/zsuite/docs/zth.md). A company that wants both publishes
one of each, usually with the same `id`.

## Layout

A `.znx` is a zip archive with a different extension:

```
theme.json     required
guide.md       required: how to write pages with these templates, read by the assistant
typst/         a normal Typst package: typst.toml, lib.typ, assets/, fonts/ ...
```

Zip the entries themselves, not a folder that contains them.

## `theme.json`

```json
{
  "format": 1,
  "id": "acme",
  "name": "Acme",
  "version": "0.1.0",
  "description": "Acme report templates.",
  "category": "Business",
  "tags": ["navy", "corporate"],
  "typst": {
    "templates": [
      {
        "label": "Report",
        "kind": "document",
        "snippet": "#import \"@themes/acme:0.1.0\": *\n#show: report.with(title: \"Title\")\n"
      }
    ]
  }
}
```

| Field | Rules |
|---|---|
| `format` | Always `1`. Fields not on this page, such as a `ui` section with a look, are refused. |
| `id` | 1 to 40 lowercase letters, digits and dashes, starting with a letter. Importing replaces the installed add-on with the same id and version; other versions stay, because a page imports one exact version. |
| `name` | Shown in the Marketplace. Letters, digits, spaces and `- _ . &`, at most 60 characters. |
| `version` | Three numbers, like `0.1.0`. |
| `description` | Optional. One or two sentences shown on the Marketplace card, at most 200 characters. |
| `category` | Optional. One word or short name the Marketplace can filter on, such as `Academic`, `Business`, `Technical`, `Education`, `Personal` or `Creative`. |
| `tags` | Optional. Up to 10 short words that the search box also looks at. |
| `typst.templates` | Entries for the template picker: `label` (at most 60 letters, digits, spaces and `- _ . &`), `kind` (`document`, `slides` or `poster`) and `snippet`. |

## The Typst package

Everything under `typst/` is installed as the package `@themes/<id>:<version>`, so a page imports
it with `#import "@themes/acme:0.1.0": *`. Fonts anywhere in the add-on are available to Typst by
their family name.

The editor colours its syntax with the chart colours of the look (`.zth`) in use: `--chart-1`
keywords, `--chart-2` strings, `--chart-3` headings and function names, `--chart-4` warnings,
`--chart-5` numbers.

A template's `snippet` is the Typst that the picker puts at the top of the page, between two
marker lines:

```typ
// znote:template acme/Report
#import "@themes/acme:0.1.0": *
#show: report.with(title: "Title")
// znote:end
```

Picking another template replaces that block and leaves the rest of the page alone.

## `guide.md`

Every add-on carries a `guide.md`: Markdown that tells znote's assistant how to write a page with
its templates. When the open page uses one of them, the assistant gets the guide, plus the
top-level `#let` lines the package exports (names, parameters, short values and the `//` comments
right above them; relative imports are followed), and is told not to style the page itself. An
add-on without a guide, or with one over 16 KB, is not imported.

Write it for a reader who knows Typst but not your package, in this order:

- **Templates**: per template, when to use it and the parameters of its show rule.
- **Writing a page**: what the template already styles (page, fonts, headings, tables, title
  block) and how to structure content; for slides, exactly how a new slide starts.
- **Helpers**: each function or colour a page is meant to use, one line on when, and a short
  example that compiles after the template's header.
- **Do not**: what the assistant must leave alone, such as `#set page`, its own colours or boxes.

Only describe how to write Typst with your templates. The assistant is told to ignore anything in
a guide about other pages, files, tools or websites.

## Limits

- The file is at most 50 MB, 200 MB unpacked, 2000 entries.
- Every entry must stay inside the add-on folder; names with `..`, `\` or `:` are refused.
- An add-on that breaks any rule on this page is not imported, and the app says which rule.

## The Marketplace

The Marketplace lists two stores on Hugging Face:

- templates from <https://huggingface.co/datasets/zal-analytics-core/znote-data>: `index.json` and
  `themes/<id>.znx`;
- looks from <https://huggingface.co/datasets/zal-analytics-core/zsuite-themes>: `index.json` and
  `themes/<id>.zth`.

Each `index.json` is an array with the `theme.json` of every published file. A download goes
through the same checks as an imported file.

The sources live in the local `data/src/<id>/` (templates) and `data-looks/src/<id>/` (looks)
folders, one folder per file laid out like the archive. `scripts/sync_data --build` zips them and
writes both `index.json` files, `--upload` also checks that every look installs and every template
compiles and then publishes both, and `--download` fetches what is published. Raise `version`
when you change a published file so the app offers the update.
