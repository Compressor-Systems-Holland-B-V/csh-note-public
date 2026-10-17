import { bundles, escaped, fallback, i18n, leaves, locales, register, resolveLocale, t, translated, type Dict } from "@zsuite/ui/i18n";
import "@zsuite/assistant/i18n";

export * from "@zsuite/ui/i18n";

register(import.meta.glob<Dict>("./assets/locale/*/*.json", { eager: true, import: "default" }));

export const languageOptions = () => [
  { value: null as string | null, label: t("settings.languageAuto") },
  ...locales.map(({ tag, name }) => ({ value: tag as string | null, label: name })),
];

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
    const folders = ["src-tauri/src", "vendor/zsuite/crates"];
    const sources = folders.flatMap((folder) =>
      (readdirSync(folder, { recursive: true }) as string[]).filter((file) => file.endsWith(".rs")).map((file) => readFileSync(`${folder}/${file}`, "utf8")),
    );
    const literals = sources.flatMap((source) => [...source.matchAll(/"((?:[^"\\\n]|\\.)*)"/g)].map((match) => match[1].replaceAll("\\n", "\n")));
    const shapes = literals
      .map((literal) => literal.split(/\{[^{}]*\}/))
      .filter((pieces) => pieces.join("").length >= 8)
      .map((pieces) => new RegExp(`^${pieces.map(escaped).join("[\\s\\S]+")}$`));
    const gone = Object.entries(english).filter(([key, text]) => key.startsWith("backend.") && !shapes.some((shape) => shape.test(text.replace(/\{\w+\}/g, "x"))));
    expect(gone).toEqual([]);
  });
}
