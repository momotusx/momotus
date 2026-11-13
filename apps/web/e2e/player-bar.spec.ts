/** The player bar (plans/player-bar.md, L1): one slim row under the scrubber with Audio, Speed and Versions at the
 * left, the transport in the middle, and Subtitles with its quick shift, Episodes, Next and Full screen at the right;
 * the subtitle and audio strips over it; the row's fit at any width (D16) and on phones (D15, D17); the remote's
 * rules on TV (section 6); Trakt's one toast in the shared notice slot (5.1). On a core of its own whose local addon
 * names a three-episode series and a film, each with subtitles in eight languages; what plays is the suite's own
 * generated test film (e2e/source.ts). */

import { expect, type Browser, type BrowserContextOptions, type Page, test } from "./fixture";
import type { ChildProcess } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { BAR_PORT, Client, cleanup, openSubtitlePicker, SHOTS, startBridge, startIfPaused } from "./core";
import { drawnContrast } from "./contrast";
import { type Json, patchedContext } from "./patch";
import { NAME, startSource } from "./source";

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let media: Server | null = null;

const AIRED = "2020-01-01T00:00:00.000Z";
const SRT = "1\n00:00:00,000 --> 00:10:00,000\nA line\nand the line under it\n";
// Eight languages: more than a strip lists (5 on a desktop and TV, 3 on a phone); Portuguese (Brazil) is the longest name.
const LANGS = ["eng", "pob", "tur", "ger", "spa", "fre", "ita", "dut"];

function barAddon(film: string): Promise<Server> {
  const episode = (n: number) => ({ id: `mqbs:1:${n}`, title: `Chapter ${n}`, season: 1, episode: n, released: AIRED });
  const metas: Record<string, object> = {
    "/meta/series/mqbs.json": { id: "mqbs", type: "series", name: "Bar Series", videos: [episode(1), episode(2), episode(3)] },
    "/meta/movie/mqbf.json": { id: "mqbf", type: "movie", name: "Bar Film", releaseInfo: "2008" },
  };
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const url = decodeURIComponent(req.url ?? "");
    if (url === "/manifest.json") return json({ id: "dev.marquee.bar", version: "1.0.0", name: "Bar Fixture", resources: ["meta", "stream"], types: ["movie", "series"], idPrefixes: ["mqb"], catalogs: [] });
    const meta = metas[url];
    if (meta) return json({ meta });
    const subtitles = LANGS.map((lang) => ({ id: `b-${lang}`, lang, url: `${base}/sub/${lang}.srt` }));
    const ep = /^\/stream\/series\/mqbs:1:(\d)\.json$/.exec(url);
    if (ep) {
      const filename = `Bar.Series.S01E0${ep[1]}.720p.WEB.VP9.webm`;
      return json({ streams: [{ name: "Bar Fixture\n720p", title: filename, url: film, behaviorHints: { filename }, subtitles }] });
    }
    if (url === "/stream/movie/mqbf.json") return json({ streams: [{ name: "Bar Fixture\n1080p", url: film, behaviorHints: { filename: NAME }, subtitles }] });
    if (url.startsWith("/sub/")) {
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
  addon = await barAddon(`http://127.0.0.1:${(media.address() as AddressInfo).port}/media/${NAME}`);
  core = await startBridge(BAR_PORT);
  const c = new Client(`ws://127.0.0.1:${BAR_PORT}/ws`);
  try {
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1 } });
  } finally {
    c.close();
  }
});

test.afterAll(async () => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  media?.close();
});

const S1E1 = "/play/series/mqbs/mqbs:1:1";
const S1E3 = "/play/series/mqbs/mqbs:1:3";
const FILM = "/play/movie/mqbf/mqbf";
const BENCH = process.env.BAR_BENCH;

type Open = { page: Page; commands: { command: Json; view: string | null }[]; repush: (view: string) => void; close: () => Promise<void> };

/** A page on this spec's core at `path` with `form`, the controls up and the video paused (so they stay), the start
 * subtitle on unless `off`. `patch` rewrites the playback view's snapshots. */
async function open(browser: Browser, path: string, form: "desktop" | "phone" | "tv", options: BrowserContextOptions, patch: (s: Json) => Json = (s) => s, keep = false): Promise<Open> {
  const { context, commands, repush } = await patchedContext(browser, BAR_PORT, { reducedMotion: "reduce", ...options }, (view, s) => (view === "playback" ? patch(s) : s));
  const page = await context.newPage();
  await page.goto(`${path}?form=${form}`);
  await startIfPaused(page);
  // The start subtitle on (the core's English): the shift shows. A test before may have left the title on another
  // choice, which the core keeps for it: English again from the strip.
  const value = page.locator('[data-f="player:subs"] small');
  try {
    await expect(value).toHaveText("English", { timeout: 15_000 });
  } catch {
    await page.locator('[data-f="player:subs"]').dispatchEvent("click");
    await page.locator('[data-f="strip:sub:en"]').dispatchEvent("click");
    await page.locator('[data-f="strip:close"]').dispatchEvent("click");
    await expect(value).toHaveText("English", { timeout: 15_000 });
  }
  if (!keep) await page.evaluate(() => document.querySelector("video")?.pause());
  await expect(page.locator(".osd")).toBeVisible();
  return { page, commands, repush, close: () => context.close() };
}

/** A page at `path` as it starts, nothing waited for: the play's first seconds (the shared notice slot). */
async function bare(browser: Browser, path: string, patch: (s: Json) => Json): Promise<Open> {
  const { context, commands, repush } = await patchedContext(browser, BAR_PORT, { viewport: { width: 1440, height: 900 } }, (view, s) => (view === "playback" ? patch(s) : s));
  const page = await context.newPage();
  await page.goto(`${path}?form=desktop`);
  return { page, commands, repush, close: () => context.close() };
}

/** Every box in the page's px. */
const boxes = (page: Page, sel: string) =>
  page.locator(sel).evaluateAll((els) => els.filter((e) => (e as HTMLElement).offsetWidth > 0).map((e) => {
    const r = e.getBoundingClientRect();
    return { key: (e as HTMLElement).dataset.f ?? e.className, x: r.x, y: r.y, w: r.width, h: r.height, right: r.right, bottom: r.bottom };
  }));
const meets = (a: { x: number; y: number; right: number; bottom: number }, b: { x: number; y: number; right: number; bottom: number }) =>
  a.x < b.right && b.x < a.right && a.y < b.bottom && b.y < a.bottom;

/** The bar's measures: its height, its control rows (distinct row tops of the groups and the transport), the fit. */
async function bar(page: Page) {
  return page.evaluate(() => {
    const osd = document.querySelector<HTMLElement>(".osd")!;
    const r = (s: string) => osd.querySelector(s)!.getBoundingClientRect();
    // Control rows: the groups' and the transport's middles, a row apart or on one line.
    const mids = [".bl", ".transport", ".br"].map((s) => (r(s).top + r(s).bottom) / 2).sort((a, b) => a - b);
    const rows = 1 + mids.slice(1).filter((m, i) => m - mids[i]! > 20).length;
    return { height: osd.getBoundingClientRect().height, rows, fit: { rows: osd.dataset.rows, lw: osd.hasAttribute("data-lw"), rw: osd.hasAttribute("data-rw"), chev: osd.hasAttribute("data-chev") }, scrub: r(".scrub").width };
  });
}

async function shot(page: Page, name: string) {
  mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: `${SHOTS}/player-bar-${name}.png` });
}

const shiftValue = (page: Page, at = "bar") => page.locator(`[data-f="${at}:shift"]`);

test.describe("desktop", () => {
  test("one slim row under the scrubber; no volume, More or Trakt mark; Tab reaches every control in order", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "desktop", { viewport: { width: 1440, height: 900 } });
    try {
      const b = await bar(page);
      expect(b.height).toBeLessThanOrEqual(96);
      // Nothing removed comes back under another name: no volume slider or mute, no More, nothing of Trakt.
      const scr = page.locator(".player-scr");
      await expect(scr.getByRole("slider", { name: /volume|level/i })).toHaveCount(0);
      await expect(scr.getByRole("button", { name: /mute|volume|^more$/i })).toHaveCount(0);
      expect(await scr.evaluate((el) => /trakt/i.test(el.textContent ?? "") || !!el.querySelector('[aria-label*="trakt" i], [title*="trakt" i]'))).toBe(false);
      await expect(page.locator(".player-scr").getByRole("button", { name: /^More$/ })).toHaveCount(0);
      await expect(page.locator('[data-f="player:next"]')).toHaveAccessibleName("Next episode, S1:E2 Chapter 2");
      // Tab from Audio walks the row, Earlier, the value and Later included (section 6).
      await page.locator('[data-f="player:audio"]').focus();
      const order: string[] = [];
      for (let i = 0; i < 13; i++) {
        order.push(await page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? ""));
        await page.keyboard.press("Tab");
      }
      expect(order).toEqual([
        "player:audio", "player:speed", "player:versionsact", "player:back10", "player:toggle", "player:fwd10",
        "player:subs", "bar:earlier", "bar:shift", "bar:later", "player:episodes", "player:next", "player:fullscreen",
      ]);
      await shot(page, "desktop-1440");
      if (BENCH) writeFileSync(`${BENCH}/desktop-1440.json`, JSON.stringify(b));
    } finally {
      await close();
    }
  });

  // Under 1840 px the desktop canvas scales with the window (form.ts, --k), so the fit is the same at every width
  // there; from 1840 the canvas grows and the right side's words come back somewhere in the band swept here.
  test("D16: the right side keeps its words or icons whatever the subtitle, at every width from 1840 to 2800", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "desktop", { viewport: { width: 1840, height: 1000 } });
    try {
      // The fit at each width, the subtitle as it is now.
      const sweep = async () => {
        const out: string[] = [];
        for (let width = 1840; width <= 2800; width += 10) {
          await page.setViewportSize({ width, height: 1000 });
          await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
          const f = (await bar(page)).fit;
          out.push(`${width} ${f.rows} ${f.lw} ${f.rw} ${f.chev}`);
        }
        return out;
      };
      // A long value on (Portuguese (Brazil), from the full picker), against Off's.
      await openSubtitlePicker(page);
      await page.locator('[data-f="sublang:pt-BR"]').dispatchEvent("click");
      await expect(page.locator('[data-f="player:subs"] small')).toHaveText("Portuguese (Brazil)");
      const on = await sweep();
      // The band is crossed: the right side has its words at the wide end and drops them at the narrow one.
      expect(on.at(-1)).toContain(" true true ");
      expect(on[0]).toContain(" false ");
      await page.locator('[data-f="player:subs"]').dispatchEvent("click");
      await page.locator('[data-f="strip:sub:off"]').dispatchEvent("click");
      await page.locator('[data-f="strip:close"]').dispatchEvent("click");
      await expect(page.locator('[data-f="player:subs"] small')).toHaveText(/^Off/);
      expect(await sweep()).toEqual(on);
      if (BENCH) writeFileSync(`${BENCH}/desktop-series-sweep.json`, JSON.stringify(on));
    } finally {
      await close();
    }
  });

  for (const [width, height] of [[721, 800], [800, 600], [1024, 768], [1280, 800], [1366, 768], [1440, 900], [1600, 900], [1920, 1080]] as const) {
    test(`D16 at ${width} x ${height}: no group meets the transport, targets 44 px, the subtitle off and on moves nothing`, async ({ browser }) => {
      const { page, close } = await open(browser, S1E1, "desktop", { viewport: { width, height } });
      try {
        // The longest values: Portuguese (Brazil) on (past the strip's five, so from the full picker).
        await openSubtitlePicker(page);
        await page.locator('[data-f="sublang:pt-BR"]').dispatchEvent("click");
        await expect(page.locator('[data-f="player:subs"] small')).toHaveText("Portuguese (Brazil)");
        await expect(page.locator(".bshift")).not.toHaveClass(/off/);
        await page.waitForTimeout(300);
        const on = await bar(page);
        const transport = (await boxes(page, ".osd .transport"))[0]!;
        for (const g of await boxes(page, ".osd .bl, .osd .br")) expect(meets(g, transport), `${g.key} meets the transport`).toBe(false);
        for (const t of await boxes(page, ".osd :is(.pill, .round, .qsb, .qv)")) {
          expect(t.w, `${t.key} width`).toBeGreaterThanOrEqual(44);
          expect(t.h, `${t.key} height`).toBeGreaterThanOrEqual(44);
        }
        expect(on.rows).toBeLessThanOrEqual(2);
        // Each control row at most 64 px.
        for (const g of await boxes(page, ".osd .bl, .osd .br, .osd .transport")) expect(g.h).toBeLessThanOrEqual(64);
        await shot(page, `desktop-${width}`);
        // Off, then on again: the same rows and height (the shift's room is always counted).
        await page.locator('[data-f="player:subs"]').dispatchEvent("click");
        await page.locator('[data-f="strip:sub:off"]').dispatchEvent("click");
        await page.locator('[data-f="strip:close"]').dispatchEvent("click");
        await expect(page.locator(".bshift")).toHaveClass(/off/);
        await page.waitForTimeout(300);
        const off = await bar(page);
        expect(off.fit).toEqual(on.fit);
        expect(off.height).toBeCloseTo(on.height, 0);
        if (BENCH) writeFileSync(`${BENCH}/desktop-${width}.json`, JSON.stringify({ on, off }));
      } finally {
        await close();
      }
    });
  }

  test("the shift: hidden with Off, a tap moves 0.25 s and reaches the core, a hold moves more, the value resets", async ({ browser }) => {
    const { page, commands, close } = await open(browser, FILM, "desktop", { viewport: { width: 1440, height: 900 } });
    try {
      const value = shiftValue(page);
      await expect(value).toHaveText("0.0 s");
      await page.locator('[data-f="bar:later"]').click();
      await expect(value).toHaveText("+0.25 s");
      await expect(value).toHaveAccessibleName("+0.25 s, subtitles later, press to reset");
      // After the settle the core has it, in ms.
      await expect.poll(() => commands.filter((c) => c.command?.type === "subtitleDelay").map((c) => c.command.ms).at(-1), { timeout: 5000 }).toBe(250);
      // One polite line after the settle.
      await expect(page.locator(".player-scr > .sr.said")).toHaveText("Subtitles 0.25 seconds later");
      for (let i = 0; i < 3; i++) await page.locator('[data-f="bar:later"]').click();
      await expect(value).toHaveText("+1.0 s");
      await value.click();
      await expect(value).toHaveText("0.0 s");
      await page.locator('[data-f="bar:earlier"]').click();
      await expect(value).toHaveText("−0.25 s");
      // A held press repeats: more than one step.
      const later = await page.locator('[data-f="bar:later"]').boundingBox();
      await page.mouse.move(later!.x + later!.width / 2, later!.y + later!.height / 2);
      await page.mouse.down();
      await page.waitForTimeout(1300);
      await page.mouse.up();
      const held = Number((await value.textContent())?.replace("−", "-").replace(" s", ""));
      expect(held).toBeGreaterThanOrEqual(1);
      // Tab to Earlier, Enter shifts.
      await value.click();
      await page.locator('[data-f="bar:earlier"]').focus();
      await page.keyboard.press("Enter");
      await expect(value).toHaveText("−0.25 s");
      // Off: no shift on the bar.
      await page.locator('[data-f="player:subs"]').dispatchEvent("click");
      await page.locator('[data-f="strip:sub:off"]').dispatchEvent("click");
      await expect(page.locator(".bshift")).toHaveClass(/off/);
      await expect(page.locator('[data-f="bar:shift"]')).toBeHidden();
      await expect(page.locator('.bstrip [data-f="strip:shift"]')).toHaveCount(0);
    } finally {
      await close();
    }
  });

  test("G and H shift on a desktop, from hidden controls too", async ({ browser }) => {
    const { page, close } = await open(browser, FILM, "desktop", { viewport: { width: 1440, height: 900 } }, undefined, true);
    try {
      // The core keeps a track's timing for the title: from 0.
      if ((await shiftValue(page).textContent()) !== "0.0 s") await shiftValue(page).click();
      await expect(shiftValue(page)).toHaveText("0.0 s");
      await page.keyboard.press("h");
      await expect(shiftValue(page)).toHaveText("+0.25 s");
      // Held G: one step for the press, none for the keyboard's repeats.
      for (let i = 0; i < 5; i++) await page.keyboard.down("g");
      await page.keyboard.up("g");
      await expect(shiftValue(page)).toHaveText("0.0 s");
      await page.keyboard.press("g");
      await expect(shiftValue(page)).toHaveText("−0.25 s");
      // Playing, the controls hide after 4 s; H wakes them and shifts in the same press.
      await expect(page.locator(".player-scr")).toHaveClass(/idle/, { timeout: 10_000 });
      await page.keyboard.press("h");
      await expect(page.locator(".player-scr")).not.toHaveClass(/idle/);
      await expect(shiftValue(page)).toHaveText("0.0 s");
    } finally {
      await close();
    }
  });

  test("the strip opens by click and Enter, closes by Close, Escape, an outside click and its button; Style first", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "desktop", { viewport: { width: 1440, height: 900 } });
    try {
      const subs = page.locator('[data-f="player:subs"]');
      const strip = page.locator(".bstrip.sub");
      const focused = () => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");
      await subs.click();
      await expect(strip).toBeVisible();
      await expect(subs).toHaveAttribute("aria-expanded", "true");
      await expect(strip.getByRole("radio")).toHaveCount(6);
      await expect(strip.locator('[aria-checked="true"]')).toHaveText("English");
      await subs.click();
      await expect(strip).toHaveCount(0);
      // Enter: the focus moves to the checked chip; Escape closes and gives it back to Subtitles.
      await subs.focus();
      await page.keyboard.press("Enter");
      await expect.poll(focused).toBe("strip:sub:en");
      await page.keyboard.press("Escape");
      await expect(strip).toHaveCount(0);
      await expect.poll(focused).toBe("player:subs");
      // Style: its row opens, Escape closes it first, then the strip.
      await page.keyboard.press("Enter");
      await page.locator('[data-f="strip:style"]').focus();
      await page.keyboard.press("Enter");
      await expect(page.locator(".bstrip .s2")).toBeVisible();
      await expect.poll(focused).toBe("strip:smaller");
      await page.keyboard.press("Escape");
      await expect(page.locator(".bstrip .s2")).toHaveCount(0);
      await expect(strip).toBeVisible();
      await expect.poll(focused).toBe("strip:style");
      await page.keyboard.press("Escape");
      await expect(strip).toHaveCount(0);
      // Close.
      await subs.click();
      await page.locator('[data-f="strip:close"]').click();
      await expect(strip).toHaveCount(0);
      // An outside click only closes it: the controls stay and nothing plays.
      await subs.click();
      await page.mouse.click(700, 300);
      await expect(strip).toHaveCount(0);
      await expect(page.locator(".player-scr")).not.toHaveClass(/idle/);
      expect(await page.evaluate(() => document.querySelector("video")?.paused)).toBe(true);
      // Only that press: the next click at once plays, and one more pauses again.
      await page.mouse.click(700, 300);
      await expect.poll(() => page.evaluate(() => document.querySelector("video")?.paused)).toBe(false);
      await page.mouse.click(700, 300);
      await expect.poll(() => page.evaluate(() => document.querySelector("video")?.paused)).toBe(true);
      // Audio's strip mirrors it over the left, with Choose a version and the note on other languages.
      await page.locator('[data-f="player:audio"]').click();
      await expect(page.locator(".bstrip.aud")).toBeVisible();
      await expect(page.locator('.bstrip.aud [data-f="audio:versions"]')).toHaveText("Choose a version");
      await expect(page.locator(".bstrip.aud .snote")).toHaveText("Other languages come with other versions.");
      const [aud, left] = [(await boxes(page, ".bstrip.aud"))[0]!, (await boxes(page, ".osd .bl"))[0]!];
      expect(Math.abs(aud.x - left.x)).toBeLessThan(2);
    } finally {
      await close();
    }
  });

  test("Style: size, position and background change the line live and write the profile", async ({ browser }) => {
    const { page, commands, close } = await open(browser, FILM, "desktop", { viewport: { width: 1440, height: 900 } });
    try {
      const cue = page.locator(".cues .cue").first();
      await expect(cue).toBeVisible();
      await page.locator('[data-f="player:subs"]').click();
      await page.locator('[data-f="strip:style"]').click();
      const fontSize = () => cue.evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
      const foot = () => page.locator(".cues").evaluate((el) => el.getBoundingClientRect().bottom);
      const size = await fontSize();
      await page.locator('[data-f="strip:larger"]').click();
      await expect(page.locator('.bstrip .si output').first()).toHaveText("125%");
      await expect.poll(fontSize).toBeGreaterThan(size);
      const at = await foot();
      await page.locator('[data-f="strip:higher"]').click();
      await expect(page.locator('.bstrip .si output').nth(1)).toHaveText("12%");
      await expect.poll(foot).toBeLessThan(at - 10);
      await page.locator('[data-f="strip:bg:outline"]').click();
      await expect(page.locator('[data-f="strip:bg:outline"]')).toHaveAttribute("aria-pressed", "true");
      await expect(page.locator(".player-scr")).toHaveAttribute("data-subbg", "outline");
      await expect(cue).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
      // The cue's foot keeps 8 px over the open strip, Style row included.
      const strip = (await boxes(page, ".bstrip"))[0]!;
      expect(await foot()).toBeLessThanOrEqual(strip.y - 8 + 0.5);
      // Bar, strip and Style row together: at most 240 px with the strip on one line. A strip too wide for the window wraps
      // (QA r5 R5-3) rather than hide a chip, and its second line of pills (48 px and the 10 px gap) is the one thing
      // that may go past it.
      const osd = (await boxes(page, ".osd"))[0]!;
      const lines = new Set((await boxes(page, ".bstrip .s1 [data-f]")).map((b) => Math.round(b.y / 30))).size;
      expect(osd.bottom - strip.y).toBeLessThanOrEqual(240 + (lines - 1) * 58);
      if (BENCH) writeFileSync(`${BENCH}/desktop-1440-style.json`, JSON.stringify({ stack: osd.bottom - strip.y, osd: osd.h }));
      await shot(page, "desktop-style");
      // The profile has them (the core's view, read on a fresh page).
      const c = new Client(`ws://127.0.0.1:${BAR_PORT}/ws`);
      try {
        const p = (await c.subscribe<{ subtitleSize: string; subtitlePosition: number; subtitleBackground: string }>("settings")).now();
        expect([p.subtitleSize, p.subtitlePosition, p.subtitleBackground]).toEqual(["large", 12, "outline"]);
        await c.req("setPrefs", { patch: { subtitleSize: "medium", subtitlePosition: 8, subtitleBackground: "box" } });
      } finally {
        c.close();
      }
    } finally {
      await close();
    }
  });

  test("with Style open over a white frame, the strip's labels keep 4.5:1", async ({ browser }) => {
    // Wide enough that the strip's row holds every chip unscrolled (a label clipped by the scroll is measured whole).
    const { page, close } = await open(browser, FILM, "desktop", { viewport: { width: 1920, height: 1080 } });
    try {
      await page.locator('[data-f="player:subs"]').click();
      await page.locator('[data-f="strip:style"]').click();
      // The brightest picture there is: the video gone, the screen white under the scrims.
      await page.addStyleTag({ content: ".player-scr { background: #fff !important; } .video, .player-bd, .pdim, .cues { visibility: hidden !important; }" });
      const measured = await drawnContrast(page, ".bstrip", [".chip3 .cl", ".slb", ".qv", ".qsb", ".si output", ".sseg button"], []);
      expect(measured.length).toBeGreaterThan(8);
      for (const m of measured) expect(m.ratio, m.what).toBeGreaterThanOrEqual(m.floor);
      await shot(page, "desktop-white");
    } finally {
      await close();
    }
  });

  test("hovered, Later on the capsule and a pill's value keep 4.5:1 over a white frame", async ({ browser }) => {
    const { page, close } = await open(browser, FILM, "desktop", { viewport: { width: 1920, height: 1080 } });
    try {
      await page.addStyleTag({ content: ".player-scr { background: #fff !important; } .video, .player-bd, .pdim, .cues { visibility: hidden !important; }" });
      for (const [control, label] of [
        ['[data-f="bar:later"]', '[data-f="bar:later"] .w'],
        ['[data-f="bar:earlier"]', '[data-f="bar:earlier"] .w'],
        ['[data-f="bar:shift"]', '[data-f="bar:shift"]'],
        ['[data-f="player:audio"]', '[data-f="player:audio"] small'],
        ['[data-f="player:speed"]', '[data-f="player:speed"] small'],
      ] as const) {
        await page.locator(control).hover();
        const [m] = await drawnContrast(page, ".osd", [label], []);
        expect(m, label).toBeDefined();
        expect(m!.ratio, `${label} hovered`).toBeGreaterThanOrEqual(4.5);
      }
    } finally {
      await close();
    }
  });

  test("Up Next appearing while the strip is open closes it (D23)", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "desktop", { viewport: { width: 1440, height: 900 } });
    try {
      await page.locator('[data-f="player:subs"]').click();
      await expect(page.locator(".bstrip")).toBeVisible();
      await page.evaluate(() => {
        const v = document.querySelector("video")!;
        v.currentTime = v.duration - 15;
      });
      await expect(page.locator(".next")).toBeVisible({ timeout: 15_000 });
      await expect(page.locator(".bstrip")).toHaveCount(0);
    } finally {
      await close();
    }
  });

  test("Next: shown on S1:E1 and plays S1:E2 at once; hidden on the last episode and on a film", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "desktop", { viewport: { width: 1440, height: 900 } });
    try {
      await page.locator('[data-f="player:next"]').click();
      await expect(page).toHaveURL(/\/play\/series\/mqbs\/mqbs:1:2/);
    } finally {
      await close();
    }
    for (const path of [S1E3, FILM]) {
      const o = await open(browser, path, "desktop", { viewport: { width: 1440, height: 900 } });
      try {
        await expect(o.page.locator('[data-f="player:next"]')).toBeHidden();
        if (path === FILM) await expect(o.page.locator('[data-f="player:episodes"]')).toHaveCount(0);
        else await expect(o.page.locator('[data-f="player:episodes"]')).toBeVisible();
      } finally {
        await o.close();
      }
    }
  });
});

test.describe("Trakt in the player", () => {
  test("reconnect: a 4 s text toast, then the subtitle notice that came with it; nothing for unmapped or this device off", async ({ browser }) => {
    let trakt = "reconnect";
    // The start subtitle failed: nothing turned on, the quiet line says so (the core's `startSubtitleFailed`).
    const patch = (s: Json) => ({ ...s, trakt, startSubtitleFailed: true, autoSubtitle: null });
    const { page, repush, close } = await bare(browser, FILM, patch);
    try {
      const toast = page.locator(".trakt-toast");
      const quiet = page.locator(".toast.quiet:not(.trakt-toast)");
      await expect(toast).toHaveText("Trakt: sign in again in Settings, so this play is not recorded", { timeout: 30_000 });
      await expect(toast.locator("svg, img, button, a")).toHaveCount(0);
      await expect(quiet).toHaveCount(0);
      // Its 4 s, then the subtitle line in the slot.
      await expect(toast).toHaveCount(0, { timeout: 6000 });
      await expect(quiet).toBeVisible();
      // Once per play: a later reconnect does not bring it back.
      repush("playback");
      await page.waitForTimeout(500);
      await expect(toast).toHaveCount(0);
      trakt = "unmapped";
      repush("playback");
    } finally {
      await close();
    }
    // A subtitle line already up gives way at once and comes back after.
    trakt = "on";
    const late = await bare(browser, FILM, patch);
    try {
      const quiet = late.page.locator(".toast.quiet:not(.trakt-toast)");
      await expect(quiet).toBeVisible({ timeout: 30_000 });
      trakt = "reconnect";
      late.repush("playback");
      await expect(late.page.locator(".trakt-toast")).toBeVisible();
      await expect(quiet).toHaveCount(0);
      await expect(quiet).toBeVisible({ timeout: 6000 });
    } finally {
      await late.close();
    }
    for (const state of ["unmapped", "thisDeviceOff", "on", "off"]) {
      trakt = state;
      const o = await bare(browser, FILM, patch);
      try {
        // The first line in the slot is the subtitle's, at once: no Trakt line came before it, and none after.
        // Searched by its words, not a class, so a mark under a new name would show here too.
        const traktShown = () => o.page.locator(".player-scr").evaluate((el) => /trakt/i.test(el.textContent ?? "") || !!el.querySelector('[aria-label*="trakt" i]'));
        await expect(o.page.locator(".toast")).toBeVisible({ timeout: 30_000 });
        expect(await traktShown()).toBe(false);
        await o.page.waitForTimeout(1000);
        expect(await traktShown()).toBe(false);
      } finally {
        await o.close();
      }
    }
  });
});

test.describe("phone", () => {
  const phone = (width: number, height: number): BrowserContextOptions => ({ viewport: { width, height }, hasTouch: true, isMobile: true });
  for (const [width, height, rows] of [[844, 390, "1"], [640, 360, "2"], [360, 780, "upright"]] as const) {
    test(`${width} x ${height}: ${rows === "upright" ? "three rows" : `${rows} row(s)`}, every target 48 px, the scrubber at least 120 px`, async ({ browser }) => {
      const { page, close } = await open(browser, S1E1, "phone", phone(width, height));
      try {
        await page.waitForTimeout(300);
        const b = await bar(page);
        if (rows === "upright") {
          const tops = await page.evaluate(() => [".tl", ".bl", ".bsub"].map((s) => document.querySelector(s)!.getBoundingClientRect().top));
          expect(new Set(tops.map((t) => Math.round(t))).size).toBe(3);
          expect(b.height).toBeLessThanOrEqual(176);
        } else {
          expect(b.fit.rows).toBe(rows);
          expect(b.height).toBeLessThanOrEqual(rows === "1" ? 64 : 128);
        }
        expect(b.scrub).toBeGreaterThanOrEqual(120);
        for (const t of await boxes(page, ".osd :is(.pill, .qsb, .qv), .transport .round")) {
          expect(t.w, `${t.key} width`).toBeGreaterThanOrEqual(48);
          expect(t.h, `${t.key} height`).toBeGreaterThanOrEqual(48);
        }
        await shot(page, `phone-${width}`);
        // The strip opens full width over the bar by a tap; an outside tap only closes it.
        await page.locator('[data-f="player:subs"]').tap();
        await expect(page.locator(".bstrip.sub")).toBeVisible();
        await expect(page.locator(".bstrip.sub").getByRole("radio")).toHaveCount(4);
        for (const t of await boxes(page, ".bstrip :is(.chip3, .qsb, .qv, .qb)")) expect(t.h, `${t.key} height`).toBeGreaterThanOrEqual(48);
        // Pixel walk 2026-11-09 L21: Style and Close stay whole on screen with no sideways swipe; the chips scroll.
        const ends = await boxes(page, '.bstrip :is([data-f="strip:style"], [data-f="strip:close"])');
        expect(ends.map((t) => t.key)).toEqual(["strip:style", "strip:close"]);
        for (const t of ends) expect([t.key, t.x >= 0 && t.right <= width]).toEqual([t.key, true]);
        await shot(page, `phone-${width}-strip`);
        await page.touchscreen.tap(width / 2, 80);
        await expect(page.locator(".bstrip")).toHaveCount(0);
        await expect(page.locator(".osd")).toBeVisible();
        if (BENCH) writeFileSync(`${BENCH}/phone-${width}.json`, JSON.stringify(b));
      } finally {
        await close();
      }
    });
  }
});

test.describe("TV", () => {
  const tv: BrowserContextOptions = { viewport: { width: 1920, height: 1080 } };
  const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");

  test("the focus row, the shift's value as one stop, OK to adjust, Up and Back leave adjust mode first", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "tv", tv);
    try {
      expect((await bar(page)).height).toBeLessThanOrEqual(140);
      await page.locator('[data-f="player:audio"]').focus();
      const row: string[] = [];
      for (let i = 0; i < 10; i++) {
        row.push(await focused(page));
        await page.keyboard.press("ArrowRight");
      }
      expect(row).toEqual(["player:audio", "player:speed", "player:versionsact", "player:back10", "player:toggle", "player:fwd10", "player:subs", "bar:shift", "player:episodes", "player:next"]);
      await shot(page, "tv");
      // Left and Right pass the value; OK adjusts, then Left is earlier.
      await page.locator('[data-f="bar:shift"]').focus();
      await expect(page.locator('[data-f="bar:shift"]')).toHaveAccessibleName("0.0 s, subtitles in sync, press OK to adjust");
      await page.keyboard.press("ArrowRight");
      expect(await focused(page)).toBe("player:episodes");
      await page.keyboard.press("ArrowLeft");
      expect(await focused(page)).toBe("bar:shift");
      await page.keyboard.press("Enter");
      await expect(page.locator(".bshift .qs")).toHaveClass(/adj/);
      await page.keyboard.press("ArrowLeft");
      await expect(shiftValue(page)).toHaveText("−0.25 s");
      // Up leaves adjust mode and keeps the focus; the next arrow moves it.
      await page.keyboard.press("ArrowUp");
      await expect(page.locator(".bshift .qs")).not.toHaveClass(/adj/);
      expect(await focused(page)).toBe("bar:shift");
      // Back leaves adjust mode first, the focus staying.
      await page.keyboard.press("Enter");
      await page.keyboard.press("Backspace");
      await expect(page.locator(".bshift .qs")).not.toHaveClass(/adj/);
      expect(await focused(page)).toBe("bar:shift");
      await expect(page.locator(".osd")).toBeVisible();
    } finally {
      await close();
    }
  });

  test("held Left and Right in adjust mode: exactly +1.0 s at 990 ms, +0.25 s at 300 ms, the remote's repeats swallowed", async ({ browser }) => {
    const { page, close } = await open(browser, FILM, "tv", tv);
    try {
      // The core keeps a track's timing for the title: from 0, by the strip's Reset (a remote has no other).
      if ((await shiftValue(page).textContent()) !== "0.0 s") {
        await page.locator('[data-f="player:subs"]').focus();
        await page.keyboard.press("Enter");
        await page.locator('[data-f="strip:reset"]').focus();
        await page.keyboard.press("Enter");
        await page.keyboard.press("Escape");
        await expect(page.locator(".bstrip")).toHaveCount(0);
      }
      await expect(shiftValue(page)).toHaveText("0.0 s");
      await page.clock.install();
      await page.locator('[data-f="bar:shift"]').focus();
      await page.keyboard.press("Enter");
      const holdFor = async (key: string, ms: number) => {
        await page.clock.pauseAt((await page.evaluate(() => Date.now())) + 100);
        await page.keyboard.down(key);
        for (let t = 50; t < ms; t += 50) {
          await page.clock.runFor(50);
          await page.keyboard.down(key);
        }
        await page.clock.runFor(ms % 50 || 50);
        await page.keyboard.up(key);
        await page.clock.resume();
      };
      await holdFor("ArrowRight", 990);
      await expect(shiftValue(page)).toHaveText("+1.0 s");
      await holdFor("ArrowRight", 300);
      await expect(shiftValue(page)).toHaveText("+1.25 s");
      // Reset is a chip on TV while the value is not 0.
      await page.keyboard.press("Enter");
      await page.locator('[data-f="player:subs"]').focus();
      await page.keyboard.press("Enter");
      await expect(page.locator('[data-f="strip:reset"]')).toBeVisible();
      await page.locator('[data-f="strip:reset"]').focus();
      await page.keyboard.press("Enter");
      await expect(shiftValue(page)).toHaveText("0.0 s");
      await expect(page.locator('[data-f="strip:reset"]')).toHaveCount(0);
    } finally {
      await close();
    }
  });

  test("a held Back closes the strip once and never leaves the player in the same press", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "tv", tv);
    try {
      await page.locator('[data-f="player:subs"]').focus();
      await page.keyboard.press("Enter");
      await expect(page.locator(".bstrip")).toBeVisible();
      // Held: the first keydown and the remote's repeats, then the key let go.
      for (let i = 0; i < 4; i++) await page.keyboard.down("Escape");
      await page.keyboard.up("Escape");
      await page.waitForTimeout(500);
      await expect(page.locator(".bstrip")).toHaveCount(0);
      await expect(page).toHaveURL(/\/play\/series\/mqbs\/mqbs:1:1/);
      await expect(page.locator(".osd")).toBeVisible();
    } finally {
      await close();
    }
  });

  test("the strip on a remote: OK opens it on the checked chip, Down to the opener and Up back in, Back closes it", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "tv", tv);
    try {
      await page.locator('[data-f="player:subs"]').focus();
      await page.keyboard.press("Enter");
      await expect.poll(() => focused(page)).toBe("strip:sub:en");
      // Left and Right walk the chips.
      await page.keyboard.press("ArrowRight");
      await expect.poll(() => focused(page)).toMatch(/^strip:sub:/);
      await page.keyboard.press("ArrowLeft");
      // Down to Subtitles, the strip open; Up returns to the checked chip.
      await page.keyboard.press("ArrowDown");
      expect(await focused(page)).toBe("player:subs");
      await expect(page.locator(".bstrip")).toBeVisible();
      await page.keyboard.press("ArrowUp");
      expect(await focused(page)).toBe("strip:sub:en");
      // Up from the scrubber does the same.
      await page.locator('[data-f="player:scrub"]').focus();
      await page.keyboard.press("ArrowUp");
      expect(await focused(page)).toBe("strip:sub:en");
      // The strip's shift on a remote: the value, OK, Back leaves adjust mode, then Back closes the strip.
      await page.locator('[data-f="strip:shift"]').focus();
      await page.keyboard.press("Enter");
      await page.keyboard.press("ArrowRight");
      await expect(shiftValue(page, "strip")).toHaveText("+0.25 s");
      await page.keyboard.press("Backspace");
      await expect(page.locator(".bstrip")).toBeVisible();
      await page.keyboard.press("Backspace");
      await expect(page.locator(".bstrip")).toHaveCount(0);
      await expect.poll(() => focused(page)).toBe("player:subs");
      await shot(page, "tv-after");
      // Open again for the shot with Style.
      await page.keyboard.press("Enter");
      await page.locator('[data-f="strip:style"]').focus();
      await page.keyboard.press("Enter");
      await expect.poll(() => focused(page)).toBe("strip:smaller");
      await shot(page, "tv-style");
    } finally {
      await close();
    }
  });
});

// QA r5 R5-3, R5-4: a strip row too wide for its screen wraps, so nothing is clipped under a neighbour or off the
// screen, and a remote visits every control in order.
test.describe("strips show every control", () => {
  /** The strip's controls that are not whole on screen, or that a neighbour covers at their centre, and its words cut by their box. */
  const hidden = (page: Page) =>
    page.evaluate(() => {
      const out: string[] = [];
      for (const el of document.querySelectorAll<HTMLElement>(".bstrip [data-f]")) {
        if (el.offsetWidth === 0 || (el as HTMLButtonElement).disabled) continue;
        const r = el.getBoundingClientRect();
        const top = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
        const whole = r.left >= 0 && r.right <= innerWidth && r.top >= 0 && r.bottom <= innerHeight;
        if (!whole || !top || !el.contains(top)) out.push(`${el.dataset.f} ${whole ? "covered" : "off screen"}`);
      }
      for (const el of document.querySelectorAll<HTMLElement>(".bstrip :is(.cl, .snote, .slb)")) if (el.scrollWidth > el.clientWidth + 1) out.push(`${el.textContent} cut`);
      return out;
    });
  const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");

  test("desktop 1440: the language chips, More languages, the shift, Style and Background are all in sight", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "desktop", { viewport: { width: 1440, height: 900 } });
    try {
      await page.locator('[data-f="player:subs"]').click();
      await expect(page.locator(".bstrip.sub")).toBeVisible();
      await expect(page.locator('[data-f="strip:more"]')).toBeVisible();
      expect(await hidden(page)).toEqual([]);
      await page.locator('[data-f="strip:style"]').click();
      await expect(page.locator('[data-f="strip:bg:shadow"]')).toBeVisible();
      expect(await hidden(page)).toEqual([]);
    } finally {
      await close();
    }
  });

  test("phone upright: the chips, the shift, Style's note and Background are all in sight", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "phone", { viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
    try {
      await page.locator('[data-f="player:subs"]').tap();
      await expect(page.locator(".bstrip.sub")).toBeVisible();
      expect(await hidden(page)).toEqual([]);
      await page.locator('[data-f="strip:style"]').tap();
      await expect(page.locator('[data-f="strip:bg:shadow"]')).toBeVisible();
      expect(await hidden(page)).toEqual([]);
      await page.waitForTimeout(500);
      await shot(page, "phone-390-style-wrapped");
    } finally {
      await close();
    }
  });

  test("TV: Right visits every control of the strip in order and Left comes back the same way", async ({ browser }) => {
    const { page, close } = await open(browser, S1E1, "tv", { viewport: { width: 1920, height: 1080 } });
    try {
      await page.locator('[data-f="player:subs"]').focus();
      await page.keyboard.press("Enter");
      await expect(page.locator(".bstrip.sub")).toBeVisible();
      expect(await hidden(page)).toEqual([]);
      const order = await page.evaluate(() => [...document.querySelectorAll<HTMLElement>(".bstrip .s1 [data-f]")].filter((e) => e.offsetWidth > 0).map((e) => e.dataset.f ?? ""));
      // Off, the languages, More languages, the shift's value, Style, Close.
      expect(order.length).toBeGreaterThan(6);
      await page.locator(`[data-f="${order[0]}"]`).focus();
      const forward = [order[0]];
      for (let i = 1; i < order.length; i++) {
        await page.keyboard.press("ArrowRight");
        forward.push(await focused(page));
      }
      expect(forward).toEqual(order);
      const back = [order.at(-1)];
      for (let i = 1; i < order.length; i++) {
        await page.keyboard.press("ArrowLeft");
        back.push(await focused(page));
      }
      expect(back).toEqual([...order].reverse());
    } finally {
      await close();
    }
  });
});
