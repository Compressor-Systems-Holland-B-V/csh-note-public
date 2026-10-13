import * as ipc from "$lib/ipc";
import type { TemplateKind, Theme } from "$lib/ipc";
import { messageOf } from "$lib/utils";
import { doc } from "./document.svelte";
import { settings } from "./settings.svelte";
import { t } from "$lib/i18n.svelte";

export type TemplateChoice = { id: string; label: string; kind: TemplateKind; snippet: string };

const fallbackFonts = "ui-sans-serif, system-ui, sans-serif";

const headerBlock = /^\/\/ znote:template [^\n]*\n[\s\S]*?\/\/ znote:end[^\S\n]*\n?/;

export const builtInTemplates: TemplateChoice[] = [
  {
    id: "znote/Plain document",
    get label() {
      return t("workspace.templates.plain");
    },
    kind: "document",
    snippet: '#set page(paper: "a4", numbering: "1")\n#set par(justify: true)\n#set heading(numbering: "1.1")\n',
  },
  {
    id: "znote/Slides",
    get label() {
      return t("workspace.templates.slides");
    },
    kind: "slides",
    snippet:
      '#import "@preview/touying:0.8.0": *\n#import themes.simple: *\n#show: simple-theme.with(aspect-ratio: "16-9")\n#set text(font: "Arial")\n',
  },
];

export function withTemplate(text: string, template: Pick<TemplateChoice, "id" | "snippet">) {
  const block = `// znote:template ${template.id}\n${template.snippet.trimEnd()}\n// znote:end\n`;
  return headerBlock.test(text) ? text.replace(headerBlock, () => block) : `${block}\n${text}`;
}

export function templateOf(text: string, templates: TemplateChoice[]) {
  const id = /^\/\/ znote:template ([^\n]*)/.exec(text)?.[1].trim();
  return templates.find((template) => template.id === id);
}

export const templateKind = (text: string, templates: TemplateChoice[]) => templateOf(text, templates)?.kind;

export function templatesOf(themes: Theme[]): TemplateChoice[] {
  return [
    ...builtInTemplates,
    ...themes.flatMap(({ id, name, templates }) =>
      templates.map((template) => ({ ...template, id: `${id}/${template.label}`, label: `${name}: ${template.label}` })),
    ),
  ];
}

export type Entry = { theme: Theme; installed: boolean; update: boolean };

export type Kind = "look" | "both" | "templates";

export type Filter = { search: string; category: string | null; kind: Kind | null; installedOnly: boolean };

export function newer(candidate: string | undefined, current: string) {
  if (!candidate) return false;
  const [theirs, ours] = [candidate, current].map((version) => version.split(".").map(Number));
  const at = theirs.findIndex((part, index) => part !== ours[index]);
  return at >= 0 && theirs[at] > ours[at];
}

export function entriesOf(installed: Theme[], store: Theme[]): Entry[] {
  const offered = (id: string) => store.find((theme) => theme.id === id);
  return [
    ...installed.map((theme) => ({ theme, installed: true, update: newer(offered(theme.id)?.version, theme.version) })),
    ...store
      .filter(({ id }) => !installed.some((theme) => theme.id === id))
      .map((theme) => ({ theme, installed: false, update: false })),
  ];
}

export const hasLook = ({ light, dark, fontFamily, logo }: Theme) =>
  Object.keys(light).length + Object.keys(dark).length > 0 || fontFamily !== null || logo !== null;

export function kindOf(theme: Theme): Kind {
  if (theme.templates.length === 0) return "look";
  return hasLook(theme) ? "both" : "templates";
}

export function looksOf(installed: Theme[], hidden: string[]) {
  return [
    { value: null as string | null, label: t("common.default") },
    ...installed.filter((theme) => hasLook(theme) && !hidden.includes(theme.id)).map(({ id, name }) => ({ value: id, label: name })),
  ];
}

export function matches({ theme, installed }: Entry, { search, category, kind, installedOnly }: Filter) {
  const words = [theme.name, theme.description, theme.category, ...theme.tags, ...theme.templates.map(({ label }) => label)]
    .join(" ")
    .toLowerCase();
  return (
    search.toLowerCase().split(/\s+/).every((word) => words.includes(word)) &&
    (category === null || theme.category === category) &&
    (kind === null || kindOf(theme) === kind) &&
    (!installedOnly || installed)
  );
}

export function rootStyle(theme: Theme | undefined, dark: boolean): Record<string, string> {
  if (!theme) return {};
  const style = { ...(dark ? theme.dark : theme.light) };
  if (theme.fontFamily) style["--font-sans"] = `"${theme.fontFamily}", ${fallbackFonts}`;
  return style;
}

class ThemeState {
  installed = $state<Theme[]>([]);
  store = $state<Theme[]>([]);
  installing = $state<string | null>(null);
  failure = $state<string | null>(null);
  storeFailure = $state<string | null>(null);

  active = $derived(this.installed.find((theme) => theme.id === settings.values.uiTheme));
  dark = $derived(settings.values.theme === "dark");
  style = $derived(rootStyle(this.active, this.dark));
  templates = $derived(templatesOf(this.installed));
  offered = $derived(templatesOf(this.installed.filter(({ id }) => !settings.values.hiddenTemplates.includes(id))));
  entries = $derived(entriesOf(this.installed, this.store));
  looks = $derived(looksOf(this.installed, settings.values.hiddenLooks));

  logo = $derived.by(() => {
    const file = (this.dark ? this.active?.logoDark : null) ?? this.active?.logo;
    return file ? ipc.themeFileUrl(file) : null;
  });

  fontFaces = $derived(
    (this.active?.fonts ?? []).map(({ path, weight, italic }) => ({
      family: this.active?.fontFamily ?? "",
      source: `url("${ipc.themeFileUrl(path)}")`,
      weight: String(weight),
      style: italic ? "italic" : "normal",
    })),
  );

  async attempt(task: () => Promise<void>) {
    try {
      await task();
      this.failure = null;
    } catch (error) {
      this.failure = messageOf(error);
    }
  }

  private async changed() {
    this.installed = await ipc.listThemes();
    if (doc.page) doc.request();
  }

  load() {
    return this.attempt(async () => {
      this.installed = await ipc.listThemes();
    });
  }

  import() {
    return this.attempt(async () => {
      const added = await ipc.importTheme();
      if (added === null) return;
      await this.changed();
      settings.set("uiTheme", added.id);
    });
  }

  async browse() {
    if (!ipc.inDesktopApp()) return;
    try {
      this.store = await ipc.storeThemes();
      this.storeFailure = null;
    } catch (error) {
      this.storeFailure = messageOf(error);
    }
  }

  install(id: string) {
    return this.attempt(async () => {
      this.installing = id;
      try {
        await ipc.installStoreTheme(id);
        await this.changed();
      } finally {
        this.installing = null;
      }
    });
  }

  offer(part: "hiddenLooks" | "hiddenTemplates", id: string, on: boolean) {
    const others = settings.values[part].filter((hidden) => hidden !== id);
    settings.set(part, on ? others : [...others, id]);
    if (part === "hiddenLooks" && !on && settings.values.uiTheme === id) settings.set("uiTheme", null);
  }

  remove(id: string) {
    return this.attempt(async () => {
      await ipc.removeTheme(id);
      if (settings.values.uiTheme === id) settings.set("uiTheme", null);
      this.offer("hiddenLooks", id, true);
      this.offer("hiddenTemplates", id, true);
      await this.changed();
    });
  }
}

export const theme = new ThemeState();

export function loadThemes() {
  if (ipc.inDesktopApp()) theme.load();
}

if (import.meta.vitest) {
  const { test, expect } = import.meta.vitest;

  const report = { id: "acme/Report", snippet: '#import "@themes/acme:0.1.0": *\n#show: report\n' };
  const slides = { id: "acme/Slides", snippet: "#show: slides.with(price: [$1 $& $$])\n" };

  test("a template goes on top of a page that has none and leaves the text alone", () => {
    expect(withTemplate("= Title\n", report)).toBe(
      '// znote:template acme/Report\n#import "@themes/acme:0.1.0": *\n#show: report\n// znote:end\n\n= Title\n',
    );
    expect(withTemplate("", report)).toBe(
      '// znote:template acme/Report\n#import "@themes/acme:0.1.0": *\n#show: report\n// znote:end\n\n',
    );
  });

  test("picking another template replaces only the marked block", () => {
    const first = withTemplate("= Title\n\n// znote:end is just text here\n", report);
    const second = withTemplate(first, slides);
    expect(second).toBe(
      "// znote:template acme/Slides\n#show: slides.with(price: [$1 $& $$])\n// znote:end\n\n= Title\n\n// znote:end is just text here\n",
    );
    expect(withTemplate(second, slides)).toBe(second);
  });

  test("a block the user edited by hand is still replaced, and one lower down is not a header", () => {
    const edited = "// znote:template acme/Report\n#show: report.with(title: [Mine])\n\n#let x = 1\n// znote:end\nBody";
    expect(withTemplate(edited, slides)).toBe(
      "// znote:template acme/Slides\n#show: slides.with(price: [$1 $& $$])\n// znote:end\nBody",
    );
    const lower = "Intro\n// znote:template acme/Report\n// znote:end\n";
    expect(withTemplate(lower, report).endsWith(`\n\n${lower}`)).toBe(true);
  });

  test("a page knows the kind of template it uses", () => {
    expect(templateKind(withTemplate("= Title\n", builtInTemplates[1]), builtInTemplates)).toBe("slides");
    expect(templateKind(withTemplate("= Title\n", builtInTemplates[0]), builtInTemplates)).toBe("document");
    expect(templateKind("= Title\n", builtInTemplates)).toBeUndefined();
    expect(templateKind(withTemplate("", slides), builtInTemplates)).toBeUndefined();
  });

  test("an imported theme adds its templates and overrides only what it sets", () => {
    const acme: Theme = {
      id: "acme",
      name: "Acme",
      version: "0.1.0",
      description: "Company look",
      category: "Business",
      tags: ["navy"],
      logo: null,
      logoDark: null,
      fontFamily: "Acme Sans",
      fonts: [],
      light: { "--primary": "#0F172A" },
      dark: { "--primary": "#2563EB" },
      templates: [{ label: "Report", kind: "document", snippet: report.snippet }],
    };
    expect(templatesOf([acme]).at(-1)).toEqual({ ...report, label: "Acme: Report", kind: "document" });
    expect(rootStyle(undefined, false)).toEqual({});
    expect(rootStyle(acme, true)).toEqual({
      "--primary": "#2563EB",
      "--font-sans": '"Acme Sans", ui-sans-serif, system-ui, sans-serif',
    });
    expect(rootStyle({ ...acme, fontFamily: null }, false)).toEqual({ "--primary": "#0F172A" });

    const paper: Theme = { ...acme, id: "paper", name: "Paper", category: "Interface", tags: [], templates: [] };
    const plain: Theme = { ...paper, id: "plain", light: {}, dark: {}, fontFamily: null, templates: acme.templates };
    const entries = entriesOf([acme], [{ ...acme, version: "0.2.0" }, paper, plain]);
    expect(entries.map(({ theme, installed, update }) => [theme.id, installed, update])).toEqual([
      ["acme", true, true],
      ["paper", false, false],
      ["plain", false, false],
    ]);
    expect(newer("0.10.0", "0.9.9")).toBe(true);
    expect(newer("1.0.0", "1.0.0")).toBe(false);
    expect(newer("0.9.0", "1.0.0")).toBe(false);

    const all = { search: "", category: null, kind: null, installedOnly: false };
    const found = (filter: Partial<Filter>) => entries.filter((entry) => matches(entry, { ...all, ...filter })).map(({ theme }) => theme.id);
    expect(found({})).toEqual(["acme", "paper", "plain"]);
    expect(found({ search: "  NAVY report" })).toEqual(["acme"]);
    expect(found({ category: "Interface" })).toEqual(["paper", "plain"]);
    expect(found({ kind: "look" })).toEqual(["paper"]);
    expect(found({ kind: "both" })).toEqual(["acme"]);
    expect(found({ kind: "templates" })).toEqual(["plain"]);
    expect(looksOf([acme, paper, plain], ["paper"])).toEqual([
      { value: null, label: "Default" },
      { value: "acme", label: "Acme" },
    ]);
    expect(found({ installedOnly: true })).toEqual(["acme"]);
  });
}
