/** The core's journey API (journey reviews 2026-10-20), on a core of its
 * own with a local addon that names its own titles: a series whose next
 * episode is not out yet, a series watched to its end, and a film with an
 * English subtitle. "Next episode airs <date>" stands in for Play and
 * "Play again from S1:E1" restarts a finished series; Continue Watching
 * tiles carry a remove button off TV and the title page carries it on TV;
 * the language pickers search every language the core knows; the
 * subtitle size is the profile's and the timing is per play; a foreground
 * whose stream server moved opens the playback again. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, JOURNEY_PORT, PORTS, SHOTS, startBridge, startIfPaused, until, watch } from "./core";
import { NAME, startSource } from "./source";
import { applyDiffs } from "../src/core/diff";
import type { HomeRow } from "../src/core/generated/HomeRow";
import type { HomeUpdate } from "../src/core/generated/HomeUpdate";

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let late: Server | null = null;
let media: Server | null = null;

const AIRED = "2020-01-01T00:00:00.000Z";
const LATER = "2099-01-01T00:00:00.000Z";
// Two lines, so a placement is checked against the tallest common case.
const SRT = "1\n00:00:01,000 --> 00:00:30,000\nA line\nand the line under it\n";

/** Its own titles: "mqj1" (S1:E2 not out), "mqj2" (two episodes out), the
 * film "mqjm", and "mqjv", a film with five 1080p versions of release groups
 * of their own (the pick, then one Versions group of four, three shown until
 * "Show all"). */
const MANY = ["A", "B", "C", "D", "E"].map((g) => `Journey.Versions.2008.1080p.WEB.VP9-GRP${g}.webm`);
function journeyAddon(film: string): Promise<Server> {
  const episode = (series: string, n: number, released: string) => ({ id: `${series}:1:${n}`, title: `Chapter ${n}`, season: 1, episode: n, released });
  const metas: Record<string, object> = {
    "/meta/series/mqj1.json": { id: "mqj1", type: "series", name: "Journey Waiting", description: "A synopsis comes first on the page.", videos: [episode("mqj1", 1, AIRED), episode("mqj1", 2, LATER)] },
    "/meta/series/mqj2.json": { id: "mqj2", type: "series", name: "Journey Finished", videos: [episode("mqj2", 1, AIRED), episode("mqj2", 2, AIRED)] },
    "/meta/movie/mqjm.json": { id: "mqjm", type: "movie", name: "Journey Film", releaseInfo: "2008", trailerStreams: [{ title: "Journey Film", ytId: "jrnyTrailer1" }] },
    "/meta/movie/mqjv.json": { id: "mqjv", type: "movie", name: "Journey Versions", releaseInfo: "2008" },
  };
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const url = req.url ?? "";
    if (url === "/manifest.json") {
      return json({ id: "dev.marquee.journey", version: "1.0.0", name: "Journey Fixture", resources: ["meta", "stream"], types: ["movie", "series"], idPrefixes: ["mqj"], catalogs: [] });
    }
    const meta = metas[url];
    if (meta) return json({ meta });
    if (url === "/stream/movie/mqjv.json") {
      // Each at an address of its own (a version is known by its address), all the same film.
      return json({ streams: MANY.map((file) => ({ name: "Journey Fixture", title: file, url: `${base}/v/${file}`, behaviorHints: { filename: file } })) });
    }
    if (url.startsWith("/v/")) {
      res.writeHead(302, { location: film });
      return res.end();
    }
    // An episode's file names its episode, or the core would hide it as another one.
    const episode = /^\/stream\/series\/(mqj\d):1:(\d)\.json$/.exec(decodeURIComponent(url));
    if (episode) {
      const name = episode[1] === "mqj2" ? "Journey.Finished" : "Journey.Waiting";
      const filename = `${name}.S01E0${episode[2]}.720p.WEB.VP9.webm`;
      return json({ streams: [{ name: "Journey Fixture\n720p", title: filename, url: film, behaviorHints: { filename }, subtitles: [{ id: "j1", lang: "eng", url: `${base}/j1.srt` }] }] });
    }
    if (url.startsWith("/stream/")) {
      return json({
        streams: [{ name: "Journey Fixture\n240p", url: film, behaviorHints: { filename: NAME }, subtitles: [{ id: "j1", lang: "eng", url: `${base}/j1.srt` }] }],
      });
    }
    if (url === "/j1.srt") {
      res.writeHead(200, { "content-type": "text/plain" });
      return res.end(SRT);
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

type Header = { state: { state: string } };

async function markWatched(c: Client, kind: string, id: string, videoIds: string[]) {
  const title = await c.subscribe<{ header?: Header }>("title", { kind, id });
  await until(`details for ${id}`, () => (c.header<Header>(title.sub) ?? title.now().header)?.state.state === "ready");
  for (const videoId of videoIds) await c.command(title.sub, { type: "setWatched", videoId, value: true });
  await c.req("unsubscribe", { sub: title.sub });
}

const film = { kind: "movie", itemId: "mqjm", videoId: "mqjm", title: "Journey Film", via: "Journey Fixture" };

/** A subtitle addon that answers 5 s late with one Brazilian Portuguese file,
 * as a slow subtitle addon does while the Subtitles panel is open. */
function lateSubs(): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const url = req.url ?? "";
    if (url === "/manifest.json") {
      return json({ id: "dev.marquee.late-subs", version: "1.0.0", name: "Late Subs", resources: ["subtitles"], types: ["movie", "series"], idPrefixes: ["mqj"], catalogs: [] });
    }
    if (url.startsWith("/subtitles/")) return void setTimeout(() => json({ subtitles: [{ id: "late-pob", lang: "pob", url: `${base}/pob.srt` }] }), 5000);
    if (url === "/pob.srt") {
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
  addon = await journeyAddon(`http://127.0.0.1:${(media.address() as AddressInfo).port}/media/${NAME}`);
  core = await startBridge(JOURNEY_PORT);
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  try {
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    late = await lateSubs();
    await c.req("installAddon", { url: `http://127.0.0.1:${(late.address() as AddressInfo).port}/manifest.json` });
    // An earlier picker's code: the core and the new picker both read "eng" as English.
    await c.req("setPrefs", { patch: { defaultsSeen: 1, subtitleLanguages: ["eng"] } });
    await markWatched(c, "series", "mqj1", ["mqj1:1:1"]);
    await markWatched(c, "series", "mqj2", ["mqj2:1:1", "mqj2:1:2"]);
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  late?.close();
  media?.close();
});

type Sent = { id?: number; op?: string; view?: string; event?: string; sub?: number; command?: { type?: string; rowId?: string } };

/** A page on this spec's core; `sent` is what it asked, in order, and
 * `fake` may answer a message at the socket instead of the core (a reply's
 * `ok` and `result` or `error`), or hold it with null. */
type Reply = { ok: true; result: unknown } | { ok: false; error: string };
async function page(browser: Browser, viewport = { width: 1440, height: 900 }, fake?: (m: Sent) => Reply | null | undefined) {
  const sent: Sent[] = [];
  // What the core sent the page, and a way to send it more as if from the core.
  const received: { id?: number; sub?: number; result?: { sub?: number; snapshot?: unknown }; updates?: unknown[] }[] = [];
  let toPage: ((text: string) => void) | null = null;
  const context = await browser.newContext({ viewport, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  // Every player starts muted.
  await context.addInitScript(() => {
    const play = HTMLMediaElement.prototype.play;
    HTMLMediaElement.prototype.play = function () {
      this.muted = true;
      return play.call(this);
    };
  });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
    const early: string[] = [];
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => {
      received.push(JSON.parse(String(e.data)));
      ws.send(String(e.data));
    });
    upstream.addEventListener("close", () => ws.close());
    toPage = (text) => ws.send(text);
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m)) as Sent;
      sent.push(msg);
      const answer = fake?.(msg);
      if (answer === null) return;
      if (answer !== undefined) return ws.send(JSON.stringify({ id: msg.id, ...answer }));
      if (upstream.readyState === WebSocket.OPEN) upstream.send(String(m));
      else early.push(String(m));
    });
    ws.onClose(() => upstream.close());
  });
  /** A view's latest snapshot as the core sent it, and a push of `next` to the page as an update of it. */
  const view = (name: string) => {
    const asked = [...sent].reverse().find((m) => m.op === "subscribe" && m.view === name);
    const reply = received.find((m) => m.id === asked?.id && m.result?.sub !== undefined);
    const sub = reply?.result?.sub;
    const updates = received.filter((m) => m.sub === sub && m.updates?.length);
    const latest = updates.length ? updates[updates.length - 1]?.updates?.at(-1) : reply?.result?.snapshot;
    return { latest, push: (next: unknown) => toPage?.(JSON.stringify({ sub, updates: [next] })) };
  };
  return { context, page: await context.newPage(), sent, view };
}

const focusedKey = (p: Page) => p.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
/** Marks the focused element, to tell it from a rebuilt one with the same key. */
const markFocused = (p: Page) => p.evaluate(() => ((document.activeElement as HTMLElement).dataset.mark = "kept"));
const focusedIsMarked = (p: Page) => p.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.mark === "kept");

async function prefs() {
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  try {
    return await c.req<{ subtitleLanguages: string[]; subtitleSize: string; streams: { languages: string[] } }>("setPrefs", { patch: {} });
  } finally {
    c.close();
  }
}

/** Home's Continue Watching ids as the core keeps them: the snapshot, then
 * its row diffs. */
async function watchHome() {
  const ws = new WebSocket(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  await new Promise((ok) => ws.addEventListener("open", ok));
  let rows: HomeRow[] = [];
  ws.addEventListener("message", (e) => {
    const m = JSON.parse(String(e.data));
    if (m.id === 1 && m.ok) rows = m.result.snapshot.rows;
    else if ("updates" in m) for (const u of m.updates as HomeUpdate[]) if (u.kind === "rows") rows = applyDiffs(rows, u.diffs);
  });
  ws.send(JSON.stringify({ id: 1, op: "subscribe", view: "home", params: {} }));
  return { ids: () => rows.find((r) => r.rowId === "continue-watching")?.tiles.map((t) => t.id) ?? [], close: () => ws.close() };
}

async function seedFilm(positionMs: number) {
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  try {
    await watch(c, film, positionMs, 596_000);
  } finally {
    c.close();
  }
}

const openPlayer = async (p: Page) => {
  await p.goto("/play/movie/mqjm/mqjm?form=desktop");
  await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await startIfPaused(p);
};

test("an episode not out yet: its day stands where Play was", async ({ browser }) => {
  for (const form of ["desktop", "tv"] as const) {
    const viewport = form === "tv" ? { width: 1920, height: 1080 } : { width: 1440, height: 900 };
    const { context, page: p } = await page(browser, viewport);
    await p.goto(`/title/series/mqj1?form=${form}`);
    await expect(p.locator(".airs-next")).toHaveText("Next episode airs Jan 1, 2099", { timeout: 30_000 });
    await expect(p.locator('[data-f="title:play"]')).toHaveCount(0);
    // No episode to play, so no versions to look for either.
    await expect(p.locator('[data-f="title:versions"]')).toHaveCount(0);
    await expect(p.locator(".vline")).toHaveCount(0);
    // The episode itself is a plain row with its day: no play link, no versions.
    const later = p.locator('[data-f="ep:mqj1:1:2"]');
    await expect(later).toContainText("Airs Jan 1, 2099");
    expect(await later.evaluate((el) => el.tagName)).toBe("DIV");
    await expect(p.locator('[data-f="epv:mqj1:1:2"]')).toHaveCount(0);
    await expect(p.locator('[data-f="ep:mqj1:1:1"]')).toHaveAttribute("href", "/play/series/mqj1/mqj1:1:1");
    // On TV focus goes to the first action, not to the synopsis above it.
    if (form === "tv") await expect(p.locator('[data-f="title:library"]')).toBeFocused();
    await context.close();
  }
});

test("a series watched to its end plays again from its first episode", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto("/title/series/mqj2?form=desktop");
  const play = p.locator('[data-f="title:play"]');
  await expect(play).toContainText("Play again from S1:E1", { timeout: 30_000 });
  await expect(play).toHaveAttribute("href", "/play/series/mqj2/mqj2:1:1");
  await context.close();
});

test("subtitle size is the profile's; timing moves the cues for this play", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto("/settings?form=desktop");
  const large = p.locator('[data-f="pref-subsize:large"]');
  await expect(p.locator('[data-f="pref-subsize:medium"]')).toHaveAttribute("aria-checked", "true", { timeout: 30_000 });
  await large.click();
  await expect(large).toHaveAttribute("aria-checked", "true");
  await expect.poll(async () => (await prefs()).subtitleSize).toBe("large");

  await openPlayer(p);
  await expect(p.locator(".player-scr")).toHaveAttribute("data-subsize", "large");
  // English ("eng" in the profile) turns on at start.
  const cue = () => p.evaluate(() => {
    const c = document.querySelector("video")?.textTracks[0]?.cues?.[0];
    return c ? [c.startTime, c.endTime] : null;
  });
  await expect.poll(cue, { timeout: 30_000 }).toEqual([1, 30]);
  // Timing sits under the options of a language.
  await p.locator('[data-f="player:subs"]').dispatchEvent("click");
  await p.locator('[data-f="subopen:en"]').dispatchEvent("click");
  const timing = p.locator(".sub-delay output");
  await expect(timing).toHaveText("In step");
  await p.locator('[data-f="sub:later"]').dispatchEvent("click");
  await expect(timing).toHaveText("0.5 s later");
  await expect.poll(cue).toEqual([1.5, 30.5]);
  await p.locator('[data-f="sub:earlier"]').dispatchEvent("click");
  await p.locator('[data-f="sub:earlier"]').dispatchEvent("click");
  await expect(timing).toHaveText("0.5 s earlier");
  await expect.poll(cue).toEqual([0.5, 29.5]);
  // A new play starts in step.
  await openPlayer(p);
  await expect.poll(cue, { timeout: 30_000 }).toEqual([1, 30]);
  await context.close();
});

test("the language pickers search every language the core knows", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto("/settings?form=desktop");
  // "eng" from an earlier picker reads as English, chosen, and first.
  const english = p.locator('[data-f="pref-subs:en"]');
  await expect(english).toHaveAttribute("aria-checked", "true", { timeout: 30_000 });
  const first = p.locator('[data-fgroup="pref-subs"] > .kicker small');
  await expect(first).toHaveText("English first");
  await p.locator('[data-f="pref-subs:more"]').click();
  await expect(p.locator('[data-fgroup="pref-subs"] .lang-all .chip')).toHaveCount(113);
  await p.locator('[data-f="pref-subs:search"]').fill("brazil");
  const brazil = p.locator('[data-f="pref-subs-all:pt-BR"]');
  await expect(p.locator('[data-fgroup="pref-subs"] .lang-all .chip')).toHaveCount(1);
  await brazil.click();
  await expect(brazil).toHaveAttribute("aria-checked", "true");
  // The common languages keep their places; a chosen one from the long list
  // joins the row after them, and each chosen one shows its rank.
  const row = p.locator('[data-fgroup="pref-subs"] .chips-track > .chip');
  await expect(row.nth(0)).toHaveAttribute("data-f", "pref-subs:en");
  await expect(row.nth(8)).toHaveAttribute("data-f", "pref-subs:pt-BR");
  await expect(english.locator(".chip-rank")).toHaveText("1");
  await expect(p.locator('[data-f="pref-subs:pt-BR"] .chip-rank')).toHaveText("2");
  await expect.poll(async () => (await prefs()).subtitleLanguages).toEqual(["eng", "pt-BR"]);
  // Turning English off takes the old code out too.
  await english.click();
  await expect.poll(async () => (await prefs()).subtitleLanguages).toEqual(["pt-BR"]);
  await expect(first).toHaveText("Portuguese (Brazil) first");
  await p.locator('[data-f="pref-subs:search"]').fill("klingon");
  await expect(p.locator('[data-fgroup="pref-subs"] .note')).toHaveText('No language is called "klingon".');

  // Audio keeps the language without its region: one Chinese, sent as "zh".
  await p.goto("/stream-choice?form=desktop");
  await p.locator('[data-f="choice-audio:more"]').click({ timeout: 30_000 });
  await p.locator('[data-f="choice-audio:search"]').fill("chinese");
  await expect(p.locator('[data-f="choice-audio-all:zh"]')).toHaveText("Chinese");
  await expect(p.locator('[data-fgroup="choice-audio"] .lang-all [data-f^="choice-audio-all:zh-"]')).toHaveCount(0);
  await p.locator('[data-f="choice-audio-all:zh"]').click();
  await expect.poll(async () => (await prefs()).streams.languages).toContain("zh");
  await context.close();
});

// The focus loop's tile with no art (plans/design-v3-rollout.md, phase 1a): the journey film has
// no poster and no backdrop, so its picture is the bare art box, and focus still lifts that box.
// (mqj1, the plan's example, never reaches Continue Watching: its next episode is not out.)
test("TV: a Continue Watching tile with no art lifts its art box on focus", async ({ browser }) => {
  const home = await watchHome();
  await seedFilm(120_000);
  await expect.poll(home.ids).toContain("mqjm");
  home.close();
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/?form=tv");
  const tile = p.locator('[data-f="tile:home:continue-watching:mqjm"]');
  await expect(tile).toBeVisible({ timeout: 30_000 });
  await expect(tile.locator(".pic img.in")).toHaveCount(0);
  const art = () =>
    tile.evaluate((el) => {
      for (const a of document.getAnimations()) {
        try {
          a.finish();
        } catch {
          // An endless pulse cannot finish.
        }
      }
      const s = getComputedStyle(el.querySelector(".pic") as HTMLElement);
      return { transform: s.transform, ring: s.outlineStyle === "none" ? 0 : parseFloat(s.outlineWidth) };
    });
  await p.locator('[data-f="nav:/"]').focus();
  const rest = await art();
  await tile.focus();
  const lifted = await art();
  expect(lifted.transform).not.toBe(rest.transform);
  expect(lifted.ring).toBe(0);
  await context.close();
});

test("Settings > Playback: Subtitle language names the first language, at every size", async ({ browser }) => {
  const set = async (subtitleLanguages: string[]) => {
    const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
    await c.req("setPrefs", { patch: { subtitleLanguages } }).finally(() => c.close());
  };
  await set(["tr", "eng"]);
  const sizes = { tv: { width: 1920, height: 1080 }, desktop: { width: 1440, height: 900 }, phone: { width: 390, height: 844 } };
  for (const [form, viewport] of Object.entries(sizes)) {
    const { context, page: p } = await page(browser, viewport);
    await p.goto(`/settings?form=${form}`);
    const row = p.locator('[data-fgroup="pref-subs"]');
    await expect(row.locator("> .kicker")).toContainText("Subtitle language", { timeout: 30_000 });
    await expect(row.locator("> .kicker small")).toHaveText("Turkish first");
    await expect(row.locator('[data-f="pref-subs:tr"] .chip-rank')).toHaveText("1");
    await expect(row.locator('[data-f="pref-subs:en"] .chip-rank')).toHaveText("2");
    // One row for it: no second subtitle language picker on the page.
    await expect(p.locator('[data-fgroup="pref-subs"]')).toHaveCount(1);
    await expect(p.locator(".kicker", { hasText: /^Subtitles/ })).toHaveCount(0);
    await row.scrollIntoViewIfNeeded();
    await p.waitForTimeout(500);
    await p.screenshot({ path: `${SHOTS}/settings-subtitle-language-${form}.jpg`, type: "jpeg", quality: 80 });
    await context.close();
  }
  await set(["eng"]);
});

test("Continue Watching: the tile's remove button off TV, the title page on TV", async ({ browser }) => {
  const home = await watchHome();
  await seedFilm(120_000);
  await expect.poll(home.ids).toContain("mqjm");
  {
    const { context, page: p } = await page(browser);
    await p.goto("/?form=desktop");
    const tile = p.locator('[data-f="tile:home:continue-watching:mqjm"]');
    await expect(tile).toBeVisible({ timeout: 30_000 });
    const slot = p.locator(".tile-slot", { has: tile });
    await slot.hover();
    const remove = slot.locator(".tile-x");
    await expect(remove).toHaveAttribute("aria-label", "Remove Journey Film from Continue Watching");
    // It reaches every synced device, so the first press only asks.
    await remove.click();
    await expect(remove).toHaveText("Press again to remove");
    await p.waitForTimeout(500);
    await expect(tile).toBeVisible();
    expect(await home.ids()).toContain("mqjm");
    await remove.click();
    await expect(tile).toHaveCount(0);
    // The core dropped it, not just this page.
    await expect.poll(home.ids).not.toContain("mqjm");
    await context.close();
  }
  // A newer play brings it back; TV has no button on the tile, the title page has one.
  await seedFilm(180_000);
  await expect.poll(home.ids).toContain("mqjm");
  {
    const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
    await p.goto("/?form=tv");
    const tile = p.locator('[data-f="tile:home:continue-watching:mqjm"]');
    await expect(tile).toBeVisible({ timeout: 30_000 });
    await expect(p.locator(".tile-x")).toHaveCount(0);
    // From Home's tile to the title page, by remote.
    await tile.focus();
    await p.keyboard.press("Enter");
    const remove = p.locator('[data-f="title:uncontinue"]');
    await expect(remove).toBeVisible({ timeout: 30_000 });
    await remove.focus();
    // It reaches every synced device: the first OK only asks.
    await p.keyboard.press("Enter");
    await expect(remove).toContainText("Press again to remove");
    await p.waitForTimeout(500);
    expect(await home.ids()).toContain("mqjm");
    await p.keyboard.press("Enter");
    await expect(remove).toHaveCount(0);
    await expect.poll(home.ids).not.toContain("mqjm");
    // Focus never falls to the page.
    await expect.poll(() => p.evaluate(() => document.activeElement !== document.body)).toBe(true);
    // Back to Home, where the tile it remembers is gone: the focus lands on
    // the screen at once, never on nothing for seconds.
    await p.keyboard.press("Escape");
    await expect(p).toHaveURL(/127\.0\.0\.1:\d+\/(\?.*)?$/);
    await expect.poll(() => p.evaluate(() => document.activeElement !== document.body && !!document.activeElement), { timeout: 1500 }).toBe(true);
    await context.close();
  }
  home.close();
});

test("back in front after the stream server moved: the playback opens again", async ({ browser }) => {
  const { context, page: p, sent } = await page(browser, undefined, (m) => (m.op === "lifecycle" && m.event === "foreground" ? { ok: true, result: { serverMoved: true } } : undefined));
  await openPlayer(p);
  const opens = () => sent.filter((m) => m.op === "subscribe" && m.view === "playback").length;
  expect(opens()).toBe(1);
  const visibility = (hidden: boolean) =>
    p.evaluate((h) => {
      Object.defineProperty(document, "hidden", { value: h, configurable: true });
      document.dispatchEvent(new Event("visibilitychange"));
    }, hidden);
  await visibility(true);
  await expect.poll(() => sent.some((m) => m.op === "lifecycle" && m.event === "background")).toBe(true);
  expect(opens()).toBe(1);
  await visibility(false);
  await expect.poll(opens).toBe(2);
  await expect.poll(() => p.evaluate(() => (document.querySelector("video")?.readyState ?? 0) >= 1), { timeout: 30_000 }).toBe(true);
  await context.close();
});

test("a screen that could not open offers Try again, focused, which opens it anew", async ({ browser }) => {
  // Each view is refused once at the socket, as a core that did not answer.
  const refused = new Set<string>();
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 }, (m) => {
    if (m.op !== "subscribe" || !m.view || refused.has(m.view) || !["title", "library", "discover", "import"].includes(m.view)) return undefined;
    refused.add(m.view);
    return { ok: false, error: "the core did not answer" };
  });
  const screens = [
    { path: "/title/series/mqj2", key: "title:retry", opened: '[data-f="title:play"]' },
    { path: "/library", key: "library:retry", opened: '[data-f="library:discover"]' },
    { path: "/discover", key: "discover:retry", opened: "text=Nothing to browse yet" },
  ];
  for (const s of screens) {
    await p.goto(`${s.path}?form=tv`);
    const retry = p.locator(`[data-f="${s.key}"]`);
    await expect(retry).toBeVisible({ timeout: 30_000 });
    await expect(retry).toBeFocused();
    await p.keyboard.press("Enter");
    await expect(p.locator(s.opened).first()).toBeVisible({ timeout: 30_000 });
    await expect(retry).toHaveCount(0);
  }
  // Import: the second try asks the core again.
  await p.goto("/import?form=tv");
  const retry = p.locator('[data-f="import:retry"]');
  await expect(retry).toBeFocused({ timeout: 30_000 });
  await p.keyboard.press("Enter");
  await expect(p.locator("h2", { hasText: "Import could not start" })).toHaveCount(0, { timeout: 30_000 });
  await context.close();
});

test("Layout: Automatic follows the device; a chosen form is kept", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto("/settings?form=tv");
  const auto = p.locator('[data-f="dev-form:null"]');
  await expect(p.locator('[data-f="dev-form:tv"]')).toHaveAttribute("aria-checked", "true", { timeout: 30_000 });
  await expect(auto).toHaveAttribute("aria-checked", "false");
  await auto.click();
  await expect(auto).toHaveAttribute("aria-checked", "true");
  // A 1440 wide window is a desktop by detection, and stays automatic.
  await expect(p.locator("html")).toHaveAttribute("data-form", "desktop");
  await p.goto("/settings");
  await expect(p.locator('[data-f="dev-form:null"]')).toHaveAttribute("aria-checked", "true", { timeout: 30_000 });
  expect(await p.evaluate(() => localStorage.getItem("mq.form"))).toBeNull();
  await context.close();
});

test("the player: Left and Right skip with the controls hidden, Space and K play or pause, a click and a drag", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await openPlayer(p);
  const time = () => p.evaluate(() => document.querySelector("video")?.currentTime ?? 0);
  const paused = () => p.evaluate(() => document.querySelector("video")?.paused ?? true);
  await expect.poll(time, { timeout: 30_000 }).toBeGreaterThan(30);
  // Controls hidden: Left skips back 10 s at once and brings them up.
  await expect(p.locator(".player-scr")).toHaveClass(/idle/, { timeout: 10_000 });
  const before = await time();
  await p.keyboard.press("ArrowLeft");
  await expect(p.locator(".player-scr")).not.toHaveClass(/idle/);
  expect(await time()).toBeLessThan(before - 8);
  await expect(p.locator(".player-scr")).toHaveClass(/idle/, { timeout: 10_000 });
  const back = await time();
  await p.keyboard.press("ArrowRight");
  expect(await time()).toBeGreaterThan(back + 8);
  // K, then Space with nothing focused, play and pause.
  await p.keyboard.press("k");
  await expect.poll(paused).toBe(true);
  await p.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await p.keyboard.press(" ");
  await expect.poll(paused).toBe(false);
  // With the controls up, a click on the picture pauses.
  await p.mouse.move(700, 300);
  await expect(p.locator(".player-scr")).not.toHaveClass(/idle/);
  await p.mouse.click(700, 300);
  await expect.poll(paused).toBe(true);
  // A drag on the bar follows the pointer: from a quarter to the middle.
  const bar = await p.locator(".scrub").boundingBox();
  if (!bar) throw new Error("no progress bar");
  const total = await p.evaluate(() => document.querySelector("video")?.duration ?? 0);
  const y = bar.y + bar.height / 2;
  await p.mouse.move(bar.x + bar.width * 0.25, y);
  await p.mouse.down();
  await expect.poll(time).toBeLessThan(total * 0.3);
  await p.mouse.move(bar.x + bar.width * 0.5, y, { steps: 8 });
  await p.mouse.up();
  await expect.poll(async () => Math.abs((await time()) - total * 0.5)).toBeLessThan(total * 0.03);
  await context.close();
});

test("Start over beside Resume plays the same video from its beginning", async ({ browser }) => {
  await seedFilm(240_000);
  const { context, page: p } = await page(browser);
  await p.goto("/title/movie/mqjm?form=desktop");
  await expect(p.locator('[data-f="title:play"]')).toContainText("Resume", { timeout: 30_000 });
  const again = p.locator('[data-f="title:startover"]');
  await expect(again).toHaveText("Start over");
  await again.click();
  await expect(p).toHaveURL(/\/play\/movie\/mqjm\/mqjm$/);
  await startIfPaused(p);
  const time = () => p.evaluate(() => document.querySelector("video")?.currentTime ?? 0);
  await expect.poll(time, { timeout: 30_000 }).toBeGreaterThan(0.5);
  expect(await time()).toBeLessThan(30);
  await context.close();
});

test("a raw reason reads in plain words, the core's text behind Details", async ({ browser }) => {
  const raw = "network: error sending request for url (http://127.0.0.1:9/catalog.json): Connection refused (os error 61)";
  const { context, page: p } = await page(browser, undefined, (m) => (m.op === "subscribe" && m.view === "library" ? { ok: false, error: raw } : undefined));
  await p.goto("/library?form=desktop");
  const problem = p.locator(".problem").first();
  await expect(problem).toContainText("It could not be reached. Check the connection, then try again.", { timeout: 30_000 });
  await expect(problem.locator(".why-raw code")).toBeHidden();
  await problem.locator(".why-raw summary").click();
  await expect(problem.locator(".why-raw code")).toHaveText(raw);
  await context.close();
});

test("a trailer pauses and plays from the bar and the remote's play key", async ({ browser }) => {
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  // A stand-in for YouTube's player that tells the page each command it got.
  await p.route("https://www.youtube-nocookie.com/embed/**", (route) =>
    route.fulfill({ contentType: "text/html", body: "<script>addEventListener('message', (e) => parent.postMessage({ got: e.data }, '*'));</script>" }),
  );
  await p.addInitScript(() => {
    const w = window as unknown as { got: string[] };
    w.got = [];
    addEventListener("message", (e) => e.data?.got && w.got.push(JSON.parse(e.data.got).func));
  });
  await p.goto("/title/movie/mqjm?form=tv");
  await p.locator('[data-f="title:trailer"]').focus({ timeout: 30_000 });
  await p.keyboard.press("Enter");
  const toggle = p.locator('[data-f="trailer:toggle"]');
  await expect(toggle).toHaveText("Pause");
  await expect(p.locator(".trailer iframe")).toHaveAttribute("src", /enablejsapi=1/);
  const got = () => p.evaluate(() => (window as unknown as { got: string[] }).got);
  await toggle.focus();
  await p.keyboard.press("Enter");
  await expect(toggle).toHaveText("Play");
  await expect.poll(got).toEqual(["pauseVideo"]);
  await p.keyboard.press("MediaPlayPause");
  await expect(toggle).toHaveText("Pause");
  await expect.poll(got).toEqual(["pauseVideo", "playVideo"]);
  await context.close();
});

test("screen readers: each page is named, the title has its heading, episodes say their number", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto("/library?form=desktop");
  await expect(p).toHaveTitle("Library · Marquee", { timeout: 30_000 });
  await p.goto("/title/series/mqj2?form=desktop");
  await expect(p).toHaveTitle("Journey Finished · Marquee", { timeout: 30_000 });
  await expect(p.getByRole("heading", { level: 1 })).toHaveText("Journey Finished");
  // The season's episodes are the panel its tab controls; each says its number.
  await expect(p.locator('[role="tab"][aria-selected="true"]')).toHaveAttribute("aria-controls", "season-episodes");
  await expect(p.locator('#season-episodes[role="tabpanel"]')).toBeVisible();
  await expect(p.getByRole("link", { name: /^Episode 2, Chapter 2/ })).toBeVisible();
  await p.goto("/title/series/mqj2/versions/mqj2:1:1?form=desktop");
  await expect(p).toHaveTitle(/^Versions · .+ · Marquee$/, { timeout: 30_000 });
  await context.close();
});

test("before the core answers, the wordmark shows, never a blank screen", async ({ browser }) => {
  // The core never says who is watching on this page.
  const { context, page: p } = await page(browser, undefined, (m) => (m.op === "subscribe" && m.view === "profiles" ? null : undefined));
  await p.goto("/?form=desktop");
  const mark = p.locator(".boot.brand");
  await expect(mark).toHaveText("Marquee");
  await expect(mark).toHaveAttribute("role", "status");
  await context.close();
  // Once it has, the app replaces it.
  const { context: next, page: q } = await page(browser);
  await q.goto("/library?form=desktop");
  await expect(q.locator("h1")).toHaveText("Library", { timeout: 30_000 });
  await expect(q.locator(".boot")).toHaveCount(0);
  await next.close();
});

test("Search with no addon that can search says so, with the way to one", async ({ browser }) => {
  // This core's only addon has no search catalog (core `searchable` is 0).
  const { context, page: p } = await page(browser);
  await p.goto("/search?form=desktop");
  await p.locator('[data-f="search:input"]').fill("journey", { timeout: 30_000 });
  await expect(p.locator(".none-search p")).toHaveText("None of your addons can search.", { timeout: 30_000 });
  await expect(p.getByText(/No matches/)).toHaveCount(0);
  await p.locator('[data-f="search:addons"]').click();
  await expect(p).toHaveURL(/\/recommended-addons/);
  await context.close();
});

test("the Continue Watching button says when the core refused", async ({ browser }) => {
  await seedFilm(200_000);
  const { context, page: p } = await page(browser, undefined, (m) => (m.op === "removeFromContinue" ? { ok: false, error: "network: connection refused" } : undefined));
  await p.goto("/?form=desktop");
  const tile = p.locator('[data-f="tile:home:continue-watching:mqjm"]');
  await expect(tile).toBeVisible({ timeout: 30_000 });
  const slot = p.locator(".tile-slot", { has: tile });
  await slot.hover();
  const remove = slot.locator(".tile-x");
  await remove.click();
  await remove.click();
  await expect(remove).toHaveText("Not removed, try again");
  await expect(tile).toBeVisible();
  await context.close();
});

test("TV: a focused element rebuilt by live data keeps the focus; one gone hands it to its neighbour", async ({ browser }) => {
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/settings?form=tv");
  const chip = p.locator('[data-f="pref-subsize:large"]');
  await expect(chip).toBeVisible({ timeout: 30_000 });
  await chip.focus();
  // A copy with the same key comes back at the end of the row (a list
  // rebuilt in a new order, a title moved to another row): the focus follows
  // the key there, not the chip that took its old place.
  await p.evaluate(() => {
    const el = document.activeElement as HTMLElement;
    const copy = el.cloneNode(true);
    el.parentElement?.append(copy);
    el.remove();
  });
  await expect.poll(() => focusedKey(p)).toBe("pref-subsize:large");
  // The remote goes on from there.
  await p.keyboard.press("ArrowLeft");
  await expect.poll(() => focusedKey(p)).toBe("pref-subsize:extraLarge");
  // Gone for good: a chip of its own row, not the top of Settings.
  await p.evaluate(() => (document.activeElement as HTMLElement).remove());
  await expect.poll(() => focusedKey(p)).toMatch(/^pref-subsize:(medium|large)$/);
  await context.close();
});

test("TV: OK on a language chip keeps the focus on that chip, numbered by its rank", async ({ browser }) => {
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/settings?form=tv");
  const spanish = p.locator('[data-f="pref-subs:es"]');
  await expect(spanish).toBeVisible({ timeout: 30_000 });
  await spanish.focus();
  await markFocused(p);
  const before = await p.locator('[data-fgroup="pref-subs"] .chips-track > .chip').evaluateAll((els) => els.map((e) => e.getAttribute("data-f")));
  await p.keyboard.press("Enter");
  await expect(spanish).toHaveAttribute("aria-checked", "true");
  // The same chip, in the same place, now with its rank.
  expect(await focusedKey(p)).toBe("pref-subs:es");
  expect(await focusedIsMarked(p)).toBe(true);
  const rank = (await prefs()).subtitleLanguages.length;
  await expect(spanish.locator(".chip-rank")).toHaveText(String(rank));
  expect(await p.locator('[data-fgroup="pref-subs"] .chips-track > .chip').evaluateAll((els) => els.map((e) => e.getAttribute("data-f")))).toEqual(before);
  await p.keyboard.press("Enter");
  await expect(spanish).toHaveAttribute("aria-checked", "false");
  expect(await focusedKey(p)).toBe("pref-subs:es");
  expect(await focusedIsMarked(p)).toBe(true);
  await context.close();
});

test("TV: a late subtitle addon leaves the remote on the subtitle it was on", async ({ browser }) => {
  // Brazilian Portuguese is the profile's first language: its late track
  // lists above English, so the English option moves while the remote is on it.
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  await c.req("setPrefs", { patch: { subtitleLanguages: ["pt-BR", "eng"] } }).finally(() => c.close());
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/play/movie/mqjm/mqjm?form=tv");
  await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await p.locator('[data-f="player:subs"]').focus();
  await p.keyboard.press("Enter");
  await expect.poll(() => focusedKey(p)).toBe("sub:off");
  await expect(p.locator('[data-f^="sublang:"]').first()).toBeVisible({ timeout: 30_000 });
  // Brazilian Portuguese is still on its way (the Late Subs addon answers 5 s in).
  const late = p.locator('[data-f="sublang:pt-BR"]');
  await expect(late).toHaveCount(0);
  await p.keyboard.press("ArrowDown");
  await expect.poll(() => focusedKey(p)).toBe("sublang:en");
  const on = await focusedKey(p);
  await markFocused(p);
  await expect(late).toHaveCount(1, { timeout: 15_000 });
  // It lists first, above the English row the remote is on.
  await expect(p.locator('[data-f^="sublang:"]').first()).toHaveAttribute("data-f", "sublang:pt-BR");
  await p.waitForTimeout(300);
  expect(await focusedKey(p)).toBe(on);
  expect(await focusedIsMarked(p)).toBe(true);
  await context.close();
});

test("desktop: full screen, the level and mute carry on to the next episode; F and a double click", async ({ browser }) => {
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  await c.req("setPrefs", { patch: { autoplayNext: false } }).finally(() => c.close());
  const { context, page: p } = await page(browser);
  await p.goto("/play/series/mqj2/mqj2:1:1?form=desktop");
  await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await startIfPaused(p);
  const full = () => p.evaluate(() => !!document.fullscreenElement);
  // F in and out; a double click on the picture in.
  await p.keyboard.press("f");
  await expect.poll(full).toBe(true);
  await p.keyboard.press("f");
  await expect.poll(full).toBe(false);
  await p.mouse.move(700, 300);
  await p.mouse.dblclick(700, 300);
  await expect.poll(full).toBe(true);
  // Lower, then muted.
  await p.mouse.move(700, 320);
  await p.locator('[data-f="player:volume"]').evaluate((el) => {
    (el as HTMLInputElement).value = "30";
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await p.locator('[data-f="player:mute"]').click();
  // The next episode from Up next, near the end.
  await p.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.currentTime = v.duration - 12;
  });
  await p.locator('[data-f="next:play"]').click({ timeout: 30_000 });
  await expect(p).toHaveURL(/mqj2:1:2/);
  await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  expect(await full()).toBe(true);
  await expect(p.locator('[data-f="player:volume"]')).toHaveValue("30");
  await expect(p.locator('[data-f="player:mute"]')).toHaveAttribute("aria-pressed", "true");
  // Leaving the play leaves full screen.
  await p.locator('[data-f="player:leave"]').click();
  await expect(p).toHaveURL(/\/title\/series\/mqj2/);
  await expect.poll(full).toBe(false);
  await context.close();
});

test("the next episode card says Back, naming the title only when Back goes there", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto("/play/series/mqj2/mqj2:1:1?form=desktop");
  await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await startIfPaused(p);
  await p.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.currentTime = v.duration - 1;
  });
  await expect(p.locator('[data-f="end:next"]')).toBeVisible({ timeout: 30_000 });
  await expect(p.locator('[data-f="end:back"]')).toHaveText("Back");
  await context.close();
});

test("Episodes from a play opened on Versions keeps the title once in history", async ({ browser }) => {
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/title/series/mqj2?form=tv");
  const versions = p.locator('[data-f="title:versions"]');
  await expect(versions).toBeVisible({ timeout: 30_000 });
  const depth = () => p.evaluate(() => (history.state as { _depth?: number } | null)?._depth ?? -1);
  const atTitle = await depth();
  await versions.focus();
  await p.keyboard.press("Enter");
  await expect(p).toHaveURL(/\/versions\//);
  await expect(p.locator('[data-f="versions:play"]')).toBeVisible({ timeout: 30_000 });
  await p.locator('[data-f="versions:play"]').focus();
  await p.keyboard.press("Enter");
  await expect(p).toHaveURL(/\/play\//);
  await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await p.keyboard.press("ArrowUp");
  await p.locator('[data-f="player:episodes"]').focus();
  await p.keyboard.press("Enter");
  await expect(p).toHaveURL(/\/title\/series\/mqj2(\?|$)/);
  // Back past Versions to the title's own entry, not a second one over it.
  await expect.poll(depth).toBe(atTitle);
  await context.close();
});

test("Start over leaves the history entry once the play has started", async ({ browser }) => {
  await seedFilm(240_000);
  const { context, page: p } = await page(browser);
  await p.goto("/title/movie/mqjm?form=desktop");
  await p.locator('[data-f="title:startover"]').click({ timeout: 30_000 });
  await expect(p).toHaveURL(/\/play\/movie\/mqjm\/mqjm$/);
  expect(await p.evaluate(() => (history.state as { startOver?: boolean } | null)?.startOver)).toBe(true);
  await startIfPaused(p);
  // A reload or Forward from here resumes, it does not start over again.
  await expect.poll(() => p.evaluate(() => (history.state as { startOver?: boolean } | null)?.startOver ?? null), { timeout: 30_000 }).toBe(null);
  await context.close();
});

test("TV: Versions keeps an opened group on Back, and the focused row when better rows arrive", async ({ browser }) => {
  const { context, page: p, view } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/title/movie/mqjv/versions/mqjv?form=tv");
  await expect(p.locator(".vrow")).toHaveCount(3, { timeout: 30_000 });
  await expect(p.locator(".vstate .live")).toHaveCount(0, { timeout: 30_000 });
  // Show all 4, play the fourth, Back: the group is still open, on that row.
  const fold = p.locator('[data-f^="fold:"]');
  await fold.focus();
  await p.keyboard.press("Enter");
  await expect(p.locator(".vrow")).toHaveCount(4);
  const fourth = await p.locator(".vrow").nth(3).getAttribute("data-f");
  await p.locator(".vrow").nth(3).focus();
  await p.keyboard.press("Enter");
  await expect(p).toHaveURL(/\/play\//);
  await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await p.keyboard.press("Escape");
  await expect(p).toHaveURL(/\/versions\//);
  await expect.poll(() => focusedKey(p), { timeout: 2000 }).toBe(fourth);
  // Show the best 3 again; the remote on the third row.
  await fold.focus();
  await p.keyboard.press("Enter");
  await expect(p.locator(".vrow")).toHaveCount(3);
  await p.locator(".vrow").nth(2).focus();
  const third = await focusedKey(p);
  await markFocused(p);
  await p.waitForTimeout(500);
  // Two better rows arrive at the top of that group: the focused row is now
  // fifth, and stays listed and focused.
  type Snap = { rows: { key: string; group: string; release?: unknown }[] };
  const snap = view("versions").latest as Snap;
  const first = snap.rows[0];
  if (!first) throw new Error("no rows");
  view("versions").push({ ...snap, rows: [{ ...first, key: `${first.key}-new1` }, { ...first, key: `${first.key}-new2` }, ...snap.rows] });
  await expect(p.locator(`[data-f="v:${first.key}-new1"]`)).toHaveCount(1);
  expect(await focusedKey(p)).toBe(third);
  expect(await focusedIsMarked(p)).toBe(true);
  await context.close();
});

test("the player, Calendar and Search say why they could not open in plain words, the core's text behind Details", async ({ browser }) => {
  const raw = "network: error sending request for url (http://127.0.0.1:9/x): Connection refused (os error 61)";
  const plain = "It could not be reached. Check the connection, then try again.";
  const { context, page: p } = await page(browser, undefined, (m) =>
    m.op === "subscribe" && (m.view === "versions" || m.view === "calendar" || m.view === "search") ? { ok: false, error: raw } : undefined,
  );
  await p.goto("/play/movie/mqjm/mqjm?form=desktop");
  const card = p.locator(".endcard");
  await expect(card.locator(".st")).toHaveText("Versions could not be loaded", { timeout: 30_000 });
  await expect(card.locator(".end-detail")).toHaveText(plain);
  await expect(card.locator(".why-raw code")).toHaveText(raw);
  await p.goto("/calendar?form=desktop");
  await expect(p.locator(".cal-problem")).toContainText("The calendar could not open.", { timeout: 30_000 });
  await expect(p.locator(".cal-problem")).toContainText(plain);
  await p.goto("/search?form=desktop");
  await expect(p.locator(".palette-body")).toContainText("Search could not open.", { timeout: 30_000 });
  await expect(p.locator(".palette-body")).toContainText(plain);
  await context.close();
});

/** A catalog addon with a catalog that always answers and one whose first
 * answer is an error (500), then its list: a catalog that failed once.
 * `asked` counts the requests to that one. */
async function flakyCatalog() {
  let asked = 0;
  const server = createServer((req, res) => {
    const json = (status: number, body: unknown) => {
      res.writeHead(status, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    if (req.url === "/manifest.json") {
      const catalogs = [
        { type: "movie", id: "jclassics", name: "Journey Classics" },
        { type: "movie", id: "jpicks", name: "Journey Picks" },
      ];
      return json(200, { id: "dev.marquee.flaky", version: "1.0.0", name: "Flaky Catalog", resources: ["catalog"], types: ["movie"], idPrefixes: ["mqj"], catalogs });
    }
    // A catalog that always answers, so Home has rows besides the failed one.
    if (req.url?.startsWith("/catalog/movie/jclassics")) return json(200, { metas: [{ id: "mqjv", type: "movie", name: "Journey Versions" }] });
    if (req.url?.startsWith("/catalog/movie/jpicks")) {
      asked += 1;
      if (asked === 1) return json(500, { error: "busy" });
      return void setTimeout(() => json(200, { metas: [{ id: "mqjm", type: "movie", name: "Journey Film" }] }), 1500);
    }
    res.writeHead(404);
    res.end();
  });
  await new Promise<void>((ok) => server.listen(0, "127.0.0.1", () => ok()));
  return { url: `http://127.0.0.1:${(server.address() as AddressInfo).port}/manifest.json`, asked: () => asked, close: () => server.close() };
}

test("Home: Try again on a failed row asks that one catalog again, not all of Home", async ({ browser }) => {
  const flaky = await flakyCatalog();
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  try {
    await c.req("installAddon", { url: flaky.url });
    const { context, page: p, sent } = await page(browser, { width: 1920, height: 1080 });
    await p.goto("/?form=tv");
    const row = p.locator('section.row[data-fgroup*="jpicks"]');
    const retry = row.locator('[data-f^="retry:"]');
    await expect(retry).toBeVisible({ timeout: 30_000 });
    await expect(row.locator(".row-msg")).toContainText("It had an error on its side (500).");
    const opened = () => sent.filter((m) => m.op === "subscribe" && m.view === "home").length;
    const before = opened();
    // By remote: the row's Try again. The focus stays on it while the row
    // asks again ("Trying again"), then goes to the row's first title.
    await retry.focus();
    const key = await focusedKey(p);
    await p.keyboard.press("Enter");
    await expect(retry).toHaveText("Trying again");
    await expect(retry).toHaveAttribute("aria-disabled", "true");
    expect(await focusedKey(p)).toBe(key);
    await expect(row.locator(".tile")).toHaveCount(1, { timeout: 30_000 });
    await expect.poll(() => focusedKey(p)).toMatch(/^tile:home:.*jpicks.*:mqjm$/);
    expect(sent.filter((m) => m.op === "command" && m.command?.type === "retryRow").map((m) => m.command?.rowId)).toEqual([expect.stringContaining("jpicks")]);
    // That catalog was asked once more; Home was not opened again.
    expect(flaky.asked()).toBe(2);
    expect(opened()).toBe(before);
    await context.close();
  } finally {
    await c.req("removeAddon", { url: flaky.url }).catch(() => undefined);
    c.close();
    flaky.close();
  }
});

test("TV: Remove on the title page says when the core refused", async ({ browser }) => {
  await seedFilm(210_000);
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 }, (m) => (m.op === "removeFromContinue" ? { ok: false, error: "network: connection refused" } : undefined));
  await p.goto("/title/movie/mqjm?form=tv");
  const remove = p.locator('[data-f="title:uncontinue"]');
  await expect(remove).toBeVisible({ timeout: 30_000 });
  await remove.focus();
  await p.keyboard.press("Enter");
  await p.keyboard.press("Enter");
  await expect(remove).toContainText("Not removed, try again");
  await context.close();
});

test("TV: Back after the remembered title left the list lands in its place, not at the top", async ({ browser }) => {
  // Four titles in the library, the third one opened and taken out of it.
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  try {
    for (const [kind, id] of [["series", "mqj1"], ["series", "mqj2"], ["movie", "mqjm"], ["movie", "mqjv"]] as const) {
      const title = await c.subscribe<{ header?: Header }>("title", { kind, id });
      await until(`details for ${id}`, () => (c.header<Header>(title.sub) ?? title.now().header)?.state.state === "ready");
      await c.command(title.sub, { type: "setInLibrary", value: true });
      await c.req("unsubscribe", { sub: title.sub });
    }
  } finally {
    c.close();
  }
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/library?form=tv");
  const tiles = p.locator('[data-fgroup="grid:library"] [data-f^="g:library:"]');
  await expect(tiles).toHaveCount(4, { timeout: 30_000 });
  const third = tiles.nth(2);
  const gone = await third.getAttribute("data-f");
  const next = await tiles.nth(3).getAttribute("data-f");
  await third.focus();
  await p.keyboard.press("Enter");
  await expect(p).toHaveURL(/\/title\//);
  const library = p.locator('[data-f="title:library"]');
  await expect(library).toHaveAttribute("aria-pressed", "true", { timeout: 30_000 });
  await library.focus();
  await p.keyboard.press("Enter");
  await expect(library).toHaveAttribute("aria-pressed", "false");
  await p.keyboard.press("Escape");
  await expect(p).toHaveURL(/\/library/);
  await expect(tiles).toHaveCount(3);
  // The title that took its place has the focus, not the top of Library.
  await expect.poll(() => focusedKey(p), { timeout: 3000 }).toBe(next);
  expect(next).not.toBe(gone);
  await context.close();
});

test("one title entry in history after autoplay too: Episodes and Versions from the next episode", async ({ browser }) => {
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  await c.req("setPrefs", { patch: { autoplayNext: true } }).finally(() => c.close());
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  const depth = () => p.evaluate(() => (history.state as { _depth?: number } | null)?._depth ?? -1);
  // The first episode's Versions from its row on the title page, its play,
  // and autoplay on to the second episode.
  const toNextEpisode = async () => {
    await p.goto("/title/series/mqj2?form=tv");
    const episode = p.locator('[data-f="ep:mqj2:1:1"]');
    await expect(episode).toBeVisible({ timeout: 30_000 });
    const atTitle = await depth();
    // Right from the episode reaches its Choose a version.
    await episode.focus();
    await p.keyboard.press("ArrowRight");
    await expect.poll(() => focusedKey(p)).toBe("epv:mqj2:1:1");
    await p.keyboard.press("Enter");
    await expect(p).toHaveURL(/\/versions\/mqj2:1:1/);
    await expect(p.locator('[data-f="versions:play"]')).toBeVisible({ timeout: 30_000 });
    await p.locator('[data-f="versions:play"]').focus();
    await p.keyboard.press("Enter");
    await expect(p).toHaveURL(/mqj2:1:1/);
    await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
    await startIfPaused(p);
    // The episode ends; autoplay opens the next one in its place.
    await p.evaluate(() => {
      const v = document.querySelector("video");
      if (v) v.currentTime = v.duration - 0.5;
    });
    await expect(p).toHaveURL(/mqj2:1:2/, { timeout: 30_000 });
    await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
    return atTitle;
  };
  // Episodes: back to the title's own entry.
  let atTitle = await toNextEpisode();
  await p.keyboard.press("ArrowUp");
  await p.locator('[data-f="player:episodes"]').focus();
  await p.keyboard.press("Enter");
  await expect(p).toHaveURL(/\/title\/series\/mqj2(\?|$)/);
  await expect.poll(depth).toBe(atTitle);
  // Versions: this episode's, in place of the first one's.
  atTitle = await toNextEpisode();
  await p.keyboard.press("ArrowUp");
  await p.locator('[data-f="player:versionsact"]').focus();
  await p.keyboard.press("Enter");
  await expect(p).toHaveURL(/\/versions\/mqj2:1:2/);
  await expect.poll(depth).toBe(atTitle + 1);
  await context.close();
});

test("TV: Up and Down on an addon keep the remote on that addon", async ({ browser }) => {
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/addons?form=tv");
  const names = () => p.locator(".addon h2").evaluateAll((els) => els.map((e) => e.childNodes[0]?.textContent?.trim()));
  await expect(p.locator('[data-f="addon:1:up"]')).toBeVisible({ timeout: 30_000 });
  const before = await names();
  await p.locator('[data-f="addon:1:up"]').focus();
  await p.keyboard.press("Enter");
  await expect.poll(names).toEqual([before[1], before[0], ...before.slice(2)]);
  // The focus moved up with it: OK again would move it, not the other one.
  await expect.poll(() => focusedKey(p)).toBe("addon:0:up");
  await p.locator('[data-f="addon:0:down"]').focus();
  await p.keyboard.press("Enter");
  await expect.poll(names).toEqual(before);
  await expect.poll(() => focusedKey(p)).toBe("addon:1:down");
  await context.close();
});

test("TV: closing Search leaves the page where it was scrolled", async ({ browser }) => {
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/settings?form=tv");
  const far = p.locator('[data-f="pref-subsize:large"]');
  await expect(far).toBeVisible({ timeout: 30_000 });
  await far.focus();
  await p.keyboard.press("ArrowRight");
  const scrolled = () => p.evaluate(() => document.querySelector(".settings-page")?.scrollTop ?? document.scrollingElement?.scrollTop ?? 0);
  await expect.poll(scrolled).toBeGreaterThan(100);
  // Once the page has come to rest (the move scrolls it smoothly).
  await expect
    .poll(async () => {
      const a = await scrolled();
      await p.waitForTimeout(200);
      return (await scrolled()) === a;
    })
    .toBe(true);
  const at = await scrolled();
  await p.locator('[data-f="nav:/search"]').focus();
  await p.keyboard.press("Enter");
  await expect(p.locator(".palette-layer .palette")).toBeVisible();
  await p.keyboard.press("Escape");
  await expect(p.locator(".palette-layer")).toHaveCount(0);
  await expect.poll(() => focusedKey(p)).toBe("nav:/search");
  await p.waitForTimeout(500);
  expect(await scrolled()).toBe(at);
  await context.close();
});

test("TV: Up Next waits while a panel is open", async ({ browser }) => {
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  await c.req("setPrefs", { patch: { autoplayNext: false } }).finally(() => c.close());
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/play/series/mqj2/mqj2:1:1?form=tv");
  await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await startIfPaused(p);
  await p.locator('[data-f="player:subs"]').focus();
  await p.keyboard.press("Enter");
  await expect.poll(() => focusedKey(p)).toBe("sub:off");
  // The credits start with the Subtitles panel open.
  await p.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.currentTime = v.duration - 15;
  });
  await p.waitForTimeout(1500);
  await expect(p.locator(".next")).toHaveCount(0);
  expect(await focusedKey(p)).toMatch(/^sub:/);
  // Closed, Up Next comes.
  await p.keyboard.press("Escape");
  await expect(p.locator(".next")).toBeVisible({ timeout: 10_000 });
  await context.close();
});

/** A Discover catalog that pages: its first answer fails, then 30 titles;
 * the next page fails once, then 10 more; then nothing. */
async function shelfCatalog() {
  const asked = new Map<number, number>();
  const server = createServer((req, res) => {
    const json = (status: number, body: unknown) => {
      res.writeHead(status, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const url = decodeURIComponent(req.url ?? "");
    if (url === "/manifest.json") {
      const catalogs = [{ type: "movie", id: "jshelf", name: "Journey Shelf", extra: [{ name: "skip" }] }];
      return json(200, { id: "dev.marquee.shelf", version: "1.0.0", name: "Journey Shelf", resources: ["catalog"], types: ["movie"], idPrefixes: ["mqjs"], catalogs });
    }
    if (url.startsWith("/catalog/movie/jshelf")) {
      const skip = Number(/skip=(\d+)/.exec(url)?.[1] ?? 0);
      const n = (asked.get(skip) ?? 0) + 1;
      asked.set(skip, n);
      if ((skip === 0 || skip === 30) && n === 1) return json(500, { error: "busy" });
      const count = skip === 0 ? 30 : skip === 30 ? 10 : 0;
      const metas = Array.from({ length: count }, (_, i) => ({ id: `mqjs${skip + i}`, type: "movie", name: `Shelf Title ${skip + i}` }));
      return void setTimeout(() => json(200, { metas }), 800);
    }
    res.writeHead(404);
    res.end();
  });
  await new Promise<void>((ok) => server.listen(0, "127.0.0.1", () => ok()));
  return { url: `http://127.0.0.1:${(server.address() as AddressInfo).port}/manifest.json`, close: () => server.close() };
}

test("Discover: Try again asks a failed catalog, and a failed page, again", async ({ browser }) => {
  const shelf = await shelfCatalog();
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  try {
    await c.req("installAddon", { url: shelf.url });
    const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
    await p.goto("/discover?form=tv");
    const tiles = p.locator('[data-f^="g:discover:"]');
    // The catalog failed: Try again, by remote, keeps the focus while it asks.
    const first = p.locator('[data-f="discover:retry-catalog"]');
    await expect(first).toBeVisible({ timeout: 30_000 });
    await first.focus();
    await p.keyboard.press("Enter");
    await expect(first).toHaveText("Trying again");
    // Still there while the catalog loads again (it answers 0.8 s in).
    await p.waitForTimeout(400);
    await expect(first).toHaveText("Trying again");
    expect(await focusedKey(p)).toBe("discover:retry-catalog");
    await expect(tiles).toHaveCount(30, { timeout: 30_000 });
    // The next page fails: Try again in the footer asks that page again.
    await p.locator(".sentinel").scrollIntoViewIfNeeded();
    const more = p.locator('[data-f="discover:retry-more"]');
    await expect(more).toBeVisible({ timeout: 30_000 });
    await expect(p.locator(".grid-foot")).toContainText("Could not load more.");
    await more.focus();
    await p.keyboard.press("Enter");
    await expect(tiles).toHaveCount(40, { timeout: 30_000 });
    await context.close();
  } finally {
    await c.req("removeAddon", { url: shelf.url }).catch(() => undefined);
    c.close();
    shelf.close();
  }
});

test("TV: a focused chip gone after the page scrolled hands the focus to its nearest neighbour where it is now", async ({ browser }) => {
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/settings?form=tv");
  const more = p.locator('[data-f="pref-subs:more"]');
  await expect(more).toBeVisible({ timeout: 30_000 });
  await more.focus();
  await p.keyboard.press("Enter");
  const chips = p.locator('[data-fgroup="pref-subs"] .lang-all .chip');
  await expect(chips).toHaveCount(113);
  // A chip in the middle of the 113, then the page scrolls under it.
  await chips.nth(60).focus();
  await p.keyboard.press("ArrowRight");
  const page_ = () => p.evaluate(() => document.querySelector(".settings-page")?.scrollTop ?? 0);
  const at = await page_();
  await p.evaluate(() => document.querySelector(".settings-page")?.scrollBy(0, 320));
  await expect.poll(page_).toBeGreaterThan(at + 200);
  await p.waitForTimeout(300);
  // It leaves the list: the chip nearest to where it is on screen now takes the focus.
  const nearest = await p.evaluate(() => {
    const gone = document.activeElement as HTMLElement;
    const r = gone.getBoundingClientRect();
    const [x, y] = [r.left + r.width / 2, r.top + r.height / 2];
    const others = [...document.querySelectorAll<HTMLElement>('[data-fgroup="pref-subs"] [data-f]')].filter((el) => el !== gone);
    gone.remove();
    let best: HTMLElement | null = null;
    let d = Infinity;
    for (const el of others) {
      const b = el.getBoundingClientRect();
      const dd = Math.hypot(b.left + b.width / 2 - x, b.top + b.height / 2 - y);
      if (dd < d) [best, d] = [el, dd];
    }
    return best?.dataset.f ?? null;
  });
  await expect.poll(() => focusedKey(p)).toBe(nearest);
  await context.close();
});

test("phone player, portrait and landscape: transport centred, scrubber then options at the foot, subtitles placed clear of every control", async ({ browser }) => {
  // The largest text, and a cue of two lines: the hardest case for a phone.
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  await c.req("setPrefs", { patch: { subtitleLanguages: ["eng"], subtitleSize: "extraLarge" } }).finally(() => c.close());
  try {
    for (const size of [{ width: 390, height: 844 }, { width: 844, height: 390 }]) {
      const { context, page: p } = await page(browser, size);
      await p.goto("/play/movie/mqjm/mqjm?form=phone");
      await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
      await startIfPaused(p);
      // The line on screen (the English file's first, from 1 s to 30 s).
      await p.evaluate(() => {
        const v = document.querySelector("video");
        if (v) v.currentTime = 2;
      });
      await expect.poll(() => p.evaluate(() => document.querySelector("video")?.textTracks[0]?.activeCues?.length ?? 0), { timeout: 30_000 }).toBeGreaterThan(0);
      // The controls up, by a tap on the picture.
      await p.mouse.click(size.width / 2, size.height / 3);
      await expect(p.locator(".player-scr")).not.toHaveClass(/idle/);
      await p.waitForTimeout(400);
      const box = (sel: string) => p.locator(sel).first().evaluate((el) => el.getBoundingClientRect().toJSON() as DOMRect);
      const [transport, scrub, options] = [await box(".transport"), await box(".tl"), await box(".pacts")];
      // The transport in the middle of the screen.
      expect(Math.abs(transport.left + transport.width / 2 - size.width / 2)).toBeLessThan(3);
      expect(Math.abs(transport.top + transport.height / 2 - size.height / 2)).toBeLessThan(3);
      // At the foot: the scrubber, then the options row under it, above the safe area.
      expect(transport.bottom).toBeLessThan(scrub.top);
      expect(scrub.bottom).toBeLessThanOrEqual(options.top);
      expect(options.bottom).toBeLessThanOrEqual(size.height - 8);
      expect(size.height - options.bottom).toBeLessThan(40);
      // A phone draws its lines itself (the track runs hidden): their box,
      // their size, and the picture they belong to.
      expect(await p.evaluate(() => document.querySelector("video")?.textTracks[0]?.mode)).toBe("hidden");
      const cue = () =>
        p.evaluate(() => {
          const el = document.querySelector<HTMLElement>(".cues");
          const v = document.querySelector("video") as HTMLVideoElement;
          if (!el) return null;
          const r = el.getBoundingClientRect();
          const box = v.getBoundingClientRect();
          const scale = Math.min(box.width / v.videoWidth, box.height / v.videoHeight);
          const band = (box.height - v.videoHeight * scale) / 2;
          const span = el.querySelector("span") as HTMLElement;
          return { top: r.top, bottom: r.bottom, lines: el.querySelectorAll("span").length, font: parseFloat(getComputedStyle(span).fontSize), pictureBottom: box.bottom - band, foot: box.bottom };
        });
      const clear = (c: { top: number; bottom: number } | null, b: DOMRect) => !!c && (c.bottom <= b.top || c.top >= b.bottom);
      const portrait = size.height > size.width;
      if (portrait) {
        // The band under the picture holds the two lines: drawn there at full
        // size, 8 to 16 px under the picture, clear of the scrubber.
        await expect.poll(async () => ((await cue())?.top ?? 0) - ((await cue())?.pictureBottom ?? 0)).toBeGreaterThanOrEqual(8);
        const c = await cue();
        expect(c?.lines).toBe(2);
        expect((c?.top ?? 0) - (c?.pictureBottom ?? 0)).toBeLessThanOrEqual(16);
        expect(c?.font).toBe(24);
        expect(c?.bottom ?? Infinity).toBeLessThanOrEqual(scrub.top);
        expect(clear(c, transport)).toBe(true);
      } else {
        // The controls cover the picture: the foot 8 px over the scrubber, and
        // two large lines that would meet the transport drawn at 85%.
        await expect.poll(async () => (await cue())?.font).toBeCloseTo(20.4, 1);
        const track = await box(".scrub");
        await expect.poll(async () => track.top - ((await cue())?.bottom ?? 0)).toBeGreaterThanOrEqual(6);
        const c = await cue();
        expect(track.top - (c?.bottom ?? 0)).toBeLessThanOrEqual(14);
        expect(clear(c, transport)).toBe(true);
        // The title beside Back on one line; the options between the bar's ends.
        const [back, title, bar] = [await box(".pback"), await box(".ptitle h1"), await box(".scrub")];
        expect(Math.abs(back.top + back.height / 2 - (title.top + title.height / 2))).toBeLessThan(4);
        expect(title.left).toBeGreaterThan(back.right);
        const first = await box(".pacts .pa:first-child");
        const last = await box(".pacts .pa:last-child");
        expect(first.left).toBeGreaterThanOrEqual(bar.left - 2);
        expect(last.right).toBeLessThanOrEqual(bar.right + 2);
      }
      await p.screenshot({ path: `${SHOTS}/phone-player-${size.width}x${size.height}.png` });
      // Controls hidden: portrait keeps the lines in the band; landscape takes
      // them back to the foot, full size.
      await expect(p.locator(".player-scr")).toHaveClass(/idle/, { timeout: 10_000 });
      if (portrait) {
        const c = await cue();
        expect((c?.top ?? 0) - (c?.pictureBottom ?? 0)).toBeGreaterThanOrEqual(8);
        expect((c?.top ?? 0) - (c?.pictureBottom ?? 0)).toBeLessThanOrEqual(16);
      } else {
        await expect.poll(async () => (await cue())?.font).toBe(24);
        const c = await cue();
        expect((c?.foot ?? 0) - (c?.bottom ?? 0)).toBeLessThanOrEqual(30);
      }
      await context.close();
    }
  } finally {
    const r = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
    await r.req("setPrefs", { patch: { subtitleSize: "medium" } }).finally(() => r.close());
  }
});
