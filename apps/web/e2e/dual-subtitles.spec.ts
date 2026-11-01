/** Dual subtitles (plans/dual-subtitles.md), on a core of its own: Big Buck Bunny from a local addon whose stream
 * names Turkish, English and German subtitle files, its release name English (the spoken language), and the
 * profile's subtitle language Turkish. The first line is Turkish; the second line is the viewer's or the
 * profile's, drawn in the profile's layout. The film comes from a sample source of its own. */

import { expect, type Page, test } from "./fixture";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, DUAL_PORT, startBridge, startIfPaused } from "./core";
import { type Json, patchedContext } from "./patch";
import { BBB_ID, NAME, startSource } from "./source";

const vtt = (text: string, extra = "") => `WEBVTT\n\n00:00:00.000 --> 00:10:00.000\n${text}\n${extra}`;
const FILES: Record<string, string> = {
  // A sign the first line's file placed at the top: drawn where its file put it, Split's top line under it.
  tur: vtt("Turkish line", "\n00:00:00.000 --> 00:10:00.000 line:10%\nTURKISH SIGN\n"),
  // A sign its file placed at the top: on the second line it joins the line (plans/dual-subtitles.md 2.7).
  // Its line changes at 5:00, so a seek while paused shows the new time's line.
  eng: `WEBVTT\n\n00:00:00.000 --> 00:05:00.000\nEnglish line\n\n00:00:00.000 --> 00:10:00.000 line:10%\nENGLISH SIGN\n\n00:05:00.000 --> 00:10:00.000\nEnglish later\n`,
  ger: vtt("German line"),
};

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let media: Server | null = null;

function subsAddon(film: string): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const url = req.url ?? "";
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    if (url === "/manifest.json") {
      return json({ id: "dev.marquee.dual", version: "1.0.0", name: "Dual", resources: ["stream"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] });
    }
    if (url === `/stream/movie/${BBB_ID}.json`) {
      const file = (lang: string) => ({ id: lang, lang, url: `${base}/${lang}.vtt` });
      return json({
        streams: [{ name: "Dual\n240p", url: film, behaviorHints: { filename: "Big.Buck.Bunny.2008.ENGLISH.240p.WEB.VP9.webm" }, subtitles: ["tur", "eng", "ger"].map(file) }],
      });
    }
    const lang = url.slice(1).replace(/\.vtt.*$/, "");
    if (FILES[lang]) {
      res.writeHead(200, { "content-type": "text/vtt" });
      return res.end(FILES[lang]);
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

const setPrefs = async (patch: Json) => {
  const c = new Client(`ws://127.0.0.1:${DUAL_PORT}/ws`);
  try {
    await c.req("setPrefs", { patch });
  } finally {
    c.close();
  }
};

test.beforeAll(async () => {
  media = await startSource(0);
  addon = await subsAddon(`http://127.0.0.1:${(media.address() as AddressInfo).port}/media/${NAME}`);
  core = await startBridge(DUAL_PORT);
  const c = new Client(`ws://127.0.0.1:${DUAL_PORT}/ws`);
  try {
    await c.req("installAddon", { url: "https://v3-cinemeta.strem.io/manifest.json" });
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1, subtitleLanguages: ["tur"] } });
  } finally {
    c.close();
  }
});

/** The profile's second line as the core keeps it. */
const storedSecond = async () => {
  const c = new Client(`ws://127.0.0.1:${DUAL_PORT}/ws`);
  try {
    return (await c.subscribe<{ secondSubtitle: string | null }>("settings")).now().secondSubtitle;
  } finally {
    c.close();
  }
};

test.beforeEach(() => setPrefs({ secondSubtitle: null, subtitleLayout: "stacked", secondOnPause: false }));

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  media?.close();
});

/** A player page on the spec's core at `size` and `form`, muted; `patch` may rewrite the playback snapshots. */
async function open(browser: Parameters<typeof patchedContext>[0], form = "desktop", size = { width: 1440, height: 900 }, patch: (s: Json) => Json = (s) => s) {
  const { context } = await patchedContext(browser, DUAL_PORT, { viewport: size }, (view, s) => (view === "playback" && s ? patch(s) : s));
  const page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=${form}`);
  await expect(page.locator('[data-f="player:toggle"]')).toBeAttached({ timeout: 30_000 });
  await page.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
  await startIfPaused(page);
  return { context, page, errors };
}

// Cues that start together draw in the track's cue order, the later end first (HTML's text track cue order):
// the English file's sign (to 10:00) over its line (to 5:00).
type Line = { text: string; second: boolean; font: number; top: number; bottom: number };
/** The lines drawn: each `.cue` span of the foot block and of Split's top block, with its box. */
const drawn = (page: Page) =>
  page.evaluate(() =>
    Array.from(document.querySelectorAll<HTMLElement>(".cues .cue"), (el) => {
      const r = el.getBoundingClientRect();
      return { text: el.textContent ?? "", second: el.classList.contains("cue2") || !!el.closest(".cues-top"), font: parseFloat(getComputedStyle(el).fontSize), top: r.top, bottom: r.bottom };
    }),
  ) as Promise<Line[]>;
const texts = async (page: Page) => (await drawn(page)).map((l) => l.text);
const subsValue = (page: Page) => page.locator('[data-f="player:subs"] small').textContent();

test("a second line in the spoken language over the first, at 85%, its own timing, kept for the next play, gone with the first", async ({ browser }) => {
  const { context, page, errors } = await open(browser);
  await expect.poll(() => texts(page), { timeout: 30_000 }).toEqual(["Turkish line"]);
  await expect.poll(() => subsValue(page)).toBe("Turkish");
  // The switch shows with a first line on; the Second line tab names the spoken language and disables Turkish.
  await page.locator('[data-f="player:subs"]').click();
  await page.locator('[data-f="sub:line2"]').click();
  await expect(page.locator('[data-f="sub:spoken"]')).toContainText("Spoken language");
  await expect(page.locator('[data-f="sub:spoken"]')).toContainText("English");
  await expect(page.locator('[data-f="sublang:tr"]')).toBeDisabled();
  await expect(page.locator('[data-f="sublang:tr"]')).toContainText("On the first line");
  await page.locator('[data-f="sub:spoken"]').click();
  // Both lines: the second (with its file's sign joined to it, not placed) over the first, at 85% of its size.
  await expect.poll(() => texts(page)).toEqual(["ENGLISH SIGN", "English line", "Turkish line"]);
  const lines = await drawn(page);
  const first = lines.find((l) => !l.second) as Line;
  for (const l of lines.filter((x) => x.second)) {
    expect(l.font).toBeCloseTo(first.font * 0.85, 1);
    expect(l.bottom).toBeLessThanOrEqual(first.top + 1);
  }
  // Only the first line's sign is placed; the second's joined its line.
  await expect(page.locator(".cues-at .cue")).toHaveText(["TURKISH SIGN"]);
  await expect.poll(() => subsValue(page)).toBe("Turkish + English");
  // Each line's own track element, both running hidden.
  expect(await page.evaluate(() => Array.from(document.querySelectorAll<HTMLTrackElement>("video track"), (t) => [t.dataset.line, t.track.mode]))).toEqual([
    ["1", "hidden"],
    ["2", "hidden"],
  ]);
  // The second line's timing moves its cues alone.
  const starts = () => page.evaluate(() => [1, 2].map((n) => document.querySelector<HTMLTrackElement>(`video track[data-line="${n}"]`)?.track.cues?.[0]?.startTime ?? -1));
  expect(await starts()).toEqual([0, 0]);
  await page.locator('[data-f="player:subs"]').click();
  await expect(page.locator('[data-f="sub:line2"]')).toHaveAttribute("aria-checked", "true");
  await page.locator('[data-f="subopen:en"]').click();
  for (let i = 0; i < 3; i++) await page.locator('[data-f="sub:later"]').click();
  await expect.poll(starts).toEqual([0, expect.closeTo(0.3, 3)]);
  await page.keyboard.press("Escape");
  // The profile keeps the Spoken language (as "audio", not the language it named here): the next play draws both again.
  expect(await storedSecond()).toBe("audio");
  await page.reload();
  await expect(page.locator('[data-f="player:toggle"]')).toBeAttached({ timeout: 30_000 });
  await page.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
  await startIfPaused(page);
  await expect.poll(() => texts(page), { timeout: 30_000 }).toEqual(["ENGLISH SIGN", "English line", "Turkish line"]);
  // Off on the first line: both go.
  await page.locator('[data-f="player:subs"]').click();
  await page.locator('[data-f="sub:line1"]').click();
  await expect(page.locator('[data-f="sublang:en"]')).toBeDisabled();
  await page.locator('[data-f="sub:off"]').click();
  await expect.poll(() => texts(page)).toEqual([]);
  // Turkish again, so the title's memory is not Off for the next tests; with no first line, no switch.
  await page.locator('[data-f="player:subs"]').click();
  await expect(page.locator('[data-f="sub:line1"]')).toHaveCount(0);
  await page.locator('[data-f="sublang:tr"]').click();
  await expect.poll(() => texts(page)).toEqual(["ENGLISH SIGN", "English line", "Turkish line"]);
  expect(errors).toEqual([]);
  await context.close();
});

test("each layout: Stacked one block, One box one background, Split from the top under the title and its signs", async ({ browser }) => {
  await setPrefs({ secondSubtitle: "en" });
  const { context, page } = await open(browser);
  await expect.poll(() => texts(page), { timeout: 30_000 }).toEqual(["ENGLISH SIGN", "English line", "Turkish line"]);
  // Stacked: one foot block, no box around both.
  await expect(page.locator(".cues")).toHaveCount(1);
  await expect(page.locator(".cue-box")).toHaveCount(0);
  // Each line's box is the design's 60% black (owner, 2026-10-28: 4.5:1 over a white picture).
  expect(await page.evaluate(() => Array.from(document.querySelectorAll(".cues .cue"), (el) => getComputedStyle(el).backgroundColor))).toEqual(Array(3).fill("rgba(0, 0, 0, 0.6)"));
  // One box: one background holds both lines.
  await setPrefs({ subtitleLayout: "oneBox" });
  await expect(page.locator(".cue-box .cue")).toHaveCount(3);
  const backgrounds = await page.evaluate(() => [".cue-box", ".cue-box .cue"].map((sel) => getComputedStyle(document.querySelector(sel) as Element).backgroundColor));
  expect(backgrounds[0]).toBe("rgba(0, 0, 0, 0.6)");
  expect(backgrounds[1]).toBe("rgba(0, 0, 0, 0)");
  // Split: the second line from the top at full size, 8% of the picture under the picture's top.
  await setPrefs({ subtitleLayout: "split" });
  await expect(page.locator(".cues-top .cue")).toHaveCount(2);
  const at = () =>
    page.evaluate(() => {
      // In the canvas's own px (the page scales it on TV and desktop), where the 8 px clearances are.
      const v = document.querySelector("video") as HTMLVideoElement;
      const k = v.getBoundingClientRect().height / v.offsetHeight;
      const r = (el: Element | null) => {
        const b = el?.getBoundingClientRect();
        return { top: (b?.top ?? 0) / k, bottom: (b?.bottom ?? 0) / k, width: (b?.width ?? 0) / k, height: (b?.height ?? 0) / k };
      };
      const box = r(v);
      const picture = v.videoHeight * Math.min(box.width / v.videoWidth, box.height / v.videoHeight);
      const top = r(document.querySelector(".cues-top")).top;
      const title = r(document.querySelector(".ptitle")).bottom;
      const sign = r(document.querySelector(".cues-at .cue-at")).bottom;
      return { top, expected: box.top + (box.height - picture) / 2 + picture * 0.08, title, sign, safe: box.top + box.height * 0.05 };
    });
  await expect(page.locator(".player-scr")).toHaveClass(/idle/, { timeout: 10_000 });
  // The first line's sign at line:10% sits lower than 8% of the picture: the top line goes under it, 8 px clear.
  await expect.poll(async () => (await at()).sign).toBeGreaterThan(0);
  await expect.poll(async () => Math.abs((await at()).top - Math.max((await at()).expected, (await at()).sign + 8))).toBeLessThanOrEqual(2);
  expect((await at()).top).toBeGreaterThanOrEqual((await at()).safe);
  const fonts = await drawn(page);
  expect(fonts.find((l) => l.text === "English line")?.font).toBeCloseTo(fonts.find((l) => l.text === "Turkish line")?.font ?? 0, 1);
  // The controls up: under the title block.
  await page.mouse.move(700, 450);
  await page.mouse.move(720, 450);
  await expect(page.locator(".player-scr")).not.toHaveClass(/idle/);
  await expect.poll(async () => (await at()).top - (await at()).title).toBeGreaterThanOrEqual(0);
  await context.close();
});

test("Only when paused: the second line hides while playing and shows at once on a pause", async ({ browser }) => {
  await setPrefs({ secondSubtitle: "en", secondOnPause: true });
  const { context, page } = await open(browser);
  await expect.poll(() => texts(page), { timeout: 30_000 }).toEqual(["Turkish line"]);
  await page.evaluate(() => document.querySelector("video")?.pause());
  await expect.poll(() => texts(page)).toEqual(["ENGLISH SIGN", "English line", "Turkish line"]);
  // A seek while paused keeps it, showing the new time's line.
  const seek = (sec: number) =>
    page.evaluate((at) => {
      const v = document.querySelector("video");
      if (v) v.currentTime = at;
    }, sec);
  await seek(301);
  await expect.poll(() => texts(page)).toContain("English later");
  expect(await texts(page)).not.toContain("English line");
  // Back to the start, so the title's progress leaves the next tests on the first cue.
  await seek(1);
  await expect.poll(() => texts(page)).toContain("English line");
  await page.evaluate(() => void document.querySelector("video")?.play());
  await expect.poll(() => texts(page)).toEqual(["Turkish line"]);
  await context.close();
});

test("never two lines in one language: a first line that turns into the second's language hides the second", async ({ browser }) => {
  await setPrefs({ secondSubtitle: "en" });
  // The core's start pick says English for the first line too (a handover the core would not repeat): the
  // page's own guard keeps one line.
  const { context, page } = await open(browser, "desktop", { width: 1440, height: 900 }, (s) => {
    const en = (s.subtitles as { url: string; language: string }[]).find((t) => t.language === "en");
    if (en && s.autoSubtitle) s.autoSubtitle = en.url;
    return s;
  });
  await expect.poll(() => texts(page), { timeout: 30_000 }).toEqual(["English line"]);
  await expect(page.locator(".cue2")).toHaveCount(0);
  await expect.poll(() => subsValue(page)).toBe("English");
  await context.close();
});

test("the core's second line going null turns it off", async ({ browser }) => {
  await setPrefs({ secondSubtitle: "en" });
  let drop = false;
  const { context, page } = await open(browser, "desktop", { width: 1440, height: 900 }, (s) => (drop ? { ...s, autoSecondSubtitle: null } : s));
  await expect.poll(() => texts(page), { timeout: 30_000 }).toEqual(["ENGLISH SIGN", "English line", "Turkish line"]);
  drop = true;
  // Any next snapshot carries it: a delay sent for the first line makes one.
  await page.locator('[data-f="player:subs"]').click();
  await page.locator('[data-f="sub:line1"]').click();
  await page.locator('[data-f="subopen:tr"]').click();
  await page.locator('[data-f="sub:later"]').click();
  await expect.poll(() => texts(page), { timeout: 10_000 }).toEqual(["Turkish line"]);
  await context.close();
});

test("TV: Up from the search reaches the switch, Left and Right move on it, OK lands on the tab's chosen row", async ({ browser }) => {
  await setPrefs({ secondSubtitle: "en" });
  const { context, page } = await open(browser, "tv", { width: 960, height: 540 });
  await expect.poll(() => texts(page), { timeout: 30_000 }).toEqual(["ENGLISH SIGN", "English line", "Turkish line"]);
  await page.locator('[data-f="player:subs"]').focus();
  await page.keyboard.press("Enter");
  const focused = () => page.evaluate(() => document.activeElement?.getAttribute("data-f"));
  await expect.poll(focused).toMatch(/^sub/);
  // Up through the rows, Off and the search field to the switch.
  for (let i = 0; i < 8 && !(await focused())?.startsWith("sub:line"); i++) await page.keyboard.press("ArrowUp");
  expect(await focused()).toMatch(/^sub:line[12]$/);
  // Left and Right move along it.
  await page.keyboard.press("ArrowLeft");
  expect(await focused()).toBe("sub:line1");
  // Its ends stop the remote: Left from the first stays.
  await page.keyboard.press("ArrowLeft");
  expect(await focused()).toBe("sub:line1");
  await page.keyboard.press("ArrowRight");
  expect(await focused()).toBe("sub:line2");
  // Right from the last stays too, never into the options column.
  await page.keyboard.press("ArrowRight");
  expect(await focused()).toBe("sub:line2");
  await page.keyboard.press("Enter");
  await expect.poll(focused).toBe("sublang:en");
  // Only when paused, focused by the remote: its label reads dark on the white fill, as the panel's rows do.
  await page.locator('[data-f="sub:paused"]').focus();
  const look = () =>
    page.evaluate(() => {
      const el = document.querySelector('[data-f="sub:paused"]') as HTMLElement;
      const own = getComputedStyle(el);
      return { fill: own.backgroundColor, ink: own.color, label: getComputedStyle(el.querySelector("span:not(.knob)") as Element).color };
    });
  // The row's own colour eases in with the focus (app.css [data-f]): read it once the move has ended.
  await expect.poll(async () => ((l) => l.label === l.ink)(await look())).toBe(true);
  expect((await look()).label).not.toBe((await look()).fill);
  await context.close();
});

test("TV: the Second line tab with no second line never opens the first line's language", async ({ browser }) => {
  await setPrefs({ secondSubtitle: null });
  const { context, page } = await open(browser, "tv", { width: 960, height: 540 });
  await expect.poll(() => texts(page), { timeout: 30_000 }).toEqual(["Turkish line"]);
  await page.locator('[data-f="player:subs"]').focus();
  await page.keyboard.press("Enter");
  const focused = () => page.evaluate(() => document.activeElement?.getAttribute("data-f"));
  await expect.poll(focused).toMatch(/^sub/);
  for (let i = 0; i < 8 && !(await focused())?.startsWith("sub:line"); i++) await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowRight");
  expect(await focused()).toBe("sub:line2");
  await page.keyboard.press("Enter");
  await expect.poll(focused).toBe("sub:off");
  // The right column shows a language that can be picked here, never Turkish (the first line's).
  await expect(page.locator(".subp-opts h2")).toBeVisible();
  await expect(page.locator(".subp-opts h2")).toHaveText(/English/);
  await context.close();
});
