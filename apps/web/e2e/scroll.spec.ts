/** How smoothly Home and Discover scroll: the wheel and a fling on desktop
 * (1440x900), a held arrow key on TV (1920x1080), down the page and along a
 * row. Each scenario samples the scroll position every frame and records a
 * Chromium trace: the compositor's draw per frame and its render passes (each
 * live blur is one), raster, and the main thread's style, layout and paint.
 * Headless Chromium draws in software, so rAF deltas stay at 16.7 ms even
 * when a real GPU would drop frames; the draw cost and the position steps are
 * the numbers to read. A hitch is a frame where a scroll that is still going
 * nearly stops (its speed, judged by the frames' own timestamps, falls below a
 * third of the frame before's, then rises again), as a smooth scroll restarted
 * from rest does on every key press. Frames next to one longer than 50 ms,
 * and frames after a press that came more than 200 ms after the one before
 * (a loaded machine), are not judged: the glide may rightly ease out then.
 * SCROLL_BENCH=1 keeps the numbers in bench/web/scroll.jsonl, SCROLL_LABEL
 * names the run. */

import { appendFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { type Browser, expect, type Page, test } from "@playwright/test";

const BENCH = fileURLToPath(new URL("../../../bench/web", import.meta.url));

const pct = (xs: number[], p: number) => {
  const s = [...xs].sort((a, b) => a - b);
  return Math.round((s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))] ?? 0) * 10) / 10;
};

/** Frames where a scroll still under way nearly stops, then picks up again. `dts[i]` is the time
 * from sample i - 1 to sample i; `late[i]`, the last press before sample i came late. Speeds are
 * px per ms; 0.5 is 8 px a 60 Hz frame. */
function hitches(positions: number[], dts: number[], late: boolean[]): number {
  const v: (number | null)[] = positions.map((p, i) => {
    const dt = dts[i] ?? 0;
    return i === 0 || dt <= 0 || dt > 50 ? null : (p - (positions[i - 1] as number)) / dt;
  });
  let n = 0;
  for (let i = 2; i < v.length - 1; i++) {
    const [before, now, after] = [v[i - 1], v[i], v[i + 1]];
    if (before == null || now == null || after == null || late[i] || late[i + 1]) continue;
    if (Math.abs(before) >= 0.5 && Math.abs(now) < Math.abs(before) / 3 && Math.sign(after) === Math.sign(before) && Math.abs(after) > Math.abs(now)) n++;
  }
  return n;
}

type TraceEvent = { name: string; ph: string; dur?: number; ts: number; pid: number; tid: number; args?: { name?: string } };

function costs(events: TraceEvent[]) {
  const threads = new Map<string, string>();
  for (const e of events) if (e.ph === "M" && e.name === "thread_name") threads.set(`${e.pid}:${e.tid}`, e.args?.name ?? "");
  const ms = (xs: TraceEvent[]) => Math.round(xs.reduce((t, e) => t + (e.dur ?? 0), 0) / 100) / 10;
  const main = events.filter((e) => e.ph === "X" && threads.get(`${e.pid}:${e.tid}`) === "CrRendererMain");
  const draws = events.filter((e) => e.ph === "X" && e.name === "DirectRenderer::DrawFrame").map((e) => (e.dur ?? 0) / 1000);
  const passes = events.filter((e) => e.ph === "X" && e.name === "DirectRenderer::DrawRenderPass").length;
  return {
    drawMs: { p50: pct(draws, 50), p95: pct(draws, 95), max: pct(draws, 100) },
    passesPerFrame: Math.round((passes / Math.max(1, draws.length)) * 10) / 10,
    rasterMs: ms(events.filter((e) => e.ph === "X" && e.name === "RasterTask")),
    styleMs: ms(main.filter((e) => e.name === "UpdateLayoutTree")),
    layoutMs: ms(main.filter((e) => e.name === "Layout")),
    paintMs: ms(main.filter((e) => e.name === "Paint" || e.name === "PrePaint" || e.name === "Layerize")),
    longTasks: main.filter((e) => e.name === "RunTask" && (e.dur ?? 0) > 50_000).length,
  };
}

type Rec = { frames: number[]; tops: number[]; lefts: number[]; late: boolean[]; keys: number[]; on: boolean };

// Headless wheel events jump the page with no animation, so steps (hitches) are read for keys only.
async function measure(page: Page, browser: Browser, drive: () => Promise<void>, keys = false) {
  await page.evaluate(() => {
    const w = window as unknown as { rec: Rec; listening?: boolean };
    w.rec = { frames: [], tops: [], lefts: [], late: [], keys: [], on: true };
    if (!w.listening) document.addEventListener("keydown", () => w.rec.keys.push(performance.now()), true);
    w.listening = true;
    let last = performance.now();
    const tick = (t: number) => {
      w.rec.frames.push(t - last);
      last = t;
      w.rec.tops.push(document.querySelector("[data-vscroll]")?.scrollTop ?? 0);
      w.rec.lefts.push(document.activeElement?.closest("[data-hscroll]")?.scrollLeft ?? 0);
      const k = w.rec.keys;
      w.rec.late.push(k.length > 1 && (k.at(-1) as number) - (k.at(-2) as number) > 200);
      if (w.rec.on) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await browser.startTracing(page, { categories: ["devtools.timeline", "disabled-by-default-devtools.timeline", "cc", "viz", "toplevel"] });
  await drive();
  await page.waitForTimeout(600);
  const trace = JSON.parse((await browser.stopTracing()).toString()) as { traceEvents: TraceEvent[] };
  const rec = await page.evaluate(() => {
    const w = window as unknown as { rec: Rec };
    w.rec.on = false;
    return w.rec;
  });
  const frames = rec.frames.slice(1);
  return {
    frames: frames.length,
    frameMs: { p50: pct(frames, 50), p95: pct(frames, 95), p99: pct(frames, 99) },
    over16ms: frames.filter((f) => f > 17.7).length,
    over33ms: frames.filter((f) => f > 33.4).length,
    ...(keys ? { hitches: hitches(rec.tops, rec.frames, rec.late) + hitches(rec.lefts, rec.frames, rec.late) } : {}),
    ...costs(trace.traceEvents),
  };
}

/** Wheel notches down the page and back up, about one every 50 ms, as a mouse wheel turns. */
async function wheel(page: Page) {
  await page.mouse.move(720, 600);
  for (const dir of [1, -1]) {
    for (let i = 0; i < 24; i++) {
      await page.mouse.wheel(0, dir * 120);
      await page.waitForTimeout(50);
    }
  }
}

/** A trackpad fling: a burst of wheel deltas decaying from 160 px a frame, down then up. */
async function fling(page: Page) {
  await page.mouse.move(720, 600);
  for (const dir of [1, -1]) {
    for (let v = 160; v > 4; v *= 0.93) {
      await page.mouse.wheel(0, dir * v);
      await page.waitForTimeout(16);
    }
  }
}

/** A held remote button: about eight presses a second. */
async function hold(page: Page, key: string, n: number) {
  for (let i = 0; i < n; i++) {
    await page.keyboard.press(key);
    await page.waitForTimeout(120);
  }
}

const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");

function record(scenario: string, canvas: string, browser: Browser, result: object) {
  console.log(`scroll ${scenario} ${JSON.stringify(result)}`);
  test.info().annotations.push({ type: "perf", description: `${scenario} ${JSON.stringify(result)}` });
  if (!process.env.SCROLL_BENCH) return;
  mkdirSync(BENCH, { recursive: true });
  const line = { date: new Date().toISOString(), check: `web-scroll-${scenario}`, label: process.env.SCROLL_LABEL ?? null, browser: browser.version(), canvas, ...result };
  appendFileSync(`${BENCH}/scroll.jsonl`, `${JSON.stringify(line)}\n`);
}

test.describe("desktop", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("Home and Discover scroll under the wheel and a fling", async ({ page, browser }) => {
    const canvas = "desktop 1440x900";
    await page.goto("/?form=desktop");
    await expect(page.locator(".row .tile img.in").first()).toBeVisible();
    await page.waitForTimeout(1500);
    const home = await measure(page, browser, () => wheel(page));
    record("desktop-home-wheel", canvas, browser, home);
    const flung = await measure(page, browser, () => fling(page));
    record("desktop-home-fling", canvas, browser, flung);

    await page.goto("/discover?form=desktop");
    await expect(page.locator(".grid .tile img.in").first()).toBeVisible();
    await page.waitForTimeout(1500);
    const grid = await measure(page, browser, () => wheel(page));
    record("desktop-discover-wheel", canvas, browser, grid);

    // Loose: software rendering on a loaded machine. Long tasks are reported, not asserted.
    for (const r of [home, flung, grid]) expect(r.frameMs.p95).toBeLessThan(34);
  });
});

test.describe("tv", () => {
  test.use({ viewport: { width: 1920, height: 1080 } });

  test("Home and Discover glide under a held arrow key", async ({ page, browser }) => {
    const canvas = "tv 1920x1080";
    await page.goto("/?form=tv");
    await expect.poll(() => focused(page)).toMatch(/^(tile:|hero:)/);
    await page.waitForTimeout(1500);
    const down = await measure(page, browser, async () => {
      await hold(page, "ArrowDown", 8);
      await hold(page, "ArrowUp", 8);
    }, true);
    record("tv-home-keys", canvas, browser, down);
    // The first catalog row: long enough to scroll sideways.
    await page.locator('.row .tile[data-f^="tile:home:"]:not([data-f*="continue-watching"])').first().focus();
    await page.waitForTimeout(800);
    const along = await measure(page, browser, async () => {
      await hold(page, "ArrowRight", 12);
      await hold(page, "ArrowLeft", 12);
    }, true);
    record("tv-home-row-keys", canvas, browser, along);

    await page.goto("/discover?form=tv");
    await expect(page.locator(".grid .tile img.in").first()).toBeVisible();
    await page.locator(".grid .tile").first().focus();
    await page.waitForTimeout(1500);
    const grid = await measure(page, browser, async () => {
      await hold(page, "ArrowDown", 8);
      await hold(page, "ArrowUp", 8);
    }, true);
    record("tv-discover-keys", canvas, browser, grid);

    // A native smooth scroll restarts from rest on every press: one hitch a press.
    for (const r of [down, along, grid]) expect(r.hitches).toBe(0);
  });
});

test.describe("phone", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("a keyboard along a snapping row brings each focused poster into view", async ({ page }) => {
    await page.goto("/?form=phone");
    const first = page.locator('.row .tile[data-f^="tile:home:"]:not([data-f*="continue-watching"])').first();
    await expect(first).toBeVisible();
    await first.focus();
    await hold(page, "ArrowRight", 6);
    await page.waitForTimeout(800);
    const seen = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement;
      const track = el.closest("[data-hscroll]") as HTMLElement;
      const [r, t] = [el.getBoundingClientRect(), track.getBoundingClientRect()];
      return { left: track.scrollLeft, inside: r.left >= t.left - 1 && r.right <= t.right + 1 };
    });
    expect(seen.left).toBeGreaterThan(0);
    expect(seen.inside).toBe(true);
  });
});
