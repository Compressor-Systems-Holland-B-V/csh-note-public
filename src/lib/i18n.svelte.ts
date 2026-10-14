type Dict = { [key: string]: string | Dict };

export type Params = Record<string, string | number>;

export const fallback = "en";

export const locales = [
  { tag: "en", name: "English" },
  { tag: "fr", name: "Français" },
  { tag: "de", name: "Deutsch" },
  { tag: "nl", name: "Nederlands" },
  { tag: "es", name: "Español" },
  { tag: "pt", name: "Português" },
  { tag: "it", name: "Italiano" },
  { tag: "nb", name: "Norsk" },
  { tag: "sv", name: "Svenska" },
  { tag: "da", name: "Dansk" },
  { tag: "el", name: "Ελληνικά" },
  { tag: "ru", name: "Русский" },
  { tag: "uk", name: "Українська" },
  { tag: "pl", name: "Polski" },
  { tag: "he", name: "עברית" },
  { tag: "ar", name: "العربية" },
  { tag: "fa", name: "فارسی" },
  { tag: "tr", name: "Türkçe" },
  { tag: "zh-CN", name: "简体中文" },
  { tag: "zh-TW", name: "繁體中文" },
  { tag: "ja", name: "日本語" },
];

const rightToLeft = ["he", "ar", "fa"];

const bundles: Record<string, Dict> = {};
for (const [path, data] of Object.entries(import.meta.glob<Dict>("./assets/locale/*/*.json", { eager: true, import: "default" }))) {
  const [, tag, file] = /\/([^/]+)\/([^/]+)\.json$/.exec(path) ?? [];
  (bundles[tag] ??= {})[file] = data;
}

export function resolveLocale(wanted: string | null, preferred: readonly string[] = navigator.languages) {
  const tags = locales.map(({ tag }) => tag);
  for (const candidate of wanted === null ? preferred : [wanted]) {
    const base = candidate.split("-")[0];
    const near = base === "zh" ? (/-(Hant|TW|HK|MO)\b/.test(candidate) ? "zh-TW" : "zh-CN") : base === "no" || base === "nn" ? "nb" : base;
    const found = tags.find((tag) => tag === candidate) ?? tags.find((tag) => tag === near);
    if (found !== undefined) return found;
  }
  return fallback;
}

export const i18n = $state({
  locale: resolveLocale(null),
  get dir() {
    return rightToLeft.includes(this.locale) ? ("rtl" as const) : ("ltr" as const);
  },
});

export function setLocale(wanted: string | null) {
  i18n.locale = resolveLocale(wanted);
  document.documentElement.lang = i18n.locale;
  document.documentElement.dir = i18n.dir;
}

function nodeAt(locale: string, key: string) {
  let node: string | Dict | undefined = bundles[locale];
  for (const step of key.split(".")) node = typeof node === "object" ? node[step] : undefined;
  return node;
}

function lookup(locale: string, key: string) {
  const found = nodeAt(locale, key);
  return typeof found === "string" ? found : null;
}

const filled = (text: string, params: Params) => text.replace(/\{(\w+)\}/g, (whole, name: string) => (name in params ? String(params[name]) : whole));

export function t(key: string, params: Params = {}) {
  const locale = i18n.locale;
  const count = params.count;
  const keys = typeof count === "number" ? [`${key}_${new Intl.PluralRules(locale).select(count)}`, `${key}_other`, key] : [key];
  for (const candidate of keys) {
    const text = lookup(locale, candidate) ?? lookup(fallback, candidate);
    if (text !== null) return filled(text, params);
  }
  return key;
}

export const languageOptions = () => [
  { value: null as string | null, label: t("settings.languageAuto") },
  ...locales.map(({ tag, name }) => ({ value: tag as string | null, label: name })),
];

function leaves(dict: Dict, prefix: string): [string, string][] {
  return Object.entries(dict).flatMap(([key, value]) => (typeof value === "string" ? [[`${prefix}${key}`, value] as [string, string]] : leaves(value, `${prefix}${key}.`)));
}

const escaped = (piece: string) => piece.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

function patterns(section: string) {
  const found = nodeAt(fallback, section);
  return leaves(typeof found === "object" ? found : {}, `${section}.`)
    .sort(([, a], [, b]) => b.length - a.length)
    .map(([key, english]) => ({
      key,
      names: [...english.matchAll(/\{(\w+)\}/g)].map((match) => match[1]),
      shape: new RegExp(`^${english.split(/\{\w+\}/).map(escaped).join("([\\s\\S]+?)")}$`),
    }));
}

const known: Record<string, ReturnType<typeof patterns>> = {};

// ponytail: the backend and saved chats speak English; their text is matched against the English
// strings of one section and said again in the user's language. A message with no match stays
// English. Give the backend message keys if this ever needs more than that.
export function translated(text: string, section = "backend"): string {
  if (i18n.locale === fallback) return text;
  for (const { key, names, shape } of (known[section] ??= patterns(section))) {
    const match = shape.exec(text);
    if (match) return t(key, Object.fromEntries(names.map((name, index) => [name, translated(match[index + 1], section)])));
  }
  return text;
}

if (import.meta.vitest) {
  const { describe, test, expect, afterEach } = import.meta.vitest;
  const others = locales.map(({ tag }) => tag).filter((tag) => tag !== fallback);
  const english = Object.fromEntries(leaves(bundles[fallback], ""));
  const placeholders = (text: string) => [...text.matchAll(/\{\w+\}/g)].map(String).sort();

  afterEach(() => void (i18n.locale = fallback));

  describe("the interface translations", () => {
    test.each(others)("%s has exactly the English keys and keeps every placeholder", (tag) => {
      const theirs = Object.fromEntries(leaves(bundles[tag] ?? {}, ""));
      expect(Object.keys(theirs).sort()).toEqual(Object.keys(english).sort());
      for (const [key, text] of Object.entries(theirs)) expect([key, placeholders(text)]).toEqual([key, placeholders(english[key])]);
    });
  });

  test("the language of the computer is used when it is one znote speaks, and English otherwise", () => {
    expect(resolveLocale(null, ["sw-KE", "de-AT", "fr"])).toBe("de");
    expect(resolveLocale(null, ["nl-BE", "fr"])).toBe("nl");
    expect(resolveLocale(null, ["no"])).toBe("nb");
    expect([resolveLocale(null, ["zh-Hant-HK"]), resolveLocale(null, ["zh-TW"]), resolveLocale(null, ["zh"]), resolveLocale(null, ["zh-SG"])]).toEqual(["zh-TW", "zh-TW", "zh-CN", "zh-CN"]);
    expect(resolveLocale(null, ["sw"])).toBe("en");
    expect(resolveLocale("he", ["fr"])).toBe("he");
    expect(resolveLocale("xx", ["fr"])).toBe("en");
  });

  test("a string is filled in, counted, and falls back to its key", () => {
    expect(t("workspace.menu.moveTo", { notebook: "Notes" })).toBe("Move to Notes");
    expect([t("assistant.summary.read", { count: 1 }), t("assistant.summary.read", { count: 3 })]).toEqual(["read 1 page", "read 3 pages"]);
    expect(t("no.such.key")).toBe("no.such.key");
    i18n.locale = "de";
    expect(t("common.cancel")).toBe("Abbrechen");
  });

  test("an English message from the backend is said in the user's language, inside and out", () => {
    const message = "Could not save “Work/Notes/A”: Access is denied.";
    expect(translated(message)).toBe(message);
    i18n.locale = "de";
    expect(translated(message)).toBe(t("backend.workspace.notSaved", { path: "Work/Notes/A", error: "Access is denied." }));
    expect(translated(message)).not.toBe(message);
    expect(translated("This chat could not be saved: it has grown too long. Start a new chat.")).toBe(t("backend.chat.notSaved", { error: t("backend.chat.tooLong") }));
    expect(translated("Edited Work/Notes/A", "assistant.note")).toBe(t("assistant.note.edited", { page: "Work/Notes/A" }));
    expect(translated("typst: unknown variable")).toBe("typst: unknown variable");
  });

  test("every backend message znote translates is still one the backend can send", async () => {
    const { readFileSync, readdirSync } = await import("node:fs");
    const sources = (readdirSync("src-tauri/src", { recursive: true }) as string[]).filter((file) => file.endsWith(".rs")).map((file) => readFileSync(`src-tauri/src/${file}`, "utf8"));
    const literals = sources.flatMap((source) => [...source.matchAll(/"((?:[^"\\\n]|\\.)*)"/g)].map((match) => match[1].replaceAll("\\n", "\n")));
    const shapes = literals
      .map((literal) => literal.split(/\{[^{}]*\}/))
      .filter((pieces) => pieces.join("").length >= 8)
      .map((pieces) => new RegExp(`^${pieces.map(escaped).join("[\\s\\S]+")}$`));
    const gone = Object.entries(english).filter(([key, text]) => key.startsWith("backend.") && !shapes.some((shape) => shape.test(text.replace(/\{\w+\}/g, "x"))));
    expect(gone).toEqual([]);
  });
}
