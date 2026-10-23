/** Home's paint guard for design v3 phase 2 (plans/design-v3-phase2-web.md, rule 6), until
 * fix/scroll-web's scroll.spec.ts lands and takes it in. A Chromium trace around a desktop
 * wheel run and a TV Play to Continue Watching round trip: the main thread's paint (Paint,
 * PrePaint, Layerize) and the compositor's render passes per frame. The desktop hover lift
 * that repainted whole rows (phase 1b) cost 178 to 288 ms of paint per wheel run; without it,
 * about 20 (baseline 25 to 29 with phase 1b on a543d87d). HOME_PERF_BENCH=1 prints the rows for bench/design-v3/perf.jsonl. */

import { type Browser, expect, type Page, test } from "@playwright/test";

type TraceEvent = { name: string; ph: string; dur?: number; pid: number; tid: number; args?: { name?: string } };

function costs(events: TraceEvent[]) {
  const threads = new Map<string, string>();
  for (const e of events) if (e.ph === "M" && e.name === "thread_name") threads.set(`${e.pid}:${e.tid}`, e.args?.name ?? "");
  const main = events.filter((e) => e.ph === "X" && threads.get(`${e.pid}:${e.tid}`) === "CrRendererMain");
  const ms = (xs: TraceEvent[]) => Math.round(xs.reduce((t, e) => t + (e.dur ?? 0), 0) / 100) / 10;
  const draws = events.filter((e) => e.ph === "X" && e.name === "DirectRenderer::DrawFrame").length;
  const passes = events.filter((e) => e.ph === "X" && e.name === "DirectRenderer::DrawRenderPass").length;
  return {
    paintMs: ms(main.filter((e) => e.name === "Paint" || e.name === "PrePaint" || e.name === "Layerize")),
    styleMs: ms(main.filter((e) => e.name === "UpdateLayoutTree")),
    passesPerFrame: Math.round((passes / Math.max(1, draws)) * 10) / 10,
  };
}

async function traced(page: Page, browser: Browser, drive: () => Promise<void>) {
  await browser.startTracing(page, { categories: ["devtools.timeline", "disabled-by-default-devtools.timeline", "viz"] });
  await drive();
  await page.waitForTimeout(600);
  return costs((JSON.parse((await browser.stopTracing()).toString()) as { traceEvents: TraceEvent[] }).traceEvents);
}

const report = (scenario: string, result: object) => {
  if (process.env.HOME_PERF_BENCH) console.log(`home-perf ${scenario} ${JSON.stringify(result)}`);
};

test("desktop: a wheel run over Home paints little", async ({ browser }) => {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.goto("/?form=desktop");
  await expect(page.locator(".row .tile img.in").first()).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(1500);
  // The pointer over the shelves, so tiles slide under it, as scroll.spec's wheel does.
  const result = await traced(page, browser, async () => {
    await page.mouse.move(720, 600);
    for (const dir of [1, -1]) {
      for (let i = 0; i < 24; i++) {
        await page.mouse.wheel(0, dir * 120);
        await page.waitForTimeout(50);
      }
    }
  });
  report("desktop-home-wheel", result);
  expect(result.paintMs, JSON.stringify(result)).toBeLessThan(60);
  // No live blur over the shelves: baseline 1.0 since phase 2 step 2 (1.4 to 1.7 before, while the hero's
  // secondaries blurred); a live blur adds a pass to every frame.
  expect(result.passesPerFrame, JSON.stringify(result)).toBeLessThanOrEqual(1.5);
  await page.close();
});

test("desktop: a trackpad fling over Home paints little", async ({ browser }) => {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.goto("/?form=desktop");
  await expect(page.locator(".row .tile img.in").first()).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(1500);
  // Wheel deltas decaying from 160 px a frame, down then up, the pointer over the shelves.
  const result = await traced(page, browser, async () => {
    await page.mouse.move(720, 600);
    for (const dir of [1, -1]) {
      for (let v = 160; v > 4; v *= 0.93) {
        await page.mouse.wheel(0, dir * v);
        await page.waitForTimeout(16);
      }
    }
  });
  report("desktop-home-fling", result);
  expect(result.paintMs, JSON.stringify(result)).toBeLessThan(60);
  expect(result.passesPerFrame, JSON.stringify(result)).toBeLessThanOrEqual(1.5);
  await page.close();
});

test("TV: Play to Continue Watching and back paints little", async ({ browser }) => {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  await page.goto("/?form=tv");
  const play = page.locator('[data-f="hero:play"]');
  await expect(play).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".row .tile img.in").first()).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(1500);
  await play.focus();
  const result = await traced(page, browser, async () => {
    for (let i = 0; i < 3; i++) {
      await page.keyboard.press("ArrowDown");
      await page.waitForTimeout(500);
      await page.keyboard.press("ArrowUp");
      await page.waitForTimeout(500);
    }
  });
  report("tv-home-play-row", result);
  // The tile's lift shadow transitions over the hero's ramp while the page moves (about 700 px each way).
  expect(result.paintMs, JSON.stringify(result)).toBeLessThan(80);
  // Baseline 58 to 63 ms and 1.0 pass a frame since phase 2 step 2 (the hero's secondaries lost their
  // 20 px blur: 2.9 passes before); a live blur over the art adds a pass to every frame.
  expect(result.passesPerFrame, JSON.stringify(result)).toBeLessThanOrEqual(1.5);
  // Held keys glide only with fix/scroll-web's dom.ts; until it lands this branch scrolls with the
  // browser's smooth scroll, so its hitches are not asserted here.
  test.info().annotations.push({ type: "skipped assert", description: "TV hitches: glide lands with fix/scroll-web" });
  await page.close();
});

test("phone: a fling down Home and back paints little", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
  const page = await context.newPage();
  await page.goto("/?form=phone");
  await expect(page.locator(".row .tile img.in").first()).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(1500);
  // The phone scrolls the document. Headless Chromium moves it neither by wheel nor by a synthesized touch
  // gesture in a mobile context, so the browser's own smooth scroll stands in for the fling: 1600 down, back up.
  const glide = (top: number) =>
    page.evaluate(
      (top) =>
        new Promise<number>((done) => {
          window.scrollBy({ top, behavior: "smooth" });
          setTimeout(() => done(window.scrollY), 900);
        }),
      top,
    );
  let scrolled = 0;
  const result = await traced(page, browser, async () => {
    scrolled = await glide(1600);
    await glide(-1600);
  });
  report("phone-home-fling", { ...result, scrolled });
  expect(scrolled).toBeGreaterThan(400);
  // Baseline 3 to 3.5 ms of paint and 1.8 to 1.9 passes (phase 2 step 4).
  expect(result.paintMs, JSON.stringify(result)).toBeLessThan(12);
  // The floating tab bar's glass is the one live blur, kept (cheap: 3 ms): 1.8 to 1.9 passes a frame.
  expect(result.passesPerFrame, JSON.stringify(result)).toBeLessThanOrEqual(2.5);
  await context.close();
});

// The Title page (plans/design-v3-phase34-web.md, rule 6): Game of Thrones, a series with episodes.
const TITLE = "/title/series/tt0944947";

test("TV: the title's episodes walked by the remote and back paint little", async ({ browser }) => {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  await page.goto(`${TITLE}?form=tv`);
  const play = page.locator('[data-f="title:play"]');
  await expect(play).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-f^="ep:"]').first()).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(1500);
  await page.locator('[data-f^="ep:"]').first().focus();
  const result = await traced(page, browser, async () => {
    for (const key of ["ArrowDown", "ArrowDown", "ArrowDown", "ArrowUp", "ArrowUp", "ArrowUp"]) {
      await page.keyboard.press(key);
      await page.waitForTimeout(400);
    }
  });
  report("tv-title-episodes", result);
  // Step 0 baseline (three runs, before phase 3/4): bounds at 1.5x the worst run and the passes plus 0.5;
  // they tighten once step 1 takes the title's live blurs and masks away.
  expect(result.paintMs, JSON.stringify(result)).toBeLessThan(36);
  expect(result.passesPerFrame, JSON.stringify(result)).toBeLessThanOrEqual(5.5);
  await page.close();
});

test("desktop: a wheel run over the title page paints little", async ({ browser }) => {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.goto(`${TITLE}?form=desktop`);
  await expect(page.locator('[data-f^="ep:"]').first()).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(1500);
  // The pointer over the episodes, so a hover's transitioned shadow would show.
  const ep = await page.locator('[data-f^="ep:"]').first().boundingBox();
  const result = await traced(page, browser, async () => {
    await page.mouse.move((ep?.x ?? 1000) + 40, (ep?.y ?? 400) + 20);
    for (const dir of [1, -1]) {
      for (let i = 0; i < 16; i++) {
        await page.mouse.wheel(0, dir * 120);
        await page.waitForTimeout(50);
      }
    }
  });
  report("desktop-title-wheel", result);
  // Step 0 baseline (three runs, before phase 3/4): bounds at 1.5x the worst run and the passes plus 0.5;
  // they tighten once step 1 takes the title's live blurs and masks away.
  expect(result.paintMs, JSON.stringify(result)).toBeLessThan(29);
  expect(result.passesPerFrame, JSON.stringify(result)).toBeLessThanOrEqual(5);
  await page.close();
});

test("phone: a glide down the title page and back paints little", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
  const page = await context.newPage();
  await page.goto(`${TITLE}?form=phone`);
  await expect(page.locator('[data-f^="ep:"]').first()).toBeAttached({ timeout: 30_000 });
  await page.waitForTimeout(1500);
  const glide = (top: number) =>
    page.evaluate(
      (top) =>
        new Promise<number>((done) => {
          window.scrollBy({ top, behavior: "smooth" });
          setTimeout(() => done(window.scrollY), 900);
        }),
      top,
    );
  let scrolled = 0;
  const result = await traced(page, browser, async () => {
    scrolled = await glide(1200);
    await glide(-1200);
  });
  report("phone-title-glide", { ...result, scrolled });
  expect(scrolled).toBeGreaterThan(400);
  // Step 0 baseline (three runs, before phase 3/4): bounds at 1.5x the worst run and the passes plus 0.5;
  // they tighten once step 1 takes the title's live blurs and masks away.
  expect(result.paintMs, JSON.stringify(result)).toBeLessThan(7);
  expect(result.passesPerFrame, JSON.stringify(result)).toBeLessThanOrEqual(4.2);
  await context.close();
});
