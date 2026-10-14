import type { ChatPart, Template } from "$lib/ipc";
import { t } from "$lib/i18n.svelte";

export type Action = { do: "create" | "delete"; path: string } | { do: "rename"; from: string; to: string };

export type Reference = { label: string; line: string };

export type Access = "read" | "edit" | "full";

export type Preset = {
  id: string;
  name: string;
  hint: string;
  model: string | null;
  effort: string | null;
  access: Access;
  fast: boolean;
  web: boolean | null;
};

export type Provider = { id: string; needs: "sign-in" | "key" | "server"; sizes?: Record<string, string>; fast?: boolean };

export type Model = { id: string; label: string };

export type Group = { kind: "text"; text: string } | { kind: "steps"; steps: ChatPart[] };

type Templates = Pick<Template, "label" | "kind" | "snippet">[];

const fence = "```znote";
const blocks = /```znote[^\n]*\n?([\s\S]*?)(?:```|$)/g;
const startedFence = /(^|\n)`{1,3}z?n?o?t?e?$/;
const maxThought = 2000;

export const providers: Provider[] = [
  { id: "claude", needs: "sign-in", sizes: { quick: "haiku", balanced: "sonnet", thorough: "opus" }, fast: true },
  { id: "openrouter", needs: "key" },
  { id: "custom", needs: "server" },
];

export const defaultProvider = providers[0].id;

export const providerOf = (id: string) => providers.find((provider) => provider.id === id);

export const listsModels = (id: string) => providerOf(id)?.needs !== "sign-in";

export const curated: Record<string, Model[]> = Object.fromEntries(
  Object.entries(import.meta.glob<Model[]>("./assets/models/*.json", { eager: true, import: "default" })).map(([path, list]) => [path.replace(/^.*\/|\.json$/g, ""), list]),
);

export function offeredModels(provider: string, listed: string[]): Model[] {
  const known = curated[provider];
  if (known === undefined) return listed.map((id) => ({ id, label: id }));
  return listed.length > 0 ? known.filter(({ id }) => listed.includes(id)) : known;
}

export const efforts = ["low", "medium", "high", "xhigh", "max"];

export const accesses: Access[] = ["read", "edit", "full"];

const sizes = [
  { id: "quick", name: "Quick", hint: "Short notes and small fixes. Fastest.", effort: "low" },
  { id: "balanced", name: "Balanced", hint: "Everyday documents. A good default.", effort: "medium" },
  { id: "thorough", name: "Thorough", hint: "Long reports and careful rewrites. Slowest.", effort: "high" },
];

export const defaultPreset = "balanced";

export const providerName = (id: string) => t(`providers.list.${id}`);

export const standard = (id: string) => sizes.some((size) => size.id === id);

export const presetName = ({ id, name }: Pick<Preset, "id" | "name">) => (standard(id) ? t(`assistant.size.${id}`) : name);

export const presetHint = ({ id, hint }: Pick<Preset, "id" | "hint">) => (standard(id) ? t(`assistant.size.${id}Hint`) : hint);

export function builtIn(provider: string): Preset[] {
  const models = providerOf(provider)?.sizes;
  return sizes.map(({ id, name, hint, effort }) => ({
    id,
    name,
    hint,
    model: models?.[id] ?? null,
    effort: models === undefined ? null : effort,
    access: "edit",
    fast: false,
    web: null,
  }));
}

export function presetsFor(provider: string, saved: Preset[]) {
  const sized = builtIn(provider).map((preset) => saved.find(({ id }) => id === preset.id) ?? preset);
  return [...sized, ...saved.filter(({ id }) => !standard(id))];
}

export function movePreset(all: Record<string, Preset[]>, from: string, to: string, preset: Preset): Record<string, Preset[]> {
  const without = (provider: string) => (all[provider] ?? []).filter(({ id }) => id !== preset.id);
  return { ...all, ...(standard(preset.id) ? {} : { [from]: without(from) }), [to]: [...without(to), preset] };
}

export function chosenPreset(provider: string, saved: Preset[], id: string | null) {
  const offered = presetsFor(provider, saved);
  return offered.find((preset) => preset.id === id) ?? offered.find((preset) => preset.id === defaultPreset) ?? offered[0];
}

export function storedPresets(value: unknown): Record<string, Preset[]> {
  if (typeof value !== "object" || value === null) return {};
  const text = (field: unknown) => (typeof field === "string" && field !== "" ? field : null);
  return Object.fromEntries(
    Object.entries(value).map(([provider, list]) => [
      provider,
      (Array.isArray(list) ? list : []).flatMap((preset: Record<string, unknown> | null): Preset[] => {
        const id = text(preset?.id);
        const name = text(preset?.name);
        if (!preset || id === null || name === null) return [];
        const access = accesses.find((access) => access === preset.access) ?? "edit";
        return [{ id, name, hint: text(preset.hint) ?? "", model: text(preset.model), effort: text(preset.effort), access, fast: preset.fast === true, web: typeof preset.web === "boolean" ? preset.web : null }];
      }),
    ]),
  );
}

export function describe(tool: string, page: string) {
  if (tool === "Thinking") return t("assistant.activity.thinking");
  if (tool === "Read") return page === "" ? t("assistant.activity.reading") : t("assistant.activity.readingPage", { page });
  if (tool === "Edit" || tool === "Write") return page === "" ? t("assistant.activity.writing") : t("assistant.activity.writingPage", { page });
  return t("assistant.activity.looking");
}

export function visible(text: string) {
  const shown = text.replace(blocks, "");
  const start = shown.search(startedFence);
  const opens = start >= 0 && shown.slice(0, start).split("```").length % 2 === 1;
  return (opens ? shown.slice(0, start) : shown).trimEnd();
}

const withoutExtension = (path: string) => path.replace(/\.typ$/, "");

export function actionsIn(text: string): Action[] {
  const lines = [...text.matchAll(blocks)].flatMap((block) => block[1].split("\n"));
  return lines.flatMap((line): Action[] => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(line);
    } catch {
      return [];
    }
    if (typeof parsed !== "object" || parsed === null) return [];
    const { do: verb, path, from, to } = parsed as Record<string, unknown>;
    if ((verb === "create" || verb === "delete") && typeof path === "string")
      return [{ do: verb, path: withoutExtension(path) }];
    if (verb === "rename" && typeof from === "string" && typeof to === "string")
      return [{ do: verb, from: withoutExtension(from), to: withoutExtension(to) }];
    return [];
  });
}

export function changedLines(before: string, after: string) {
  if (before === after) return null;
  const old = before.split("\n");
  const now = after.split("\n");
  let same = 0;
  while (same < old.length && same < now.length && old[same] === now[same]) same += 1;
  let tail = 0;
  while (tail < old.length - same && tail < now.length - same && old[old.length - 1 - tail] === now[now.length - 1 - tail])
    tail += 1;
  const from = Math.min(same + 1, now.length);
  return { from, to: Math.max(from, now.length - tail) };
}

export function referenceTo(path: string): Reference {
  const kind = ["project", "notebook", "page"][path.split("/").length - 1];
  return { label: path, line: kind === "page" ? `the page ${path}.typ` : `the ${kind} folder ${path}` };
}

export type ReferenceKind = "project" | "notebook" | "page" | "image" | "sheet" | "slides" | "document" | "code" | "file";

const fileKinds: Record<string, ReferenceKind> = Object.fromEntries(
  Object.entries({
    image: ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"],
    sheet: ["xlsx", "ods", "csv", "tsv"],
    slides: ["pptx", "odp"],
    document: ["pdf", "docx", "odt", "md", "txt", "rtf"],
    code: ["typ", "json", "toml", "yaml", "yml", "xml", "html", "css", "js", "ts", "py", "rs", "sh", "bib"],
  }).flatMap(([kind, extensions]) => extensions.map((extension) => [extension, kind as ReferenceKind])),
);

// ponytail: attachments are file names and workspace references are slash paths, so a project
// whose name ends in a file extension shows as that file. Store the kind on the chat part if it matters.
export function referenceKind(label: string): ReferenceKind {
  const depth = label.split("/").length;
  if (depth > 1) return depth === 2 ? "notebook" : "page";
  const extension = /\.([a-z0-9]{1,5})$/i.exec(label)?.[1].toLowerCase();
  if (extension === undefined) return "project";
  return fileKinds[extension] ?? "file";
}

export function promptFor(text: string, page: string | null, references: Reference[]) {
  const open = page === null ? "No page is open in the editor." : `The page open in the editor is ${page}.typ.`;
  const pointed = references.map((reference) => `\n- ${reference.line}`).join("");
  return `${text}\n\n${open}${pointed === "" ? "" : `\nThe user points you to:${pointed}`}`;
}

export function say(parts: ChatPart[], kind: ChatPart["kind"], text: string, page?: string) {
  const last = parts.at(-1);
  if (kind === "note" || last?.kind !== kind) parts.push(page === undefined ? { kind, text } : { kind, text, page });
  else if (kind === "thought" && last.text.length + text.length > maxThought) last.text = `…${(last.text + text).slice(1 - maxThought)}`;
  else last.text += text;
}

export function grouped(parts: ChatPart[]): Group[] {
  const groups: Group[] = [];
  for (const part of parts) {
    const last = groups.at(-1);
    if (part.kind === "text") {
      if (visible(part.text) !== "") groups.push({ kind: "text", text: visible(part.text) });
    } else if (last?.kind === "steps") last.steps.push(part);
    else groups.push({ kind: "steps", steps: [part] });
  }
  return groups;
}

export function summaryOf(steps: ChatPart[]) {
  const count = (word: string) => steps.filter(({ kind, text }) => kind === "note" && text.startsWith(word)).length;
  const [read, edited] = [count("Read"), count("Edited")];
  const other = steps.filter(({ kind }) => kind === "note").length - read - edited;
  const said = [
    read > 0 ? t("assistant.summary.read", { count: read }) : "",
    edited > 0 ? t("assistant.summary.edited", { count: edited }) : "",
    other > 0 ? t("assistant.summary.other", { count: other }) : "",
  ].filter((piece) => piece !== "");
  const summary = said.length > 0 ? said.join(", ") : t("assistant.summary.thought");
  return summary[0].toUpperCase() + summary.slice(1);
}

export function systemPrompt(templates: Templates, access: Access) {
  const listed = templates.map(({ label, kind, snippet }) => `${label} (${kind}):\n${snippet.trimEnd()}`).join("\n\n");
  const changing =
    access === "read"
      ? "In this chat you may only read pages. Do not try to change, create, rename or delete anything; explain what you would change instead."
      : `Change pages with your tools; the preview updates by itself. You can only write .typ pages inside the workspace. To make a new page, write it; missing folders are made for you.

To create an empty project or notebook, to rename or move something, or to delete something, end your reply with a block like this, one JSON object per line, paths without ".typ":
${fence}
{"do":"create","path":"Project/Notebook"}
{"do":"rename","from":"Project/Notebook/Old name","to":"Project/Notebook/New name"}
{"do":"delete","path":"Project/Notebook/Page"}
\`\`\`
znote carries the lines out in order after your reply and tells you if one failed. The user may be asked to confirm a deletion. The user never sees this block, so do not mention it.`;
  return `You are the writing assistant inside znote, a desktop app for writing documents in Typst. The people you help are not programmers.

Your working folder is the user's workspace. It holds projects, a project holds notebooks, and a notebook holds pages. A page is the file Project/Notebook/Page.typ. Nothing goes deeper and nothing else belongs there.

Unless the user says otherwise, a request is about the page open in the editor. Read a page before you change it. In Typst, a path that starts with "/" is resolved from the project folder of the page.

${changing}

A page uses a template when it starts with the lines of that template between "// znote:template <name>" and "// znote:end". Keep such a header when it is there. The templates are:

${listed}

Shape every reply like this, in Markdown, in under 80 words:
1. One sentence that says what you did, or answers the question.
2. Only if you changed something: up to four short bullets, one per change, with the page name in bold.
3. Only if needed: one closing line with a question or something the user should check.
Use bold, bullets and inline code only. No headings, tables, links or images. Do not repeat the Typst source unless the user asks for it.`;
}

if (import.meta.vitest) {
  const { test, expect } = import.meta.vitest;

  test("a reference is shown as the kind of thing it points to", () => {
    expect(["Work", "Work/Notes", "Work/Notes/A", "plan.PDF", "logo.png", "budget.xlsx", "deck.pptx", "main.typ", "data.bin", "Makefile"].map(referenceKind)).toEqual([
      "project",
      "notebook",
      "page",
      "document",
      "image",
      "sheet",
      "slides",
      "code",
      "file",
      "project",
    ]);
  });

  test("every hidden block is read as actions and none of it is shown", () => {
    const reply =
      'I tidied up.\n\n```znote\n{"do":"create","path":"Work/Archive"}\n{"do":"rename","from":"Work/Notes/A.typ","to":"Work/Notes/B"}\nnot json\n{"do":"format","path":"C:"}\n```\nOne more thing.\n```znote\n{"do":"delete","path":"Work/Old"}';
    expect(visible(reply)).toBe("I tidied up.\n\n\nOne more thing.");
    expect(visible("Just text.")).toBe("Just text.");
    expect(actionsIn(reply)).toEqual([
      { do: "create", path: "Work/Archive" },
      { do: "rename", from: "Work/Notes/A", to: "Work/Notes/B" },
      { do: "delete", path: "Work/Old" },
    ]);
    expect(actionsIn("Just text.")).toEqual([]);
  });

  test("a hidden block that is still arriving does not flash on screen, and other code stays", () => {
    for (const arriving of ["Done.\n`", "Done.\n```", "Done.\n```zno", "Done.\n```znote", 'Done.\n```znote\n{"do":"dele'])
      expect(visible(arriving)).toBe("Done.");
    expect(visible("Use `#set` here.\n```typ\n= Title\n```")).toBe("Use `#set` here.\n```typ\n= Title\n```");
  });

  test("the lines the assistant changed are found", () => {
    expect(changedLines("a\nb\nc", "a\nb\nc")).toBe(null);
    expect(changedLines("a\nb\nc", "a\nB\nc")).toEqual({ from: 2, to: 2 });
    expect(changedLines("a\nc", "a\nb1\nb2\nc")).toEqual({ from: 2, to: 3 });
    expect(changedLines("a\nb\nc", "a\nc")).toEqual({ from: 2, to: 2 });
    expect(changedLines("a\nb", "a")).toEqual({ from: 1, to: 1 });
    expect(changedLines("", "new\npage")).toEqual({ from: 1, to: 2 });
  });

  test("the prompt names the open page and what the user pointed to", () => {
    const prompt = promptFor("Tidy this", "Work/Notes/A", [referenceTo("Work/Notes/B"), referenceTo("Work")]);
    expect(prompt).toContain("open in the editor is Work/Notes/A.typ");
    expect(prompt).toContain("- the page Work/Notes/B.typ\n- the project folder Work");
    expect(promptFor("Hi", null, [])).toBe("Hi\n\nNo page is open in the editor.");
  });

  test("a read-only assistant is not told how to change things", () => {
    expect(systemPrompt([], "edit")).toContain(fence);
    expect(systemPrompt([], "read")).not.toContain(fence);
    expect(systemPrompt([], "read")).toContain("only read pages");
  });

  test("the three sizes are always offered, a saved one replaces its standard, and extra ones follow", () => {
    expect(presetsFor("claude", []).map(({ id, model }) => [id, model])).toEqual([["quick", "haiku"], ["balanced", "sonnet"], ["thorough", "opus"]]);
    expect(builtIn("custom").every(({ model, effort }) => model === null && effort === null)).toBe(true);
    const saved: Preset[] = [
      { id: "mine", name: "Slides", hint: "", model: "opus", effort: null, access: "read", fast: true, web: null },
      { id: "quick", name: "Quick", hint: "", model: "sonnet", effort: "low", access: "edit", fast: false, web: null },
    ];
    expect(presetsFor("claude", saved).map(({ id, model }) => [id, model])).toEqual([["quick", "sonnet"], ["balanced", "sonnet"], ["thorough", "opus"], ["mine", "opus"]]);
    expect(chosenPreset("claude", saved, "mine").name).toBe("Slides");
    expect(chosenPreset("claude", saved, "gone").id).toBe("balanced");
    expect(chosenPreset("claude", saved, null).id).toBe("balanced");
  });

  test("a preset given a model of another provider moves there, and a standard one stays where it was", () => {
    const mine: Preset = { id: "mine", name: "Mine", hint: "", model: "opus", effort: null, access: "edit", fast: false, web: null };
    const quick = { ...mine, id: "quick" };
    const other = { ...mine, id: "other" };
    expect(movePreset({ openrouter: [mine, other] }, "openrouter", "claude", mine)).toEqual({ openrouter: [other], claude: [mine] });
    expect(movePreset({ openrouter: [quick], claude: [other] }, "openrouter", "claude", quick)).toEqual({ openrouter: [quick], claude: [other, quick] });
    expect(movePreset({ claude: [{ ...mine, model: null }, other] }, "claude", "claude", mine)).toEqual({ claude: [other, mine] });
  });

  test("a provider with a curated list offers only those models, and only the ones it still has", () => {
    const [first, second] = curated.openrouter;
    expect(curated.openrouter.length).toBeLessThanOrEqual(15);
    expect(offeredModels("openrouter", [])).toEqual(curated.openrouter);
    expect(offeredModels("openrouter", ["other/model", second.id])).toEqual([second]);
    expect(offeredModels("openrouter", [first.id, second.id]).map(({ id }) => id)).toEqual([first.id, second.id]);
    expect(offeredModels("custom", ["llama"])).toEqual([{ id: "llama", label: "llama" }]);
    expect(curated.claude.map(({ id }) => id)).toContain("sonnet");
  });

  test("saved presets that make no sense are dropped", () => {
    const stored = storedPresets({ claude: [{ id: "a", name: "A", access: "everything", fast: "yes", web: "yes", model: 3 }, { id: "b" }, null, 7], custom: [{ id: "c", name: "C", web: false }] });
    expect(stored).toEqual({
      claude: [{ id: "a", name: "A", hint: "", model: null, effort: null, access: "edit", fast: false, web: null }],
      custom: [{ id: "c", name: "C", hint: "", model: null, effort: null, access: "edit", fast: false, web: false }],
    });
    expect(storedPresets("x")).toEqual({});
  });

  test("streamed pieces join up, notes stay apart, and steps are grouped around the text", () => {
    const parts: ChatPart[] = [];
    say(parts, "thought", "The user ");
    say(parts, "thought", "wants eggs.");
    say(parts, "note", "Read Work/Notes/A");
    say(parts, "note", "Edited Work/Notes/A", "Work/Notes/A");
    say(parts, "text", "Added ");
    say(parts, "text", "eggs.\n```znote\n{}");
    say(parts, "note", "Deleted Work/Old");
    expect(parts.map(({ kind, text }) => [kind, text])).toEqual([
      ["thought", "The user wants eggs."],
      ["note", "Read Work/Notes/A"],
      ["note", "Edited Work/Notes/A"],
      ["text", "Added eggs.\n```znote\n{}"],
      ["note", "Deleted Work/Old"],
    ]);
    const groups = grouped(parts);
    expect(groups.map(({ kind }) => kind)).toEqual(["steps", "text", "steps"]);
    expect(groups[1]).toEqual({ kind: "text", text: "Added eggs." });
    expect(summaryOf(parts.slice(0, 3))).toBe("Read 1 page, edited 1 page");
    expect(summaryOf(parts.slice(4))).toBe("1 other step");
    expect(summaryOf(parts.slice(0, 1))).toBe("Thought about it");
  });
}
