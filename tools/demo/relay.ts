// Lets Playwright drive the real app on Linux. The Tauri window is WebKitGTK,
// which Playwright cannot attach to, so the app runs with a hidden window that
// loads a small relay page instead of the interface. The interface itself runs
// in Chromium, where every invoke, event and channel message is passed through
// this broker to the relay page and on to Rust. Nothing in the app changes: the
// window, the Rust side and the IPC are the shipped ones.
import type { Page, Route } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, statSync, writeFileSync } from "node:fs";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { extname } from "node:path";

type Reply = { ok: boolean; value: unknown };

/** Answers a command in place of Rust, for the ones that open a native dialog. */
export type Answer = (args: Record<string, unknown>) => Reply | Promise<Reply>;

// Window calls that would show, move or close the hidden window.
const QUIET = new Set(
  [
    "show",
    "set_focus",
    "close",
    "destroy",
    "start_dragging",
    "set_size",
    "maximize",
    "unmaximize",
    "minimize",
    "toggle_maximize",
    "set_fullscreen",
    "center"
  ].map((name) => `plugin:window|${name}`)
);

// Values cross the broker as JSON, so bytes travel as base64.
const PACK = `
  const toB64 = (bytes) => {
    let text = "";
    for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(text);
  };
  const pack = (value) => {
    if (value instanceof ArrayBuffer) return { __b64: toB64(new Uint8Array(value)) };
    if (ArrayBuffer.isView(value)) return { __b64: toB64(new Uint8Array(value.buffer, value.byteOffset, value.byteLength)) };
    if (value && typeof value === "object") {
      if (typeof value.__TAURI_TO_IPC_KEY__ === "function") return value.__TAURI_TO_IPC_KEY__();
      if (typeof value.toJSON === "function") return value.toJSON();
      if (value instanceof Error) return String(value);
      if (Array.isArray(value)) return value.map(pack);
      return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, pack(v)]));
    }
    return value;
  };
  const unpack = (value) => {
    if (value && typeof value === "object") {
      if (typeof value.__b64 === "string") return Uint8Array.from(atob(value.__b64), (c) => c.charCodeAt(0)).buffer;
      if (Array.isArray(value)) return value.map(unpack);
      return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, unpack(v)]));
    }
    return value;
  };
`;

// The page the hidden Tauri window loads. It runs each invoke it is sent and
// forwards every callback Rust fires at an id Chromium handed out.
const RELAY_PAGE = `<!doctype html><meta charset="utf-8"><title>relay</title><script>
${PACK}
const I = window.__TAURI_INTERNALS__;
let queue = Promise.resolve();
const post = (message) => {
  queue = queue.then(() => fetch("/back", { method: "POST", body: JSON.stringify(message) })).catch(() => {});
};
const forward = (id) => {
  I.callbacks.set(id, (data) => {
    if (data && typeof data === "object" && "end" in data) I.callbacks.delete(id);
    post({ t: "cb", id, data: pack(data) });
  });
};
const hook = (value) => {
  if (typeof value === "string") {
    const channel = /^__CHANNEL__:(\\d+)$/.exec(value);
    if (channel) forward(Number(channel[1]));
  } else if (value && typeof value === "object") {
    Object.values(value).forEach(hook);
  }
};
const events = new EventSource("/events");
events.onmessage = async (event) => {
  const { id, cmd, args, options } = JSON.parse(event.data);
  hook(args);
  if (cmd === "plugin:event|listen") forward(args.handler);
  try {
    const value = await I.invoke(cmd, unpack(args), options);
    post({ t: "res", id, ok: true, value: pack(value) });
  } catch (error) {
    post({ t: "res", id, ok: false, value: pack(error) });
  }
};
events.onopen = () => post({ t: "ready" });
</script>`;

// Runs in Chromium before the app: a stand-in for what Tauri injects into its
// own webview, with invoke sent through the binding below.
function installShim(pack: string): void {
  // eslint-disable-next-line no-new-func
  const codec = new Function(`${pack}; return { pack, unpack };`)() as {
    pack: (value: unknown) => unknown;
    unpack: (value: unknown) => unknown;
  };
  const relay = (window as unknown as { __relayInvoke: (...args: unknown[]) => Promise<Reply> }).__relayInvoke;
  const callbacks = new Map<number, (data: unknown) => void>();
  const register = (callback: (data: unknown) => void, once = false): number => {
    const id = crypto.getRandomValues(new Uint32Array(1))[0];
    callbacks.set(id, (data) => {
      if (once) callbacks.delete(id);
      callback?.(data);
    });
    return id;
  };
  const w = window as unknown as Record<string, unknown>;
  w.isTauri = true;
  w.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" }
    },
    plugins: { path: { sep: "/", delimiter: ":" } },
    callbacks,
    transformCallback: register,
    unregisterCallback: (id: number) => callbacks.delete(id),
    runCallback: (id: number, data: unknown) => callbacks.get(id)?.(codec.unpack(data)),
    convertFileSrc: (path: string, protocol = "asset") =>
      `http://${protocol}.localhost/${encodeURIComponent(path)}`,
    invoke: async (cmd: string, args: unknown = {}, options?: unknown) => {
      const reply = await relay(cmd, codec.pack(args), options ?? null);
      if (!reply.ok) throw codec.unpack(reply.value);
      return codec.unpack(reply.value);
    }
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (_event: string, id: number) => callbacks.delete(id)
  };
}

const TYPES: Record<string, string> = {
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".ttf": "font/ttf",
  ".otf": "font/otf"
};

export class Relay {
  private server: Server;
  private stream: ServerResponse | null = null;
  private waiting = new Map<number, (reply: Reply) => void>();
  private next = 1;
  private page: Page | null = null;
  private delivery = Promise.resolve();
  private connected: () => void = () => undefined;
  private failed: (error: Error) => void = () => undefined;
  private app: ChildProcess | null = null;
  private exited: string | null = null;
  private readonly answers = new Map<string, Answer>();
  /** Every command the interface sent, in order, for checking what ran. */
  readonly sent: string[] = [];
  readonly ready: Promise<void>;

  constructor(readonly port: number) {
    this.ready = new Promise((resolve, reject) => {
      this.connected = resolve;
      this.failed = reject;
    });
    this.ready.catch(() => undefined);
    this.server = createServer((request, response) => this.serve(request, response));
  }

  listen(): Promise<void> {
    return new Promise((resolve) => this.server.listen(this.port, "127.0.0.1", resolve));
  }

  /** Answers `cmd` here instead of in Rust until `forget` is called. */
  answer(cmd: string, answer: Answer): void {
    this.answers.set(cmd, answer);
  }

  forget(cmd: string): void {
    this.answers.delete(cmd);
  }

  /** Runs a command in Rust directly, outside the interface. */
  call(cmd: string, args: Record<string, unknown> = {}): Promise<Reply> {
    return this.forward(cmd, args, null);
  }

  /**
   * Starts `pnpm tauri dev` with a hidden window on this broker. `identifier`
   * keeps its data apart from a real install of the app.
   */
  start(root: string, identifier: string, log: string): void {
    const config = {
      identifier,
      build: { devUrl: `http://127.0.0.1:${this.port}`, beforeDevCommand: null },
      app: {
        windows: [{ label: "main", title: "znote relay", width: 800, height: 600, visible: false }]
      }
    };
    const file = `${log}.config.json`;
    writeFileSync(file, JSON.stringify(config));
    const out = `${log}.log`;
    writeFileSync(out, "");
    this.app = spawn("pnpm", ["tauri", "dev", "--no-watch", "--config", file], {
      cwd: root,
      detached: true,
      stdio: ["ignore", "pipe", "pipe"]
    });
    const append = (chunk: Buffer) => writeFileSync(out, chunk, { flag: "a" });
    this.app.stdout?.on("data", append);
    this.app.stderr?.on("data", append);
    this.app.on("exit", (code, signal) => {
      this.exited = `the app exited (${signal ?? code}), see ${out}`;
      console.log(`==> ${this.exited}`);
      this.failed(new Error(this.exited));
      for (const resolve of this.waiting.values()) resolve({ ok: false, value: this.exited });
      this.waiting.clear();
    });
  }

  /** Points `page` at this relay. Call before the first navigation. */
  async attach(page: Page): Promise<void> {
    this.page = page;
    await page.exposeBinding("__relayInvoke", (_source, cmd: string, args: Record<string, unknown>, options) =>
      this.invoke(cmd, args, options)
    );
    await page.addInitScript(installShim, PACK);
    await page.route(/^http:\/\/asset\.localhost\//, (route) => this.asset(route));
  }

  async stop(): Promise<void> {
    if (this.app?.pid !== undefined) {
      try {
        process.kill(-this.app.pid, "SIGTERM");
      } catch {
        // already gone
      }
    }
    this.stream?.end();
    this.server.closeAllConnections();
    await new Promise((resolve) => this.server.close(resolve));
  }

  private async invoke(cmd: string, args: Record<string, unknown>, options: unknown): Promise<Reply> {
    this.sent.push(cmd);
    if (QUIET.has(cmd)) return { ok: true, value: null };
    const answer = this.answers.get(cmd);
    if (answer !== undefined) return answer(args);
    return this.forward(cmd, args, options);
  }

  private async forward(cmd: string, args: unknown, options: unknown): Promise<Reply> {
    await this.ready;
    if (this.exited !== null) return { ok: false, value: this.exited };
    const id = this.next++;
    const reply = new Promise<Reply>((resolve) => this.waiting.set(id, resolve));
    this.stream?.write(`data: ${JSON.stringify({ id, cmd, args, options })}\n\n`);
    return reply;
  }

  // Serves an app file for the asset protocol, which Chromium has no handler for.
  private async asset(route: Route): Promise<void> {
    const path = decodeURIComponent(new URL(route.request().url()).pathname.slice(1));
    if (!existsSync(path) || !statSync(path).isFile()) return route.fulfill({ status: 404 });
    await route.fulfill({
      path,
      headers: { "content-type": TYPES[extname(path).toLowerCase()] ?? "application/octet-stream" }
    });
  }

  private serve(request: IncomingMessage, response: ServerResponse): void {
    const url = request.url ?? "/";
    if (url.startsWith("/events")) {
      this.stream = response;
      response.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        connection: "keep-alive"
      });
      response.write(": open\n\n");
      return;
    }
    if (url.startsWith("/back") && request.method === "POST") {
      let body = "";
      request.on("data", (chunk) => (body += chunk));
      request.on("end", () => {
        response.end();
        this.receive(JSON.parse(body));
      });
      return;
    }
    // Anything else is the relay page, whatever path the window asks for.
    response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    response.end(RELAY_PAGE);
  }

  private receive(message: { t: string; id: number; ok: boolean; value: unknown; data: unknown }): void {
    if (message.t === "ready") {
      this.connected();
    } else if (message.t === "res") {
      // after the callbacks queued before it, or a reply overtakes the last
      // channel messages of its own command (a download's progress)
      const resolve = this.waiting.get(message.id);
      this.waiting.delete(message.id);
      this.delivery = this.delivery.then(() => resolve?.({ ok: message.ok, value: message.value }));
    } else if (message.t === "cb") {
      const page = this.page;
      if (page === null) return;
      // in order, so channel indices and streamed text arrive as sent
      this.delivery = this.delivery
        .then(() =>
          page.evaluate(
            ([id, data]) =>
              (window as unknown as { __TAURI_INTERNALS__: { runCallback: (i: number, d: unknown) => void } })
                .__TAURI_INTERNALS__.runCallback(id as number, data),
            [message.id, message.data] as const
          )
        )
        .catch(() => undefined);
    }
  }
}
