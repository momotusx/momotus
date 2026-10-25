/** The title page's ratings row (plans/title-page-ratings-genres-similar.md) on a core of its own whose
 * meta addon, TMDB (MQ_DEV_TMDB) and Trakt (MQ_DEV_TRAKT) are one local fake: at the end of the meta line,
 * IMDb as x/10, TMDB and Trakt as percentages, each after its service's mark and spoken once; the row's
 * height is held from the first draw and a pending slot keeps its width, so nothing moves when TMDB and
 * Trakt land (the fake holds them back a moment); badge heights per form. RPDB's rated cover (an addon
 * installed with an RPDB key) is a fixed poster clear of the facts, never focusable, not drawn beside a
 * series' episodes panel on TV and desktop, and gone when it fails; More like this lands late under the
 * hero without moving anything above, names no addon, and Back from one of its titles focuses its tile. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, PORTS, SHOTS, startBridge, TITLE_SCORES_PORT } from "./core";

const HOLD_MS = 2500;
/** A one-pixel grey PNG: RPDB's cover, so nothing is asked of it. */
const PNG = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==", "base64");
const RELATED = [
  { title: "Cool Hand Luke", year: 1967, ids: { imdb: "tt0061512" } },
  { title: "Papillon", year: 1973, ids: { imdb: "tt0070511" } },
];
const SERIES = {
  meta: {
    id: "tt0903747",
    type: "series",
    name: "Breaking Bad",
    releaseInfo: "2008-2013",
    videos: [{ id: "tt0903747:1:1", title: "Pilot", season: 1, episode: 1, released: "2008-01-20T00:00:00.000Z" }],
  },
};
/** One title per form for the row's test: each lands its numbers fresh (the core keeps answers a day). */
const ROW_ID = { tv: "tt0111161", desktop: "tt0111162", phone: "tt0111163" } as const;
const meta = (id: string) => ({
  meta: {
    id,
    type: "movie",
    name: "The Shawshank Redemption",
    releaseInfo: "1994",
    runtime: "142 min",
    links: [
      { name: "9.3", category: "imdb", url: `https://imdb.com/title/${id}` },
      { name: "Drama", category: "Genres", url: "stremio:///discover" },
    ],
  },
});
/** The hidden-tab test's own title, so no other test has asked for it first. */
const HIDDEN_ID = "tt0111164";
const MAIN = [...Object.values(ROW_ID), HIDDEN_ID] as string[];
/** Every path the fake answered, in order. */
const asked: string[] = [];

let core: { proc: ChildProcess; data: string } | null = null;
let fake: Server | null = null;
let keyed: Server | null = null;

/** The meta addon, TMDB's find and Trakt's ratings and related, the last three held `HOLD_MS`. */
function fakeServer(): Promise<Server> {
  const server = createServer((req, res) => {
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const url = decodeURIComponent(req.url ?? "");
    asked.push(url);
    if (url === "/manifest.json") {
      return json({ id: "dev.marquee.scores", version: "1.0.0", name: "Scores Test", resources: ["meta"], types: ["movie", "series"], idPrefixes: ["tt"], catalogs: [] });
    }
    if (url.startsWith("/meta/series/tt0903747")) return json(SERIES);
    const movie = /^\/meta\/movie\/(tt\d+)\.json$/.exec(url)?.[1];
    if (movie && MAIN.includes(movie)) return json(meta(movie));
    if (movie) return json({ meta: { id: movie, type: "movie", name: RELATED.find((r) => r.ids.imdb === movie)?.title ?? "Other" } });
    const held = (body: unknown) => setTimeout(() => json(body), HOLD_MS);
    const lookup = /^\/(?:3\/find|movies)\/(tt\d+)(\/ratings|\/related)?/.exec(url);
    if (lookup && MAIN.includes(lookup[1] as string)) {
      if (url.startsWith("/3/find/")) return held({ movie_results: [{ id: 278, vote_average: 8.733, vote_count: 31655 }], tv_results: [] });
      if (lookup[2] === "/ratings") return held({ rating: 9.16432, votes: 48623 });
      if (lookup[2] === "/related") return held(RELATED);
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok(server)));
}

/** An addon installed with an RPDB key in its link's config, as the Rotten Tomatoes default is: the key
 * the rated cover is drawn with. It lists nothing. */
function keyedAddon(): Promise<Server> {
  const server = createServer((req, res) => {
    if (req.url?.endsWith("/manifest.json")) {
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify({ id: "pw.ers.rottentomatoes", version: "1.0.9", name: "Rotten Tomatoes", resources: ["catalog"], types: ["movie"], catalogs: [] }));
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok(server)));
}

test.beforeAll(async () => {
  fake = await fakeServer();
  keyed = await keyedAddon();
  const base = `http://127.0.0.1:${(fake.address() as AddressInfo).port}`;
  core = await startBridge(TITLE_SCORES_PORT, { MQ_DEV_TMDB: base, MQ_DEV_TRAKT: base });
  const c = new Client(`ws://127.0.0.1:${TITLE_SCORES_PORT}/ws`);
  try {
    await c.req("installAddon", { url: `${base}/manifest.json` });
    await c.req("installAddon", { url: `http://127.0.0.1:${(keyed.address() as AddressInfo).port}/%7B%22rpdb_key%22%3A%22t0-free-rpdb%22%7D/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1 } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  fake?.close();
  keyed?.close();
});

async function open(browser: Browser, form: "tv" | "desktop" | "phone", rpdb = 200): Promise<Page> {
  const viewport = form === "tv" ? { width: 1920, height: 1080 } : form === "desktop" ? { width: 1440, height: 900 } : { width: 390, height: 844 };
  const context = await browser.newContext({ viewport, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.route(/^https:\/\/api\.ratingposterdb\.com\//, (r) => (rpdb === 200 ? r.fulfill({ status: 200, contentType: "image/png", body: PNG }) : r.fulfill({ status: rpdb })));
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${TITLE_SCORES_PORT}/ws`);
    const early: string[] = [];
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => ws.send(String(e.data)));
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((m) => (upstream.readyState === WebSocket.OPEN ? upstream.send(String(m)) : early.push(String(m))));
    ws.onClose(() => upstream.close());
  });
  return context.newPage();
}

const rect = (page: Page, selector: string) =>
  page.locator(selector).first().evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height) };
  });

/** Where the row starts and how tall it is: its end may move by a digit's width at the line's end. */
const place = ({ x, y, h }: { x: number; y: number; w: number; h: number }) => ({ x, y, h });

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: the ratings row ends the meta line, with marks, and nothing moves when TMDB and Trakt land`, async ({ browser }) => {
    const id = ROW_ID[form];
    const page = await open(browser, form);
    await page.goto(`/title/movie/${id}?form=${form}`);
    const scores = page.locator(".info .meta .scores");
    await expect(scores.locator(".score").first()).toHaveAttribute("aria-label", form === "tv" ? "IMDb 9.3 out of 10" : "IMDb 9.3 out of 10, on IMDb", { timeout: 30_000 });
    // Before TMDB and Trakt answer: their slots are held, hidden, at full width.
    await expect(scores.locator(".score.pending")).toHaveCount(2);
    const before = { year: await rect(page, ".info .meta .num"), row: place(await rect(page, ".info .meta .scores")), imdb: await rect(page, ".info .meta .score"), syn: await rect(page, ".info .syn, .info .play") };
    // The meta line's own text comes first, the ratings row last (on the phone it may wrap below).
    expect(before.row.y > before.year.y || before.row.x > before.year.x).toBe(true);
    await expect(scores.locator(".score.pending")).toHaveCount(0, { timeout: 30_000 });
    const labels = await scores.locator(".score").evaluateAll((els) => els.map((e) => e.getAttribute("aria-label")));
    const on = (name: string) => (form === "tv" ? "" : `, on ${name}`);
    expect(labels).toEqual([`IMDb 9.3 out of 10${on("IMDb")}`, `TMDB 87 percent${on("TMDB")}`, `Trakt 92 percent${on("Trakt")}`]);
    // Off TV each badge opens the title's page there; TV has no links at all.
    const hrefs = await scores.locator("a.score").evaluateAll((els) => els.map((e) => e.getAttribute("href")));
    // Each badge stays a link to a screen reader (its list item wraps it), one list item per number.
    expect(await scores.locator("a.score").evaluateAll((els) => els.map((e) => e.getAttribute("role")))).toEqual(form === "tv" ? [] : [null, null, null]);
    await expect(scores.locator('[role="listitem"]')).toHaveCount(3);
    expect(hrefs).toEqual(form === "tv" ? [] : [`https://www.imdb.com/title/${id}/`, "https://www.themoviedb.org/movie/278", `https://trakt.tv/movies/${id}`]);
    // The targets split the gaps: no two meet (edges may touch), and none reaches the chips below.
    const targets = await scores.locator("a.score").evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON() as DOMRect));
    targets.slice(1).forEach((t, i) => expect(t.left).toBeGreaterThanOrEqual((targets[i] as DOMRect).right - 0.5));
    const chipRow = page.locator(".info .chips, .info .genres");
    const chips = (await chipRow.count()) ? await chipRow.first().boundingBox() : null;
    if (chips) for (const t of targets) expect(t.bottom).toBeLessThanOrEqual(chips.y + 0.5);
    await expect(scores.locator(".score .num")).toHaveText(["9.3/10", "87%", "92%"]);
    // More like this lands with them, under the hero (TV, desktop) or before the facts (phone).
    const similar = page.locator('.row[aria-label="More like this"]');
    await expect(similar.locator(".tile")).toHaveCount(2, { timeout: 30_000 });
    expect(await similar.textContent()).not.toContain("Scores Test");
    // Nothing moved: the year, the row, the IMDb slot and what sits below.
    const after = { year: await rect(page, ".info .meta .num"), row: place(await rect(page, ".info .meta .scores")), imdb: await rect(page, ".info .meta .score"), syn: await rect(page, ".info .syn, .info .play") };
    expect(after).toEqual(before);
    // Badge heights per form: legible, never cap height.
    const marks = await scores.locator(".mark").evaluateAll((els) => els.map((e) => Math.round(e.getBoundingClientRect().height)));
    expect(new Set(marks)).toEqual(new Set([form === "tv" ? 28 : 18]));
    expect(after.row.h).toBe(form === "tv" ? 36 : 24);
    // The marks are images with no text of their own: one spoken name per item.
    expect(await scores.locator(".mark").evaluateAll((els) => els.map((e) => e.getAttribute("alt")))).toEqual(["", "", ""]);
    await page.context().close();
  });

  test(`${form}: the rated cover is a fixed poster clear of the facts and never focusable`, async ({ browser }) => {
    const page = await open(browser, form);
    await page.goto(`/title/movie/tt0111161?form=${form}`);
    const cover = page.locator(".rated-cover img");
    await expect(cover).toBeVisible({ timeout: 30_000 });
    await expect(cover).toHaveAttribute("alt", "Cover with ratings");
    const box = await rect(page, ".rated-cover");
    expect([box.w, box.h]).toEqual(form === "tv" ? [300, 450] : form === "desktop" ? [240, 360] : [200, 300]);
    const apart = (a: { x: number; y: number; w: number; h: number }, b: typeof a) => a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
    for (const other of [".info .facts", ".info .meta", ".info .play"]) expect(apart(box, await rect(page, other)), other).toBe(true);
    if (form === "phone") expect(box.y + box.h).toBeLessThanOrEqual((await rect(page, ".info .facts")).y);
    await expect(page.locator('.row[aria-label="More like this"] .tile')).toHaveCount(2, { timeout: 30_000 });
    await page.screenshot({ path: `${SHOTS}/title-scores-${form}.jpg`, type: "jpeg", quality: 80, fullPage: true });
    // Decoration: nothing in it takes focus.
    expect(await page.locator(".rated-cover").evaluate((el) => el.querySelectorAll("a, button, [tabindex], [data-f]").length)).toBe(0);
    if (form === "tv") {
      // More like this: Back from a title opened there focuses its tile.
      const tile = page.locator('.row[aria-label="More like this"] .tile').first();
      await expect(tile).toBeVisible({ timeout: 30_000 });
      const key = await tile.getAttribute("data-f");
      await tile.focus();
      await page.keyboard.press("Enter");
      await expect(page).toHaveURL(/tt0061512/);
      await page.goBack();
      await expect(page).toHaveURL(/tt0111161/);
      await expect.poll(() => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null), { timeout: 15_000 }).toBe(key);
    }
    await page.context().close();
  });

  test(`${form}: the rated cover is gone when RPDB fails, and a series shows it clear of its episodes`, async ({ browser }) => {
    const page = await open(browser, form, 404);
    await page.goto(`/title/movie/tt0111161?form=${form}`);
    await expect(page.locator(".info .meta .scores .score").first()).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(".rated-cover")).toHaveCount(0);
    await page.context().close();
    const series = await open(browser, form);
    await series.goto(`/title/series/tt0903747?form=${form}`);
    await expect(series.locator(".eps")).toBeVisible({ timeout: 30_000 });
    // Rotten Tomatoes is never dropped: a series shows its cover on every form. The phone draws it in the body; TV
    // and desktop leave the episodes panel's place alone and lead the body under the hero with it.
    const cover = series.locator(".rated-cover img");
    await expect(cover).toHaveCount(1, { timeout: 30_000 });
    if (form === "phone") await expect(cover).toBeVisible();
    else {
      await expect(series.locator(".title-more .rated-cover")).toHaveCount(1);
      const [card, eps] = [(await rect(series, ".rated-cover"))!, (await rect(series, ".eps"))!];
      expect(card.y, "the cover sits under the hero").toBeGreaterThanOrEqual(eps.y + eps.h);
    }
    await series.context().close();
  });
}

// A tab in the background asks TMDB and Trakt nothing: the page sends visible only once it shows.
test("desktop: a hidden tab asks nothing until it shows", async ({ browser }) => {
  const page = await open(browser, "desktop");
  await page.addInitScript(() => {
    let hidden = true;
    Object.defineProperty(Document.prototype, "hidden", { configurable: true, get: () => hidden });
    Object.defineProperty(Document.prototype, "visibilityState", { configurable: true, get: () => (hidden ? "hidden" : "visible") });
    (window as unknown as { show: () => void }).show = () => {
      hidden = false;
      document.dispatchEvent(new Event("visibilitychange"));
    };
  });
  await page.goto(`/title/movie/${HIDDEN_ID}?form=desktop`);
  await expect(page.locator(".info .meta .scores .score").first()).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(2500);
  expect(asked.filter((p) => p.includes(`/${HIDDEN_ID}/`) || p.startsWith(`/3/find/${HIDDEN_ID}`))).toEqual([]);
  await page.evaluate(() => (window as unknown as { show: () => void }).show());
  await expect(page.locator(".info .meta .scores .score.pending")).toHaveCount(0, { timeout: 30_000 });
  expect(asked.some((p) => p.startsWith(`/3/find/${HIDDEN_ID}`))).toBe(true);
  await page.context().close();
});
