import { expect, test, type Locator, type Page } from "@playwright/test";
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
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
const files = join(work, "files");
const data = join(process.env.XDG_DATA_HOME ?? join(homedir(), ".local/share"), IDENTIFIER);
const NOTES = join(files, "Larkspur notes.znote");

const intro = { title: "znote", lines: ["Typst notes with a live preview and an assistant"] };
const outro = {
  title: "znote",
  lines: ["Typst, live preview, an assistant that cannot see your names", "github.com/ZAL-Analytics/znote"]
};

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
  freshStart();
  await relay.listen();
  relay.start(root, IDENTIFIER, join(work, "app"));
  try {
    await relay.attach(page);
    await page.addInitScript(installStage, intro);
    console.log("==> waiting for the app (the first run compiles it)");
    await relay.ready;

    const claude = await relay.call("agent_status", { provider: "claude" });
    const signedIn = claude.ok && (claude.value as { connected: boolean }).connected;
    if (ASSISTANT && !signedIn) throw new Error("Claude Code is not signed in for the demo, run scripts/record.sh --sign-in first");

    // Native dialogs cannot be clicked from here, so the commands that open one
    // are answered with the demo's own files.
    relay.answer("project_open", (args) =>
      relay.call("project_open", { file: args.file ?? NOTES, accept: args.accept })
    );
    relay.answer("agent_attach", async (args) => {
      const chat = String(args.chat);
      // chats live with the open file, in projects/<hash of its path>/chats
      const projects = join(data, "projects");
      const home = readdirSync(projects).find((name) => /^[0-9a-f]{32}$/.test(name));
      if (home === undefined) return { ok: false, value: "no znote file is open" };
      const folder = join(projects, home, "chats", chat);
      mkdirSync(folder, { recursive: true });
      const source = join(files, nextAttachment.shift() ?? "Quarterly report.pdf");
      const copy = join(folder, `${readdirSync(folder).length + 1}-${basename(source)}`);
      copyFileSync(source, copy);
      const listed = await relay.call("agent_files", { chat });
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
    await this.tap(this.page.getByRole("navigation", { name: "Pages" }).getByRole("button", { name, exact: true }));
    await this.rendered();
  }

  async rendered(): Promise<void> {
    await expect(this.page.getByAltText("Page 1").first()).toBeVisible({ timeout: 60_000 });
    await this.stage.beat(500);
  }

  // Picks an option in one of the app's pickers.
  async pick(picker: Locator, option: string | RegExp): Promise<void> {
    await this.tap(picker, 250);
    await this.tap(this.page.getByRole("option", { name: option }).first(), 300);
  }

  async layout(name: RegExp): Promise<void> {
    await this.tap(this.button("Settings"));
    await this.tap(this.dialog().getByRole("tab", { name: "General" }));
    await this.tap(this.page.getByRole("radio", { name }));
    await this.page.keyboard.press("Escape");
    await this.stage.beat(500);
  }
}

async function firstStart(ui: Ui): Promise<void> {
  const { page, stage } = ui;
  await expect(ui.button("Open znote file…")).toBeVisible({ timeout: 60_000 });
  await stage.beat(900);
  await stage.hideCard();

  await stage.caption("One file holds all your notes", 900);
  await stage.still("Welcome");
  await ui.tap(ui.button("Open znote file…"), 600);

  await stage.caption("Encrypted with a password of your own", 300);
  await ui.tap(page.locator("#project-password"), 100);
  await stage.type(PASSWORD, 30);
  await stage.still("Password");
  await ui.tap(page.getByRole("button", { name: "Open", exact: true }), 600);
  stage.mark("opened the example file");

  await expect(page.getByText("How do you want to work?")).toBeVisible({ timeout: 30_000 });
  await stage.caption("First start: pick a layout", 900);
  await stage.still("FirstStart_Layout");
  await ui.tap(ui.button("Next"));

  await stage.caption("Hide names before anything reaches the assistant", 600);
  for (const id of ["veil", "names", "knowledge"]) {
    const toggle = page.locator(`#start-${id}`);
    if ((await toggle.getAttribute("aria-checked")) !== "true") await ui.tap(toggle, 350);
  }
  await stage.still("FirstStart_Privacy");
  await ui.tap(ui.button("Next"));

  await stage.caption("A knowledge file, locked with your own password", 300);
  await ui.tap(ui.button("New knowledge file"));
  await ui.tap(page.locator("#knowledge-name"), 100);
  await page.keyboard.press("ControlOrMeta+a");
  await stage.type("Larkspur knowledge");
  await ui.tap(page.locator("#knowledge-password"), 100);
  await stage.type(PASSWORD, 30);
  await ui.tap(page.locator("#knowledge-again"), 100);
  await stage.type(PASSWORD, 30);
  // the password store may not be there on every desktop
  const remember = page.locator("#knowledge-remember");
  if ((await remember.getAttribute("aria-checked")) === "true") await ui.tap(remember);
  await ui.tap(ui.dialog().getByRole("button", { name: "Create", exact: true }), 600);

  await expect(page.getByText("Downloads", { exact: true })).toBeVisible({ timeout: 30_000 });
  await stage.caption("Two small models that run on this computer", 400);
  // a started download turns its button into Cancel
  const download = ui.dialog().getByRole("button", { name: "Download", exact: true });
  while (await download.count()) await ui.tap(download.first(), 300);
  await stage.fast(() =>
    expect(ui.dialog().getByText("Downloaded", { exact: true })).toHaveCount(2, { timeout: 600_000 })
  );
  await stage.still("FirstStart_Downloads");
  await ui.tap(ui.button("Next"));

  await stage.caption("Looks and templates from the marketplace", 700);
  await ui.tap(ui.button("Finish"), 800);
  await stage.hideCaption();
  stage.mark("first start done");
}

async function layouts(ui: Ui): Promise<void> {
  const { page, stage } = ui;
  await stage.caption("Typst on the left, the page on the right", 300);
  await ui.open("Project brief");
  await stage.still("Edit_ProjectBrief");

  await stage.caption("Every keystroke renders", 200);
  const editor = page.locator(".cm-content").first();
  await editor.click();
  await page.keyboard.press("ControlOrMeta+End");
  await stage.type("\n\n= Next steps\n\n- Pick the payment provider.\n- Test with staff in the Utrecht store.\n");
  await ui.rendered();
  await stage.beat(800);
  await stage.still("Edit_Typing");

  await stage.caption("Pages live in projects and notebooks", 200);
  await ui.open("Market notes");
  await stage.beat(600);

  await stage.caption("Chat first, with the page beside it", 200);
  await ui.layout(/^Assistant/);
  await stage.still("Layout_Assistant");

  await stage.caption("Or find pages and control what is hidden", 200);
  await ui.layout(/^Redact/);
  await stage.still("Layout_Redact");

  await ui.layout(/^Edit/);
  await stage.hideCaption();
  stage.mark("layouts");
}

async function marketplace(ui: Ui): Promise<void> {
  const { page, stage } = ui;
  await stage.caption("The marketplace: looks and templates", 200);
  await ui.tap(ui.button("Marketplace"), 800);
  await ui.tap(page.getByRole("searchbox", { name: "Search add-ons" }).or(page.getByPlaceholder("Search add-ons")).first(), 100);
  await stage.type("ZAL");
  await stage.beat(600);
  await stage.still("Marketplace");

  // the look (.zth) and the templates (.znx) are two cards of the same name
  const cards = page.locator("article").filter({ has: page.getByRole("heading", { name: "ZAL Analytics" }) });
  await expect(cards).toHaveCount(2, { timeout: 60_000 });
  const install = cards.getByRole("button", { name: "Install", exact: true });
  while (await install.count()) {
    await ui.tap(install.first(), 300);
    await stage.fast(() => expect(cards.getByRole("button", { name: "Installing…" })).toHaveCount(0, { timeout: 120_000 }));
  }
  await stage.caption("Use the look for the whole app", 200);
  await ui.tap(page.getByRole("button", { name: "Use", exact: true }).first(), 900);
  await stage.still("Marketplace_Installed");

  await stage.caption("Templates apply to any page", 200);
  await ui.tap(ui.button(/^(Edit|Assistant|Redact)$/, false).first(), 500);
  await ui.open("Market notes");
  await ui.tap(page.getByRole("button", { name: /^Page template/ }), 300);
  await ui.tap(page.getByRole("menuitem", { name: "ZAL Analytics: Note" }).or(page.getByRole("menuitemradio", { name: "ZAL Analytics: Note" })).first(), 600);
  await ui.rendered();
  await stage.beat(1200);
  await stage.still("Template_ZalNote");
  await stage.hideCaption();
  stage.mark("marketplace");
}

async function send(ui: Ui, prompt: string): Promise<void> {
  const { page, stage } = ui;
  const box = page.getByRole("textbox", { name: "Message to the assistant" });
  await ui.tap(box, 100);
  await stage.type(prompt, 18);
  await ui.tap(ui.button("Send"), 400);
  await expect(ui.button("Stop")).toBeVisible();
  await stage.fast(() => expect(ui.button("Stop")).toHaveCount(0, { timeout: 600_000 }));
  await stage.beat(800);
}

async function assistant(ui: Ui, attachments: string[]): Promise<void> {
  const { page, stage } = ui;
  await ui.layout(/^Assistant/);
  await stage.caption("The assistant runs on your own Claude Code", 400);

  await stage.caption("Write a new page", 200);
  await send(
    ui,
    "Write a new page 'Launch plan' in the Planning notebook: a one page launch plan for the ordering app with a timeline table and three risks."
  );
  await stage.still("Assistant_NewPage");

  await stage.caption("Fix and tighten an existing page", 200);
  await ui.tap(ui.button("New chat"), 400);
  // the assistant sees only the pages added to the chat
  await ui.tap(ui.button("Add page or file"), 300);
  for (const name of ["Larkspur Bakery", "Planning", "Meeting notes"]) {
    await ui.tap(page.getByRole("menuitem", { name, exact: true }), 300);
  }
  await send(ui, "Fix the spelling mistakes in the page 'Meeting notes' and keep everything else as it is.");
  await stage.still("Assistant_Edit");
  // a finished reply folds its steps, the Review link among them
  const steps = page.locator("details").last();
  if ((await steps.count()) && !(await steps.evaluate((node) => (node as HTMLDetailsElement).open)))
    await ui.tap(steps.locator("summary"), 400);
  const review = ui.button("Review").last();
  if (await review.isVisible()) {
    await ui.tap(review, 800);
    await stage.still("Assistant_Review");
    const keep = ui.button("Keep all changes");
    if (await keep.isVisible()) await ui.tap(keep, 600);
  }

  await stage.caption("Read a PDF and summarise it", 200);
  await ui.tap(ui.button("New chat"), 400);
  attachments.push("Quarterly report.pdf");
  await ui.tap(ui.button("Add page or file"), 300);
  await ui.tap(page.getByRole("menuitem", { name: "Attach files…" }), 600);
  await send(ui, "Summarise the attached report in five bullet points and list the open risks.");
  await stage.still("Assistant_AnalysePdf");
  await stage.hideCaption();
  stage.mark("assistant");
}

async function filtering(ui: Ui, attachments: string[]): Promise<void> {
  const { page, stage } = ui;
  if (ASSISTANT) {
    await stage.caption("What the assistant sees: names become tokens", 300);
    await ui.tap(ui.button("What the assistant sees"), 800);
    await stage.still("Filter_ChatFiles");
    await page.keyboard.press("Escape");
  }

  await ui.layout(/^Redact/);
  await stage.caption("Choose how much each page hides", 200);
  // a page button also names the page's level, so only its start is matched
  await ui.tap(page.getByRole("region", { name: "Notebooks" }).getByRole("button", { name: /^Planning/ }), 300);
  await ui.tap(page.getByRole("region", { name: "Pages" }).getByRole("button", { name: /^Meeting notes/ }), 800);
  const slider = page.getByRole("slider", { name: "How much is hidden" }).or(page.locator('input[type="range"]')).first();
  for (const level of ["1", "2", "3"]) {
    await slider.fill(level);
    await stage.beat(1100);
  }
  await stage.still("Filter_Strict");
  await slider.fill("1");
  await stage.beat(900);
  await stage.still("Filter_Light");

  if (!ASSISTANT) return stage.hideCaption();
  await stage.caption("Files you attach are filtered the same way", 200);
  await ui.layout(/^Assistant/);
  await ui.tap(ui.button("New chat"), 400);
  attachments.push("Supplier letter.pdf");
  await ui.tap(ui.button("Add page or file"), 300);
  await ui.tap(page.getByRole("menuitem", { name: "Attach files…" }), 600);
  await ui.tap(ui.button("What the assistant sees"), 800);
  await stage.still("Filter_Pdf");
  await page.keyboard.press("Escape");

  await stage.caption("Check a message before it is sent", 200);
  await ui.tap(page.getByRole("textbox", { name: "Message to the assistant" }), 100);
  await stage.type("Draft a reply to Pieter de Graaf at Millbrook Grain Co. about the new flour price.", 18);
  await ui.tap(ui.button("Show what this message would send"), 1200);
  await stage.still("Filter_Message");
  await page.keyboard.press("Escape");
  await stage.hideCaption();
  stage.mark("filtering");
}

async function extras(ui: Ui): Promise<void> {
  const { page, stage } = ui;
  await ui.layout(/^Edit/);
  await ui.open("Project brief");

  await stage.caption("21 languages, right to left included", 200);
  // its name changes with the language, so it is found by its place
  const language = page.locator("header [data-slot=select-trigger]").first();
  await ui.pick(language, "Deutsch");
  await stage.beat(800);
  await stage.still("Language_German");
  await ui.pick(language, "العربية");
  await stage.beat(800);
  await stage.still("Language_Arabic");
  await ui.pick(language, "English");

  await stage.caption("Dark mode", 200);
  await ui.tap(ui.button("Switch to dark mode"), 900);
  await stage.still("Dark");

  await stage.caption("Export to PDF or PowerPoint", 200);
  await ui.tap(ui.button("Export"), 900);
  await stage.still("Export");
  await ui.tap(ui.button("Switch to light mode"), 400);
  await stage.hideCaption();
  stage.mark("extras");
}
