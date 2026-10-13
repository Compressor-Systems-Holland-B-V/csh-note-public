import { Channel, convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LazyStore } from "@tauri-apps/plugin-store";

export type Diagnostic = {
  severity: "error" | "warning";
  message: string;
  from: number;
  to: number;
};

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

export const writePage = (path: string, text: string) => invoke<void>("write_page", { path, text });

export const onPageChanged = (handler: (page: string) => void) =>
  listen<string>("page-changed", (event) => handler(event.payload));

export const onWorkspaceChanged = (handler: () => void) => listen("workspace-changed", handler);

export const onClosing = (handler: (keepOpen: () => void) => Promise<void>) =>
  getCurrentWindow().onCloseRequested((event) => handler(() => event.preventDefault()));

export type TemplateKind = "document" | "slides" | "poster";

export type Template = { label: string; kind: TemplateKind; snippet: string };

export type FontFile = { path: string; weight: number; italic: boolean };

export type Theme = {
  id: string;
  name: string;
  version: string;
  description: string;
  category: string;
  tags: string[];
  logo: string | null;
  logoDark: string | null;
  fontFamily: string | null;
  fonts: FontFile[];
  light: Record<string, string>;
  dark: Record<string, string>;
  templates: Template[];
};

export const listThemes = () => invoke<Theme[]>("list_themes");

export const importTheme = () => invoke<Theme | null>("import_theme");

export const storeThemes = () => invoke<Theme[]>("store_themes");

export const installStoreTheme = (id: string) => invoke<Theme>("install_store_theme", { id });

export const removeTheme = (id: string) => invoke<void>("remove_theme", { id });

export const themeFileUrl = (path: string) => convertFileSrc(path);

export const saveSnapshot = () => invoke<string | null>("save_snapshot");

export type OpenedProject =
  | { kind: "opened"; file: string; folder: string; look: string | null; settings: unknown; missingFonts: string[] }
  | { kind: "asks"; file: string; themes: string[] };

export const projectOpen = (file: string | null, accept: boolean) => invoke<OpenedProject | null>("project_open", { file, accept });

export const projectAsked = () => invoke<string | null>("project_asked");

export const onProjectAsked = (handler: (file: string) => void) => listen<string>("project-asked", (event) => handler(event.payload));

export const projectNew = (from: string | null) => invoke<OpenedProject | null>("project_new", { from });

export const projectNote = (look: string | null, settings: Record<string, unknown>) => invoke<void>("project_note", { look, settings });

export const onProjectFontsLeftOut = (handler: (families: string[]) => void) =>
  listen<string[]>("project-fonts-left-out", (event) => handler(event.payload));

export const onProjectSaveFailed = (handler: (message: string) => void) =>
  listen<string>("project-save-failed", (event) => handler(event.payload));

export const onProjectSaved = (handler: () => void) => listen("project-saved", handler);

export type ExportFormat = "pdf" | "pptx";

export type Exported = { path: string; missingFonts: string[] };

export const exportPage = (page: string, text: string, format: ExportFormat) =>
  invoke<Exported | null>("export_page", { page, text, format });

export const openExport = () => invoke<void>("open_export");

export const showExport = () => invoke<void>("show_export");

export type AgentStatus = { installed: boolean; connected: boolean; url: string | null };

export type AgentEvent =
  | { kind: "text"; text: string }
  | { kind: "thought"; text: string }
  | { kind: "step"; tool: string; page: string }
  | { kind: "edited"; page: string; before: string | null }
  | { kind: "read"; page: string }
  | { kind: "done"; session: string; failure: string | null };

export type AgentTurn = {
  chat: string;
  prompt: string;
  system: string;
  session: string | null;
  provider: string;
  model: string | null;
  effort: string | null;
  readOnly: boolean;
  fast: boolean;
  web: boolean;
};

export type Attachment = { name: string; path: string };

export const agentStatus = (provider: string) => invoke<AgentStatus>("agent_status", { provider });

export const agentInstall = () => invoke<void>("agent_install");

export function agentConnect(method: string | null, onLink: (link: string) => void) {
  const link = new Channel<string>();
  link.onmessage = onLink;
  return invoke<AgentStatus>("agent_connect", { method, link });
}

export const agentCode = (code: string) => invoke<void>("agent_code", { code });

export const agentDisconnect = (provider: string) => invoke<AgentStatus>("agent_disconnect", { provider });

export const agentCancel = (key: string | null) => invoke<void>("agent_cancel", { key });

export const providerConnect = (provider: string, url: string | null, key: string) =>
  invoke<string[]>("provider_connect", { provider, url, key });

export const providerModels = (provider: string) => invoke<string[]>("provider_models", { provider });

export function agentRun(turn: AgentTurn, handler: (event: AgentEvent) => void) {
  const events = new Channel<AgentEvent>();
  events.onmessage = handler;
  return invoke<void>("agent_run", { turn, events });
}

export const agentAttach = (chat: string) => invoke<Attachment[]>("agent_attach", { chat });

export const agentForget = (chat: string) => invoke<void>("agent_forget", { chat });

export type ChatPart = { kind: "text" | "note" | "thought"; text: string; page?: string };

export type ChatMessage = { from: "you" | "assistant"; parts: ChatPart[] };

export type StoredChat = {
  id: string;
  title: string;
  session: string | null;
  provider: string | null;
  messages: ChatMessage[];
  archived: boolean;
  updated: number;
};

const oldChatStore = new LazyStore("chats.json");

export const readChats = () => invoke<unknown[]>("chats_list");

export const readOldChats = () => oldChatStore.get<unknown>("chats");

export const forgetOldChats = () => oldChatStore.delete("chats");

export const writeChat = (chat: StoredChat) => invoke<void>("chat_save", { chat });

const settingsStore = new LazyStore("settings.json");

export const readSettings = async () => Object.fromEntries(await settingsStore.entries<unknown>());

export const writeSetting = (key: string, value: unknown) => settingsStore.set(key, value);
