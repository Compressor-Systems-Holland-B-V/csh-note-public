# Privacy policy and detection rules

znote decides what reaches an assistant in Rust, from three kinds of data file in
`src-tauri/resources/`. Each has a JSON schema next to it, so an editor that reads `$schema`
checks it as you type. All three are built into the app: change them, then build znote again.

## `policy.json`: what users may change

The built-in policy leaves every choice to the user and sets `dataCollection` to `deny`. A company
that ships its own znote locks choices here:

```json
{
  "$schema": "./policy.schema.json",
  "hideNames": "required",
  "findNames": "optional",
  "hideAmounts": "required",
  "minimumLevel": 2,
  "web": "blocked",
  "providers": ["custom"],
  "dataCollection": "deny",
  "zeroRetention": true
}
```

| Field | Values | Effect |
|---|---|---|
| `hideNames` | `optional`, `required` | `required` turns hiding on for every chat. The switch in Settings and in the chat is locked, and nothing is sent until a knowledge file is open. |
| `findNames` | `optional`, `required` | `required` turns on the downloaded name finder. Nothing is sent until it is downloaded. |
| `hideAmounts` | `optional`, `required` | `required` always hides amounts of money. |
| `minimumLevel` | `0` to `3` | The lowest hiding level any page or attached file can have. Lower levels set by users or carried in a `.znote` are raised to it. |
| `web` | `allowed`, `blocked` | `blocked` takes the web tools away from every assistant. |
| `providers` | `claude`, `openrouter`, `custom` | The assistants users may connect. |
| `dataCollection` | `allow`, `deny` | OpenRouter only. `deny` routes only to providers that do not store or train on what is sent. |
| `zeroRetention` | `true`, `false` | OpenRouter only. `true` routes only to providers that keep nothing after the answer. |

Every field is required and no other field is allowed. The hiding levels are: `0` the name finder
finds nothing, `1` people, `2` also organisations, places and the glossary's own categories, `3`
also products and projects. The glossary, "Always hide this" and the patterns in
`detection.json` hide at every level.

If the file cannot be read, or holds a field or value znote does not know, znote falls back to
the strictest policy and offers no assistant.

## `languages/<code>.json`: words of one language

One file per language, named after its code (`en.json`, `nl.json`). Every file in the folder is
built in and used together, so mixed Dutch and English text works. To support a language, copy
`en.json`, rename it and translate the lists:

| Field | Used for |
|---|---|
| `code` | The language code (BCP 47), the same as the file name. |
| `name` | The language's own name. |
| `stopwords` | Lower-case words the name finder sometimes tags at the start of a sentence that are never a name on their own. |
| `suffixes` | Endings a name carries in running text (`Acmes`, `Rotterdamse`). The ending is hidden with the name. |
| `prefixes` | The same for beginnings, such as articles some languages write onto a name. |
| `sentenceEnds` | Where a sentence ends, so one name finder hit is not read across two sentences. |
| `scaleWords` | Words that scale an amount (`k`, `mln`, `miljoen`). |
| `phoneTrunks` | The digits a national phone number starts with, such as `0`. |

Every field is required. The schema is `languages/language.schema.json`.

## `detection.json`: what is hidden besides the glossary

Language-neutral rules: patterns such as e-mail addresses and amounts, and how the kinds the name
finder returns map to hiding levels. Every field is required.

| Field | Used for |
|---|---|
| `ignorable` | Unicode code points in hex, or a range such as `200B-200F`, that are invisible and skipped when matching: zero-width spaces, soft hyphens. |
| `spaces` | Single characters read as a space when matching, such as Typst's `~`. |
| `currencyCodes` | Three-letter codes such as `EUR`, for amounts. |
| `currencySymbols` | Symbols such as `€`, for amounts. |
| `number` | Regular expression for the number in an amount. |
| `patterns` | What to hide, one entry per pattern; see below. |
| `finder` | `threshold` (above 0, below 1): how sure the name finder must be. `labels`: pairs of the label the name finder is asked for and the kind of token it becomes, such as `["person", "PERSON"]`. |
| `levels` | Per kind, the lowest hiding level (0 to 3) at which the name finder hides it. |
| `defaultLevel` | The level for kinds not listed in `levels`. |

A pattern has these fields:

| Field | Used for |
|---|---|
| `kind` | The kind in the token, upper-case letters and `_`, such as `EMAIL`. |
| `regex` | A regular expression in Rust syntax. `{symbols}`, `{codes}`, `{number}`, `{scale}` and `{trunk}` are filled in from the currencies, `number`, and every language's scale words and phone trunks. |
| `check` | Optional. A check a hit must pass as well: `iban`, `luhn`, `ipv4`, `ipv6`, `phone` or `amount`. |
| `trim` | Optional. Characters taken off the end of a hit. |
| `amounts` | Optional. `true` hides the pattern only when hiding amounts is on. |
