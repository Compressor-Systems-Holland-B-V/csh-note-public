import type { Locator, Page } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";

export type Card = { title: string; lines: string[] };

type StageApi = {
  caption: (text: string) => void;
  hideCaption: () => void;
  card: (card: Card) => void;
  hideCard: () => void;
  quiet: (on: boolean) => void;
};

declare global {
  interface Window {
    __stage: StageApi;
  }
}

// The cursor, click ripple, captions and title cards drawn over the app for the
// recording. Stills are taken with it hidden.
export function installStage(intro: Card): void {
  const css = `
    #stage-layer, #stage-layer * { box-sizing: border-box; }
    #stage-layer.quiet { display: none; }
    #stage-cursor {
      position: fixed; left: 50%; top: 50%; z-index: 2147483000;
      width: 24px; height: 24px; margin: -12px 0 0 -12px; border-radius: 999px;
      border: 2px solid #111; background: rgba(17, 17, 17, 0.16);
      box-shadow: 0 2px 10px rgba(17, 17, 17, 0.25);
      pointer-events: none; opacity: 0;
      transition: opacity 200ms ease, left 70ms linear, top 70ms linear;
    }
    #stage-cursor.on { opacity: 1; }
    #stage-tap {
      position: fixed; left: 50%; top: 50%; z-index: 2147482999;
      width: 24px; height: 24px; margin: -12px 0 0 -12px; border-radius: 999px;
      border: 2px solid #111; pointer-events: none; opacity: 0;
    }
    #stage-tap.go { animation: stage-tap 420ms ease-out; }
    @keyframes stage-tap {
      from { opacity: 0.8; transform: scale(1); }
      to { opacity: 0; transform: scale(3.2); }
    }
    #stage-caption {
      position: fixed; left: 50%; bottom: 64px; z-index: 2147483001;
      transform: translate(-50%, 14px);
      max-width: 80vw; padding: 12px 28px; border-radius: 999px;
      background: #111; color: #fafafa;
      font: 600 20px/1.25 ui-sans-serif, system-ui, sans-serif;
      text-align: center; white-space: nowrap;
      box-shadow: 0 12px 30px rgba(17, 17, 17, 0.22);
      opacity: 0; transition: opacity 220ms ease, transform 220ms ease;
      pointer-events: none;
    }
    #stage-caption.on { opacity: 1; transform: translate(-50%, 0); }
    #stage-card {
      position: fixed; inset: 0; z-index: 2147483002;
      display: flex; flex-direction: column; align-items: center; justify-content: center;
      gap: 20px; background: #fafafa; color: #111;
      font-family: ui-sans-serif, system-ui, sans-serif; text-align: center;
      opacity: 0; transition: opacity 320ms ease; pointer-events: none;
    }
    #stage-card.on { opacity: 1; }
    #stage-card .title { font-size: 64px; font-weight: 800; letter-spacing: -0.03em; }
    #stage-card .line { font-size: 24px; color: #666; }
    #stage-card .line.mono { font-family: ui-monospace, monospace; color: #111; }
  `;

  const build = (): void => {
    const style = document.createElement("style");
    style.textContent = css;
    document.head.appendChild(style);

    const layer = document.createElement("div");
    layer.id = "stage-layer";
    layer.innerHTML =
      '<div id="stage-cursor"></div><div id="stage-tap"></div><div id="stage-caption"></div><div id="stage-card"></div>';
    document.body.appendChild(layer);

    const cursor = document.getElementById("stage-cursor") as HTMLElement;
    const tap = document.getElementById("stage-tap") as HTMLElement;
    const caption = document.getElementById("stage-caption") as HTMLElement;
    const card = document.getElementById("stage-card") as HTMLElement;

    window.addEventListener(
      "mousemove",
      (event) => {
        cursor.classList.add("on");
        cursor.style.left = `${event.clientX}px`;
        cursor.style.top = `${event.clientY}px`;
      },
      true
    );
    window.addEventListener(
      "mousedown",
      (event) => {
        tap.style.left = `${event.clientX}px`;
        tap.style.top = `${event.clientY}px`;
        tap.classList.remove("go");
        void tap.offsetWidth;
        tap.classList.add("go");
      },
      true
    );

    const paint = (next: Card): void => {
      const lines = next.lines
        .map((line) => `<div class="line${line.startsWith("$") ? " mono" : ""}">${line}</div>`)
        .join("");
      card.innerHTML = `<div class="title">${next.title}</div>${lines}`;
    };

    window.__stage = {
      caption: (text) => {
        caption.textContent = text;
        caption.classList.add("on");
      },
      hideCaption: () => caption.classList.remove("on"),
      card: (next) => {
        paint(next);
        card.classList.add("on");
      },
      hideCard: () => card.classList.remove("on"),
      quiet: (on) => layer.classList.toggle("quiet", on)
    };

    paint(intro);
    card.style.transition = "none";
    card.classList.add("on");
    requestAnimationFrame(() => (card.style.transition = ""));
  };

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", build);
  else build();
}

export class Stage {
  private readonly started = Date.now();
  private readonly skips: [number, number][] = [];
  private trim = 0;
  private taken = 0;

  constructor(
    private readonly page: Page,
    private readonly stills: string
  ) {
    mkdirSync(stills, { recursive: true });
  }

  private now(): number {
    return (Date.now() - this.started) / 1000;
  }

  mark(label: string): void {
    console.log(`  ${this.now().toFixed(1)}s  ${label}`);
  }

  beat(ms: number): Promise<void> {
    return this.page.waitForTimeout(ms);
  }

  async caption(text: string, settle = 0): Promise<void> {
    await this.page.evaluate((value) => window.__stage.caption(value), text);
    if (settle > 0) await this.beat(settle);
  }

  async hideCaption(): Promise<void> {
    await this.page.evaluate(() => window.__stage.hideCaption());
  }

  async card(card: Card, hold: number): Promise<void> {
    await this.page.evaluate((value) => window.__stage.card(value), card);
    await this.beat(hold);
  }

  async hideCard(): Promise<void> {
    await this.page.evaluate(() => window.__stage.hideCard());
    await this.beat(340);
  }

  /**
   * Runs a long wait (a download, the assistant at work) that the clip plays
   * sped up. The first and last second stay at normal speed.
   */
  async fast<T>(wait: () => Promise<T>): Promise<T> {
    const from = this.now();
    const result = await wait();
    const to = this.now();
    if (to - from > 3) this.skips.push([from + 1, to - 1]);
    return result;
  }

  /** Marks where the clip starts: anything before is the app starting up. */
  begin(): void {
    this.trim = this.now();
  }

  /**
   * Writes the cuts for scripts/record.sh: `trim <seconds>` first, then one
   * `<from> <to>` line per sped up stretch.
   */
  saveCuts(file: string): void {
    const lines = [`trim ${this.trim.toFixed(2)}`, ...this.skips.map(([from, to]) => `${from.toFixed(2)} ${to.toFixed(2)}`)];
    writeFileSync(file, `${lines.join("\n")}\n`);
  }

  /** A screenshot without the cursor and captions. */
  async still(name: string): Promise<void> {
    this.taken += 1;
    const file = `${String(this.taken).padStart(2, "0")}_${name}.png`;
    await this.page.evaluate(() => window.__stage.quiet(true));
    await this.page.screenshot({ path: `${this.stills}/${file}`, caret: "hide" });
    await this.page.evaluate(() => window.__stage.quiet(false));
    console.log(`  ${file}`);
  }

  private async point(target: Locator): Promise<{ x: number; y: number } | null> {
    await target.scrollIntoViewIfNeeded().catch(() => undefined);
    const box = await target.boundingBox();
    return box === null ? null : { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  }

  async hover(target: Locator, settle = 220): Promise<void> {
    const point = await this.point(target);
    if (point !== null) await this.page.mouse.move(point.x, point.y, { steps: 12 });
    await this.beat(settle);
  }

  async tap(target: Locator, settle = 300): Promise<void> {
    await this.hover(target, 80);
    await target.click();
    await this.beat(settle);
  }

  async type(text: string, delay = 45): Promise<void> {
    await this.page.keyboard.type(text, { delay });
  }

  async press(key: string, settle = 200): Promise<void> {
    await this.page.keyboard.press(key);
    await this.beat(settle);
  }
}
