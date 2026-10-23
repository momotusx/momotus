/** Versions with 400 rows in one open section, on TV (plans/versions-web-spec.md 3.5): opening it,
 * a held Down through it, and the rows' offsets after a scroll to the end and back. A paint skip on
 * the rows (`content-visibility`) was measured against none on 2026-10-23 and lost (both runs in
 * bench/versions-ux/web-perf.jsonl). Headless Chromium renders in software, so the printed numbers
 * are the ones to compare. PERF_BENCH=1 keeps them in that file. */

import { appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, type Page, test } from "@playwright/test";
import { GOT, PORTS } from "./core";
import { type Json, patchedContext } from "./patch";

const BENCH = fileURLToPath(new URL("../../../bench/versions-ux/web-perf.jsonl", import.meta.url));
const ROWS = 400;

const pct = (xs: number[], p: number) => {
  const s = [...xs].sort((a, b) => a - b);
  return Math.round((s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))] ?? 0) * 10) / 10;
};

/** The core's list with ROWS copies of its first row in its 1080p section. */
const many = (view: string, s: Json) => {
  const first = s?.rows?.[0] ?? s?.recommended;
  if (view !== "versions" || !first || !s.done) return s;
  const rows = Array.from({ length: ROWS }, (_, i) => ({ ...first, key: `${first.key}#p${i}`, group: "best" }));
  return { ...s, rows, sections: [{ quality: "fhd", keys: rows.map((r: Json) => r.key) }] };
};

const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");

test("TV: 400 rows in one open section open at once, scroll at frame rate and stay where they are", async ({ browser }) => {
  const { context } = await patchedContext(browser, PORTS.bridge, { viewport: { width: 1920, height: 1080 } }, many);
  const page = await context.newPage();
  await page.goto(`/title/series/${GOT.id}/versions/${GOT.resume}?form=tv`);
  const fold = page.locator('[data-f^="fold:"]').first();
  await expect(fold).toBeVisible({ timeout: 60_000 });
  // Opening the section: Enter on Show all to its 4th row focused and painted.
  await fold.focus();
  const openMs = await page.evaluate(
    () =>
      new Promise<number>((done) => {
        const t0 = performance.now();
        (document.activeElement as HTMLElement | null)?.click();
        const wait = () => {
          const f = (document.activeElement as HTMLElement | null)?.dataset.f ?? "";
          if (f.startsWith("v:")) requestAnimationFrame(() => done(performance.now() - t0));
          else requestAnimationFrame(wait);
        };
        wait();
      }),
  );
  expect(await focused(page)).toMatch(/^v:/);
  const nodes = await page.locator(".vslot").evaluateAll((slots) => Math.max(...slots.map((s) => s.querySelectorAll("*").length + 1)));
  await page.evaluate(() => {
    const w = window as unknown as { frames: number[]; lat: number[]; rec: boolean };
    w.frames = [];
    w.lat = [];
    w.rec = true;
    let last = performance.now();
    const tick = (t: number) => {
      w.frames.push(t - last);
      last = t;
      if (w.rec) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    document.addEventListener(
      "keydown",
      () => {
        const t0 = performance.now();
        requestAnimationFrame(() => w.lat.push(performance.now() - t0));
      },
      true,
    );
  });
  // A held Down, about eight presses a second.
  for (let i = 0; i < 60; i++) {
    await page.keyboard.press("ArrowDown");
    await page.waitForTimeout(120);
  }
  await page.waitForTimeout(500);
  const { frames, lat } = await page.evaluate(() => {
    const w = window as unknown as { frames: number[]; lat: number[]; rec: boolean };
    w.rec = false;
    return { frames: w.frames.slice(1), lat: w.lat };
  });
  // To the end and back: every sampled row where it was.
  const offsets = () => page.locator(".vslot").evaluateAll((s) => [0, 50, 150, 300, 399].map((i) => (s[i] as HTMLElement | undefined)?.offsetTop ?? -1));
  const before = await offsets();
  await page.evaluate(() => {
    const list = document.querySelector<HTMLElement>(".alist");
    if (list) list.scrollTop = list.scrollHeight;
  });
  await page.waitForTimeout(300);
  await page.evaluate(() => {
    const list = document.querySelector<HTMLElement>(".alist");
    if (list) list.scrollTop = 0;
  });
  await page.waitForTimeout(300);
  const after = await offsets();
  const result = {
    rows: ROWS,
    openMs: Math.round(openMs * 10) / 10,
    nodesPerRow: nodes,
    frames: frames.length,
    frameMs: { p50: pct(frames, 50), p95: pct(frames, 95), p99: pct(frames, 99) },
    over33ms: frames.filter((f) => f > 33.4).length,
    keyToFrameMs: { p50: pct(lat, 50), p95: pct(lat, 95), max: pct(lat, 100) },
    offsetsKept: JSON.stringify(before) === JSON.stringify(after),
  };
  console.log(`versions-perf ${JSON.stringify(result)}`);
  if (process.env.PERF_BENCH)
    appendFileSync(BENCH, `${JSON.stringify({ date: new Date().toISOString(), check: "web-versions-400-rows", paintSkip: false, browser: browser.version(), canvas: "tv 1920x1080", ...result })}\n`);
  expect(result.offsetsKept).toBe(true);
  expect(result.nodesPerRow).toBeLessThanOrEqual(12);
  // About 1.5 times the worst of three runs on 2026-10-23 (open 42.8 ms, frame p95 16.8 ms, key to frame
  // p95 15.9 ms); one long frame is allowed in a 7 s run.
  expect(result.openMs).toBeLessThan(65);
  expect(result.frameMs.p95).toBeLessThan(25);
  expect(result.keyToFrameMs.p95).toBeLessThan(24);
  expect(result.over33ms).toBeLessThanOrEqual(1);
  await context.close();
});
