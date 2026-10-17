import { expect, test, type Locator, type Page } from "@playwright/test";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Relay } from "./relay";
import { installStage, Stage } from "./stage";

// A separate identifier keeps the demo's data, settings and Claude Code sign-in
// apart from a real install of znote.
const IDENTIFIER = "com.zalanalytics.znote.demo";
// scripts/record.sh encrypts the example file with a new password on each run,
// so the app has none remembered and asks for it.
const PASSWORD = process.env.DEMO_PASSWORD ?? "";
// DEMO_ASSISTANT=0 records everything but the parts that need Claude Code.
const ASSISTANT = process.env.DEMO_ASSISTANT !== "0";

const root = fileURLToPath(new URL("../..", import.meta.url));
const work = join(root, ".demo");
// scripts/record.sh writes the PDFs and the encrypted .znote file here
const files = join(work, "files");
const data = join(process.env.XDG_DATA_HOME ?? join(homedir(), ".local/share"), IDENTIFIER);

// The app's own English strings, so the buttons are found by the names the app gives them.
type Dict = { [key: string]: string | Dict };
const strings: Dict = {};
for (const folder of ["src/lib/assets/locale", "vendor/zsuite/packages/assistant/src/locale"]) {
  const dir = join(root, folder, "en");
  for (const file of readdirSync(dir)) strings[basename(file, ".json")] = JSON.parse(readFileSync(join(dir, file), "utf8"));
}

function t(key: string, vars: Record<string, string | number> = {}): string {
  const text = key.split(".").reduce<string | Dict | undefined>((node, part) => (typeof node === "object" ? node[part] : undefined), strings);
  if (typeof text !== "string") throw new Error(`no string ${key}`);
  return text.replace(/\{(\w+)\}/g, (all, name: string) => String(vars[name] ?? all));
}

// The captions, prompts, typed text and example names of the demo; the pages and PDFs
// themselves are in tools/demo/examples/pages and tools/demo/fixtures.py.
const WORDS = {
  brief: "Project brief",
  market: "Market notes",
  meeting: "Meeting notes",
  report: "Quarterly report.pdf",
  letter: "Supplier letter.pdf",
  intro: ["Typst notes with a live preview and an assistant"],
  outro: ["Typst, live preview, an assistant that cannot see your names", "github.com/ZAL-Analytics/znote"],
  welcome: "One file holds all your notes",
  password: "Encrypted with a password of your own",
  layout: "First start: pick a layout",
  privacy: "Hide names before anything reaches the assistant",
  knowledge: "A knowledge file, locked with your own password",
  knowledgeName: "Larkspur knowledge",
  models: "Two small models that run on this computer",
  marketplaceStart: "Looks and templates from the marketplace",
  edit: "Typst on the left, the page on the right",
  typing: "Every keystroke renders",
  typed: "\n\n= Next steps\n\n- Pick the payment provider.\n- Test with staff in the Utrecht store.\n",
  sidebar: "Pages live in projects and notebooks",
  chatLayout: "Chat first, with the page beside it",
  redactLayout: "Or find pages and control what is hidden",
  marketplace: "The marketplace: looks and templates",
  look: "Use the look for the whole app",
  templates: "Templates apply to any page",
  addon: "ZAL Analytics",
  claude: "The assistant runs on your own Claude Code",
  document: "Write a document straight into a notebook, with a template",
  documentPrompt: "Write a one page launch plan called 'Launch plan' for the ordering app, with a timeline table and three risks.",
  fix: "Fix and tighten an existing page",
  fixPrompt: "Fix the spelling mistakes in the page 'Meeting notes' and keep everything else as it is.",
  pdf: "Read a PDF and summarise it",
  pdfPrompt: "Summarise the attached report in five bullet points and list the open risks.",
  sees: "What the assistant sees: names become tokens",
  levels: "Choose how much each page hides",
  files: "Files you attach are filtered the same way",
  stricter: "Try a stricter level, then undo it",
  message: "See exactly what a message sent",
  messagePrompt: "Draft a short reply to Pieter de Graaf at Millbrook Grain Co. about the new flour price.",
  languages: "21 languages, right to left included",
  dark: "Dark mode",
  export: "Export to PDF, PowerPoint or Word"
};

const intro = { title: "znote", lines: WORDS.intro };
const outro = { title: "znote", lines: WORDS.outro };

// Starts from a first run: everything the demo app kept is removed except the
// Claude Code folder, which holds the sign-in made with `--sign-in`.
function freshStart(): void {
  if (!existsSync(data)) return;
  for (const entry of readdirSync(data)) {
    if (entry !== "claude") rmSync(join(data, entry), { recursive: true, force: true });
  }
}

test("record the demo", async ({ page }) => {
  const stage = new Stage(page, join(work, "stills"));
  const relay = new Relay(Number(process.env.DEMO_RELAY_PORT ?? 4192));

  if (PASSWORD.length < 8) throw new Error("DEMO_PASSWORD is not set, run scripts/record.sh --record");
  const notes = join(files, readdirSync(files).find((name) => name.endsWith(".znote")) ?? "");
  freshStart();
  await relay.listen();
  relay.start(root, IDENTIFIER, join(work, "app"));
  try {
    await relay.attach(page);
    await page.addInitScript(installStage, intro);
    console.log("==> waiting for the app (the first run compiles it)");
    await relay.ready;

    const claude = await relay.call("plugin:zsuite-assistant|agent_status", { provider: "claude" });
    const signedIn = claude.ok && (claude.value as { connected: boolean }).connected;
    if (ASSISTANT && !signedIn) throw new Error("Claude Code is not signed in for the demo, run scripts/record.sh --sign-in first");

    // Native dialogs cannot be clicked from here, so the commands that open one
    // are answered with the demo's own files.
    relay.answer("project_open", (args) =>
      relay.call("project_open", { file: args.file ?? notes, accept: args.accept })
    );
    relay.answer("plugin:zsuite-assistant|agent_attach", async (args) => {
      const chat = String(args.chat);
      // chats live with the open file, in projects/<hash of its path>/chats
      const projects = join(data, "projects");
      const home = readdirSync(projects).find((name) => /^[0-9a-f]{32}$/.test(name));
      if (home === undefined) return { ok: false, value: "no znote file is open" };
      const folder = join(projects, home, "chats", chat);
      mkdirSync(folder, { recursive: true });
      const source = join(files, nextAttachment.shift() ?? WORDS.report);
      const copy = join(folder, `${readdirSync(folder).length + 1}-${basename(source)}`);
      copyFileSync(source, copy);
      const listed = await relay.call("plugin:zsuite-assistant|agent_files", { chat });
      if (!listed.ok) return listed;
      return { ok: true, value: (listed.value as { path: string }[]).filter((file) => file.path === copy) };
    });
    const nextAttachment: string[] = [];

    await page.goto("/");
    stage.begin();
    const ui = new Ui(page, stage);

    await firstStart(ui);
    await layouts(ui);
    await marketplace(ui);
    if (ASSISTANT) await assistant(ui, nextAttachment);
    await filtering(ui, nextAttachment);
    await extras(ui);

    await stage.card(outro, 2600);
    stage.saveCuts(join(work, "cuts.txt"));
  } finally {
    await relay.stop();
  }
});

class Ui {
  constructor(
    readonly page: Page,
    readonly stage: Stage
  ) {}

  button(name: string | RegExp, exact = true): Locator {
    return this.page.getByRole("button", { name, exact });
  }

  dialog(): Locator {
    return this.page.getByRole("dialog");
  }

  async tap(target: Locator, settle = 300): Promise<void> {
    await expect(target).toBeEnabled();
    await this.stage.tap(target, settle);
  }

  async open(name: string): Promise<void> {
    await this.tap(this.page.getByRole("navigation", { name: t("workspace.pages") }).getByRole("button", { name, exact: true }));
    await this.rendered();
  }

  async rendered(): Promise<void> {
    await expect(this.page.getByAltText(t("editor.preview.page", { number: 1 })).first()).toBeVisible({ timeout: 60_000 });
    await this.stage.beat(500);
  }

  // Picks an option in one of the app's pickers.
  async pick(picker: Locator, option: string | RegExp): Promise<void> {
    await this.tap(picker, 250);
    await this.tap(this.page.getByRole("option", { name: option }).first(), 300);
  }

  // Picks an item in one of the composer's menus.
  async choose(menu: Locator, item: string): Promise<void> {
    await this.tap(menu, 250);
    await this.tap(this.page.getByRole("menuitemradio", { name: item }), 300);
  }

  // The levels are radios hidden inside their labels, so a label is clicked.
  level(scope: Locator, name: string): Locator {
    return scope
      .getByRole("group", { name: t("privacy.level.label") })
      .locator("label")
      .filter({ has: this.page.getByRole("radio", { name, exact: true }) });
  }

  // A layout radio is named by the layout, then its hint.
  // The chat overview, on its tab of pages and files as the assistant sees them.
  async chatFiles(): Promise<void> {
    await this.tap(this.button(t("assistant.overview.title")), 600);
    await this.tap(this.dialog().getByRole("tab", { name: t("assistant.overview.files") }), 800);
  }

  async layout(key: "edit" | "agent" | "redaction"): Promise<void> {
    await this.tap(this.button(t("common.settings")));
    await this.tap(this.dialog().getByRole("tab", { name: t("settings.general") }));
    await this.tap(this.page.getByRole("radio", { name: new RegExp(`^${t(`common.layout.${key}`)}`) }));
    await this.page.keyboard.press("Escape");
    await this.stage.beat(500);
  }
}

async function firstStart(ui: Ui): Promise<void> {
  const { page, stage } = ui;
  await expect(ui.button(t("workspace.file.open"))).toBeVisible({ timeout: 60_000 });
  await stage.beat(900);
  await stage.hideCard();

  await stage.caption(WORDS.welcome, 900);
  await stage.still("Welcome");
  await ui.tap(ui.button(t("workspace.file.open")), 600);

  await stage.caption(WORDS.password, 300);
  await ui.tap(page.locator("#project-password"), 100);
  await stage.type(PASSWORD, 30);
  await stage.still("Password");
  await ui.tap(page.getByRole("button", { name: t("workspace.password.open"), exact: true }), 600);
  stage.mark("opened the example file");

  await expect(page.getByText(t("settings.start.layout"))).toBeVisible({ timeout: 30_000 });
  await stage.caption(WORDS.layout, 900);
  await stage.still("FirstStart_Layout");
  await ui.tap(ui.button(t("common.next")));

  await stage.caption(WORDS.privacy, 600);
  for (const id of ["veil", "names", "knowledge"]) {
    const toggle = page.locator(`#start-${id}`);
    if ((await toggle.getAttribute("aria-checked")) !== "true") await ui.tap(toggle, 350);
  }
  await stage.still("FirstStart_Privacy");
  await ui.tap(ui.button(t("common.next")));

  await stage.caption(WORDS.knowledge, 300);
  await ui.tap(ui.button(t("privacy.start.create")));
  await ui.tap(page.locator("#knowledge-name"), 100);
  await page.keyboard.press("ControlOrMeta+a");
  await stage.type(WORDS.knowledgeName);
  await ui.tap(page.locator("#knowledge-password"), 100);
  await stage.type(PASSWORD, 30);
  await ui.tap(page.locator("#knowledge-again"), 100);
  await stage.type(PASSWORD, 30);
  // the password store may not be there on every desktop
  const remember = page.locator("#knowledge-remember");
  if ((await remember.getAttribute("aria-checked")) === "true") await ui.tap(remember);
  await ui.tap(ui.dialog().getByRole("button", { name: t("common.create"), exact: true }), 600);

  await expect(page.getByText(t("settings.start.downloads"), { exact: true })).toBeVisible({ timeout: 30_000 });
  await stage.caption(WORDS.models, 400);
  // a started download turns its button into Cancel
  const download = ui.dialog().getByRole("button", { name: t("privacy.models.download"), exact: true });
  while (await download.count()) await ui.tap(download.first(), 300);
  await stage.fast(() =>
    expect(ui.dialog().getByText(t("privacy.models.ready"), { exact: true })).toHaveCount(2, { timeout: 600_000 })
  );
  await stage.still("FirstStart_Downloads");
  await ui.tap(ui.button(t("common.next")));

  await stage.caption(WORDS.marketplaceStart, 700);
  await ui.tap(ui.button(t("common.finish")), 800);
  await stage.hideCaption();
  stage.mark("first start done");
}

async function layouts(ui: Ui): Promise<void> {
  const { page, stage } = ui;
  await stage.caption(WORDS.edit, 300);
  await ui.open(WORDS.brief);
  await stage.still("Edit_ProjectBrief");

  await stage.caption(WORDS.typing, 200);
  const editor = page.locator(".cm-content").first();
  await editor.click();
  await page.keyboard.press("ControlOrMeta+End");
  await stage.type(WORDS.typed);
  await ui.rendered();
  await stage.beat(800);
  await stage.still("Edit_Typing");

  await stage.caption(WORDS.sidebar, 200);
  await ui.open(WORDS.market);
  await stage.beat(600);

  await stage.caption(WORDS.chatLayout, 200);
  await ui.layout("agent");
  await stage.still("Layout_Assistant");

  await stage.caption(WORDS.redactLayout, 200);
  await ui.layout("redaction");
  await stage.still("Layout_Redact");

  await ui.layout("edit");
  await stage.hideCaption();
  stage.mark("layouts");
}

async function marketplace(ui: Ui): Promise<void> {
  const { page, stage } = ui;
  await stage.caption(WORDS.marketplace, 200);
  await ui.tap(ui.button(t("common.mode.marketplace")), 800);
  const search = t("themes.search");
  await ui.tap(page.getByRole("searchbox", { name: search }).or(page.getByPlaceholder(search)).first(), 100);
  await stage.type("ZAL");
  await stage.beat(600);
  await stage.still("Marketplace");

  // the look (.zth) and the templates (.znx) are two cards of the same name
  const cards = page.locator("article").filter({ has: page.getByRole("heading", { name: WORDS.addon }) });
  await expect(cards).toHaveCount(2, { timeout: 60_000 });
  const install = cards.getByRole("button", { name: t("themes.install"), exact: true });
  while (await install.count()) {
    await ui.tap(install.first(), 300);
    await stage.fast(() => expect(cards.getByRole("button", { name: t("themes.installing") })).toHaveCount(0, { timeout: 120_000 }));
  }
  await stage.caption(WORDS.look, 200);
  await ui.tap(page.getByRole("button", { name: t("themes.use"), exact: true }).first(), 900);
  await stage.still("Marketplace_Installed");

  await stage.caption(WORDS.templates, 200);
  await ui.tap(ui.button(t("common.layout.edit")), 500);
  await ui.open(WORDS.market);
  await ui.tap(page.getByRole("button", { name: new RegExp(`^${t("editor.templateTitle")}`) }), 300);
  const note = `${WORDS.addon}: Note`;
  await ui.tap(page.getByRole("menuitem", { name: note }).or(page.getByRole("menuitemradio", { name: note })).first(), 600);
  await ui.rendered();
  await stage.beat(1200);
  await stage.still("Template_ZalNote");
  await stage.hideCaption();
  stage.mark("marketplace");
}

async function send(ui: Ui, prompt: string): Promise<void> {
  const { page, stage } = ui;
  const box = page.getByRole("textbox", { name: t("assistant.composer.message") });
  await ui.tap(box, 100);
  await stage.type(prompt, 18);
  await ui.tap(ui.button(t("assistant.composer.send")), 400);
  await expect(ui.button(t("assistant.composer.stop"))).toBeVisible();
  await stage.fast(() => expect(ui.button(t("assistant.composer.stop"))).toHaveCount(0, { timeout: 600_000 }));
  await stage.beat(800);
}

async function assistant(ui: Ui, attachments: string[]): Promise<void> {
  const { page, stage } = ui;
  await ui.layout("agent");
  await stage.caption(WORDS.claude, 400);

  await stage.caption(WORDS.document, 200);
  const target = ui.button(t("assistant.composer.target"));
  const template = ui.button(t("assistant.composer.template"));
  await ui.tap(page.getByRole("radio", { name: t("assistant.composer.generate"), exact: true }), 300);
  await ui.choose(target, "Larkspur Bakery/Planning");
  await ui.choose(template, `${WORDS.addon}: Documentation`);
  await stage.still("Assistant_Output");
  await send(ui, WORDS.documentPrompt);
  await stage.still("Assistant_NewPage");
  // the composer keeps these choices across chats, so they are cleared for the next ones;
  // generate mode stays on, so the next chat may edit the page
  await ui.tap(ui.button(t("assistant.composer.targetClear")), 200);
  await ui.tap(ui.button(t("assistant.composer.templateClear")), 200);

  await stage.caption(WORDS.fix, 200);
  await ui.tap(ui.button(t("assistant.history.newChat")), 400);
  // the assistant sees only the pages added to the chat
  await ui.tap(ui.button(t("assistant.composer.add")), 300);
  for (const name of ["Larkspur Bakery", "Planning", WORDS.meeting]) {
    await ui.tap(page.getByRole("menuitem", { name, exact: true }), 300);
  }
  await send(ui, WORDS.fixPrompt);
  await stage.still("Assistant_Edit");
  // a finished reply folds its steps, the Review link among them
  const steps = page.locator("details").last();
  if ((await steps.count()) && !(await steps.evaluate((node) => (node as HTMLDetailsElement).open)))
    await ui.tap(steps.locator("summary"), 400);
  const review = ui.button(t("assistant.steps.review")).last();
  if (await review.isVisible()) {
    await ui.tap(review, 800);
    await stage.still("Assistant_Review");
    const keep = ui.button(t("editor.review.keepAllTitle"));
    if (await keep.isVisible()) await ui.tap(keep, 600);
  }

  await stage.caption(WORDS.pdf, 200);
  await ui.tap(ui.button(t("assistant.history.newChat")), 400);
  await ui.tap(page.getByRole("radio", { name: t("assistant.composer.review"), exact: true }), 200);
  attachments.push(WORDS.report);
  await ui.tap(ui.button(t("assistant.composer.add")), 300);
  await ui.tap(page.getByRole("menuitem", { name: t("assistant.composer.attach") }), 600);
  await send(ui, WORDS.pdfPrompt);
  await stage.still("Assistant_AnalysePdf");
  await stage.hideCaption();
  stage.mark("assistant");
}

async function filtering(ui: Ui, attachments: string[]): Promise<void> {
  const { page, stage } = ui;
  if (ASSISTANT) {
    await stage.caption(WORDS.sees, 300);
    await ui.chatFiles();
    await stage.still("Filter_ChatFiles");
    await page.keyboard.press("Escape");
  }

  await ui.layout("redaction");
  await stage.caption(WORDS.levels, 200);
  // a page button also names the page's level, so only its start is matched
  await ui.tap(page.getByRole("region", { name: t("workspace.browser.notebooks") }).getByRole("button", { name: /^Planning/ }), 300);
  await ui.tap(page.getByRole("region", { name: t("workspace.pages") }).getByRole("button", { name: new RegExp(`^${WORDS.meeting}`) }), 800);
  const redaction = page.getByRole("region", { name: t("privacy.redact.title") });
  for (const level of ["medium", "strict"]) await ui.tap(ui.level(redaction, t(`privacy.level.short.${level}`)), 1100);
  await stage.still("Filter_Strict");
  await ui.tap(ui.level(redaction, t("privacy.level.short.light")), 900);
  await stage.still("Filter_Light");

  if (!ASSISTANT) return stage.hideCaption();
  await stage.caption(WORDS.files, 200);
  await ui.layout("agent");
  await ui.tap(ui.button(t("assistant.history.newChat")), 400);
  attachments.push(WORDS.letter);
  await ui.tap(ui.button(t("assistant.composer.add")), 300);
  await ui.tap(page.getByRole("menuitem", { name: t("assistant.composer.attach") }), 600);
  await ui.chatFiles();
  await stage.still("Filter_Pdf");
  await stage.caption(WORDS.stricter, 200);
  await ui.tap(ui.level(ui.dialog(), t("privacy.level.short.strict")), 1100);
  await ui.tap(ui.dialog().getByRole("button", { name: t("assistant.files.undo"), exact: true }), 900);
  await page.keyboard.press("Escape");

  await stage.caption(WORDS.message, 200);
  await send(ui, WORDS.messagePrompt);
  // a sent message has an eye that shows it as the assistant got it
  await ui.tap(ui.button(t("privacy.preview")).last(), 1200);
  await stage.still("Filter_Message");
  await page.keyboard.press("Escape");
  await stage.hideCaption();
  stage.mark("filtering");
}

async function extras(ui: Ui): Promise<void> {
  const { page, stage } = ui;
  await ui.layout("edit");
  await ui.open(WORDS.brief);

  await stage.caption(WORDS.languages, 200);
  // its name changes with the language, so it is found by its place
  const language = page.locator("header [data-slot=select-trigger]").first();
  await ui.pick(language, "Deutsch");
  await stage.beat(800);
  await stage.still("Language_German");
  await ui.pick(language, "العربية");
  await stage.beat(800);
  await stage.still("Language_Arabic");
  await ui.pick(language, "English");

  await stage.caption(WORDS.dark, 200);
  await ui.tap(ui.button(t("common.appearance.toDark")), 900);
  await stage.still("Dark");

  await stage.caption(WORDS.export, 200);
  await ui.tap(ui.button(t("common.mode.export")), 900);
  await stage.still("Export");
  await ui.tap(ui.button(t("common.appearance.toLight")), 400);
  await stage.hideCaption();
  stage.mark("extras");
}
