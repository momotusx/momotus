import { readFileSync } from "node:fs";
import { expect, type Page, test } from "@playwright/test";
import { fakeYouTube, ytLog } from "./yt-fake";

/** The native apps' trailer page (plans/trailer-inline.md 1): the same script in Android's HeroTrailer.kt and Apple's
 * HeroTrailer.swift, run here as each app runs it, in a page of its own on the stand-in for YouTube's API, so the two
 * copies keep one behaviour: a play's generation on every message, a length of its own (cap 0) ended a second early,
 * pause and seek, sound only when asked, and a script that does not load ending in an error. */

const SOURCES = {
  android: { file: "../../android/design/src/main/kotlin/app/marquee/design/HeroTrailer.kt", shim: `console.log = (s) => String(s).startsWith("MQT ") && window.msgs.push(JSON.parse(String(s).slice(4)));` },
  apple: { file: "../../apple/Marquee/Home/HeroTrailer.swift", shim: `window.webkit = { messageHandlers: { trailer: { postMessage: (s) => window.msgs.push(JSON.parse(s)) } } };` },
} as const;

type Msg = { gen: number; full: boolean; event: string; why?: string; t?: number; d?: number; state?: string; from?: number; to?: number };

function script(file: string): string {
  const text = readFileSync(new URL(file, import.meta.url), "utf8");
  const start = text.indexOf("<script>");
  const end = text.indexOf("</script>", start);
  expect(start).toBeGreaterThan(0);
  return text.slice(start + "<script>".length, end);
}

const msgs = (page: Page) => page.evaluate(() => (window as unknown as { msgs: Msg[] }).msgs);

for (const [platform, { file, shim }] of Object.entries(SOURCES)) {
  async function open(page: Page, opts: { duration?: number; noApi?: boolean } = {}) {
    if (opts.noApi) await page.route("https://www.youtube.com/iframe_api", (r) => r.abort());
    else await fakeYouTube(page);
    await page.goto("about:blank");
    await page.evaluate((d) => ((window as unknown as { ytDuration?: number }).ytDuration = d), opts.duration);
    await page.setContent(`<div id="p"></div><script>window.msgs = []; ${shim}</script><script>${script(file)}</script>`);
  }

  test(`${platform}: full screen plays to its own length, pauses, seeks, and ends a second early`, async ({ page }) => {
    await open(page, { duration: 6 });
    await page.evaluate(() => (window as unknown as { play: (...a: unknown[]) => void }).play("abcdefghijk", 0, 1, true, false));
    await expect.poll(async () => (await msgs(page)).some((m) => m.event === "playing")).toBe(true);
    await expect.poll(async () => (await msgs(page)).find((m) => m.event === "time")).toMatchObject({ gen: 1, full: true, d: 6, state: "playing" });
    // No sound unless asked.
    expect(await ytLog(page)).not.toContain("unMute");
    await page.evaluate(() => (window as unknown as { toggle: () => void }).toggle());
    await expect.poll(async () => (await msgs(page)).at(-1)?.state).toBe("paused");
    await page.evaluate(() => (window as unknown as { seek: (s: number) => void }).seek(3));
    expect(await ytLog(page)).toContain("seekTo 3 true");
    expect((await msgs(page)).find((m) => m.event === "seek")).toMatchObject({ gen: 1, to: 3 });
    await page.evaluate(() => (window as unknown as { toggle: () => void }).toggle());
    // From 3 s, a 6 s trailer ends at 5 s: YouTube's end screen never shows.
    await expect.poll(async () => (await msgs(page)).find((m) => m.event === "end"), { timeout: 10_000 }).toMatchObject({ gen: 1, why: "ended" });
    const last = (await msgs(page)).filter((m) => m.event === "time").at(-1);
    expect(last?.t).toBeLessThan(6);
  });

  test(`${platform}: every message carries its play's generation, so a late stop never reaches the next`, async ({ page }) => {
    await open(page);
    await page.evaluate(() => {
      const w = window as unknown as { play: (...a: unknown[]) => void };
      w.play("abcdefghijk", 45, 1, false, false);
      w.play("bcdefghijkl", 45, 2, false, false);
    });
    await expect.poll(async () => (await msgs(page)).some((m) => m.event === "playing")).toBe(true);
    const all = await msgs(page);
    expect(all.filter((m) => m.event === "end")).toEqual([{ gen: 1, full: false, event: "end", why: "stopped" }]);
    expect(all.filter((m) => m.event !== "end").every((m) => m.gen === 2)).toBe(true);
    // The first play's player was never made: its id never reached YouTube.
    expect((await ytLog(page)).filter((l) => l.startsWith("new"))).toEqual(["new bcdefghijkl https://www.youtube-nocookie.com"]);
  });

  test(`${platform}: full screen with sound asked turns it on once it plays`, async ({ page }) => {
    await open(page);
    await page.evaluate(() => (window as unknown as { play: (...a: unknown[]) => void }).play("abcdefghijk", 0, 1, true, true));
    await expect.poll(() => ytLog(page)).toContain("unMute");
    const log = await ytLog(page);
    expect(log.findIndex((l) => l.startsWith("playVideo muted=true"))).toBeLessThan(log.indexOf("unMute"));
  });

  test(`${platform}: a script that does not load ends in an error`, async ({ page }) => {
    await open(page, { noApi: true });
    await page.evaluate(() => (window as unknown as { play: (...a: unknown[]) => void }).play("abcdefghijk", 0, 1, true, false));
    await expect.poll(async () => (await msgs(page)).find((m) => m.event === "end")).toMatchObject({ gen: 1, why: "error" });
  });
}
