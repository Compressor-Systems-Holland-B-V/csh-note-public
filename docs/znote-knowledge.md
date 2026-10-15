# The `.znote-knowledge` format

A `.znote-knowledge` file holds what znote needs to hide names from the assistant and to search
your notes: the glossary, the tokens it gave out with the text each one stands for, and the search
index. It is separate from the `.znote` file on purpose: your notes never depend on it, and you
back it up and move it on your own.

## How znote works with the file

- **Where.** Imported and created files live in `<local app data>/knowledge/<id>.znote-knowledge`,
  with `index.json` next to them listing each file's name and which one is in use. Local app data
  is not synced between the computers of a company domain.
- **Password.** The whole file is encrypted with SQLCipher. The password, at least 8 characters,
  is chosen when the file is made. "Remember on this computer" keeps it in the password store of
  the operating system under the file's id; otherwise znote asks for it once per start. The
  password is never written anywhere else.
- **Several files, one in use.** You can import several. Exactly one is in use. With none in use
  the knowledge base is off, and with hiding on nothing is sent to the assistant until one is
  open.
- **Import.** znote opens the chosen file read-only with the password you give, checks it
  (`PRAGMA integrity_check`) and its format version, and only then copies it in. A wrong password,
  an unknown version, a damaged file or one with database triggers or views is refused and nothing
  is copied. A file whose id is already imported is refused too.
- **Export.** Writes a copy with its own password (`sqlcipher_export`), opens the copy with that
  password and checks it before reporting the export as done. The copy keeps the id, the glossary
  and every token, so it gives the same tokens on another computer.
- **Change password.** Asks for the current password, then re-encrypts the file in place.
- **Delete.** Removes the file and its remembered password. Chats that used it can no longer be
  continued, because nothing else knows what their tokens stand for.

## Inside

The file is a SQLite database encrypted with SQLCipher 4 and its default settings, the same as a
`.znote` file (see [the `.znote` format](znote-file.md#layout)). Format version 1 has these tables:

| Table | Holds |
|---|---|
| `meta` | `version` (`1`), `id` (16 hex characters), `name` |
| `concepts` | Terms of the imported glossary: IRI, kind, hidden or not, category or not, notes |
| `labels` | Every spelling of every term, in order, with whether it is a hidden label |
| `edges` | `broader` and `related` links between terms |
| `overrides` | "Don't hide this" (`kind` empty) and "Always hide this" (`kind` set) |
| `vault` | Every token ever given out, its kind, number, variant and the exact text it stands for |
| `veil_cache` | What the name finder found per line, keyed by a SHA-256 of the line |
| `sources` | Each page or file in the knowledge base: kind, znote file, path, SHA-256, time |
| `chunks` | Pieces of about 1 000 characters of each source, in real text |
| `chunks_fts` | FTS5 full-text index of the chunks |
| `vectors` | Smart search vectors of the chunks (256 numbers each) |
| `mentions` | Which chunk mentions which term |

## Rules that keep tokens safe

- Tokens are only ever added. A token never changes meaning and a number is never reused, so a
  reply or an edit that arrives later still puts back the right text.
- The same text always gets the same token, across chats and znote files.
- `veil(text)` followed by putting the tokens back gives the exact same bytes. Text that already
  looks like a token is hidden as a token of its own (`⟦TOKEN_n⟧`) so this always holds.
- No token spans a line break, so hidden pages have the same line numbers as the real ones.
