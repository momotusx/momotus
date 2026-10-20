/** Frame times while a remote scrolls Home, and key press to next frame.
 * Headless Chromium renders in software, so the bounds are loose; the
 * printed percentiles are the numbers to watch. The perf gate sets
 * PERF_BENCH=1 to keep them in bench/m3/web-perf.jsonl. */

import { appendFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";

const BENCH = fileURLToPath(new URL("../../../bench/m3", import.meta.url));

test.use({ viewport: { width: 1920, height: 1080 } });

const pct = (xs: number[], p: number) => {
  const s = [...xs].sort((a, b) => a - b);
  return Math.round((s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))] ?? 0) * 10) / 10;
};

test("Home scrolls at frame rate under remote key repeat", async ({ page }) => {
  const started = Date.now();
  await page.goto("/?form=tv");
  await expect.poll(() => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "")).toMatch(/^tile:/);
  const firstFocusMs = Date.now() - started;

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

  // About eight presses a second, like a held remote button: down through
  // the rows, then along one row and back.
  const keys = [...Array(5).fill("ArrowDown"), ...Array(14).fill("ArrowRight"), ...Array(14).fill("ArrowLeft"), ...Array(5).fill("ArrowUp")];
  for (const k of keys) {
    await page.keyboard.press(k);
    await page.waitForTimeout(120);
  }
  await page.waitForTimeout(500);
  const { frames, lat, tiles } = await page.evaluate(() => {
    const w = window as unknown as { frames: number[]; lat: number[]; rec: boolean };
    w.rec = false;
    return { frames: w.frames.slice(1), lat: w.lat, tiles: document.querySelectorAll(".tile").length };
  });
  const result = {
    firstFocusMs,
    frames: frames.length,
    frameMs: { p50: pct(frames, 50), p95: pct(frames, 95), p99: pct(frames, 99) },
    over33ms: frames.filter((f) => f > 33.4).length,
    keyToFrameMs: { p50: pct(lat, 50), p95: pct(lat, 95), max: pct(lat, 100) },
    mountedTiles: tiles,
  };
  console.log(`perf ${JSON.stringify(result)}`);
  // Live checks are kept with their date (repo rule: bench/<milestone>/*.jsonl).
  const line = { date: new Date().toISOString(), check: "web-home-remote-scroll", browser: page.context().browser()?.version(), canvas: "tv 1920x1080", ...result };
  if (process.env.PERF_BENCH) {
    mkdirSync(BENCH, { recursive: true });
    appendFileSync(`${BENCH}/web-perf.jsonl`, `${JSON.stringify(line)}\n`);
  }
  test.info().annotations.push({ type: "perf", description: JSON.stringify(result) });
  expect(result.frameMs.p50).toBeLessThan(20);
  expect(result.keyToFrameMs.p95).toBeLessThan(50);
});
