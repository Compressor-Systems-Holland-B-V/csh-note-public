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
import { Agent, maxFixes, type Backend } from "@zsuite/assistant/state/agent";

export * from "@zsuite/assistant/state/agent";

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
        if (turn.design) handler({ kind: "saw", label: "Work/Notes/A", text: "", picture: "seen-1-1.png" });
        handler({ kind: "text", text });
        handler({ kind: "done", session: "session-1", failure: null });
      }),
      agentAttach: vi.fn(async () => [{ name: "report.pdf", id: "1.pdf", path: "/data/assistant/1-report.pdf", text: null, level: 1 }]),
      agentFiles: vi.fn(async () => [{ name: "report.pdf", id: "1.pdf", path: "/data/assistant/1-report.pdf", text: null, level: 1 }]),
      agentSeen: vi.fn(async () => new ArrayBuffer(0)),
      agentDetach: vi.fn(async () => {}),
      agentLevel: vi.fn(async () => {}),
      agentForget: vi.fn(async () => {}),
      createEntry: files.createEntry,
      writePage: files.writePage,
    };
    const space = new Workspace(files, document, chosen);
    const history = new Chats(() => Promise.resolve());
    const agent: Agent = new Agent(backend, space, document, chosen, () => [], history, linked, () => knowledge);
    agent.mode = "generate";
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

  test("in generate mode the target is asked for, Typst only in the chat is sent back, and the target stays for the next message", async () => {
    const { agent, prompts } = setup(["Here is the report:\n```typ\n= Report\n```"]);
    agent.target = "Work/Reports";
    await agent.send("Write a report");
    expect(prompts[0]).toContain("Work/Reports/<page name>.typ");
    expect(prompts[1]).toContain("no page was saved");
    expect(agent.target).toBe("Work/Reports");
  });

  test("a template picked for the chat is asked for, and dropped when the chat may only read", async () => {
    const { agent, prompts, chosen } = setup(["Done.", "Done."]);
    agent.template = "acme/Report";
    await agent.send("Write a report");
    expect(prompts[0]).toContain("Use the template acme/Report");
    chosen.set("assistantPresets", { claude: [preset("read")] });
    chosen.set("assistantPreset", "mine");
    await agent.send("Read it");
    expect(prompts.at(-1)).not.toContain("Use the template");
  });

  test("outside generate mode the target and template are not asked for, review only reads, and design may edit", async () => {
    const { agent, prompts, backend } = setup(["Like this:\n```typ\n= Summary\n```", "Done."]);
    agent.mode = "review";
    agent.target = "Work/Reports";
    agent.template = "acme/Report";
    await agent.send("How should it look?");
    expect(prompts).toHaveLength(1);
    expect(backend.agentRun.mock.calls[0][0]).toMatchObject({ readOnly: true });
    expect(prompts[0]).not.toContain("Work/Reports");
    expect(prompts[0]).not.toContain("Use the template");
    agent.mode = "design";
    await agent.send("And the cover?");
    expect(prompts.at(-1)).toContain("how the pages look");
    expect(prompts.at(-1)).not.toContain("Work/Reports");
    expect(backend.agentRun.mock.calls[0][0]).toMatchObject({ design: false });
    expect(backend.agentRun.mock.calls.at(-1)?.[0]).toMatchObject({ design: true, readOnly: false, session: "session-1" });
    // Switching modes in the same chat tells the assistant what it may do now.
    expect(prompts[0]).toContain("may only read pages");
    expect(prompts.at(-1)).toContain("may create and change pages");
    expect(agent.messages.at(-1)?.saw).toEqual([{ label: "Work/Notes/A", text: "", picture: "seen-1-1.png" }]);
  });

  test("a removed file leaves the chat and the message box", async () => {
    const { agent, backend } = setup(["Done."]);
    await agent.attach();
    await agent.detach("/data/assistant/1-report.pdf");
    expect(backend.agentDetach).toHaveBeenCalledWith(agent.chatId, "/data/assistant/1-report.pdf");
    expect(agent.references).toEqual([]);
  });

  test("attaching the same file again adds it only once", async () => {
    const { agent } = setup(["Done."]);
    await agent.attach();
    await agent.attach();
    expect(agent.references.map(({ label }) => label)).toEqual(["report.pdf"]);
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
}
