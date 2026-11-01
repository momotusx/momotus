/** Back from a title lands where the viewer was (QA r1, Q4 rows 7, 42-44): Home and Discover keep their scroll on the
 * phone (the document scrolls) and the desktop (the page scrolls), and the TV's restored focus is on screen. The phone's
 * pages scroll by wheel and touch at all (Q4 row 50). */

import { expect, type Page, test } from "./fixture";

const FORMS = {
  desktop: { viewport: { width: 1440, height: 900 } },
  phone: { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true },
} as const;

/** Where the page is scrolled to: the document on a phone, the page itself elsewhere. */
const scrollOf = (page: Page) =>
  page.evaluate(() => (document.documentElement.dataset.form === "phone" ? window.scrollY : (document.querySelector("main[data-vscroll]")?.scrollTop ?? 0)));
const scrollTo = (page: Page, top: number) =>
  page.evaluate((top) => {
    if (document.documentElement.dataset.form === "phone") window.scrollTo({ top, behavior: "instant" });
    else document.querySelector("main[data-vscroll]")?.scrollTo({ top, behavior: "instant" });
  }, top);

/** Holds every message from the core for `lag()` ms on its way to the page, in order: a slow network, so the page Back
 * lands on gets its rows a moment after it opens. */
async function slowCore(page: Page, lag: () => number) {
  await page.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    server.onMessage((m) => (lag() ? setTimeout(() => ws.send(m), lag()) : ws.send(m)));
  });
}

/** Opens the first title link wholly on screen. */
async function openVisibleTitle(page: Page) {
  const index = await page.locator('main a[href^="/title/"]').evaluateAll((links) =>
    links.findIndex((a) => {
      const r = a.getBoundingClientRect();
      return r.top > 120 && r.bottom < innerHeight - 120 && r.left >= 0 && r.right <= innerWidth;
    }),
  );
  expect(index).toBeGreaterThanOrEqual(0);
  await page.locator('main a[href^="/title/"]').nth(index).click();
  await expect(page).toHaveURL(/\/title\//);
}

for (const [form, options] of Object.entries(FORMS)) {
  for (const [path, at] of [
    ["/", 1200],
    ["/discover", 1500],
  ] as const) {
    test(`${form}: Back from a title lands on ${path} where it was`, async ({ browser }) => {
      const context = await browser.newContext({ ...options, reducedMotion: "reduce" });
      const page = await context.newPage();
      let lag = 0;
      await slowCore(page, () => lag);
      await page.goto(`${path}?form=${form}`);
      await expect(page.locator('main a[href^="/title/"]').nth(12)).toBeAttached({ timeout: 30_000 });
      await expect.poll(async () => (await scrollTo(page, at), scrollOf(page))).toBeGreaterThanOrEqual(at - 1);
      await page.waitForTimeout(300);
      const before = await scrollOf(page);
      await openVisibleTitle(page);
      await expect(page.locator(".info .meta")).toBeVisible({ timeout: 30_000 });
      lag = 800;
      await page.goBack();
      await expect(page).toHaveURL(new RegExp(`${path === "/" ? "/\\?" : path}`));
      await expect.poll(() => scrollOf(page)).toBeGreaterThan(before - 2);
      // Still there once the browser's own restore and the late rows have had their turn.
      await page.waitForTimeout(1000);
      expect(Math.abs((await scrollOf(page)) - before)).toBeLessThan(2);
      await context.close();
    });
  }
}

test("tv: Back from a Discover title puts the focused tile back on screen", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  let lag = 0;
  await slowCore(page, () => lag);
  await page.goto("/discover?form=tv");
  await expect(page.locator('main a[href^="/title/"]').nth(30)).toBeAttached({ timeout: 30_000 });
  const focused = () => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");
  await expect.poll(focused).not.toBe("");
  for (let i = 0; i < 12 && !(await focused()).startsWith("g:"); i++) await page.keyboard.press("ArrowDown");
  for (let i = 0; i < 4; i++) await page.keyboard.press("ArrowDown");
  await page.waitForTimeout(500);
  const tile = await focused();
  expect(tile).toMatch(/^g:/);
  expect(await scrollOf(page)).toBeGreaterThan(500);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/title\//);
  await expect(page.locator(".info .meta")).toBeVisible({ timeout: 30_000 });
  lag = 800;
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/discover/);
  await expect.poll(focused).toBe(tile);
  await expect
    .poll(() =>
      page.evaluate(() => {
        const r = (document.activeElement as HTMLElement).getBoundingClientRect();
        return r.top >= 0 && r.bottom <= innerHeight;
      }),
    )
    .toBe(true);
  await context.close();
});

test("phone: the wheel and a swipe scroll Home and Discover", async ({ browser }) => {
  const context = await browser.newContext({ ...FORMS.phone });
  const page = await context.newPage();
  for (const path of ["/", "/discover"]) {
    await page.goto(`${path}?form=phone`);
    await expect(page.locator("main[data-vscroll]")).toBeVisible();
    await expect(page.locator('main a[href^="/title/"]').first()).toBeVisible({ timeout: 30_000 });
    // Content taller than the screen, or there is nothing to scroll.
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollHeight > innerHeight + 300)).toBe(true);
    await page.evaluate(() => window.scrollTo({ top: 0, behavior: "instant" }));
    await page.mouse.move(195, 500);
    await page.mouse.wheel(0, 300);
    await expect.poll(() => page.evaluate(() => window.scrollY), { message: `${path}: wheel` }).toBeGreaterThan(100);
    await page.evaluate(() => window.scrollTo({ top: 0, behavior: "instant" }));
    const cdp = await context.newCDPSession(page);
    await cdp.send("Input.synthesizeScrollGesture", { x: 195, y: 600, yDistance: -300, speed: 1200, gestureSourceType: "touch" });
    await expect.poll(() => page.evaluate(() => window.scrollY), { message: `${path}: swipe` }).toBeGreaterThan(100);
    await cdp.detach();
  }
  await context.close();
});
