import * as ipc from "$lib/ipc";
import type { Part, TemplateKind, Theme } from "$lib/ipc";
import { fontFaces, logoOf, rootStyle } from "@zsuite/ui/look";
import { messageOf } from "$lib/utils";
import { doc } from "./document.svelte";
import { settings } from "./settings.svelte";
import { t } from "$lib/i18n.svelte";

export type TemplateChoice = { id: string; label: string; kind: TemplateKind; snippet: string; outline: string; guide: string; about: string };

const headerBlock = /^\/\/ znote:template [^\n]*\n[\s\S]*?\/\/ znote:end[^\S\n]*\n?/;

export const builtInTemplates: TemplateChoice[] = [
  {
    id: "znote/Plain document",
    get label() {
      return t("workspace.templates.plain");
    },
    kind: "document",
    snippet: '#set page(paper: "a4", numbering: "1")\n#set par(justify: true)\n#set heading(numbering: "1.1")\n',
    outline: "",
    guide: "",
    about: "",
  },
  {
    id: "znote/Slides",
    get label() {
      return t("workspace.templates.slides");
    },
    kind: "slides",
    snippet:
      '#import "@preview/touying:0.8.0": *\n#import themes.simple: *\n#show: simple-theme.with(aspect-ratio: "16-9")\n#set text(font: "Arial")\n',
    outline: "",
    guide: "",
    about: "",
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
    ...themes.flatMap(({ id, name, description, templates, outline, guide }) =>
      templates.map((template) => ({ ...template, id: `${id}/${template.label}`, label: `${name}: ${template.label}`, outline, guide, about: description })),
    ),
  ];
}

export type Entry = { theme: Theme; installed: boolean; update: boolean };

export type Kind = Part;

// A look and a set of templates can share an id, as when a company makes both.
export const keyOf = ({ part, id }: Pick<Theme, "part" | "id">) => `${part}:${id}`;

export type Filter = { search: string; category: string | null; kind: Kind | null; installedOnly: boolean };

export function newer(candidate: string | undefined, current: string) {
  if (!candidate) return false;
  const [theirs, ours] = [candidate, current].map((version) => version.split(".").map(Number));
  const at = theirs.findIndex((part, index) => part !== ours[index]);
  return at >= 0 && theirs[at] > ours[at];
}

export function entriesOf(installed: Theme[], store: Theme[]): Entry[] {
  const offered = (theme: Theme) => store.find((other) => keyOf(other) === keyOf(theme));
  return [
    ...installed.map((theme) => ({ theme, installed: true, update: newer(offered(theme)?.version, theme.version) })),
    ...store
      .filter((theme) => !installed.some((other) => keyOf(other) === keyOf(theme)))
      .map((theme) => ({ theme, installed: false, update: false })),
  ];
}

export const kindOf = (theme: Theme): Kind => theme.part;

export function looksOf(installed: Theme[], hidden: string[]) {
  return [
    { value: null as string | null, label: t("common.default") },
    ...installed.filter((theme) => theme.part === "look" && !hidden.includes(theme.id)).map(({ id, name }) => ({ value: id, label: name })),
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

export { rootStyle };

class ThemeState {
  installed = $state<Theme[]>([]);
  store = $state<Theme[]>([]);
  installing = $state<string | null>(null);
  failure = $state<string | null>(null);
  storeFailure = $state<string | null>(null);

  active = $derived(this.installed.find((theme) => theme.part === "look" && theme.id === settings.values.uiTheme));
  dark = $derived(settings.values.theme === "dark");
  style = $derived(rootStyle(this.active, this.dark));
  templates = $derived(templatesOf(this.installed));
  offered = $derived(templatesOf(this.installed.filter(({ id }) => !settings.values.hiddenTemplates.includes(id))));
  entries = $derived(entriesOf(this.installed, this.store));
  looks = $derived(looksOf(this.installed, settings.values.hiddenLooks));

  logo = $derived.by(() => {
    const file = logoOf(this.active, this.dark);
    return file ? ipc.themeFileUrl(file) : null;
  });

  fontFaces = $derived(fontFaces(this.active, ipc.themeFileUrl));

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
      if (added.part === "look") settings.set("uiTheme", added.id);
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

  install(theme: Theme) {
    return this.attempt(async () => {
      this.installing = keyOf(theme);
      try {
        await ipc.installStoreTheme(theme.id, theme.part);
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

  remove({ id, part }: Theme) {
    return this.attempt(async () => {
      await ipc.removeTheme(id, part);
      if (part === "look") {
        if (settings.values.uiTheme === id) settings.set("uiTheme", null);
        this.offer("hiddenLooks", id, true);
      } else {
        this.offer("hiddenTemplates", id, true);
      }
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
      part: "look",
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
      templates: [],
      outline: "",
      guide: "",
    };
    const acmeTemplates: Theme = {
      ...acme,
      part: "templates",
      light: {},
      dark: {},
      fontFamily: null,
      templates: [{ label: "Report", kind: "document", snippet: report.snippet }],
      outline: "#let report(body)\n",
      guide: "Use `report`.",
    };
    expect(templatesOf([acme, acmeTemplates]).at(-1)).toEqual({ ...report, label: "Acme: Report", kind: "document", outline: "#let report(body)\n", guide: "Use `report`.", about: acme.description });
    expect(rootStyle(undefined, false)).toEqual({});
    expect(rootStyle(acme, true)).toEqual({
      "--primary": "#2563EB",
      "--font-sans": '"Acme Sans", ui-sans-serif, system-ui, sans-serif',
    });
    expect(rootStyle({ ...acme, fontFamily: null }, false)).toEqual({ "--primary": "#0F172A" });

    const paper: Theme = { ...acme, id: "paper", name: "Paper", category: "Interface", tags: [] };
    const plain: Theme = { ...acmeTemplates, id: "plain", category: "Interface", tags: [] };
    const entries = entriesOf([acme], [{ ...acme, version: "0.2.0" }, acmeTemplates, paper, plain]);
    expect(entries.map(({ theme, installed, update }) => [keyOf(theme), installed, update])).toEqual([
      ["look:acme", true, true],
      ["templates:acme", false, false],
      ["look:paper", false, false],
      ["templates:plain", false, false],
    ]);
    expect(newer("0.10.0", "0.9.9")).toBe(true);
    expect(newer("1.0.0", "1.0.0")).toBe(false);
    expect(newer("0.9.0", "1.0.0")).toBe(false);

    const all = { search: "", category: null, kind: null, installedOnly: false };
    const found = (filter: Partial<Filter>) => entries.filter((entry) => matches(entry, { ...all, ...filter })).map(({ theme }) => keyOf(theme));
    expect(found({})).toEqual(["look:acme", "templates:acme", "look:paper", "templates:plain"]);
    expect(found({ search: "  NAVY report" })).toEqual(["templates:acme"]);
    expect(found({ category: "Interface" })).toEqual(["look:paper", "templates:plain"]);
    expect(found({ kind: "look" })).toEqual(["look:acme", "look:paper"]);
    expect(found({ kind: "templates" })).toEqual(["templates:acme", "templates:plain"]);
    expect(looksOf([acme, acmeTemplates, paper, plain], ["paper"])).toEqual([
      { value: null, label: "Default" },
      { value: "acme", label: "Acme" },
    ]);
    expect(found({ installedOnly: true })).toEqual(["look:acme"]);
  });
}
