/** A double tap on a phone presses one thing (QA r1, Q1 row 30, Q6 row 39): when the first tap closes a sheet or
 * changes the page, the second, a moment later at the same spot, presses nothing on what is now under the finger. A
 * tap after the moment works as ever. */

import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { expect, type Page, test } from "./fixture";

const PHONE = { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true } as const;

/** Clicks that reached the page, counted where every control's own handler would see them. */
const countClicks = (page: Page) =>
  page.evaluate(() => {
    const w = window as unknown as { clicks: number };
    w.clicks = 0;
    document.addEventListener("click", () => w.clicks++);
  });
const clicks = (page: Page) => page.evaluate(() => (window as unknown as { clicks: number }).clicks);

async function centre(page: Page, selector: string) {
  const b = await page.locator(selector).boundingBox();
  if (!b) throw new Error(`${selector} is not on screen`);
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
}

test("phone: the second tap of a double tap on a sheet's button presses nothing under it", async ({ browser }) => {
  const server = createServer((req, res) => {
    if (req.url !== "/manifest.json") return void res.writeHead(404).end();
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ id: "dev.marquee.ghost", version: "1.0.0", name: "Ghost Test", resources: ["subtitles"], types: ["movie"], catalogs: [] }));
  });
  await new Promise<void>((ok) => server.listen(0, "127.0.0.1", ok));
  const context = await browser.newContext(PHONE);
  const page = await context.newPage();
  try {
    await page.goto("/addons?form=phone");
    await page.locator('[data-f="install:url"]').fill(`http://127.0.0.1:${(server.address() as AddressInfo).port}/manifest.json`);
    await page.locator('[data-f="install:go"]').click();
    await expect(page.locator(".is-layer[data-armed]")).toHaveCount(1);
    const at = await centre(page, '[data-f="sheet:cancel"]');
    await countClicks(page);
    await page.touchscreen.tap(at.x, at.y);
    await page.waitForTimeout(150);
    await page.touchscreen.tap(at.x, at.y);
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await page.waitForTimeout(100);
    expect(await clicks(page)).toBe(1);
    // Later, a tap is a tap.
    await page.waitForTimeout(400);
    await page.touchscreen.tap(at.x, at.y);
    await expect.poll(() => clicks(page)).toBe(2);
  } finally {
    await context.close();
    server.close();
  }
});

test("phone: the second tap of a double tap on a link presses nothing on the next page", async ({ browser }) => {
  const context = await browser.newContext(PHONE);
  const page = await context.newPage();
  await page.goto("/settings?form=phone");
  const row = '[data-f="settings:profiles"]';
  await expect(page.locator(row)).toBeVisible();
  const at = await centre(page, row);
  await countClicks(page);
  await page.touchscreen.tap(at.x, at.y);
  await page.waitForTimeout(150);
  await page.touchscreen.tap(at.x, at.y);
  await expect(page).toHaveURL(/\/profiles/);
  await page.waitForTimeout(100);
  expect(await clicks(page)).toBe(1);
  await context.close();
});

test("phone: a double tap on Continue answers no question on the next step", async ({ browser }) => {
  const context = await browser.newContext(PHONE);
  const page = await context.newPage();
  const ops: string[] = [];
  // The addons step's install is answered here: the shared core keeps its addons.
  await page.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((raw) => {
      const m = JSON.parse(String(raw));
      ops.push(m.op);
      if (m.op === "installDefaults") ws.send(JSON.stringify({ id: m.id, ok: true, result: null }));
      else server.send(raw);
    });
  });
  await page.goto("/welcome?step=2&form=phone");
  const next = page.locator('[data-f="welcome:next"]');
  await expect(next).toHaveAttribute("aria-disabled", "false", { timeout: 30_000 });
  await next.scrollIntoViewIfNeeded();
  await page.waitForTimeout(300);
  const at = await centre(page, '[data-f="welcome:next"]');
  await page.touchscreen.tap(at.x, at.y);
  await page.waitForTimeout(150);
  await page.touchscreen.tap(at.x, at.y);
  await expect(page).toHaveURL(/step=choice/);
  await page.waitForTimeout(600);
  // The stream question is still asked: no door was pressed for the viewer.
  expect(ops).not.toContain("setPrefs");
  await expect(page).toHaveURL(/step=choice/);
  await context.close();
});
