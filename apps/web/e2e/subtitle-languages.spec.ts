/** The subtitle picker (plans/player-polish.md 2.4), on a core of its own:
 * Big Buck Bunny from a local addon whose stream names subtitles in many
 * languages and spellings (OpenSubtitles' specials among them), with a
 * profile that prefers Turkish. The first level is the search field, Off,
 * then the languages: Turkish, then the browser's language, then the rest
 * by name, each with its flag and count; a language opens its options,
 * told apart by name. Turkish turns on at start, until the viewer says
 * otherwise. While a slow subtitle addon is still asked, the Subtitles
 * button counts the languages that are in and marks the search. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, LANGS_PORT, PORTS, SHOTS, startBridge, startIfPaused } from "./core";
import { patchedContext } from "./patch";
import { BBB_ID, NAME, startSource } from "./source";

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let media: Server | null = null;

const SRT = "1\n00:00:01,000 --> 00:00:30,000\nA line\n";
// The release the stream names (a group of its own), and an English file made for that group, so
// the core's release match marks it (`fits`) and names it by what it says.
const PLAYING = "Big.Buck.Bunny.2008.240p.WEB.VP9-OPENFILM";

function manySubs(film: string): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const sub = (id: string, lang: string, label?: string) => ({ id, lang, url: `${base}/${id}.srt`, ...(label ? { label } : {}) });
    if (req.url === "/manifest.json") {
      return json({ id: "dev.marquee.many-subs", version: "1.0.0", name: "Many Subs", resources: ["stream"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] });
    }
    if (req.url === `/stream/movie/${BBB_ID}.json`) {
      return json({
        streams: [
          {
            name: "Many Subs\n240p",
            url: film,
            behaviorHints: { filename: `${PLAYING}.webm` },
            subtitles: [sub("1", "pob"), sub("2", "eng"), sub("3", "tur", "TOPAZ"), sub("4", "ger"), sub("5", "zht"), sub("6", "tur", "TOPAZ"), sub("7", "deu"), sub("8", "eng", PLAYING)],
          },
        ],
      });
    }
    if (req.url?.endsWith(".srt")) {
      res.writeHead(200, { "content-type": "text/plain" });
      return res.end(SRT);
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

/** A subtitle addon that answers after `ms` with one Finnish file, a language no other source has. */
function slowSubs(ms: number): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    if (req.url === "/manifest.json") {
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify({ id: "dev.marquee.slow-subs", version: "1.0.0", name: "Slow Subs", resources: ["subtitles"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] }));
    }
    if (req.url?.startsWith("/subtitles/")) {
      setTimeout(() => {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ subtitles: [{ id: "slow-1", lang: "fin", url: `${base}/slow-1.srt` }] }));
      }, ms);
      return;
    }
    if (req.url === "/slow-1.srt") {
      res.writeHead(200, { "content-type": "text/plain" });
      return res.end(SRT);
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

test.beforeAll(async () => {
  media = await startSource(0);
  addon = await manySubs(`http://127.0.0.1:${(media.address() as AddressInfo).port}/media/${NAME}`);
  core = await startBridge(LANGS_PORT);
  const c = new Client(`ws://127.0.0.1:${LANGS_PORT}/ws`);
  try {
    for (let attempt = 1; ; attempt++) {
      try {
        await c.req("installAddon", { url: "https://v3-cinemeta.strem.io/manifest.json" });
        break;
      } catch (e) {
        if (attempt === 3) throw e;
      }
    }
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1, subtitleLanguages: ["tur"] } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  media?.close();
});

async function playerPage(browser: Browser) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${LANGS_PORT}/ws`);
    const early: string[] = [];
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => ws.send(String(e.data)));
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((m) => (upstream.readyState === WebSocket.OPEN ? upstream.send(String(m)) : early.push(String(m))));
    ws.onClose(() => upstream.close());
  });
  const page = await context.newPage();
  const open = async () => {
    await page.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=desktop`);
    await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
    await page.evaluate(() => {
      const v = document.querySelector("video");
      if (v) v.muted = true;
    });
    await startIfPaused(page);
  };
  return { context, page, open };
}

// First in the file: it leaves the title's remembered choice at Off.
test("the profile's language turns on at start; Off wins and is remembered", async ({ browser }) => {
  const { context, page, open } = await playerPage(browser);
  await open();
  const summary = page.locator('[data-f="player:subs"] small');
  // Turkish is the profile's subtitle language, the audio is not Turkish.
  await expect(summary).toHaveText("Turkish", { timeout: 30_000 });
  await expect.poll(() => page.evaluate(() => document.querySelector("video")?.textTracks[0]?.mode)).toBe("showing");
  // The viewer turns it off: off now, and on the next play of the title.
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  await page.locator('[data-f="sub:off"]').dispatchEvent("click");
  await expect(summary).toHaveText(/^Off, \d+ languages$/);
  await open();
  await expect(summary).toHaveText(/^Off, \d+ languages$/, { timeout: 30_000 });
  await page.waitForTimeout(1000);
  await expect(summary).toHaveText(/^Off, \d+ languages$/);
  await context.close();
});

// Second: the remembered choice is still Off, so nothing turns on by itself.
test("while the core still asks, Subtitles counts what is in and marks the search", async ({ browser }) => {
  const slow = await slowSubs(6000);
  const c = new Client(`ws://127.0.0.1:${LANGS_PORT}/ws`);
  const url = `http://127.0.0.1:${(slow.address() as AddressInfo).port}/manifest.json`;
  await c.req("installAddon", { url });
  try {
    const { context, page, open } = await playerPage(browser);
    await open();
    const summary = page.locator('[data-f="player:subs"] small');
    const mark = summary.locator("svg.mark");
    // The stream's own languages are in; the slow addon is still being asked.
    await expect(summary).toHaveText(/^Off, \d+ languages$/);
    await expect(mark).toBeVisible();
    await expect(mark).toHaveAttribute("aria-label", "Still searching");
    const before = Number(/(\d+)/.exec((await summary.textContent()) ?? "")?.[1]);
    // It answers: its language joins the count and the mark goes.
    await expect(summary).toHaveText(`Off, ${before + 1} languages`, { timeout: 30_000 });
    await expect(mark).toHaveCount(0);
    await context.close();
  } finally {
    await c.req("removeAddon", { url }).catch(() => undefined);
    c.close();
    slow.close();
  }
});

const VIEWPORTS = { tv: { width: 1920, height: 1080 }, desktop: { width: 1440, height: 900 }, phone: { width: 844, height: 390 } };
/** A player on this spec's core at `form`'s size; `missingFlag` names a flag file that answers 404. */
async function picker(browser: Browser, form: keyof typeof VIEWPORTS, missingFlag?: string) {
  // The core's own fields reach the page as it sent them (a pass-through proxy to this spec's core).
  const { context } = await patchedContext(browser, LANGS_PORT, { viewport: VIEWPORTS[form] }, (_view, snapshot) => snapshot);
  // The image alone: the dev server also serves the flag's URL as a script the page imports.
  if (missingFlag) await context.route(new RegExp(`/flags/${missingFlag}\\.svg`), (r) => (r.request().resourceType() === "image" ? r.fulfill({ status: 404 }) : r.continue()));
  const page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=${form}`);
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await page.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
  await startIfPaused(page);
  return { context, page, errors, panel: page.locator('aside[aria-label="Subtitles"]') };
}
const focusedKey = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");

test("the picker lists languages first, in the core's order, with flags and counts", async ({ browser }) => {
  const { context, page, errors, panel } = await picker(browser, "desktop");
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  const rows = panel.locator('[data-f^="sublang:"]');
  // Turkish, the profile's; English, the browser's language standing in for audio languages
  // the profile never set; then the rest by name.
  await expect(rows.locator(".nm")).toHaveText(["Turkish", "English", "Chinese (Traditional)", "German", "Portuguese (Brazil)"]);
  // "ger" and "deu" are one language.
  await expect(rows.locator(".ct")).toHaveText(["2", "2", "1", "2", "1"]);
  // Its own name, quietly, where it differs; none for English.
  await expect(panel.locator('[data-f="sublang:tr"] .own')).toHaveText("Türkçe");
  await expect(panel.locator('[data-f="sublang:en"] .own')).toHaveCount(0);
  await expect(panel.locator('[data-f="sublang:de"] .own')).toHaveText("Deutsch");
  // A circle flag, drawn from the bundled file; Off and the search come first, Off chosen (remembered).
  for (const tag of ["tr", "en", "zh-Hant", "de", "pt-BR"])
    await expect.poll(() => panel.locator(`[data-f="sublang:${tag}"] img.flag`).evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth)).toBeGreaterThan(0);
  await expect(panel.locator('[data-f="sub:search"]')).toBeVisible();
  await expect(panel.locator('[data-f="sub:off"]')).toHaveClass(/\bon\b/);
  // What is on is said, not only drawn: Off is the checked radio, the languages are not.
  await expect(panel.locator('[data-f="sub:off"]')).toHaveAttribute("role", "menuitemradio");
  await expect(panel.locator('[data-f="sub:off"]')).toHaveAttribute("aria-checked", "true");
  await expect(panel.locator('[data-f="sub:off"]')).toHaveAccessibleName("Off, selected");
  await expect(panel.locator('[data-f="sublang:en"]')).toHaveAttribute("aria-checked", "false");
  await expect(panel.locator('[data-f="sublang:tr"]')).toHaveAccessibleName("Turkish, Türkçe, 2 options");
  await expect(page.locator('[data-f="player:subs"] small')).toHaveText("Off, 5 languages");
  await page.screenshot({ path: `${SHOTS}/subtitle-picker-desktop.png` });

  // One tap on a language turns on its best option at once and closes the picker.
  await panel.locator('[data-f="sublang:pt-BR"]').dispatchEvent("click");
  await expect(panel).toHaveCount(0);
  await expect(page.locator('[data-f="player:subs"] small')).toHaveText("Portuguese (Brazil)");
  await expect.poll(() => page.evaluate(() => document.querySelector("video track")?.getAttribute("srclang"))).toBe("pob");
  expect(errors).toEqual([]);
  await context.close();
});

test("the search field matches English names, own names and codes", async ({ browser }) => {
  const { context, page, errors, panel } = await picker(browser, "desktop");
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  const names = panel.locator('[data-f^="sublang:"] .nm');
  const search = panel.locator('[data-f="sub:search"]');
  for (const [typed, shown] of [
    ["deu", ["German"]],
    ["Deutsch", ["German"]],
    ["turkce", ["Turkish"]],
    ["brasil", ["Portuguese (Brazil)"]],
    ["pob", ["Portuguese (Brazil)"]],
    ["chin", ["Chinese (Traditional)"]],
    // A region-tagged code as typed.
    ["pt-br", ["Portuguese (Brazil)"]],
    ["zh-hant", ["Chinese (Traditional)"]],
  ] as const) {
    await search.fill(typed);
    await expect(names).toHaveText([...shown]);
  }
  // Backspace edits the field; it does not close the picker.
  await search.focus();
  await page.keyboard.press("Backspace");
  await expect(panel).toBeVisible();
  await search.fill("zzz");
  await expect(names).toHaveCount(0);
  await expect(panel.locator(".subp-none")).toHaveText("No language matches “zzz”");
  // The clear button empties the field in one press and leaves the focus in it.
  await panel.locator('[data-f="sub:clear"]').click();
  await expect(search).toHaveValue("");
  await expect(search).toBeFocused();
  await expect(names).toHaveCount(5);
  await expect(panel.locator('[data-f="sub:clear"]')).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});

test("a language opens its options: names, the quiet detail, the fit, the chosen one checked", async ({ browser }) => {
  const { context, page, errors, panel } = await picker(browser, "desktop");
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  await panel.locator('[data-f="subopen:en"]').dispatchEvent("click");
  const opts = panel.locator(".subp-opts");
  await expect(opts.locator(".subp-head h2")).toHaveText("English");
  // The file made for the playing release's group comes first, named by what it says.
  await expect(opts.locator(".opt b")).toHaveText([/^OPENFILM, WEB\b/, "Many Subs"]);
  // Its detail says the core's match words, then the release text, cut in the middle at a dot so
  // its group stays in view.
  const fitting = opts.locator(".opt").first();
  await expect(fitting.locator(".fit")).toHaveText("Same release group");
  await expect(fitting.locator(".rel")).toHaveText(/^Big\.Buck.*….*-OPENFILM$/);
  await expect(fitting.locator(".rel")).toHaveAttribute("title", PLAYING);
  await expect(opts.locator(".opt").nth(1).locator(".fit")).toHaveCount(0);
  // A separator goes with the part after it, so a wrapped line never ends on one.
  await expect(fitting.locator(".part:has(.rel) .sep")).toHaveCount(1);
  expect(await fitting.locator("small").evaluate((el) => [...el.querySelectorAll(".part")].every((p, i) => (i === 0) === !p.querySelector(".sep")))).toBe(true);
  // The timing row waits for a track; size is always there.
  await expect(opts.locator(".sub-size output")).toHaveText(/^\d+%$/);
  await page.screenshot({ path: `${SHOTS}/subtitle-options-desktop.png` });

  await fitting.dispatchEvent("click");
  await expect(page.locator('[data-f="player:subs"] small')).toHaveText("English");
  // Opened again: the languages, English checked; its options, that one checked, and timing.
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  await expect(panel.locator('[data-f="sublang:en"]')).toHaveClass(/\bon\b/);
  await expect(panel.locator('[data-f="sublang:en"] .ck svg')).toHaveCount(1);
  await expect(panel.locator('[data-f="sublang:en"]')).toHaveAttribute("aria-checked", "true");
  await expect(panel.locator('[data-f="sublang:en"]')).toHaveAccessibleName("English, 2 options, selected");
  await expect(panel.locator('[data-f="sub:off"]')).toHaveAttribute("aria-checked", "false");
  await panel.locator('[data-f="subopen:en"]').dispatchEvent("click");
  await expect(opts.locator(".opt.on b")).toHaveText(/^OPENFILM, WEB\b/);
  await expect(opts.locator(".opt.on")).toHaveAttribute("aria-checked", "true");
  await expect(opts.locator(".opt.on")).toHaveAccessibleName(/^OPENFILM, WEB.*, selected$/);
  await expect(opts.locator('.opt:not(.on)[aria-checked="false"]')).toHaveCount(1);
  // The delay is the track's: Many Subs starts at 0 and OPENFILM keeps its own.
  const timing = opts.locator(".sub-val");
  const reopen = async () => {
    await page.locator('[data-f="player:subs"]').dispatchEvent("click");
    await panel.locator('[data-f="subopen:en"]').dispatchEvent("click");
  };
  await expect(timing).toHaveText("0.0 s");
  await opts.locator('[data-f="sub:later"]').dispatchEvent("click");
  await expect(timing).toHaveText("+0.1 s");
  await opts.locator(".opt").nth(1).dispatchEvent("click");
  await reopen();
  await expect(opts.locator(".opt.on b")).toHaveText("Many Subs");
  await expect(timing).toHaveText("0.0 s");
  await fitting.dispatchEvent("click");
  await reopen();
  await expect(opts.locator(".opt.on b")).toHaveText(/^OPENFILM, WEB\b/);
  await expect(timing).toHaveText("+0.1 s");
  // Back to the languages, by the link and by the key.
  await opts.locator('[data-f="sub:languages"]').dispatchEvent("click");
  await expect(panel.locator('[data-f="sub:off"]')).toBeVisible();
  await panel.locator('[data-f="subopen:de"]').dispatchEvent("click");
  await expect(opts.locator(".subp-head h2")).toHaveText("German");
  await page.keyboard.press("Escape");
  await expect(panel.locator('[data-f="sub:off"]')).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});

test("TV: two columns, the right one follows the focused language, Right moves into it", async ({ browser }) => {
  // German's flag file fails: its row shows the round mark with its letters instead, at the TV's floor.
  const { context, page, errors, panel } = await picker(browser, "tv", "de");
  await page.locator('[data-f="player:subs"]').focus();
  await page.keyboard.press("Enter");
  // The remote starts on the language that is on, and the right column shows its options.
  const on = panel.locator('[data-f^="sublang:"].on');
  await expect.poll(() => focusedKey(page)).toBe((await on.getAttribute("data-f")) ?? "");
  await expect(panel.locator(".subp-opts .subp-head h2")).toHaveText((await on.locator(".nm").textContent()) ?? "");
  // The chosen option's quiet words at 92 percent on its grey fill (5.37:1, design/DECISION.md).
  await expect(panel.locator(".subp-opts .opt.on small")).toHaveCSS("color", "rgba(255, 255, 255, 0.92)");
  await panel.locator('[data-f="sublang:en"]').focus();
  await expect(panel.locator(".subp-opts .subp-head h2")).toHaveText("English");
  // No chevrons on TV: the right column is always there.
  await expect(panel.locator(".lr-open")).toHaveCount(0);
  await expect(panel.locator('[data-f="sublang:de"] .flag.none')).toHaveText("DE");
  await expect(panel.locator('[data-f="sublang:de"] .flag.none')).toHaveCSS("font-size", "23px");
  await expect(panel.locator('[data-f="sublang:de"] img.flag')).toHaveCount(0);
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => focusedKey(page)).toBe("sublang:zh-Hant");
  await expect(panel.locator(".subp-opts .subp-head h2")).toHaveText("Chinese (Traditional)");
  await page.keyboard.press("ArrowUp");
  await expect(panel.locator(".subp-opts .subp-head h2")).toHaveText("English");
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/subtitle-picker-tv.png` });
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => focusedKey(page)).toMatch(/^sub:http/);
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/subtitle-options-tv.png` });
  // Enter on an option turns it on; the remote goes back to Subtitles.
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => focusedKey(page)).toMatch(/^sub:http/);
  await page.keyboard.press("Enter");
  await expect.poll(() => focusedKey(page)).toBe("player:subs");
  await expect(page.locator('[data-f="player:subs"] small')).toHaveText("English");
  // Timing on the remote: the value holds the focus, Left and Right step it (held, they repeat), Enter resets.
  await page.keyboard.press("Enter");
  await expect.poll(() => focusedKey(page)).toBe("sublang:en");
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => focusedKey(page)).toMatch(/^sub:http/);
  for (let i = 0; i < 4 && (await focusedKey(page)) !== "sub:delay"; i++) await page.keyboard.press("ArrowDown");
  await expect.poll(() => focusedKey(page)).toBe("sub:delay");
  const timing = panel.locator('[data-f="sub:delay"]');
  await expect(timing).toHaveText("0.0 s");
  await page.keyboard.press("ArrowRight");
  await expect(timing).toHaveText("+0.1 s");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await expect(timing).toHaveText("−0.1 s");
  await expect.poll(() => focusedKey(page)).toBe("sub:delay");
  // Held 700 ms: 0.3 s a tick from 250 ms on, so about +1.4 s, far past one press.
  await page.keyboard.down("ArrowRight");
  await page.waitForTimeout(700);
  await page.keyboard.up("ArrowRight");
  expect(Number((await timing.textContent())?.replace(" s", ""))).toBeGreaterThanOrEqual(1);
  await page.keyboard.press("Enter");
  await expect(timing).toHaveText("0.0 s");
  await expect.poll(() => focusedKey(page)).toBe("sub:delay");
  expect(errors).toEqual([]);
  await context.close();
});

test("phone: a full-height sheet with Done, a language's options a level down", async ({ browser }) => {
  const { context, page, errors, panel } = await picker(browser, "phone");
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  await expect(panel.locator('[data-f="sub:done"]')).toBeVisible();
  const box = await panel.boundingBox();
  // Full height between 12 pt margins.
  expect(box?.height ?? 0).toBeGreaterThanOrEqual(390 - 24 - 1);
  await page.screenshot({ path: `${SHOTS}/subtitle-picker-phone.png` });
  await panel.locator('[data-f="subopen:en"]').dispatchEvent("click");
  await expect(panel.locator(".subp-opts .subp-head h2")).toHaveText("English");
  await expect(panel.locator('[data-f="sub:languages"]')).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/subtitle-options-phone.png` });
  // Back at the languages, the list is where it was: on a shorter screen it scrolls.
  await panel.locator('[data-f="sub:languages"]').dispatchEvent("click");
  await page.setViewportSize({ width: 844, height: 300 });
  const list = panel.locator(".subp-langs .subp-list");
  await expect.poll(() => list.evaluate((el) => el.scrollHeight - el.clientHeight)).toBeGreaterThan(40);
  await list.evaluate((el) => (el.scrollTop = 40));
  await panel.locator('[data-f="subopen:pt-BR"]').dispatchEvent("click");
  await expect(panel.locator(".subp-opts .subp-head h2")).toHaveText("Portuguese (Brazil)");
  await panel.locator('[data-f="sub:languages"]').dispatchEvent("click");
  await expect.poll(() => list.evaluate((el) => el.scrollTop)).toBe(40);
  await panel.locator('[data-f="sub:done"]').dispatchEvent("click");
  await expect(panel).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});
