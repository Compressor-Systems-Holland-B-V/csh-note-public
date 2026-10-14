import * as ipc from "$lib/ipc";
import type { OpenedProject } from "$lib/ipc";
import { messageOf } from "$lib/utils";
import { agent } from "./agent.svelte";
import { Document } from "./document.svelte";
import { SettingsState, loadSettings, merged, settings } from "./settings.svelte";
import type { Settings } from "./settings.svelte";
import { Workspace, nameOf, workspace } from "./workspace.svelte";
import { t, translated } from "$lib/i18n.svelte";

type Backend = Pick<typeof ipc, "projectOpen" | "projectNew" | "projectAsked">;

const maxRecent = 8;
const carried = ["theme", "hiddenTemplates", "hiddenLooks", "lastPage", "pageLevels"] as const;

export const noteOf = (values: Settings): Record<string, unknown> => Object.fromEntries(carried.map((key) => [key, values[key]]));
const refreshDelayMs = 200;

export const titleOf = (file: string) => nameOf(file.replaceAll("\\", "/")).replace(/\.znote$/i, "");

export class Projects {
  file = $state<string | null>(null);
  choosing = $state(false);
  busy = $state(false);
  failure = $state<string | null>(null);
  savedAt = $state<number | null>(null);
  offer = $state<{ file: string; themes: string[] } | null>(null);
  missingFonts = $state<string[]>([]);
  leftOutFonts = $state<string[]>([]);

  constructor(
    private backend: Backend,
    private workspace: Workspace,
    private settings: SettingsState,
  ) {}

  get title() {
    return this.file === null ? null : titleOf(this.file);
  }

  get recent() {
    return this.settings.values.recentProjects.filter((file) => file !== this.file);
  }

  get oldFolder() {
    return this.settings.values.project === null ? this.settings.values.workspace : null;
  }

  private async switch(pick: () => Promise<OpenedProject | null>, fresh: boolean) {
    if (this.busy) return;
    this.busy = true;
    this.failure = null;
    try {
      await this.workspace.flush();
      const opened = await pick();
      if (opened === null) return;
      if (opened.kind === "asks") {
        this.offer = opened;
        return;
      }
      this.adopt(opened.settings);
      await this.workspace.useFolder(opened.folder, fresh);
      this.missingFonts = opened.missingFonts;
      this.leftOutFonts = [];
      this.file = opened.file;
      this.savedAt = null;
      this.settings.set("project", opened.file);
      this.settings.set("recentProjects", [opened.file, ...this.settings.values.recentProjects.filter((file) => file !== opened.file)].slice(0, maxRecent));
      this.settings.set("uiTheme", opened.look);
      this.choosing = false;
    } catch (error) {
      this.failure = messageOf(error);
      this.choosing = true;
    } finally {
      this.busy = false;
    }
  }

  private adopt(sent: unknown) {
    if (typeof sent !== "object" || sent === null) return;
    const sound = merged(sent as Record<string, unknown>, this.settings.values.theme === "dark");
    for (const key of carried) if (key in sent || key === "pageLevels") this.settings.set(key, sound[key] as never);
  }

  open(file: string | null = null) {
    return this.switch(() => this.backend.projectOpen(file, false), false);
  }

  accept() {
    const offer = this.offer;
    this.offer = null;
    return offer === null ? Promise.resolve() : this.switch(() => this.backend.projectOpen(offer.file, true), false);
  }

  decline() {
    this.offer = null;
    this.choosing = this.choosing || this.file === null;
  }

  create(from: string | null = null) {
    return this.switch(() => this.backend.projectNew(from), from === null);
  }

  forget(file: string) {
    this.settings.set("recentProjects", this.settings.values.recentProjects.filter((recent) => recent !== file));
  }

  async start() {
    const last = (await this.backend.projectAsked().catch(() => null)) ?? this.settings.values.project;
    if (last === null) this.choosing = true;
    else await this.open(last);
  }
}

export const projects = new Projects(ipc, workspace, settings);

export async function start() {
  if (!ipc.inDesktopApp()) return;
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let warned = false;
  await workspace.attempt(async () => {
    await loadSettings();
    await Promise.all([
      ipc.onPageChanged((page) => workspace.attempt(() => workspace.reload(page))),
      ipc.onWorkspaceChanged(() => {
        clearTimeout(refreshTimer);
        refreshTimer = setTimeout(() => workspace.attempt(() => workspace.refresh()), refreshDelayMs);
      }),
      ipc.onProjectSaved(() => {
        projects.savedAt = Date.now();
        projects.failure = null;
      }),
      ipc.onProjectAsked((file) => {
        if (!agent.running && file !== projects.file) projects.open(file);
      }),
      ipc.onProjectSaveFailed((message) => (projects.failure = translated(message))),
      ipc.onProjectFontsLeftOut((families) => (projects.leftOutFonts = [...new Set([...projects.leftOutFonts, ...families])])),
      ipc.onClosing(async (keepOpen) => {
        try {
          await workspace.flush();
        } catch (error) {
          if (warned) return;
          warned = true;
          keepOpen();
          workspace.failure = t("workspace.unsavedOnClose", { reason: messageOf(error) });
        }
      }),
    ]);
    await projects.start();
  });
}

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;

  function setup(stored: Partial<SettingsState["values"]> = {}) {
    const disk: Record<string, string> = {};
    const order: string[] = [];
    const files = {
      openWorkspace: vi.fn(async (folder: string) => {
        order.push(`use ${folder}`);
        return [];
      }),
      listTree: vi.fn(async () => (Object.keys(disk).length > 0 ? [{ name: "Getting started", notebooks: [{ name: "Notes", pages: ["Welcome"] }] }] : [])),
      createEntry: vi.fn(async () => {}),
      moveEntry: vi.fn(async () => {}),
      deleteEntry: vi.fn(async () => {}),
      readPage: vi.fn(async (path: string) => disk[path] ?? ""),
      writePage: vi.fn(async (path: string, text: string) => {
        order.push(`write ${path}`);
        disk[path] = text;
      }),
    };
    const chosen = new SettingsState(() => Promise.resolve());
    Object.assign(chosen.values, stored);
    const opened = (file: string): OpenedProject => ({
      kind: "opened",
      file,
      folder: `/data/projects/${titleOf(file)}/files`,
      look: file.includes("Acme") ? "acme" : null,
      settings: file.includes("Acme") ? { theme: "dark", hiddenLooks: ["plain", 7], fontSize: 30, lastPage: "Work/Notes/A" } : null,
      missingFonts: file.includes("Acme") ? ["Fancy"] : [],
    });
    const asked = { file: null as string | null };
    const backend = {
      projectOpen: vi.fn(async (file: string | null, accept: boolean): Promise<OpenedProject | null> => {
        if (file?.includes("Themed") && !accept) return { kind: "asks", file, themes: ["House style 1.0.0"] };
        order.push(`open ${file}`);
        if (file?.includes("Missing")) throw new Error("“Missing.znote” could not be found.");
        return file === null ? null : opened(file);
      }),
      projectAsked: vi.fn(async () => asked.file),
      projectNew: vi.fn(async (_from: string | null) => opened("C:\\Users\\me\\Documents\\My notes.znote")),
    };
    const space = new Workspace(files, new Document(async () => {}), chosen);
    return { projects: new Projects(backend, space, chosen), backend, chosen, space, files, order, disk, asked };
  }

  test("the last znote file is opened again at start, and without one the user is asked", async () => {
    const first = setup();
    await first.projects.start();
    expect(first.projects.choosing).toBe(true);
    expect(first.backend.projectOpen).not.toHaveBeenCalled();

    const { projects, chosen, order } = setup({ project: "/home/me/Acme plan.znote", uiTheme: "paper" });
    await projects.start();
    expect(order).toEqual(["open /home/me/Acme plan.znote", "use /data/projects/Acme plan/files"]);
    expect(projects.title).toBe("Acme plan");
    expect(projects.choosing).toBe(false);
    expect(chosen.values.uiTheme).toBe("acme");
    expect(chosen.values.recentProjects).toEqual(["/home/me/Acme plan.znote"]);
    expect(chosen.values).toMatchObject({ theme: "dark", hiddenLooks: ["plain"], fontSize: 14, hiddenTemplates: [] });
    expect(projects.missingFonts).toEqual(["Fancy"]);
    expect(noteOf(chosen.values)).toEqual({ theme: "dark", hiddenTemplates: [], hiddenLooks: ["plain"], lastPage: null, pageLevels: {} });
  });

  test("a znote file the app was started with is opened instead of the last one", async () => {
    const { projects, asked, chosen } = setup({ project: "/notes/Last.znote" });
    asked.file = "D:\\Mail\\From Ann.znote";
    await projects.start();
    expect(projects.title).toBe("From Ann");
    expect(chosen.values.project).toBe("D:\\Mail\\From Ann.znote");
    expect(chosen.values.recentProjects).toEqual(["D:\\Mail\\From Ann.znote"]);
  });

  test("themes that come inside a file are installed only after the user agrees", async () => {
    const { projects, backend, order } = setup();
    projects.choosing = true;
    await projects.open("/notes/Themed.znote");
    expect(projects.offer).toEqual({ kind: "asks", file: "/notes/Themed.znote", themes: ["House style 1.0.0"] });
    expect(projects.file).toBe(null);
    expect(order).toEqual([]);
    projects.decline();
    expect(projects.offer).toBe(null);
    expect(projects.choosing).toBe(true);

    await projects.open("/notes/Themed.znote");
    await projects.accept();
    expect(backend.projectOpen).toHaveBeenLastCalledWith("/notes/Themed.znote", true);
    expect(projects.file).toBe("/notes/Themed.znote");
    expect(projects.offer).toBe(null);
    expect(projects.choosing).toBe(false);
  });

  test("a new znote file starts with the welcome page and becomes the one opened next time", async () => {
    const { projects, chosen, disk } = setup();
    projects.choosing = true;
    await projects.create();
    expect(projects.title).toBe("My notes");
    expect(chosen.values.project).toBe("C:\\Users\\me\\Documents\\My notes.znote");
    expect(Object.keys(disk)).toEqual(["Getting started/Notes/Welcome"]);
    expect(projects.choosing).toBe(false);
  });

  test("what was typed is saved before another file is opened, and the list of recent files stays short and current", async () => {
    const { projects, space, order, chosen, disk } = setup();
    await projects.create();
    space.edit("typed just now");
    order.length = 0;
    await projects.open("/notes/B.znote");
    expect(order).toEqual(["write Getting started/Notes/Welcome", "open /notes/B.znote", "use /data/projects/B/files"]);
    expect(disk["Getting started/Notes/Welcome"]).toBe("typed just now");
    for (const name of "CDEFGHIJK") await projects.open(`/notes/${name}.znote`);
    expect(chosen.values.recentProjects).toHaveLength(maxRecent);
    expect(projects.recent).not.toContain("/notes/K.znote");
    expect(projects.recent[0]).toBe("/notes/J.znote");
    projects.forget("/notes/J.znote");
    expect(projects.recent[0]).toBe("/notes/I.znote");
  });

  test("a file that is gone, or a cancelled choice, leaves things as they were and asks again", async () => {
    const { projects, chosen } = setup({ project: "/notes/Missing.znote" });
    await projects.start();
    expect(projects.failure).toContain("could not be found");
    expect(projects.choosing).toBe(true);
    expect(projects.file).toBe(null);
    await projects.open();
    expect(projects.file).toBe(null);
    expect(chosen.values.project).toBe("/notes/Missing.znote");
    expect(projects.oldFolder).toBe(null);
    expect(setup({ workspace: "C:\\old notes" }).projects.oldFolder).toBe("C:\\old notes");
  });
}
