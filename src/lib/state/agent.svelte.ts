import * as ipc from "$lib/ipc";
import type { ChatMessage, ChatPart, Seen } from "$lib/ipc";
import { actionsIn, changedLines, chosenPreset, defaultProvider, describe, promptFor, providerOf, referenceTo, say, standard, systemPrompt, unsavedTypst } from "$lib/assistant";
import type { Preset, Reference } from "$lib/assistant";
import { messageOf } from "$lib/utils";
import { Chats, chats } from "./chats.svelte";
import { Document, doc } from "./document.svelte";
import { privacy } from "./privacy.svelte";
import { Connections, connections } from "./providers.svelte";
import { SettingsState, settings } from "./settings.svelte";
import { templateOf, theme } from "./theme.svelte";
import type { TemplateChoice } from "./theme.svelte";
import { Workspace, moved, workspace } from "./workspace.svelte";
import { t, translated } from "$lib/i18n.svelte";

type Backend = Pick<typeof ipc, "agentCancel" | "agentRun" | "agentAttach" | "agentFiles" | "agentLevel" | "agentForget" | "agentInScope" | "agentPreview" | "createEntry" | "writePage">;

type Connected = Pick<Connections, "active" | "model" | "check" | "connected">;

type Run = { id: string; messages: ChatMessage[]; session: string | null; provider: string; model: string | null; preset: Preset; knowledge: string | null; hiding: boolean; pages: string[] };

type Templates = TemplateChoice[];

const maxFixes = 2;
const compileWaitMs = 20000;

export class Agent {
  messages = $state<ChatMessage[]>([]);
  references = $state<Reference[]>([]);
  web = $state(false);
  veil = $state(true);
  running = $state(false);
  started = $state<number | null>(null);
  failure = $state<string | null>(null);
  deletion = $state<{ path: string; answer: (allowed: boolean) => void } | null>(null);
  pending = $state<Record<string, string>>({});
  reviewing = $state(false);
  choice = $state<{ provider: string; model: string | null } | null>(null);
  conflicts = $state<Record<string, string>>({});

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
    private veiledWith: () => string | null = () => null,
    private hidingWanted: () => boolean = () => veiledWith() !== null,
    private hidingLocked: () => boolean = () => false,
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

  get hiding() {
    return (this.veil || this.hidingLocked()) && this.hidingWanted();
  }

  get veilLocked() {
    return this.messages.length > 0;
  }

  get otherKnowledge() {
    const kept = this.chats.list.find((chat) => chat.id === this.chatId)?.knowledge ?? null;
    return kept !== null && kept !== this.veiledWith();
  }

  get providerOrDefault() {
    return this.using ?? defaultProvider;
  }

  get preset() {
    const provider = this.providerOrDefault;
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
    this.veil = true;
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
    this.veil = (chat.knowledge ?? null) !== null;
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
    this.veil = true;
    this.session = null;
    this.provider = null;
    this.choice = null;
    this.chatId = null;
  }

  private remember({ id, messages, session, provider, knowledge }: Run) {
    const title = messages[0]?.parts[0]?.text.trim().slice(0, 60) ?? "";
    const archived = this.chats.list.find((chat) => chat.id === id)?.archived ?? false;
    this.chats.save({ id, title, session, provider, messages: $state.snapshot(messages), archived, updated: Date.now(), knowledge });
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
    const theirs = this.conflicts[page];
    await this.attempt(async () => {
      await this.workspace.open(page);
      if (theirs === undefined) return;
      delete this.conflicts[page];
      this.pending[page] = this.doc.text;
      this.workspace.rewrite(() => theirs);
    });
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
      this.references.push(...files.map(({ name, id, path, text, level }) => ({ label: name, line: `the attached file ${id}`, path, text, level })));
    });
  }

  files() {
    const id = this.chatId;
    return id === null ? Promise.resolve([]) : this.backend.agentFiles(id);
  }

  setLevel(path: string, level: number) {
    return this.attempt(async () => {
      await this.backend.agentLevel(this.chat(), path, level);
      for (const reference of this.references) if (reference.path === path) reference.level = level;
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
    const run = this.draft(provider, references);
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
        const { reply, saved } = await this.turn(run, prompt);
        const unsaved = run.preset.access === "read" ? null : unsavedTypst(reply, saved);
        const problems = [...(await this.carryOut(run, reply)), ...(await this.problemsOnPage()), ...(unsaved === null ? [] : [unsaved])];
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

  private draft(provider: string, references: Reference[]): Run {
    const session = this.provider === provider ? this.session : null;
    const pages = references.filter((reference) => reference.path === undefined).map((reference) => reference.label);
    return { id: this.chat(), messages: this.messages, session, provider, model: this.model, preset: this.preset, knowledge: this.hiding ? this.veiledWith() : null, hiding: this.hiding, pages };
  }

  async preview(text: string): Promise<Seen[]> {
    const provider = this.using;
    if (provider === null) return [];
    const run = this.draft(provider, this.references);
    const shown = await this.backend.agentPreview(this.turnOf(run, promptFor(text, this.workspace.page, this.references)));
    return shown.flatMap((event) => (event.kind === "saw" ? [{ label: event.label, text: event.text }] : []));
  }

  private turnOf(run: Run, prompt: string): ipc.AgentTurn {
    const { effort, access, fast, web } = run.preset;
    return {
      chat: run.id,
      prompt,
      system: systemPrompt(this.templates(), access, this.workspace.page === null ? null : (templateOf(this.doc.text, this.templates())?.id ?? null)),
      session: run.session,
      provider: run.provider,
      model: run.model,
      effort,
      readOnly: access === "read",
      fast: fast && providerOf(run.provider)?.fast === true,
      web: web ?? this.web,
      veil: run.hiding,
      pages: run.pages,
      page: this.workspace.page,
    };
  }

  private async turn(run: Run, prompt: string) {
    const outcome = { reply: "", saved: false, failure: null as string | null };
    run.messages.push({ from: "assistant", parts: [] });
    this.doing(t("assistant.activity.thinking"));
    await this.backend.agentRun(this.turnOf(run, prompt), (event) => {
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
        outcome.saved = true;
        this.record(event.page, event.before);
        this.say(run, "note", `Edited ${event.page}`, event.page);
      } else if (event.kind === "veiled") {
        const asked = run.messages.findLast(({ from }) => from === "you");
        if (asked !== undefined) asked.hidden ??= Object.values(event.kinds).reduce((sum, count) => sum + count, 0);
      } else if (event.kind === "restored") {
        (run.messages[run.messages.length - 1].restored ??= []).push(...event.texts);
      } else if (event.kind === "saw") {
        (run.messages[run.messages.length - 1].saw ??= []).push({ label: event.label, text: event.text });
      } else if (event.kind === "conflict") {
        outcome.saved = true;
        this.conflicts[event.page] = event.theirs;
        this.say(run, "note", `Not saved ${event.page}`, event.page);
      } else {
        this.say(run, "note", `Read ${event.page}`);
      }
    });
    if (outcome.failure !== null) throw new Error(translated(outcome.failure));
    return outcome;
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
      const target = action.do === "rename" ? action.from : action.path;
      if (action.do !== "create" && !(await this.backend.agentInScope(run.id, target, this.workspace.page).catch(() => false))) {
        problems.push(`“${target}” is not part of this chat. Ask the user to add it with @.`);
        continue;
      }
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

export const agent = new Agent(
  ipc,
  workspace,
  doc,
  settings,
  () => theme.templates,
  chats,
  connections,
  () => (privacy.hiding ? (privacy.status?.active ?? null) : null),
  () => privacy.hiding,
  () => privacy.required("hideNames"),
);

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;

  function setup(replies: string[], broken = false, knowledge: string | null = null) {
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
      agentInScope: vi.fn(async (_chat: string, path: string) => !path.startsWith("Private")),
      agentPreview: vi.fn(async (turn: ipc.AgentTurn): Promise<ipc.AgentEvent[]> => [
        { kind: "saw", label: "system", text: turn.system },
        { kind: "saw", label: "prompt", text: turn.prompt.replace("Acme", "⟦CUSTOMER_1⟧") },
        { kind: "step", tool: "", page: "" },
      ]),
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
      agentAttach: vi.fn(async () => [{ name: "report.pdf", id: "1.pdf", path: "/data/assistant/1-report.pdf", text: null, level: 1 }]),
      agentFiles: vi.fn(async () => [{ name: "report.pdf", id: "1.pdf", path: "/data/assistant/1-report.pdf", text: null, level: 1 }]),
      agentLevel: vi.fn(async () => {}),
      agentForget: vi.fn(async () => {}),
      createEntry: files.createEntry,
      writePage: files.writePage,
    };
    const space = new Workspace(files, document, chosen);
    const history = new Chats(() => Promise.resolve());
    const agent: Agent = new Agent(backend, space, document, chosen, () => [], history, linked, () => knowledge);
    return { agent, backend, files, prompts, space, chosen, document, seen, history, linked, disk };
  }

  const preset = (access: Preset["access"], model: string | null = null, web: boolean | null = null): Preset => ({ id: "mine", name: "Mine", hint: "", model, effort: "high", access, fast: true, web });

  const chosenFull = (agent: Agent) => {
    agent["settings"].set("assistantPresets", { claude: [preset("full")] });
    agent["settings"].set("assistantPreset", "mine");
  };

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

  test("Typst put in the chat instead of on a page is sent back once", async () => {
    const { agent, prompts } = setup(["Here it is.\n```typ\n= Summary\n```"]);
    await agent.send("Write a summary");
    expect(prompts).toHaveLength(2);
    expect(prompts[1]).toContain("no page was saved");
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
    expect(backend.agentInScope).toHaveBeenCalledWith(agent.chatId, "Work/Old", null);
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

  test("the assistant is told only the pages it may see and an attached file by its number", async () => {
    const { agent, backend } = setup(["Done."]);
    agent.refer("Work/Notes");
    await agent.attach();
    await agent.send("Sum it up");
    const turn = backend.agentRun.mock.calls[0][0];
    expect(turn.pages).toEqual(["Work/Notes"]);
    expect(turn.prompt).toContain("the attached file 1.pdf");
    expect(turn.prompt).not.toContain("/data/assistant");
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
    await agent.setLevel("/data/assistant/1-report.pdf", 3);
    expect(backend.agentLevel).toHaveBeenCalledWith(abandoned, "/data/assistant/1-report.pdf", 3);
    expect(agent.references[0].level).toBe(3);
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

  test("hidden names are counted, restored names and what the assistant saw are kept, and a conflict waits for review", async () => {
    const { agent, backend, space, document, disk, history } = setup([]);
    await space.useFolder("/notes", false);
    backend.agentRun.mockImplementationOnce(async (_turn, handler) => {
      handler({ kind: "veiled", kinds: { CUSTOMER: 2, EMAIL: 1 } });
      handler({ kind: "saw", label: "prompt", text: "Tell ⟦CUSTOMER_1⟧" });
      handler({ kind: "text", text: "Told Acme." });
      handler({ kind: "restored", texts: [{ text: "Acme", kind: "CUSTOMER" }] });
      handler({ kind: "conflict", page: "Work/Notes/A", theirs: "= A\nfrom the assistant\n" });
      handler({ kind: "done", session: "s", failure: null });
    });
    await agent.send("Tell Acme");
    const [asked, answer] = agent.messages;
    expect(asked.hidden).toBe(3);
    expect(answer.restored).toEqual([{ text: "Acme", kind: "CUSTOMER" }]);
    expect(answer.saw).toEqual([{ label: "prompt", text: "Tell ⟦CUSTOMER_1⟧" }]);
    expect(answer.parts.at(-1)).toEqual({ kind: "note", text: "Not saved Work/Notes/A", page: "Work/Notes/A" });
    expect(history.list[0].knowledge).toBe(null);
    expect(disk["Work/Notes/A"]).toBe("= A\n");
    await agent.review("Work/Notes/A");
    expect(document.text).toBe("= A\nfrom the assistant\n");
    expect(agent.original).toBe("= A\n");
    expect(agent.reviewing).toBe(true);
    expect(agent.conflicts).toEqual({});
  });

  test("hiding is chosen per chat before it starts, and an opened chat keeps its choice", async () => {
    const { agent, backend, history } = setup([], false, "file-1");
    expect([agent.hiding, agent.veilLocked]).toEqual([true, false]);
    agent.veil = false;
    await agent.send("Plain chat");
    expect(backend.agentRun.mock.calls[0][0].veil).toBe(false);
    expect(history.list[0].knowledge).toBe(null);
    expect(agent.veilLocked).toBe(true);
    const plain = agent.chatId ?? "";
    agent.newChat();
    expect(agent.hiding).toBe(true);
    await agent.send("Hidden chat");
    expect(backend.agentRun.mock.calls[1][0].veil).toBe(true);
    expect(history.list[0].knowledge).toBe("file-1");
    agent.openChat(plain);
    expect(agent.hiding).toBe(false);
    expect(await agent.files()).toHaveLength(1);
    agent.newChat();
    expect(await agent.files()).toEqual([]);
  });

  test("a failed turn shows the reason and checks the connection again", async () => {
    const { agent, backend, linked } = setup([]);
    backend.agentRun.mockImplementationOnce(async (_turn, handler) => handler({ kind: "done", session: "", failure: "The account has no credit left." }));
    await agent.send("Hello");
    expect(agent.failure).toBe("The account has no credit left.");
    expect(linked.check).toHaveBeenCalled();
    expect(agent.running).toBe(false);
  });

  test("a chat that wants hiding asks for it even before the knowledge file is known, so it is never sent in clear", async () => {
    const { agent, backend } = setup([], false, null);
    const unsure = new Agent(backend, agent["workspace"], agent["doc"], agent["settings"], () => [], new Chats(() => Promise.resolve()), agent["connections"], () => null, () => true);
    expect(unsure.hiding).toBe(true);
    await unsure.send("Tell me about Acme");
    expect(backend.agentRun.mock.calls[0][0].veil).toBe(true);
    const forced = new Agent(backend, agent["workspace"], agent["doc"], agent["settings"], () => [], new Chats(() => Promise.resolve()), agent["connections"], () => "file-1", () => true, () => true);
    forced.veil = false;
    expect(forced.hiding).toBe(true);
  });

  test("the assistant cannot rename or delete what the chat was not given", async () => {
    const { agent, files, prompts } = setup(['```znote\n{"do":"delete","path":"Private/Diary"}\n{"do":"rename","from":"Private/Notes","to":"Work/Notes"}\n```']);
    chosenFull(agent);
    await agent.send("Tidy up");
    expect(files.deleteEntry).not.toHaveBeenCalled();
    expect(files.moveEntry).not.toHaveBeenCalled();
    expect(prompts[1]).toContain("“Private/Diary” is not part of this chat");
    expect(prompts[1]).toContain("“Private/Notes” is not part of this chat");
  });

  test("what a message would send is shown before it is sent, and nothing is sent by looking", async () => {
    const { agent, backend } = setup([], false, "file-1");
    const shown = await agent.preview("Write to Acme");
    expect(shown.map(({ label }) => label)).toEqual(["system", "prompt"]);
    expect(shown[1].text).toContain("Write to ⟦CUSTOMER_1⟧");
    expect(backend.agentPreview.mock.calls[0][0].veil).toBe(true);
    expect(backend.agentRun).not.toHaveBeenCalled();
    expect(agent.messages).toEqual([]);
  });
}
