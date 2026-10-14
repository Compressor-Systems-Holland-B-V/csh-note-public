# znote
**znote** is a lightweight desktop editor for Typst, built with Tauri. Type on one side and watch your document render instantly on the other. No compile step, no waiting.

Make it yours with fully customizable themes for both the editor and the interface, so your workspace looks the way you want it to.

znote also has an assistant. Connect your own Claude subscription (it runs Claude Code), an OpenRouter key, or your own OpenAI-compatible server such as Ollama, and use the assistant panel to generate new documents from a prompt, restructure sections, fix formatting, or ask for edits in plain language. The assistant works directly on your `.typ` files, every change shows up live in the preview, and you can review each change in the editor and keep or undo it.

## Download

Get znote for Windows, macOS or Linux from the
[download page](https://zal-analytics.github.io/znote/), or pick a file from the
[latest release](https://github.com/ZAL-Analytics/znote/releases/latest).

**Features**
- Live Typst preview that updates as you type
- Custom themes for the editor and UI
- AI Assistant panel
- Company names hidden from the assistant, and a private knowledge base it can search
- Native, fast, and small, powered by Tauri
- Works with your local files

**Documentation**
- [The `.znote-theme` format](docs/znote-theme.md): how to build a theme file with UI colours, logos, fonts and Typst templates
- [The `.znote-knowledge` format](docs/znote-knowledge.md): the encrypted file that remembers what is hidden and holds the knowledge base
- [SKOS glossaries](docs/skos-glossary.md): which terms to hide, for whoever builds a glossary

## What the assistant sees

With "Hide names from the assistant" on (Settings > Privacy), znote replaces text before it leaves
your computer with tokens such as `⟦CUSTOMER_3⟧`, and puts the real text back in replies and edits.
Only znote knows what a token stands for; it keeps that in your encrypted `.znote-knowledge` file.

**Hidden:**
- every term in your glossary (customers, people, projects, products, ...), in every spelling it lists
- e-mail addresses, phone numbers, IBANs, card numbers, IP addresses and web addresses
- amounts of money, unless you switch that off
- with the optional name finder: names of people, organisations, products, projects and places
  that are not in the glossary. It misses some names.
- this applies to the text of your pages, attached files, web pages the assistant reads, the
  chat history sent to the provider, and the files Claude Code can read

**Not hidden:**
- page, notebook and project names, and file paths
- the instructions znote gives the assistant
- the facts around a name: "⟦CUSTOMER_3⟧ pays ⟦AMOUNT_4⟧ per month for compressor maintenance"
  still says what kind of business it is. Only a model on your own computer avoids that.
- text a glossary term is split by, such as `*Acme* Corp`, or a term broken over two lines

Click the shield in the editor toolbar to see a page the way the assistant sees it, and to choose
what is hidden. With hiding off, the assistant sees your pages as they are.

## Verifying a download

Every package on the releases page has two files next to it: a `.sha256`
checksum and a `.asc` OpenPGP signature by Arsalan Anwari (`arsalan-anwari`).
The checksum shows the file arrived intact, the signature that it was
published by the maintainer and nobody else.

```sh
# once: import the signing key, from github or from keys.openpgp.org
curl -sL https://github.com/arsalan-anwari.gpg | gpg --import
gpg --keyserver hkps://keys.openpgp.org --recv-keys 88A9AE5871B9A36EE3305977829EFCCDE81402E6

# for each download, next to its .asc and .sha256
sha256sum -c znote_0.8.0_amd64.deb.sha256
gpg --verify znote_0.8.0_amd64.deb.asc znote_0.8.0_amd64.deb
```

`gpg` must print `Good signature` and the primary key fingerprint
`88A9 AE58 71B9 A36E E330  5977 829E FCCD E814 02E6`. Any other fingerprint, or
a `BAD signature`, means the file is not the one that was published: do not install it.

Each package also carries a build provenance attestation, which shows it was
built by this repository's release workflow from the tagged source:

```sh
gh attestation verify znote_0.8.0_amd64.deb --repo ZAL-Analytics/znote
```
