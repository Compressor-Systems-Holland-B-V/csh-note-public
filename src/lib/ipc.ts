import { convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LazyStore } from "@tauri-apps/plugin-store";
import type { Diagnostic } from "@zsuite/assistant/ipc";
import type { Look } from "@zsuite/ui/look";

export type RenderedPage = { index: number; svg: string; width: number; height: number };

export type Compiled = {
  page: string;
  seq: number;
  pageCount: number | null;
  pages: RenderedPage[];
  diagnostics: Diagnostic[];
};

export const inDesktopApp = isTauri;

export const appVersion = () => invoke<string>("app_version");

export const compile = (page: string, seq: number, text: string, fresh: boolean) =>
  invoke<void>("compile", { page, seq, text, fresh });

export const onCompiled = (handler: (result: Compiled) => void) =>
  listen<Compiled>("compiled", (event) => handler(event.payload));

export const onDownloading = (handler: (packageName: string) => void) =>
  listen<string>("downloading", (event) => handler(event.payload));

export const onCompileFailed = (handler: (message: string) => void) =>
  listen<string>("compile-failed", (event) => handler(event.payload));

export type Notebook = { name: string; pages: string[] };

export type Project = { name: string; notebooks: Notebook[] };

export type PickedFile = { name: string; text: string };

export const pickTypstFile = () => invoke<PickedFile | null>("pick_typst_file");

export const openWorkspace = (folder: string) => invoke<Project[]>("open_workspace", { folder });

export const listTree = () => invoke<Project[]>("list_tree");

export const createEntry = (path: string) => invoke<void>("create_entry", { path });

export const moveEntry = (from: string, to: string) => invoke<void>("move_entry", { from, to });

export const deleteEntry = (path: string) => invoke<void>("delete_entry", { path });

export const readPage = (path: string) => invoke<string>("read_page", { path });
export type Hit = { page: string; line: number; text: string };
export const searchPages = (query: string) => invoke<Hit[]>("search_pages", { query });

export const writePage = (path: string, text: string) => invoke<void>("write_page", { path, text });

export const onPageChanged = (handler: (page: string) => void) =>
  listen<string>("page-changed", (event) => handler(event.payload));

export const onWorkspaceChanged = (handler: () => void) => listen("workspace-changed", handler);

export const onClosing = (handler: (keepOpen: () => void) => Promise<void>) =>
  getCurrentWindow().onCloseRequested((event) => handler(() => event.preventDefault()));

export type TemplateKind = "document" | "slides" | "poster";

export type Template = { label: string; kind: TemplateKind; snippet: string };

export type { FontFile } from "@zsuite/ui/look";

// A look is a .zth file and changes the interface; templates come in a .znx file.
export type Part = "look" | "templates";

export type Theme = Look & {
  part: Part;
  id: string;
  name: string;
  version: string;
  description: string;
  category: string;
  tags: string[];
  templates: Template[];
  // The top-level `#let` lines of the add-on's Typst package, shown to the assistant.
  outline: string;
  // The add-on's guide.md: how to write pages with its templates, shown to the assistant.
  guide: string;
};

export const listThemes = () => invoke<Theme[]>("list_themes");

export const importTheme = () => invoke<Theme | null>("import_theme");

export const storeThemes = () => invoke<Theme[]>("store_themes");

export const installStoreTheme = (id: string, part: Part) => invoke<Theme>("install_store_theme", { id, part });

export const removeTheme = (id: string, part: Part) => invoke<void>("remove_theme", { id, part });

export const themeFileUrl = (path: string) => convertFileSrc(path);

export const saveProject = () => invoke<void>("project_save");
export const saveSnapshot = () => invoke<string | null>("save_snapshot");

export type OpenedProject =
  | { kind: "opened"; file: string; folder: string; look: string | null; settings: unknown; missingFonts: string[] }
  | { kind: "asks"; file: string; themes: string[] }
  | { kind: "locked"; file: string };

export const projectOpen = (file: string | null, accept: boolean) => invoke<OpenedProject | null>("project_open", { file, accept });

export const projectAsked = () => invoke<string | null>("project_asked");

export const onProjectAsked = (handler: (file: string) => void) => listen<string>("project-asked", (event) => handler(event.payload));

export const projectNew = (from: string | null, password: string) => invoke<OpenedProject | null>("project_new", { from, password });

export const projectUnlock = (file: string, password: string, accept: boolean) => invoke<OpenedProject>("project_unlock", { file, password, accept });

export const projectNote = (look: string | null, settings: Record<string, unknown>) => invoke<void>("project_note", { look, settings });

export const onProjectFontsLeftOut = (handler: (families: string[]) => void) =>
  listen<string[]>("project-fonts-left-out", (event) => handler(event.payload));

export const onProjectSaveFailed = (handler: (message: string) => void) =>
  listen<string>("project-save-failed", (event) => handler(event.payload));

export const onProjectSaving = (handler: () => void) => listen("project-saving", handler);
export const onProjectSaved = (handler: () => void) => listen("project-saved", handler);

export type ExportFormat = "pdf" | "pptx" | "docx";

export type Exported = { path: string };

export const exportPage = (page: string, text: string, format: ExportFormat) =>
  invoke<Exported | null>("export_page", { page, text, format });

export const openExport = () => invoke<void>("open_export");

export const showExport = () => invoke<void>("show_export");

export * from "@zsuite/assistant/ipc";

const settingsStore = new LazyStore("settings.json");

export const readSettings = async () => Object.fromEntries(await settingsStore.entries<unknown>());

export const writeSetting = (key: string, value: unknown) => settingsStore.set(key, value);


