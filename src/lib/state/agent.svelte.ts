import * as ipc from "$lib/ipc";
import type { ChatMessage, ChatPart } from "$lib/ipc";
import { actionsIn, changedLines, chosenPreset, describe, promptFor, referenceTo, say, standard, systemPrompt } from "$lib/assistant";
import type { Preset, Reference } from "$lib/assistant";
import { messageOf } from "$lib/utils";
import { Chats, chats } from "./chats.svelte";
import { Document, doc } from "./document.svelte";
import { Connections, connections } from "./providers.svelte";
import { SettingsState, settings } from "./settings.svelte";
import { theme } from "./theme.svelte";
import type { TemplateChoice } from "./theme.svelte";
import { Workspace, moved, workspace } from "./workspace.svelte";
import { t, translated } from "$lib/i18n.svelte";

type Backend = Pick<typeof ipc, "agentCancel" | "agentRun" | "agentAttach" | "agentForget" | "createEntry" | "writePage">;

type Connected = Pick<Connections, "active" | "model" | "check" | "connected">;

type Run = { id: string; messages: ChatMessage[]; session: string | null; provider: string; model: string | null; preset: Preset };

type Templates = Pick<TemplateChoice, "label" | "kind" | "snippet">[];

const maxFixes = 2;
const compileWaitMs = 20000;

export class Agent {
  messages = $state<ChatMessage[]>([]);
  references = $state<Reference[]>([]);
  web = $state(false);
  running = $state(false);
  started = $state<number | null>(null);
  failure = $state<string | null>(null);
  deletion = $state<{ path: string; answer: (allowed: boolean) => void } | null>(null);
  pending = $state<Record<string, string>>({});
  reviewing = $state(false);
  choice = $state<{ provider: string; model: string | null } | null>(null);

  chatId = $state<string | null>(null);
  trail = $state<string[]>([]);

  private run = $state.raw<Run | null>(null);
  private touched = $state<string[]>([]);
  private edited: string[] = [];
  private before: { page: string; text: string } | null = null;
  private after = $state<string | null>(null);
  private session: string | null = null;
  private provider: string | null = null;
  private stopped = false;

  constructor(
    private backend: Backend,
    private workspace: Workspace,
    private doc: Document,
    private settings: SettingsState,
    private templates: () => Templates,
    private chats: Chats,
    private connections: Connected,
  ) {}

  get runningChat() {
    return this.run?.id ?? null;
  }

  get here() {
    return this.run !== null && this.run.id === this.chatId;
  }

  get activity() {
    return this.trail.at(-1) ?? null;
  }

  get using() {
    return this.choice !== null && this.connections.connected(this.choice.provider) ? this.choice.provider : this.connections.active;
  }

  get model() {
    const provider = this.using;
    if (provider === null) return null;
    const picked = this.choice?.provider === provider ? this.choice.model : null;
    const preset = this.preset;
    if (!standard(preset.id) && preset.model !== null) return preset.model;
    return picked ?? preset.model ?? this.connections.model(provider);
  }

  get preset() {
    const provider = this.using ?? "claude";
    return chosenPreset(provider, this.settings.values.assistantPresets[provider] ?? [], this.settings.values.assistantPreset);
  }

  get locked() {
    const page = this.workspace.page;
    return this.running && page !== null && this.touched.includes(page);
  }

  get original() {
    const page = this.workspace.page;
    const before = page === null ? undefined : this.pending[page];
    return before === undefined || before === this.doc.text ? null : before;
  }

  get highlight() {
    const before = this.original;
    if (before === null || this.reviewing) return null;
    if (!this.running && this.doc.text !== this.after) return null;
    return changedLines(before, this.doc.text);
  }

  private async attempt(task: () => Promise<void>) {
    this.failure = null;
    try {
      await task();
    } catch (error) {
      this.failure = messageOf(error);
    }
  }

  stop() {
    const id = this.run?.id;
    if (id === undefined) return Promise.resolve();
    this.stopped = true;
    this.deletion?.answer(false);
    return this.attempt(() => this.backend.agentCancel(id));
  }

  newChat() {
    this.messages = [];
    this.references = [];
    this.web = false;
    this.session = null;
    this.provider = null;
    this.choice = null;
    const id = this.chatId;
    const unsaved = id !== null && id !== this.run?.id && !this.chats.list.some((chat) => chat.id === id) ? id : null;
    this.chatId = null;
    return this.attempt(async () => {
      if (unsaved !== null) await this.backend.agentForget(unsaved);
    });
  }

  openChat(id: string) {
    const chat = this.run?.id === id ? this.run : this.chats.list.find((chat) => chat.id === id);
    if (chat === undefined) return;
    this.messages = chat === this.run ? chat.messages : $state.snapshot(chat.messages);
    this.session = chat.session;
    this.provider = chat.provider;
    this.choice = chat.provider === null ? null : { provider: chat.provider, model: null };
    this.chatId = id;
    this.references = [];
    this.web = false;
    this.failure = null;
  }

  private chat() {
    return (this.chatId ??= crypto.randomUUID());
  }

  removeChat(id: string) {
    if (id === this.run?.id) return;
    this.chats.remove(id);
    this.backend.agentForget(id).catch((error: unknown) => (this.failure = messageOf(error)));
    if (id !== this.chatId) return;
    this.messages = [];
    this.session = null;
    this.provider = null;
    this.choice = null;
    this.chatId = null;
  }

  private remember({ id, messages, session, provider }: Run) {
    const title = messages[0]?.parts[0]?.text.trim().slice(0, 60) ?? "";
    const archived = this.chats.list.find((chat) => chat.id === id)?.archived ?? false;
    this.chats.save({ id, title, session, provider, messages: $state.snapshot(messages), archived, updated: Date.now() });
  }

  private doing(activity: string) {
    if (this.activity !== activity) this.trail = [...this.trail.slice(-3), activity];
  }

  private touch(page: string) {
    if (this.touched.includes(page)) return;
    this.touched.push(page);
    if (page !== this.workspace.page) return;
    this.workspace.flush().then(
      () => {
        if (page === this.workspace.page && this.before === null) this.before = { page, text: this.doc.text };
      },
      (error: unknown) => (this.failure = messageOf(error)),
    );
  }

  private record(page: string, before: string | null) {
    if (!this.edited.includes(page)) this.edited.push(page);
    const original = before ?? (this.before?.page === page ? this.before.text : null);
    if (original !== null && !(page in this.pending)) this.pending[page] = original;
  }

  async review(page: string) {
    await this.attempt(() => this.workspace.open(page));
    this.reviewing = this.original !== null;
  }

  settle(page: string, original = this.pending[page]) {
    if (original === undefined) return;
    if (page === this.workspace.page && original === this.doc.text) this.keepAll(page);
    else this.pending[page] = original;
  }

  keepAll(page: string) {
    delete this.pending[page];
    if (page === this.workspace.page) this.reviewing = false;
  }

  undoAll(page: string) {
    const original = this.pending[page];
    if (original === undefined) return Promise.resolve();
    this.keepAll(page);
    return this.attempt(async () => {
      if (page === this.workspace.page) this.workspace.rewrite(() => original);
      else await this.backend.writePage(page, original);
    });
  }

  refer(path: string) {
    if (!this.references.some((reference) => reference.label === path)) this.references.push(referenceTo(path));
  }

  attach() {
    return this.attempt(async () => {
      const files = await this.backend.agentAttach(this.chat());
      this.references.push(...files.map(({ name, path }) => ({ label: name, line: `the attached file ${path}` })));
    });
  }

  async send(text: string) {
    const provider = this.using;
    if (this.running || text.trim() === "" || provider === null) return;
    const references = this.references;
    this.references = [];
    this.running = true;
    this.started = Date.now();
    this.stopped = false;
    const session = this.provider === provider ? this.session : null;
    const run: Run = { id: this.chat(), messages: this.messages, session, provider, model: this.model, preset: this.preset };
    this.run = run;
    this.provider = provider;
    run.messages.push({
      from: "you",
      parts: [{ kind: "text", text }, ...references.map(({ label }): ChatPart => ({ kind: "note", text: label }))],
    });
    this.remember(run);
    await this.attempt(async () => {
      await this.workspace.flush();
      this.before = null;
      this.after = null;
      this.touched = [];
      this.edited = [];
      let prompt: string | null = promptFor(text, this.workspace.page, references);
      for (let fixes = 0; prompt !== null && !this.stopped; fixes += 1) {
        const reply = await this.turn(run, prompt);
        const problems = [...(await this.carryOut(run, reply)), ...(await this.problemsOnPage())];
        prompt =
          problems.length > 0 && fixes < maxFixes
            ? `znote reports these problems after your last reply. Fix them.\n- ${problems.join("\n- ")}`
            : null;
      }
    });
    if (this.failure !== null) this.connections.check();
    await this.shown().catch(() => {});
    this.after = this.doc.text;
    this.running = false;
    this.started = null;
    this.trail = [];
    if (this.chatId === run.id) this.session = run.session;
    this.remember(run);
    this.run = null;
  }

  private say(run: Run, kind: ChatPart["kind"], text: string, page?: string) {
    say(run.messages[run.messages.length - 1].parts, kind, text, page);
  }

  private async turn(run: Run, prompt: string) {
    const outcome = { reply: "", failure: null as string | null };
    run.messages.push({ from: "assistant", parts: [] });
    this.doing(t("assistant.activity.thinking"));
    const { effort, access, fast, web } = run.preset;
    const turn = {
      chat: run.id,
      prompt,
      system: systemPrompt(this.templates(), access),
      session: run.session,
      provider: run.provider,
      model: run.model,
      effort,
      readOnly: access === "read",
      fast: fast && run.provider === "claude",
      web: web ?? this.web,
    };
    await this.backend.agentRun(turn, (event) => {
      if (event.kind === "text") {
        outcome.reply += event.text;
        this.say(run, "text", event.text);
        this.doing(t("assistant.activity.replying"));
      } else if (event.kind === "thought") {
        this.say(run, "thought", event.text);
        this.doing(t("assistant.activity.thinking"));
      } else if (event.kind === "step") {
        this.doing(describe(event.tool, event.page));
        if (event.page !== "" && event.tool !== "Thinking") this.touch(event.page);
      } else if (event.kind === "done") {
        run.session = event.session || run.session;
        outcome.failure = event.failure;
      } else if (event.kind === "edited") {
        this.record(event.page, event.before);
        this.say(run, "note", `Edited ${event.page}`, event.page);
      } else {
        this.say(run, "note", `Read ${event.page}`);
      }
    });
    if (outcome.failure !== null) throw new Error(translated(outcome.failure));
    return outcome.reply;
  }

  private allowed(run: Run, path: string) {
    if (run.preset.access === "full") return Promise.resolve(true);
    this.doing(t("assistant.activity.waiting"));
    return new Promise<boolean>((resolve) => {
      this.deletion = {
        path,
        answer: (allowed) => {
          this.deletion = null;
          resolve(allowed);
        },
      };
    });
  }

  private rekey(change: (page: string) => string | null) {
    const kept = Object.entries(this.pending).flatMap(([page, text]) => {
      const now = change(page);
      return now === null || now === "" ? [] : [[now, text]];
    });
    this.pending = Object.fromEntries(kept);
  }

  private async carryOut(run: Run, reply: string) {
    const problems: string[] = [];
    if (run.preset.access === "read") return problems;
    for (const action of actionsIn(reply)) {
      if (this.stopped) break;
      try {
        this.doing(action.do === "rename" ? t("assistant.activity.renaming", { path: action.from }) : t(action.do === "create" ? "assistant.activity.creating" : "assistant.activity.deleting", { path: action.path }));
        if (action.do === "rename") {
          await this.workspace.move(action.from, action.to);
          this.rekey((page) => moved(page, action.from, action.to));
          this.say(run, "note", `Renamed ${action.from} to ${action.to}`);
        } else if (action.do === "create") {
          await this.backend.createEntry(action.path);
          await this.workspace.refresh();
          this.say(run, "note", `Created ${action.path}`);
        } else if (await this.allowed(run, action.path)) {
          await this.workspace.remove(action.path);
          this.rekey((page) => (moved(page, action.path, "") === page ? page : null));
          this.say(run, "note", `Deleted ${action.path}`);
        } else {
          problems.push(`The user chose to keep “${action.path}”. Do not delete it.`);
        }
      } catch (error) {
        const target = action.do === "rename" ? action.from : action.path;
        problems.push(`Could not ${action.do} “${target}”: ${messageOf(error)}`);
      }
    }
    return problems;
  }

  private async shown() {
    const page = this.workspace.page;
    if (page !== null) await this.workspace.reload(page);
    return page;
  }

  private async problemsOnPage() {
    const page = await this.shown();
    if (page === null || !this.edited.includes(page)) return [];
    this.doing(t("assistant.activity.checking"));
    await Promise.race([this.doc.settled(), new Promise((resolve) => setTimeout(resolve, compileWaitMs))]);
    return this.doc.diagnostics
      .filter(({ severity }) => severity === "error")
      .map(({ from, message }) => `${page}.typ, line ${this.doc.text.slice(0, from).split("\n").length}: ${message}`);
  }
}

export const agent = new Agent(ipc, workspace, doc, settings, () => theme.offered, chats, connections);

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;

  function setup(replies: string[], broken = false) {
    const disk: Record<string, string> = { "Work/Notes/A": "= A\n", "Work/Notes/B": "= B\n" };
    const tree = () => [{ name: "Work", notebooks: [{ name: "Notes", pages: ["A", "B"] }] }];
    const files = {
      openWorkspace: vi.fn(async () => tree()),
      listTree: vi.fn(async () => tree()),
      createEntry: vi.fn(async () => {}),
      moveEntry: vi.fn(async () => {}),
      deleteEntry: vi.fn(async () => {}),
      readPage: vi.fn(async (path: string) => disk[path]),
      writePage: vi.fn(async (path: string, text: string) => {
        disk[path] = text;
      }),
    };
    const document: Document = new Document(async (page, seq) => {
      const diagnostics = broken ? [{ severity: "error" as const, message: "unknown variable", from: 5, to: 6 }] : [];
      queueMicrotask(() => document.receive({ page, seq, pageCount: null, pages: [], diagnostics }));
    });
    const chosen = new SettingsState(() => Promise.resolve());
    const prompts: string[] = [];
    const seen = { locked: false };
    const linked = { active: "claude" as string | null, also: [] as string[], model: () => null as string | null, check: vi.fn(async () => {}), connected: (provider: string): boolean => provider === linked.active || linked.also.includes(provider) };
    const backend = {
      agentCancel: vi.fn(async () => {}),
      agentRun: vi.fn(async (turn: ipc.AgentTurn, handler: (event: ipc.AgentEvent) => void) => {
        prompts.push(turn.prompt);
        const text = replies[prompts.length - 1] ?? "Done.";
        handler({ kind: "step", tool: "Thinking", page: "" });
        if (broken) {
          handler({ kind: "step", tool: "Edit", page: "Work/Notes/A" });
          const before = disk["Work/Notes/A"];
          disk["Work/Notes/A"] = `= A\n#nope ${prompts.length}\n`;
          handler({ kind: "edited", page: "Work/Notes/A", before });
          seen.locked = agent.locked;
        }
        handler({ kind: "text", text });
        handler({ kind: "done", session: "session-1", failure: null });
      }),
      agentAttach: vi.fn(async () => [{ name: "report.pdf", path: "/data/assistant/1-report.pdf" }]),
      agentForget: vi.fn(async () => {}),
      createEntry: files.createEntry,
      writePage: files.writePage,
    };
    const space = new Workspace(files, document, chosen);
    const history = new Chats(() => Promise.resolve());
    const agent: Agent = new Agent(backend, space, document, chosen, () => [], history, linked);
    return { agent, backend, files, prompts, space, chosen, document, seen, history, linked, disk };
  }

  const preset = (access: Preset["access"], model: string | null = null, web: boolean | null = null): Preset => ({ id: "mine", name: "Mine", hint: "", model, effort: "high", access, fast: true, web });

  test("a deletion waits for the user, and keeping the item is told to the assistant", async () => {
    const { agent, files, prompts, backend } = setup(['Removing it.\n```znote\n{"do":"delete","path":"Work/Old"}\n```']);
    const sent = agent.send("Delete the old project");
    await vi.waitFor(() => expect(agent.deletion?.path).toBe("Work/Old"));
    expect(agent.running).toBe(true);
    expect(agent.started).not.toBe(null);
    agent.deletion?.answer(false);
    await sent;
    expect(files.deleteEntry).not.toHaveBeenCalled();
    expect(prompts[1]).toContain("The user chose to keep “Work/Old”");
    expect(backend.agentRun.mock.calls[1][0].session).toBe("session-1");
    expect(agent.running).toBe(false);
  });

  test("the chosen preset decides the model, what the assistant may do, and whether it asks before deleting", async () => {
    const reply = '```znote\n{"do":"delete","path":"Work/Old"}\n{"do":"create","path":"Work/New"}\n```';
    const { agent, files, chosen, backend, linked } = setup([reply, reply]);
    expect(agent.preset.id).toBe("balanced");
    chosen.set("assistantPresets", { claude: [preset("full", "opus")] });
    chosen.set("assistantPreset", "mine");
    await agent.send("Delete the old project");
    expect(files.deleteEntry).toHaveBeenCalledWith("Work/Old");
    expect(files.createEntry).toHaveBeenCalledWith("Work/New");
    expect(agent.messages.at(-1)?.parts.map((part) => part.text)).toContain("Deleted Work/Old");
    expect(backend.agentRun.mock.calls[0][0]).toMatchObject({ provider: "claude", model: "opus", effort: "high", readOnly: false, fast: true, web: false });

    agent.web = true;
    linked.active = "custom";
    linked.model = () => "llama";
    chosen.set("assistantPresets", { custom: [preset("read")] });
    await agent.send("Delete it again");
    expect(files.deleteEntry).toHaveBeenCalledTimes(1);
    expect(backend.agentRun.mock.calls[1][0]).toMatchObject({ provider: "custom", model: "llama", readOnly: true, fast: false, web: true, session: null });
    expect(backend.agentRun.mock.calls[1][0].system).toContain("only read pages");

    linked.active = "claude";
    linked.also = ["custom"];
    agent.choice = { provider: "custom", model: "qwen" };
    await agent.send("Use the other one for this chat");
    expect(backend.agentRun.mock.calls[2][0]).toMatchObject({ provider: "custom", model: "qwen" });
    chosen.set("assistantPresets", { custom: [preset("read", null, false)] });
    await agent.send("Stay offline");
    expect(backend.agentRun.mock.calls[3][0]).toMatchObject({ web: false });
    chosen.set("assistantPresets", { custom: [preset("read", "mistral")] });
    expect(agent.model).toBe("mistral");
    chosen.set("assistantPresets", { custom: [preset("read")] });
    agent.newChat();
    expect([agent.using, agent.model, agent.web]).toEqual(["claude", "sonnet", false]);

    linked.active = null;
    linked.also = [];
    await agent.send("Nobody is listening");
    expect(backend.agentRun).toHaveBeenCalledTimes(4);
  });

  test("Typst errors go back to the assistant a limited number of times, and the change can be undone as a whole", async () => {
    const { agent, prompts, space, document, seen } = setup([], true);
    await space.useFolder("/notes", false);
    await agent.send("Add a list");
    expect(seen.locked).toBe(true);
    expect(agent.locked).toBe(false);
    expect(prompts).toHaveLength(1 + maxFixes);
    expect(prompts[1]).toContain("Work/Notes/A.typ, line 2: unknown variable");
    expect(agent.highlight).toEqual({ from: 2, to: 2 });
    expect(agent.original).toBe("= A\n");
    expect(agent.messages.at(-1)?.parts.find((part) => part.page)).toEqual({ kind: "note", text: "Edited Work/Notes/A", page: "Work/Notes/A" });
    await agent.review("Work/Notes/A");
    expect(agent.reviewing).toBe(true);
    expect(agent.highlight).toBe(null);
    await agent.undoAll("Work/Notes/A");
    expect(document.text).toBe("= A\n");
    expect(agent.original).toBe(null);
    expect(agent.reviewing).toBe(false);
  });

  test("changes wait per page: kept or undone piece by piece on the open page, and undone on a closed one", async () => {
    const { agent, space, backend, disk } = setup(["Done."]);
    await space.useFolder("/notes", false);
    backend.agentRun.mockImplementationOnce(async (_turn, handler) => {
      for (const page of ["Work/Notes/A", "Work/Notes/B"]) {
        const before = disk[page];
        disk[page] = `${before}one\ntwo\n`;
        handler({ kind: "edited", page, before });
        handler({ kind: "edited", page, before: disk[page] });
      }
      handler({ kind: "done", session: "session-1", failure: null });
    });
    await agent.send("Add two lines everywhere");
    expect(agent.pending).toEqual({ "Work/Notes/A": "= A\n", "Work/Notes/B": "= B\n" });

    agent.settle("Work/Notes/A", "= A\none\n");
    expect(agent.original).toBe("= A\none\n");
    space.edit("= A\none\n");
    agent.settle("Work/Notes/A");
    expect(agent.original).toBe(null);
    expect(Object.keys(agent.pending)).toEqual(["Work/Notes/B"]);

    await agent.undoAll("Work/Notes/B");
    expect(disk["Work/Notes/B"]).toBe("= B\n");
    expect(agent.pending).toEqual({});
  });

  test("a page the assistant leaves alone stays unlocked and keeps what the user typed", async () => {
    const { agent, space, document, backend } = setup(["Made it."]);
    await space.useFolder("/notes", false);
    backend.agentRun.mockImplementationOnce(async (_turn, handler) => {
      handler({ kind: "step", tool: "Write", page: "Work/Notes/New" });
      expect(agent.locked).toBe(false);
      expect(agent.activity).toBe("Writing Work/Notes/New…");
      space.edit("= A\nmine\n");
      handler({ kind: "done", session: "session-1", failure: null });
    });
    await agent.send("Make a new page");
    expect(document.text).toBe("= A\nmine\n");
    expect(agent.original).toBe(null);
    expect(agent.highlight).toBe(null);
    expect(agent.activity).toBe(null);
  });

  test("a finished chat is kept, can be reopened, and deleting the open one clears the panel", async () => {
    const { agent, history, backend } = setup(["First answer."]);
    await agent.send("First question");
    const id = agent.chatId;
    expect(history.list.map(({ title, session, provider }) => [title, session, provider])).toEqual([["First question", "session-1", "claude"]]);
    await agent.newChat();
    expect(backend.agentForget).not.toHaveBeenCalled();
    await agent.attach();
    const abandoned = agent.chatId;
    expect(backend.agentAttach).toHaveBeenCalledWith(abandoned);
    await agent.newChat();
    expect(backend.agentForget).toHaveBeenCalledWith(abandoned);
    backend.agentForget.mockClear();
    expect(agent.messages).toEqual([]);
    agent.openChat(id ?? "");
    expect(agent.messages.map(({ from }) => from)).toEqual(["you", "assistant"]);
    await agent.send("Second question");
    expect(history.list).toHaveLength(1);
    expect(history.list[0].messages).toHaveLength(4);
    expect(backend.agentRun.mock.calls[1][0].session).toBe("session-1");
    agent.removeChat(id ?? "");
    expect(backend.agentForget).toHaveBeenCalledWith(id);
    expect(backend.agentRun.mock.calls.map(([turn]) => turn.chat)).toEqual([id, id]);
    expect(history.list).toEqual([]);
    expect(agent.messages).toEqual([]);
    expect(agent.chatId).toBe(null);
  });

  test("another chat can be opened while the assistant works, and the answer lands in the right chat", async () => {
    const { agent, history, backend } = setup(["Earlier answer."]);
    await agent.send("Earlier question");
    const earlier = agent.chatId ?? "";
    await agent.newChat();

    let release = () => {};
    backend.agentRun.mockImplementationOnce(async (_turn, handler) => {
      await new Promise<void>((resolve) => (release = resolve));
      handler({ kind: "thought", text: "Let me " });
      handler({ kind: "thought", text: "think." });
      handler({ kind: "text", text: "Late answer." });
      handler({ kind: "done", session: "session-2", failure: null });
    });
    const sent = agent.send("Slow question");
    const slow = agent.chatId ?? "";
    await vi.waitFor(() => expect(backend.agentRun).toHaveBeenCalledTimes(2));
    expect(history.list.map(({ id }) => id)).toContain(slow);

    agent.openChat(earlier);
    expect(agent.here).toBe(false);
    expect(agent.running).toBe(true);
    expect(agent.messages.map(({ parts }) => parts[0].text)).toEqual(["Earlier question", "Earlier answer."]);
    agent.removeChat(slow);
    expect(history.list.map(({ id }) => id)).toContain(slow);
    await agent.stop();
    expect(backend.agentCancel).toHaveBeenCalledWith(slow);
    release();
    await sent;

    expect(agent.messages).toHaveLength(2);
    const saved = history.list.find(({ id }) => id === slow);
    expect(saved?.session).toBe("session-2");
    expect(saved?.messages.at(-1)?.parts).toEqual([{ kind: "thought", text: "Let me think." }, { kind: "text", text: "Late answer." }]);
    agent.openChat(slow);
    expect(agent.messages.at(-1)?.parts.at(-1)?.text).toBe("Late answer.");
  });

  test("stopping ends the turn without another automatic prompt", async () => {
    const { agent, prompts, backend } = setup(['```znote\n{"do":"delete","path":"Work/Old"}\n```']);
    await agent.stop();
    expect(backend.agentCancel).not.toHaveBeenCalled();
    const sent = agent.send("Delete the old project");
    await vi.waitFor(() => expect(agent.deletion).not.toBe(null));
    await agent.stop();
    await sent;
    expect(backend.agentCancel).toHaveBeenCalledWith(agent.chatId);
    expect(prompts).toHaveLength(1);
    expect(agent.running).toBe(false);
  });

  test("a failed turn shows the reason and checks the connection again", async () => {
    const { agent, backend, linked } = setup([]);
    backend.agentRun.mockImplementationOnce(async (_turn, handler) => handler({ kind: "done", session: "", failure: "The account has no credit left." }));
    await agent.send("Hello");
    expect(agent.failure).toBe("The account has no credit left.");
    expect(linked.check).toHaveBeenCalled();
    expect(agent.running).toBe(false);
  });
}
