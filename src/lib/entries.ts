import type { Ask } from "$lib/components/AskDialog.svelte";
import { pickTypstFile } from "$lib/ipc";
import { theme, withTemplate } from "$lib/state/theme.svelte";
import { childOf, nameOf, parentOf, workspace } from "$lib/state/workspace.svelte";
import { t } from "$lib/i18n.svelte";

export type EntryKind = "project" | "notebook" | "page";

export const askNew = (kind: EntryKind, parent: string): Ask => ({
  title: t(`workspace.new.${kind}`),
  value: "",
  action: t("common.create"),
  run: (name) => workspace.create(childOf(parent, name)),
});

export function askNewPage(notebook: string): Ask {
  const templates = theme.offered;
  return {
    title: t("workspace.new.page"),
    value: "",
    action: t("common.create"),
    options: [t("workspace.noTemplate"), ...templates.map((template) => template.label)],
    run: async (name, option) => {
      await workspace.create(childOf(notebook, name));
      if (option > 0) workspace.rewrite((text) => withTemplate(text, templates[option - 1]));
    },
  };
}

export const askRename = (kind: EntryKind, path: string): Ask => ({
  title: t(`workspace.rename.${kind}`),
  value: nameOf(path),
  action: t("common.rename"),
  run: (name) => workspace.move(path, childOf(parentOf(path), name)),
});

export const askRemove = (kind: EntryKind, path: string): Ask => ({
  title: t(`workspace.remove.${kind}`, { name: nameOf(path) }),
  text: t("workspace.remove.text"),
  action: t("common.delete"),
  destructive: true,
  run: () => workspace.remove(path),
});

export const importPage = (notebook: string) =>
  workspace.attempt(async () => {
    const file = await pickTypstFile();
    if (file !== null) await workspace.importPage(notebook, file);
  });

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;

  test("a new page starts from the template picked, and a rename stays in its folder", async () => {
    const create = vi.spyOn(workspace, "create").mockResolvedValue();
    const move = vi.spyOn(workspace, "move").mockResolvedValue();
    const rewrite = vi.spyOn(workspace, "rewrite").mockImplementation(() => {});
    const ask = askNewPage("Work/Notes");
    expect(ask.options).toHaveLength(theme.offered.length + 1);
    await ask.run(" Monday ", 0);
    expect(create).toHaveBeenCalledWith("Work/Notes/Monday");
    expect(rewrite).not.toHaveBeenCalled();
    await ask.run("Tuesday", 2);
    const change = rewrite.mock.calls[0][0];
    expect(change("= Tuesday\n")).toBe(withTemplate("= Tuesday\n", theme.offered[1]));

    const rename = askRename("page", "Work/Notes/Monday");
    expect(rename.value).toBe("Monday");
    await rename.run("Friday", 0);
    expect(move).toHaveBeenCalledWith("Work/Notes/Monday", "Work/Notes/Friday");
    await askNew("notebook", "Work").run("Ideas", 0);
    expect(create).toHaveBeenLastCalledWith("Work/Ideas");
    vi.restoreAllMocks();
  });
}
