import { defineConfig } from "@playwright/test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const port = Number(process.env.DEMO_PORT ?? 4191);

export const VIEW = { width: 1600, height: 900 };

export default defineConfig({
  testDir: fileURLToPath(new URL(".", import.meta.url)),
  testMatch: /\.spec\.ts$/,
  outputDir: fileURLToPath(new URL("../../.demo/raw", import.meta.url)),
  // the first run compiles the Rust side and downloads the models
  timeout: 3_600_000,
  workers: 1,
  retries: 0,
  reporter: "list",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    colorScheme: "light",
    locale: "en-US",
    actionTimeout: 15_000,
    viewport: VIEW,
    video: { mode: "on", size: VIEW },
    screenshot: "only-on-failure",
    launchOptions: { args: ["--hide-scrollbars"] }
  },
  webServer: {
    command: `pnpm preview --port ${port} --host 127.0.0.1 --strictPort`,
    cwd: root,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: true,
    timeout: 120_000
  }
});
