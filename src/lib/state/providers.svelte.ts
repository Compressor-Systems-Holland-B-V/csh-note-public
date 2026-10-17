import * as ipc from "$lib/ipc";
import type { AgentStatus } from "$lib/ipc";
import { curated, listsModels, offeredModels, providers } from "$lib/assistant";
import { messageOf } from "$lib/utils";
import { SettingsState, settings } from "./settings.svelte";
import { t } from "$lib/i18n.svelte";
import { Connections, type Backend } from "@zsuite/assistant/state/providers";

export * from "@zsuite/assistant/state/providers";

export const connections = new Connections(ipc, settings);

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;

  function setup(connected: Record<string, boolean> = {}) {
    const chosen = new SettingsState(() => Promise.resolve());
    const linked = { connected: true };
    const status = (provider: string) => ({ installed: true, connected: connected[provider] ?? false, url: null });
    const backend = {
      agentStatus: vi.fn(async (provider: string) => status(provider)),
      agentInstall: vi.fn(async () => {}),
      agentConnect: vi.fn(async (_method: string | null, onLink: (link: string) => void) => {
        onLink("https://claude.com/sign-in");
        return { installed: true, connected: linked.connected, url: null };
      }),
      agentCode: vi.fn(async () => {}),
      agentDisconnect: vi.fn(async (provider: string) => {
        connected[provider] = false;
        return status(provider);
      }),
      agentCancel: vi.fn(async () => {}),
      providerConnect: vi.fn(async (provider: string, _url: string | null, key: string) => {
        if (key === "wrong") throw new Error("The API key was not accepted.");
        connected[provider] = true;
        return provider === "custom" ? ["llama"] : ["a/one", "b/two"];
      }),
      providerModels: vi.fn(async () => ["a/one", "b/two"]),
    };
    return { connections: new Connections(backend, chosen), backend, chosen, linked };
  }

  test("a sign-in that ends without a connection says so, and the link is gone afterwards", async () => {
    const { connections, linked, backend } = setup();
    await connections.check();
    linked.connected = false;
    await connections.signIn("console");
    expect(backend.agentConnect.mock.calls[0][0]).toBe("console");
    expect(connections.failure).toContain("did not finish");
    expect(connections.link).toBe(null);
    expect(connections.stage).toBe(null);
    expect(connections.active).toBe(null);

    linked.connected = true;
    await connections.signIn();
    expect(connections.failure).toBe(null);
    expect(connections.active).toBe("claude");
    expect(connections.ready).toBe(true);
  });

  test("a provider with a key is used once it is connected, and needs a model before it is ready", async () => {
    const { connections, chosen } = setup();
    await connections.connect("openrouter", null, "wrong");
    expect(connections.failure).toBe("The API key was not accepted.");
    expect(connections.active).toBe(null);

    await connections.connect("openrouter", null, "sk-or-1");
    expect(connections.failure).toBe(null);
    expect(connections.active).toBe("openrouter");
    expect(connections.models.openrouter).toEqual(["a/one", "b/two"]);
    expect(connections.ready).toBe(false);
    connections.setModel("openrouter", " b/two ");
    expect(connections.ready).toBe(true);

    await connections.connect("custom", "http://localhost:11434/v1", "");
    expect(connections.active).toBe("openrouter");
    expect(connections.offered("custom")).toEqual([{ id: "llama", label: "llama" }]);
    expect(chosen.values.assistantModels).toEqual({ openrouter: "b/two", custom: "llama" });
  });

  test("at start the saved provider is used, or else one that is still connected; disconnecting moves on", async () => {
    const { connections, chosen } = setup({ claude: true, custom: true });
    chosen.set("assistantProvider", "openrouter");
    await connections.check();
    expect(connections.active).toBe("claude");
    connections.use("custom");
    expect(connections.active).toBe("custom");
    await connections.disconnect("custom");
    expect(connections.active).toBe("claude");
    await connections.disconnect("claude");
    expect(connections.active).toBe(null);
    expect(chosen.values.assistantProvider).toBe(null);
  });
}
