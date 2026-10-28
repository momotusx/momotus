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
  const body = text.slice(start + "<script>".length, end);
  // Read as written: no escapes the Swift or Kotlin string would turn into something else.
  expect(body).not.toContain("\\");
  expect(body).not.toContain("$");
  return body;
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
    expect(last?.t).toBeLessThanOrEqual(5.5);
  });

  test(`${platform}: a preview stops a second before its cap, muted, and a refusal ends in an error`, async ({ page }) => {
    await open(page);
    await page.evaluate(() => (window as unknown as { play: (...a: unknown[]) => void }).play("abcdefghijk", 2, 1, false, false));
    await expect.poll(async () => (await msgs(page)).find((m) => m.event === "end"), { timeout: 10_000 }).toMatchObject({ gen: 1, why: "cap" });
    expect(await ytLog(page)).not.toContain("unMute");
    await page.evaluate(() => {
      (window as unknown as { ytFail: boolean }).ytFail = true;
      (window as unknown as { play: (...a: unknown[]) => void }).play("bcdefghijkl", 45, 2, false, false);
    });
    await expect.poll(async () => (await msgs(page)).find((m) => m.event === "end" && m.gen === 2)).toMatchObject({ why: "error" });
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

  test(`${platform}: plays stopped before YouTube is ready leave no frame behind`, async ({ page }) => {
    await open(page);
    await page.evaluate(() => {
      const w = window as unknown as { ytHoldReady: boolean; play: (...a: unknown[]) => void; halt: () => void };
      w.ytHoldReady = true;
      for (let g = 1; g <= 4; g++) w.play("abcdefghijk", 45, g, false, false);
    });
    await expect.poll(async () => (await ytLog(page)).filter((l) => l.startsWith("new")).length).toBeGreaterThan(0);
    await page.evaluate(() => (window as unknown as { halt: () => void }).halt());
    await expect.poll(() => page.evaluate(() => document.querySelectorAll("#p iframe").length)).toBe(0);
    expect(await page.evaluate(() => document.getElementById("p")?.children.length)).toBe(0);
  });

  test(`${platform}: a clean-up that waits never takes the next play's frame`, async ({ page }) => {
    await open(page);
    // A preview that reaches its cap leaves over 650 ms; the next play starts at once, on its end.
    await page.evaluate(() => {
      const w = window as unknown as { msgs: Msg[]; play: (...a: unknown[]) => void };
      const push = w.msgs.push.bind(w.msgs);
      w.msgs.push = (...m: Msg[]) => {
        const n = push(...m);
        if (m.some((x) => x.event === "end" && x.gen === 1)) w.play("bcdefghijkl", 45, 2, false, false);
        return n;
      };
      w.play("abcdefghijk", 2, 1, false, false);
    });
    await expect.poll(async () => (await msgs(page)).find((m) => m.event === "playing" && m.gen === 2), { timeout: 10_000 }).toBeTruthy();
    await page.waitForTimeout(900);
    expect(await page.evaluate(() => [...document.querySelectorAll("#p iframe")].map((f) => (f as HTMLElement).dataset.id))).toEqual(["bcdefghijkl"]);
  });

  test(`${platform}: Sound pressed before the first frame holds, off or on`, async ({ page }) => {
    await open(page);
    const until = (re: RegExp) => expect.poll(async () => (await ytLog(page)).some((l) => re.test(l))).toBe(true);
    const go = (loud: boolean, g: number) =>
      page.evaluate(
        ([l, gen]) => {
          const w = window as unknown as { ytHold: boolean; play: (...a: unknown[]) => void };
          w.ytHold = true;
          w.play("abcdefghijk", 0, gen, true, l);
        },
        [loud, g] as const,
      );
    // Full screen asked loud, then Sound off before it plays: it never unmutes.
    await go(true, 1);
    await until(/^playVideo/);
    await page.evaluate(() => {
      const w = window as unknown as { sound: (on: boolean) => void; ytRelease: () => void };
      w.sound(false);
      w.ytRelease();
    });
    await expect.poll(async () => (await msgs(page)).some((m) => m.event === "playing" && m.gen === 1)).toBe(true);
    await page.waitForTimeout(300);
    expect(await ytLog(page)).not.toContain("unMute");
    // Asked quiet, then Sound on before it plays: it unmutes once it plays, not before (the read-back holds).
    await go(false, 2);
    await expect.poll(async () => (await ytLog(page)).filter((l) => l.startsWith("playVideo")).length).toBe(2);
    await page.evaluate(() => {
      const w = window as unknown as { sound: (on: boolean) => void; ytRelease: () => void };
      w.sound(true);
      w.ytRelease();
    });
    await expect.poll(() => ytLog(page)).toContain("unMute");
    const log = await ytLog(page);
    expect(log.indexOf("unMute")).toBeGreaterThan(log.lastIndexOf(log.filter((l) => l.startsWith("playVideo")).at(-1) ?? ""));
  });

  test(`${platform}: a script that does not load ends in an error`, async ({ page }) => {
    await open(page, { noApi: true });
    await page.evaluate(() => (window as unknown as { play: (...a: unknown[]) => void }).play("abcdefghijk", 0, 1, true, false));
    await expect.poll(async () => (await msgs(page)).find((m) => m.event === "end")).toMatchObject({ gen: 1, why: "error" });
  });
}
