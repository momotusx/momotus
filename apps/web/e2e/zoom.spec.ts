/** Browser zoom and narrow windows on the desktop form (WCAG 1.4.4, 1.4.10): the page's text grows with the zoom
 * instead of cancelling it, the header never lets the tabs meet the Search field, and a touch screen sees the
 * episode actions without a hover. Playwright cannot press Ctrl and plus, so a zoom is a viewport of the unzoomed width
 * over the zoom with that device pixel ratio and an unzoomed `outerWidth`, which is what Chromium reports; a saved
 * per-host zoom is also loaded as the browser itself would, in a Chrome profile. */

import { chromium, expect, type Page, test } from "./fixture";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BROWSER_ARGS, GOT, PORTS, ZOOM_HOSTS } from "./core";

/** The synopsis's size on screen: its font size in canvas units, times the canvas scale and the device pixel ratio. */
const syn = (page: Page) =>
  page.evaluate(() => {
    const el = document.querySelector<HTMLElement>(".info .syn");
    if (!el) return 0;
    const k = Number.parseFloat(document.documentElement.style.getPropertyValue("--k")) || 1;
    return Number.parseFloat(getComputedStyle(el).fontSize) * k * devicePixelRatio;
  });

async function sizeAt(browser: import("@playwright/test").Browser, zoom: number): Promise<number> {
  const context = await browser.newContext({ viewport: { width: Math.round(1440 / zoom), height: Math.round(900 / zoom) }, deviceScaleFactor: zoom, reducedMotion: "reduce" });
  await context.addInitScript(() => Object.defineProperty(window, "outerWidth", { get: () => 1440 }));
  const page = await context.newPage();
  await page.goto(`/title/series/${GOT.id}`);
  await expect(page.locator(".info .syn")).toBeVisible({ timeout: 30_000 });
  const size = await syn(page);
  await context.close();
  return size;
}

test("the page's text grows with the browser zoom: 1.25 and 1.75 on the desktop form, then the phone form from 200 percent", async ({ browser }) => {
  // 1440 CSS px at 200 percent is 720: the phone form, whose text is in CSS px and 1.6 times the desktop's at 100.
  const base = await sizeAt(browser, 1);
  const at125 = await sizeAt(browser, 1.25);
  const at175 = await sizeAt(browser, 1.75);
  const at200 = await sizeAt(browser, 2);
  const at250 = await sizeAt(browser, 2.5);
  console.log(`zoom: synopsis ${[base, at125, at175, at200, at250].map((n) => n.toFixed(2)).join(", ")} px at 100, 125, 175, 200, 250`);
  expect(at125 / base).toBeGreaterThan(1.2);
  expect(at125 / base).toBeLessThan(1.3);
  expect(at175 / base).toBeGreaterThan(1.65);
  expect(at175 / base).toBeLessThan(1.85);
  expect(at200 / base).toBeGreaterThanOrEqual(1.6);
  expect(at250 / base).toBeGreaterThanOrEqual(2);
});

test("desktop: tabs and the Search field never meet, at any width of the form", async ({ browser }) => {
  for (const width of [721, 768, 820, 900, 960, 961, 1024, 1180]) {
    const context = await browser.newContext({ viewport: { width, height: 800 }, reducedMotion: "reduce" });
    const page = await context.newPage();
    await page.goto("/?form=desktop");
    const field = page.locator(".nav-field");
    await expect(field).toBeVisible({ timeout: 30_000 });
    const tabs = await page.locator(".tabbar .nav").evaluateAll((els) => els.map((el) => el.getBoundingClientRect()).map((r) => ({ left: r.left, right: r.right })));
    const f = (await field.boundingBox()) as { x: number; width: number };
    const right = Math.max(...tabs.map((t) => t.right));
    expect(right, `tabs end at ${right}, Search starts at ${f.x} in a window of ${width}`).toBeLessThan(f.x);
    // The field keeps a name when it is only an icon, and is a real target.
    await expect(field).toHaveAccessibleName("Search");
    expect(f.width).toBeGreaterThanOrEqual(24);
    if (width <= 960) expect(f.width).toBeLessThan(40);
    await context.close();
  }
});

test("a touch screen in the desktop form sees each episode's actions without a hover, beside the card", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1024, height: 768 }, hasTouch: true, isMobile: true, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto(`/title/series/${GOT.id}?form=desktop`);
  const row = page.locator(".ep").first();
  await expect(row).toBeVisible({ timeout: 30_000 });
  const acts = row.locator(".ep-acts");
  await expect(acts).toBeVisible();
  // Beside the words, never over them.
  const [main, strip] = await Promise.all([row.locator(".ep-main").boundingBox(), acts.boundingBox()]);
  expect((main?.x ?? 0) + (main?.width ?? 0)).toBeLessThanOrEqual((strip?.x ?? 0) + 1);
  await context.close();
});

test("a saved site zoom is seen from the first load: text grows at 125, 175, 200 and 250 percent in a Chrome profile", async () => {
  // A real Chrome start is slow on a loaded machine (five in one test ran past its timeout under the full suite), so
  // one Chrome holds every zoom, each saved for its own host, and this test has three times the time.
  test.slow();
  // Chrome's per-host zoom level is the zoom's logarithm to base 1.2 (the headless shell Playwright bundles ignores it).
  const level = (zoom: number) => Math.log(zoom) / Math.log(1.2);
  const measured: { zoom: number; dpr: number; inner: number; outer: number; form: string; size: number }[] = [];
  const dir = mkdtempSync(join(tmpdir(), "mq-zoom-"));
  try {
    mkdirSync(join(dir, "Default"), { recursive: true });
    const saved = Object.fromEntries([...ZOOM_HOSTS].map(([zoom, host]) => [host, { zoom_level: level(zoom) }]));
    writeFileSync(join(dir, "Default", "Preferences"), JSON.stringify({ partition: { per_host_zoom_levels: { x: saved } } }));
    // Only a missing Chrome skips: a start that fails or hangs is a failure, within its own minute.
    const context = await chromium
      .launchPersistentContext(dir, { channel: "chrome", headless: true, viewport: null, timeout: 60_000, args: ["--window-size=1440,900", ...BROWSER_ARGS] })
      .catch((e: Error) => {
        if (/distribution 'chrome' is not found/.test(e.message)) return null;
        throw e;
      });
    test.skip(!context, "Chrome is not installed here: the saved zoom is read by the real browser only");
    if (!context) return;
    try {
      for (const [zoom, host] of ZOOM_HOSTS) {
        const page = await context.newPage();
        await page.goto(`http://${host}:${PORTS.vite}/title/series/${GOT.id}`);
        await expect(page.locator(".info .syn")).toBeVisible({ timeout: 30_000 });
        const seen = await page.evaluate(() => ({ dpr: devicePixelRatio, inner: innerWidth, outer: outerWidth, form: document.documentElement.dataset.form ?? "" }));
        // The browser applied the zoom before anything is measured.
        expect(seen.dpr).toBeCloseTo(zoom, 2);
        measured.push({ zoom, ...seen, size: await syn(page) });
        await page.close();
      }
    } finally {
      await context.close();
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  console.log(`zoom profile: ${JSON.stringify(measured)}`);
  const size = (zoom: number) => measured.find((m) => m.zoom === zoom)?.size ?? 0;
  expect(size(1.25) / size(1)).toBeGreaterThan(1.2);
  expect(size(1.25) / size(1)).toBeLessThan(1.3);
  expect(size(1.75) / size(1)).toBeGreaterThan(1.65);
  expect(size(1.75) / size(1)).toBeLessThan(1.85);
  expect(size(2) / size(1)).toBeGreaterThanOrEqual(1.6);
  expect(size(2.5) / size(1)).toBeGreaterThanOrEqual(2);
});

test("a narrow desktop canvas, by window or by zoom, keeps Resume, Start over and the actions out of the episode column", async ({ browser }) => {
  // 768 CSS px wide, and 1440 at 125 percent (1152 CSS px, a canvas of 1840 units): both are under 2000 units.
  for (const [at, viewport, zoom] of [["768 px", { width: 768, height: 900 }, 1], ["1440 px at 125 percent", { width: 1152, height: 720 }, 1.25]] as const) {
    const context = await browser.newContext({ viewport, deviceScaleFactor: zoom, reducedMotion: "reduce" });
    await context.addInitScript((outer) => Object.defineProperty(window, "outerWidth", { get: () => outer }), Math.round(viewport.width * zoom));
    const page = await context.newPage();
    await page.goto(`/title/series/${GOT.id}?form=desktop`);
    await expect(page.locator(".info .syn")).toBeVisible({ timeout: 30_000 });
    await expect(page.locator("html")).toHaveAttribute("data-narrow", "");
    // The shared core has this series part way in: Resume with its time, and Start over as its icon.
    const again = page.locator('[data-f="title:startover"]');
    await expect(again).toBeVisible();
    expect((await again.boundingBox())?.width, `${at}: Start over is a circle`).toBeLessThan(80);
    await expect(again).toHaveAccessibleName("Start over");
    const eps = (await page.locator(".eps").boundingBox())?.x ?? 0;
    for (const sel of [".play-row", ".info .acts"]) {
      const box = await page.locator(sel).boundingBox();
      expect((box?.x ?? 0) + (box?.width ?? 0), `${at}: ${sel} ends before the episode column`).toBeLessThanOrEqual(eps + 1);
    }
    await context.close();
  }
  // A wide canvas keeps its words.
  const wide = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
  const page = await wide.newPage();
  await page.goto(`/title/series/${GOT.id}?form=desktop`);
  await expect(page.locator('[data-f="title:startover"]')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator("html")).not.toHaveAttribute("data-narrow", "");
  await expect(page.locator('[data-f="title:startover"]')).toContainText("Start over");
  await wide.close();
});

test("a short desktop canvas, by window or by zoom, keeps the synopsis and the facts whole and scrolls to them", async ({ browser }) => {
  // 1440 by 465 CSS px is what Chrome leaves of a 1440 by 900 window at 175 percent: a canvas 741 units tall.
  const context = await browser.newContext({ viewport: { width: 1440, height: 465 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto(`/title/series/${GOT.id}?form=desktop`);
  const syn = page.locator(".info .syn");
  await expect(syn).toBeVisible({ timeout: 30_000 });
  // Three clamped lines, never squeezed: the synopsis is as tall as on a tall window.
  expect((await syn.boundingBox())?.height ?? 0).toBeGreaterThan(70);
  const facts = page.locator(".info .facts");
  await expect(facts).toBeVisible();
  await page.mouse.move(300, 300);
  await page.mouse.wheel(0, 2000);
  await expect(facts).toBeInViewport({ ratio: 0.95 });
  await context.close();
});
