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
import { Client, cleanup, JOURNEY_PORT, PORTS, startBridge, startIfPaused, until, watch } from "./core";
import { ensureMedia, NAME, startSource } from "./source";
import { applyDiffs } from "../src/core/diff";
import type { HomeRow } from "../src/core/generated/HomeRow";
import type { HomeUpdate } from "../src/core/generated/HomeUpdate";

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let media: Server | null = null;

const AIRED = "2020-01-01T00:00:00.000Z";
const LATER = "2099-01-01T00:00:00.000Z";
const SRT = "1\n00:00:01,000 --> 00:00:30,000\nA line\n";

/** Its own titles: "mqj1" (S1:E2 not out), "mqj2" (two episodes out), the film "mqjm". */
function journeyAddon(film: string): Promise<Server> {
  const episode = (series: string, n: number, released: string) => ({ id: `${series}:1:${n}`, title: `Chapter ${n}`, season: 1, episode: n, released });
  const metas: Record<string, object> = {
    "/meta/series/mqj1.json": { id: "mqj1", type: "series", name: "Journey Waiting", description: "A synopsis comes first on the page.", videos: [episode("mqj1", 1, AIRED), episode("mqj1", 2, LATER)] },
    "/meta/series/mqj2.json": { id: "mqj2", type: "series", name: "Journey Finished", videos: [episode("mqj2", 1, AIRED), episode("mqj2", 2, AIRED)] },
    "/meta/movie/mqjm.json": { id: "mqjm", type: "movie", name: "Journey Film", releaseInfo: "2008" },
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

test.beforeAll(async () => {
  await ensureMedia();
  media = await startSource(0);
  addon = await journeyAddon(`http://127.0.0.1:${(media.address() as AddressInfo).port}/media/${NAME}`);
  core = await startBridge(JOURNEY_PORT);
  const c = new Client(`ws://127.0.0.1:${JOURNEY_PORT}/ws`);
  try {
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
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
  media?.close();
});

type Sent = { id?: number; op?: string; view?: string; event?: string; sub?: number };

/** A page on this spec's core; `sent` is what it asked, in order, and
 * `fake` may answer a message at the socket instead of the core (a reply's
 * `ok` and `result` or `error`). */
type Reply = { ok: true; result: unknown } | { ok: false; error: string };
async function page(browser: Browser, viewport = { width: 1440, height: 900 }, fake?: (m: Sent) => Reply | undefined) {
  const sent: Sent[] = [];
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
    upstream.addEventListener("message", (e) => ws.send(String(e.data)));
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m)) as Sent;
      sent.push(msg);
      const answer = fake?.(msg);
      if (answer !== undefined) return ws.send(JSON.stringify({ id: msg.id, ...answer }));
      if (upstream.readyState === WebSocket.OPEN) upstream.send(String(m));
      else early.push(String(m));
    });
    ws.onClose(() => upstream.close());
  });
  return { context, page: await context.newPage(), sent };
}

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
  await p.locator('[data-f="player:subs"]').dispatchEvent("click");
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
  // "eng" from an earlier picker reads as English, chosen.
  const english = p.locator('[data-f="pref-subs:en"]');
  await expect(english).toHaveAttribute("aria-checked", "true", { timeout: 30_000 });
  await p.locator('[data-f="pref-subs:more"]').click();
  await expect(p.locator('[data-fgroup="pref-subs"] .lang-all .chip')).toHaveCount(113);
  await p.locator('[data-f="pref-subs:search"]').fill("brazil");
  const brazil = p.locator('[data-f="pref-subs-all:pt-BR"]');
  await expect(p.locator('[data-fgroup="pref-subs"] .lang-all .chip')).toHaveCount(1);
  await brazil.click();
  await expect(brazil).toHaveAttribute("aria-checked", "true");
  // Chosen languages lead the row in the order they were turned on.
  await expect(p.locator('[data-fgroup="pref-subs"] .chips-track > .chip').nth(1)).toHaveAttribute("data-f", "pref-subs:pt-BR");
  await expect.poll(async () => (await prefs()).subtitleLanguages).toEqual(["eng", "pt-BR"]);
  // Turning English off takes the old code out too.
  await english.click();
  await expect.poll(async () => (await prefs()).subtitleLanguages).toEqual(["pt-BR"]);
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
    await expect(p.locator('[data-f="tile:home:continue-watching:mqjm"]')).toBeVisible({ timeout: 30_000 });
    await expect(p.locator(".tile-x")).toHaveCount(0);
    await p.goto("/title/movie/mqjm?form=tv");
    const remove = p.locator('[data-f="title:uncontinue"]');
    await expect(remove).toBeVisible({ timeout: 30_000 });
    await remove.focus();
    await p.keyboard.press("Enter");
    await expect(remove).toHaveCount(0);
    await expect.poll(home.ids).not.toContain("mqjm");
    // Focus never falls to the page.
    await expect.poll(() => p.evaluate(() => document.activeElement !== document.body)).toBe(true);
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
