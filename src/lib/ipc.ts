import { invoke } from "@tauri-apps/api/core";

export const appVersion = () => invoke<string>("app_version");
