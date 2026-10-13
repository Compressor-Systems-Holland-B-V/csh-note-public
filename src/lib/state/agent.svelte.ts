import * as ipc from "$lib/ipc";
import type { AgentStatus } from "$lib/ipc";
import { messageOf } from "$lib/utils";
import { Chats, chats } from "./chats.svelte";
import { Document, doc } from "./document.svelte";
import { SettingsState, settings } from "./settings.svelte";
import { theme } from "./theme.svelte";
import type { TemplateChoice } from "./theme.svelte";
import { Workspace, workspace } from "./workspace.svelte";

type Backend = Pick<
  typeof ipc,
  | "agentStatus"
  | "agentInstall"
  | "agentConnect"
  | "agentCode"
  | "agentDisconnect"
  | "agentCancel"
  | "agentRun"
  | "agentAttach"
  | "agentForget"
  | "createEntry"
>;

export type Action = { do: "create" | "delete"; path: string } | { do: "rename"; from: string; to: string };

export type Part = { kind: "text" | "note"; text: string };

export type Message = { from: "you" | "assistant"; parts: Part[] };

export type Reference = { label: string; line: string };

type Run = { id: string; messages: Message[]; session: string | null };

type Templates = Pick<TemplateChoice, "label" | "kind" | "snippet">[];

const maxFixes = 2;
const compileWaitMs = 20000;
const fence = "```znote";

export const models = [
  { id: "fable", label: "Fable" },
  { id: "opus", label: "Opus" },
  { id: "sonnet", label: "Sonnet" },
  { id: "haiku", label: "Haiku" },
];

export const efforts = [
  { id: "low", label: "Quick" },
  { id: "medium", label: "Balanced" },
  { id: "high", label: "Thorough" },
  { id: "xhigh", label: "Very thorough" },
  { id: "max", label: "Maximum" },
];

export function describe(tool: string, page: string) {
  if (tool === "Thinking") return "Thinking…";
  if (tool === "Read") return page === "" ? "Reading…" : `Reading ${page}…`;
  if (tool === "Edit" || tool === "Write") return page === "" ? "Writing a page…" : `Writing ${page}…`;
  return "Looking through your pages…";
}

export const visible = (text: string) => text.split(fence)[0].trimEnd();

const withoutExtension = (path: string) => path.replace(/\.typ$/, "");

export function actionsIn(text: string): Action[] {
  const block = text.split(fence)[1]?.split("```")[0] ?? "";
  return block.split("\n").flatMap((line): Action[] => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(line);
    } catch {
      return [];
    }
    if (typeof parsed !== "object" || parsed === null) return [];
    const { do: verb, path, from, to } = parsed as Record<string, unknown>;
    if ((verb === "create" || verb === "delete") && typeof path === "string")
      return [{ do: verb, path: withoutExtension(path) }];
    if (verb === "rename" && typeof from === "string" && typeof to === "string")
      return [{ do: verb, from: withoutExtension(from), to: withoutExtension(to) }];
    return [];
  });
}

export function changedLines(before: string, after: string) {
  if (before === after) return null;
  const old = before.split("\n");
  const now = after.split("\n");
  let same = 0;
  while (same < old.length && same < now.length && old[same] === now[same]) same += 1;
  let tail = 0;
  while (tail < old.length - same && tail < now.length - same && old[old.length - 1 - tail] === now[now.length - 1 - tail])
    tail += 1;
  const from = Math.min(same + 1, now.length);
  return { from, to: Math.max(from, now.length - tail) };
}

export function referenceTo(path: string): Reference {
  const kind = ["project", "notebook", "page"][path.split("/").length - 1];
  return { label: path, line: kind === "page" ? `the page ${path}.typ` : `the ${kind} folder ${path}` };
}

export function promptFor(text: string, page: string | null, references: Reference[]) {
  const open = page === null ? "No page is open in the editor." : `The page open in the editor is ${page}.typ.`;
  const pointed = references.map((reference) => `\n- ${reference.line}`).join("");
  return `${text}\n\n${open}${pointed === "" ? "" : `\nThe user points you to:${pointed}`}`;
}

export function systemPrompt(templates: Templates) {
  const listed = templates.map(({ label, kind, snippet }) => `${label} (${kind}):\n${snippet.trimEnd()}`).join("\n\n");
  return `You are the writing assistant inside znote, a desktop app for writing documents in Typst. The people you help are not programmers.

Your working folder is the user's workspace. It holds projects, a project holds notebooks, and a notebook holds pages. A page is the file Project/Notebook/Page.typ. Nothing goes deeper and nothing else belongs there.

Unless the user says otherwise, a request is about the page open in the editor. Change pages with your file tools; the preview updates by itself. You can only write .typ files inside the workspace. To make a new page, write its file; missing folders are made for you. In Typst, a path that starts with "/" is resolved from the project folder of the page.

To create an empty project or notebook, to rename or move something, or to delete something, end your reply with a block like this, one JSON object per line, paths without ".typ":
${fence}
{"do":"create","path":"Project/Notebook"}
{"do":"rename","from":"Project/Notebook/Old name","to":"Project/Notebook/New name"}
{"do":"delete","path":"Project/Notebook/Page"}
\`\`\`
znote carries the lines out in order after your reply and tells you if one failed. The user may be asked to confirm a deletion. The user never sees this block, so do not mention it.

A page uses a template when it starts with the lines of that template between "// znote:template <name>" and "// znote:end". Keep such a header when it is there. The templates are:

${listed}

Shape every reply like this, in Markdown, in under 80 words:
1. One sentence that says what you did, or answers the question.
2. Only if you changed something: up to four short bullets, one per change, with the page name in bold.
3. Only if needed: one closing line with a question or something the user should check.
Use bold, bullets and inline code only. No headings, tables, links or images. Do not repeat the Typst source unless the user asks for it.`;
}

export class Agent {
  account = $state<AgentStatus | null>(null);
  stage = $state<"installing" | "connecting" | null>(null);
  link = $state<string | null>(null);
  messages = $state<Message[]>([]);
  references = $state<Reference[]>([]);
  running = $state(false);
  failure = $state<string | null>(null);
  deletion = $state<{ path: string; answer: (allowed: boolean) => void } | null>(null);

  chatId = $state<string | null>(null);
  trail = $state<string[]>([]);

  private run = $state.raw<Run | null>(null);
  private touched = $state<string[]>([]);
  private before = $state<{ page: string; text: string } | null>(null);
  private after = $state<string | null>(null);
  private session: string | null = null;
  private stopped = false;

  constructor(
    private backend: Backend,
    private workspace: Workspace,
    private doc: Document,
    private settings: SettingsState,
    private templates: () => Templates,
    private chats: Chats,
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

  get locked() {
    const page = this.workspace.page;
    return this.running && page !== null && this.touched.includes(page);
  }

  get highlight() {
    const before = this.before;
    if (before === null || before.page !== this.workspace.page) return null;
    if (!this.running && this.doc.text !== this.after) return null;
    return changedLines(before.text, this.doc.text);
  }

  get canUndo() {
    const before = this.before;
    return !this.running && before !== null && before.page === this.workspace.page && before.text !== this.doc.text;
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
      this.account = await this.backend.agentStatus();
    });
  }

  async connect(method: string | null = null) {
    this.stopped = false;
    this.link = null;
    await this.attempt(async () => {
      if (!this.account?.installed) {
        this.stage = "installing";
        await this.backend.agentInstall();
      }
      this.stage = "connecting";
      this.account = await this.backend.agentConnect(method, (link) => (this.link = link));
      if (!this.account.connected && !this.stopped) throw new Error("The sign-in did not finish. Press Connect to try again.");
    });
    this.stage = null;
    this.link = null;
  }

  sendCode(code: string) {
    return this.attempt(() => this.backend.agentCode(code));
  }

  disconnect() {
    return this.attempt(async () => {
      this.account = await this.backend.agentDisconnect();
    });
  }

  stop() {
    this.stopped = true;
    this.deletion?.answer(false);
    return this.attempt(() => this.backend.agentCancel());
  }

  newChat() {
    this.messages = [];
    this.references = [];
    this.session = null;
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
    this.chatId = id;
    this.references = [];
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
    this.chatId = null;
  }

  private remember({ id, messages, session }: Run) {
    const title = messages[0]?.parts[0]?.text.trim().slice(0, 60) ?? "";
    const archived = this.chats.list.find((chat) => chat.id === id)?.archived ?? false;
    this.chats.save({ id, title, session, messages: $state.snapshot(messages), archived, updated: Date.now() });
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
        if (page === this.workspace.page) this.before = { page, text: this.doc.text };
      },
      (error: unknown) => (this.failure = messageOf(error)),
    );
  }

  undo() {
    const before = this.before;
    if (!this.canUndo || before === null) return;
    this.before = null;
    this.workspace.rewrite(() => before.text);
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
    if (this.running || text.trim() === "") return;
    const references = this.references;
    this.references = [];
    this.running = true;
    this.stopped = false;
    const run: Run = { id: this.chat(), messages: this.messages, session: this.session };
    this.run = run;
    run.messages.push({
      from: "you",
      parts: [{ kind: "text", text }, ...references.map(({ label }): Part => ({ kind: "note", text: label }))],
    });
    this.remember(run);
    await this.attempt(async () => {
      await this.workspace.flush();
      this.before = null;
      this.after = null;
      this.touched = [];
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
    if (this.failure !== null) this.backend.agentStatus().then((account) => (this.account = account), () => {});
    await this.shown().catch(() => {});
    this.after = this.doc.text;
    this.running = false;
    this.trail = [];
    if (this.chatId === run.id) this.session = run.session;
    this.remember(run);
    this.run = null;
  }

  private say(run: Run, kind: Part["kind"], text: string) {
    const parts = run.messages[run.messages.length - 1].parts;
    const last = parts.at(-1);
    if (kind === "text" && last?.kind === "text") last.text += text;
    else parts.push({ kind, text });
  }

  private async turn(run: Run, prompt: string) {
    const outcome = { reply: "", failure: null as string | null };
    run.messages.push({ from: "assistant", parts: [] });
    this.doing("Thinking…");
    const { assistantModel: model, assistantEffort: effort } = this.settings.values;
    const turn = { chat: run.id, prompt, system: systemPrompt(this.templates()), session: run.session, model, effort };
    await this.backend.agentRun(turn, (event) => {
      if (event.kind === "text") {
        outcome.reply += event.text;
        this.say(run, "text", event.text);
        this.doing("Replying…");
      } else if (event.kind === "step") {
        this.doing(describe(event.tool, event.page));
        if (event.page !== "" && event.tool !== "Thinking") this.touch(event.page);
      } else if (event.kind === "done") {
        run.session = event.session || run.session;
        outcome.failure = event.failure;
      } else {
        this.say(run, "note", `${event.kind === "edited" ? "Edited" : "Read"} ${event.page}`);
      }
    });
    if (outcome.failure !== null) throw new Error(outcome.failure);
    return outcome.reply;
  }

  private allowed(path: string) {
    if (!this.settings.values.confirmDeletes) return Promise.resolve(true);
    this.doing("Waiting for your answer…");
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

  private async carryOut(run: Run, reply: string) {
    const problems: string[] = [];
    for (const action of actionsIn(reply)) {
      if (this.stopped) break;
      try {
        this.doing(action.do === "rename" ? `Renaming ${action.from}…` : action.do === "create" ? `Creating ${action.path}…` : `Deleting ${action.path}…`);
        if (action.do === "rename") {
          await this.workspace.move(action.from, action.to);
          this.say(run, "note", `Renamed ${action.from} to ${action.to}`);
        } else if (action.do === "create") {
          await this.backend.createEntry(action.path);
          await this.workspace.refresh();
          this.say(run, "note", `Created ${action.path}`);
        } else if (await this.allowed(action.path)) {
          await this.workspace.remove(action.path);
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
    if (page === null || page !== this.before?.page || this.doc.text === this.before.text) return [];
    this.doing("Checking the page for problems…");
    await Promise.race([this.doc.settled(), new Promise((resolve) => setTimeout(resolve, compileWaitMs))]);
    return this.doc.diagnostics
      .filter(({ severity }) => severity === "error")
      .map(({ from, message }) => `${page}.typ, line ${this.doc.text.slice(0, from).split("\n").length}: ${message}`);
  }
}

export const agent = new Agent(ipc, workspace, doc, settings, () => theme.templates, chats);

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;

  function setup(replies: string[], broken = false) {
    const disk: Record<string, string> = { "Work/Notes/A": "= A\n" };
    const tree = () => [{ name: "Work", notebooks: [{ name: "Notes", pages: ["A"] }] }];
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
    const linked = { connected: true };
    const seen = { locked: false };
    const backend = {
      agentStatus: vi.fn(async () => ({ installed: true, connected: true })),
      agentInstall: vi.fn(async () => {}),
      agentConnect: vi.fn(async (_method: string | null, onLink: (link: string) => void) => {
        onLink("https://claude.com/sign-in");
        return { installed: true, connected: linked.connected };
      }),
      agentCode: vi.fn(async () => {}),
      agentDisconnect: vi.fn(async () => ({ installed: true, connected: false })),
      agentCancel: vi.fn(async () => {}),
      agentRun: vi.fn(async (turn: ipc.AgentTurn, handler: (event: ipc.AgentEvent) => void) => {
        prompts.push(turn.prompt);
        const text = replies[prompts.length - 1] ?? "Done.";
        handler({ kind: "step", tool: "Thinking", page: "" });
        if (broken) {
          handler({ kind: "step", tool: "Edit", page: "Work/Notes/A" });
          await Promise.resolve();
          disk["Work/Notes/A"] = `= A\n#nope ${prompts.length}\n`;
          seen.locked = agent.locked;
        }
        handler({ kind: "text", text });
        handler({ kind: "done", session: "session-1", failure: null });
      }),
      agentAttach: vi.fn(async () => [{ name: "report.pdf", path: "/data/assistant/1-report.pdf" }]),
      agentForget: vi.fn(async () => {}),
      createEntry: files.createEntry,
    };
    const space = new Workspace(files, document, chosen);
    const history = new Chats(() => Promise.resolve());
    const agent: Agent = new Agent(backend, space, document, chosen, () => [], history);
    return { agent, backend, files, prompts, space, chosen, document, linked, seen, history };
  }

  test("the hidden block is read as actions and never shown", () => {
    const reply =
      'I tidied up.\n\n```znote\n{"do":"create","path":"Work/Archive"}\n{"do":"rename","from":"Work/Notes/A.typ","to":"Work/Notes/B"}\nnot json\n{"do":"format","path":"C:"}\n{"do":"delete","path":"Work/Old"}\n```';
    expect(visible(reply)).toBe("I tidied up.");
    expect(visible("Just text.")).toBe("Just text.");
    expect(actionsIn(reply)).toEqual([
      { do: "create", path: "Work/Archive" },
      { do: "rename", from: "Work/Notes/A", to: "Work/Notes/B" },
      { do: "delete", path: "Work/Old" },
    ]);
    expect(actionsIn("Just text.")).toEqual([]);
  });

  test("the lines the assistant changed are found", () => {
    expect(changedLines("a\nb\nc", "a\nb\nc")).toBe(null);
    expect(changedLines("a\nb\nc", "a\nB\nc")).toEqual({ from: 2, to: 2 });
    expect(changedLines("a\nc", "a\nb1\nb2\nc")).toEqual({ from: 2, to: 3 });
    expect(changedLines("a\nb\nc", "a\nc")).toEqual({ from: 2, to: 2 });
    expect(changedLines("a\nb", "a")).toEqual({ from: 1, to: 1 });
    expect(changedLines("", "new\npage")).toEqual({ from: 1, to: 2 });
  });

  test("the prompt names the open page and what the user pointed to", () => {
    const prompt = promptFor("Tidy this", "Work/Notes/A", [referenceTo("Work/Notes/B"), referenceTo("Work")]);
    expect(prompt).toContain("open in the editor is Work/Notes/A.typ");
    expect(prompt).toContain("- the page Work/Notes/B.typ\n- the project folder Work");
    expect(promptFor("Hi", null, [])).toBe("Hi\n\nNo page is open in the editor.");
  });

  test("a deletion waits for the user, and keeping the item is told to the assistant", async () => {
    const { agent, files, prompts, backend } = setup(['Removing it.\n```znote\n{"do":"delete","path":"Work/Old"}\n```']);
    const sent = agent.send("Delete the old project");
    await vi.waitFor(() => expect(agent.deletion?.path).toBe("Work/Old"));
    expect(agent.running).toBe(true);
    agent.deletion?.answer(false);
    await sent;
    expect(files.deleteEntry).not.toHaveBeenCalled();
    expect(prompts[1]).toContain("The user chose to keep “Work/Old”");
    expect(backend.agentRun.mock.calls[1][0].session).toBe("session-1");
    expect(agent.running).toBe(false);
  });

  test("with confirmation switched off a deletion goes straight through", async () => {
    const { agent, files, chosen } = setup(['```znote\n{"do":"delete","path":"Work/Old"}\n{"do":"create","path":"Work/New"}\n```']);
    chosen.set("confirmDeletes", false);
    await agent.send("Delete the old project");
    expect(files.deleteEntry).toHaveBeenCalledWith("Work/Old");
    expect(files.createEntry).toHaveBeenCalledWith("Work/New");
    expect(agent.messages.at(-1)?.parts.map((part) => part.text)).toContain("Deleted Work/Old");
  });

  test("Typst errors go back to the assistant a limited number of times, and undo restores the page", async () => {
    const { agent, prompts, space, document, seen } = setup([], true);
    await space.useFolder("/notes", false);
    await agent.send("Add a list");
    expect(seen.locked).toBe(true);
    expect(agent.locked).toBe(false);
    expect(prompts).toHaveLength(1 + maxFixes);
    expect(prompts[1]).toContain("Work/Notes/A.typ, line 2: unknown variable");
    expect(agent.highlight).toEqual({ from: 2, to: 2 });
    expect(agent.canUndo).toBe(true);
    agent.undo();
    expect(document.text).toBe("= A\n");
    expect(agent.canUndo).toBe(false);
    expect(agent.highlight).toBe(null);
  });

  test("a sign-in that ends without a connection says so, and the link is gone afterwards", async () => {
    const { agent, linked, backend } = setup([]);
    linked.connected = false;
    await agent.connect("console");
    expect(backend.agentConnect.mock.calls[0][0]).toBe("console");
    expect(agent.failure).toContain("did not finish");
    expect(agent.link).toBe(null);
    expect(agent.stage).toBe(null);

    linked.connected = true;
    await agent.connect();
    expect(agent.failure).toBe(null);
    expect(agent.account?.connected).toBe(true);
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
    expect(agent.canUndo).toBe(false);
    expect(agent.highlight).toBe(null);
    expect(agent.activity).toBe(null);
  });

  test("a finished chat is kept, can be reopened, and deleting the open one clears the panel", async () => {
    const { agent, history, backend } = setup(["First answer."]);
    await agent.send("First question");
    const id = agent.chatId;
    expect(history.list.map(({ title, session }) => [title, session])).toEqual([["First question", "session-1"]]);
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
    release();
    await sent;

    expect(agent.messages).toHaveLength(2);
    const saved = history.list.find(({ id }) => id === slow);
    expect(saved?.session).toBe("session-2");
    expect(saved?.messages.at(-1)?.parts[0].text).toBe("Late answer.");
    agent.openChat(slow);
    expect(agent.messages.at(-1)?.parts[0].text).toBe("Late answer.");
  });

  test("stopping ends the turn without another automatic prompt", async () => {
    const { agent, prompts, backend } = setup(['```znote\n{"do":"delete","path":"Work/Old"}\n```']);
    const sent = agent.send("Delete the old project");
    await vi.waitFor(() => expect(agent.deletion).not.toBe(null));
    await agent.stop();
    await sent;
    expect(backend.agentCancel).toHaveBeenCalled();
    expect(prompts).toHaveLength(1);
    expect(agent.running).toBe(false);
  });
}
