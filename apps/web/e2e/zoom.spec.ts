/** Browser zoom and narrow windows on the desktop form (WCAG 1.4.4, 1.4.10): the page's text grows with the zoom
 * instead of cancelling it, the header never lets the tabs meet the Search field, and a touch screen sees the
 * episode actions without a hover. Playwright cannot press Ctrl and plus, so a zoom is a viewport of the unzoomed width
 * over the zoom with that device pixel ratio and an unzoomed `outerWidth`, which is what Chromium reports; a saved
 * per-host zoom is also loaded as the browser itself would, in a Chrome profile. */

import { chromium, expect, type Page, test } from "@playwright/test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BROWSER_ARGS, GOT, PORTS } from "./core";

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

test("desktop: the page's text grows with the browser zoom: 125 percent is 1.25 times, 200 percent at least 1.9", async ({ browser }) => {
  const base = await sizeAt(browser, 1);
  const at125 = await sizeAt(browser, 1.25);
  const at200 = await sizeAt(browser, 2);
  console.log(`zoom: synopsis ${base.toFixed(2)} px at 100, ${at125.toFixed(2)} at 125, ${at200.toFixed(2)} at 200`);
  expect(at125 / base).toBeGreaterThan(1.2);
  expect(at125 / base).toBeLessThan(1.3);
  expect(at200 / base).toBeGreaterThan(1.9);
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

test("a saved site zoom is seen from the first load: text grows 1.25 and 2 times in a Chrome profile", async () => {
  // Chrome's per-host zoom level is the zoom's logarithm to base 1.2 (the headless shell Playwright bundles ignores it).
  const level = (zoom: number) => Math.log(zoom) / Math.log(1.2);
  const measured: { zoom: number; dpr: number; inner: number; outer: number; form: string; size: number }[] = [];
  for (const zoom of [1, 1.25, 2]) {
    const dir = mkdtempSync(join(tmpdir(), "mq-zoom-"));
    try {
      mkdirSync(join(dir, "Default"), { recursive: true });
      writeFileSync(join(dir, "Default", "Preferences"), JSON.stringify({ partition: { per_host_zoom_levels: { x: { "127.0.0.1": { zoom_level: level(zoom) } } } } }));
      const context = await chromium.launchPersistentContext(dir, { channel: "chrome", headless: true, viewport: null, args: ["--window-size=1440,900", ...BROWSER_ARGS] }).catch(() => null);
      test.skip(!context, "Chrome is not installed here: the saved zoom is read by the real browser only");
      if (!context) return;
      const page = context.pages()[0] ?? (await context.newPage());
      await page.goto(`http://127.0.0.1:${PORTS.vite}/title/series/${GOT.id}`);
      await expect(page.locator(".info .syn")).toBeVisible({ timeout: 30_000 });
      const seen = await page.evaluate(() => ({ dpr: devicePixelRatio, inner: innerWidth, outer: outerWidth, form: document.documentElement.dataset.form ?? "" }));
      // The browser applied the zoom before anything is measured.
      expect(seen.dpr).toBeCloseTo(zoom, 2);
      measured.push({ zoom, ...seen, size: await syn(page) });
      await context.close();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
  console.log(`zoom profile: ${JSON.stringify(measured)}`);
  const [base, z125, z200] = measured.map((m) => m.size);
  expect((z125 ?? 0) / (base ?? 1)).toBeGreaterThan(1.2);
  expect((z125 ?? 0) / (base ?? 1)).toBeLessThan(1.3);
  expect((z200 ?? 0) / (base ?? 1)).toBeGreaterThan(1.9);
});
