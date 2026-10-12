import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
