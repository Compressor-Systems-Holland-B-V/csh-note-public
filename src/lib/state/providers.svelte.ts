import * as ipc from "$lib/ipc";
import type { AgentStatus } from "$lib/ipc";
import { curated, offeredModels, providers } from "$lib/assistant";
import { messageOf } from "$lib/utils";
import { SettingsState, settings } from "./settings.svelte";
import { t } from "$lib/i18n.svelte";

type Backend = Pick<
  typeof ipc,
  "agentStatus" | "agentInstall" | "agentConnect" | "agentCode" | "agentDisconnect" | "agentCancel" | "providerConnect" | "providerModels"
>;

export class Connections {
  status = $state<Record<string, AgentStatus>>({});
  models = $state<Record<string, string[]>>({});
  checked = $state(false);
  stage = $state<"installing" | "connecting" | null>(null);
  link = $state<string | null>(null);
  failure = $state<string | null>(null);

  private stopped = false;

  constructor(
    private backend: Backend,
    private settings: SettingsState,
  ) {}

  get active() {
    const id = this.settings.values.assistantProvider;
    return id !== null && this.status[id]?.connected ? id : null;
  }

  get ready() {
    return this.active !== null && (this.active === "claude" || this.model(this.active) !== null);
  }

  model(provider: string): string | null {
    return this.settings.values.assistantModels[provider] ?? null;
  }

  connected(provider: string) {
    return this.status[provider]?.connected ?? false;
  }

  offered(provider: string) {
    return offeredModels(provider, this.models[provider] ?? []);
  }

  private async attempt(task: () => Promise<void>) {
    this.failure = null;
    try {
      await task();
    } catch (error) {
      this.failure = messageOf(error);
    }
  }

  check() {
    return this.attempt(async () => {
      const found = await Promise.all(providers.map(async ({ id }) => [id, await this.backend.agentStatus(id)] as const));
      this.status = Object.fromEntries(found);
      this.checked = true;
      const connected = found.find(([, status]) => status.connected)?.[0];
      if (this.active === null && connected !== undefined) this.settings.set("assistantProvider", connected);
      await Promise.all(found.filter(([, status]) => status.connected).map(([id]) => this.listModels(id)));
    });
  }

  private async listModels(provider: string) {
    if (provider !== "claude") this.models[provider] = await this.backend.providerModels(provider);
  }

  use(provider: string) {
    this.settings.set("assistantProvider", provider);
    return this.attempt(() => this.listModels(provider));
  }

  setModel(provider: string, model: string) {
    const { [provider]: _, ...others } = this.settings.values.assistantModels;
    this.settings.set("assistantModels", model.trim() === "" ? others : { ...others, [provider]: model.trim() });
  }

  async signIn(method: string | null = null) {
    this.stopped = false;
    this.link = null;
    await this.attempt(async () => {
      if (!this.status.claude?.installed) {
        this.stage = "installing";
        await this.backend.agentInstall();
      }
      this.stage = "connecting";
      this.status.claude = await this.backend.agentConnect(method, (link) => (this.link = link));
      if (this.status.claude.connected) {
        if (!this.ready) this.settings.set("assistantProvider", "claude");
      } else if (!this.stopped) throw new Error(t("providers.signIn.unfinished"));
    });
    this.stage = null;
    this.link = null;
  }

  sendCode(code: string) {
    return this.attempt(() => this.backend.agentCode(code));
  }

  async connect(provider: string, url: string | null, key: string) {
    this.stage = "connecting";
    await this.attempt(async () => {
      const models = await this.backend.providerConnect(provider, url, key);
      this.models[provider] = models;
      this.status[provider] = await this.backend.agentStatus(provider);
      const offered = this.offered(provider).map(({ id }) => id);
      const chosen = this.model(provider);
      if (chosen !== null && offered.length > 0 && !offered.includes(chosen)) this.setModel(provider, "");
      if (this.model(provider) === null && (offered.length === 1 || provider in curated)) this.setModel(provider, offered[0] ?? "");
      if (!this.ready) this.settings.set("assistantProvider", provider);
    });
    this.stage = null;
  }

  disconnect(provider: string) {
    return this.attempt(async () => {
      this.status[provider] = await this.backend.agentDisconnect(provider);
      const other = providers.find(({ id }) => this.status[id]?.connected)?.id ?? null;
      if (this.settings.values.assistantProvider === provider) this.settings.set("assistantProvider", other);
    });
  }

  stop() {
    this.stopped = true;
    return this.attempt(() => this.backend.agentCancel(null));
  }
}

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
