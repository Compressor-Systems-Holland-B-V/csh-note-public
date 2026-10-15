import { chromium } from "@playwright/test";
const b = await chromium.launch();
const p = await b.newPage();
p.on("console", m => console.log("console:", m.type(), m.text()));
p.on("pageerror", e => console.log("pageerror:", e.stack));
await p.goto("http://127.0.0.1:1430/");
await p.waitForTimeout(8000);
await b.close();
