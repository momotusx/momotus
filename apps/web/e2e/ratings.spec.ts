/** The IMDb score the addon gives, as text, and the RPDB cover with its ratings band
 * (plans/research/ratings-band.md), on a core of its own: the widest score there is
 * ("IMDb 10.0") never wraps or gets cut, on the hero, the title page and search, in every form.
 * A poster tile shows the core's RPDB cover, drawn with the key an installed addon was
 * configured with, and no score of its own; its label speaks the IMDb score, and the addon's
 * poster stands in when the cover does not load. Rotten Tomatoes never shows as text: it comes
 * only in the band (the owner, 2026-10-23). The measured widths are printed as
 * `ratings-measure` lines for bench/core/ratings.jsonl. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, PORTS, RATINGS_PORT, SHOTS, startBridge } from "./core";

type Form = "tv" | "desktop" | "phone";
const FORMS: Record<Form, { width: number; height: number; scale: number }> = {
  tv: { width: 1920, height: 1080, scale: 1 },
  desktop: { width: 1440, height: 900, scale: 1 },
  phone: { width: 390, height: 844, scale: 2 },
};

/** A title at the widest scores (the hero: the only one with a backdrop), one of each narrower
 * case in view beside it, then more at the widest. */
const widest = (i: number) => ({ id: `tt92000${String(i).padStart(2, "0")}`, name: `Widest ${i + 1}`, imdb: "10" as string | null });
const TITLES = [widest(0), { id: "tt9200100", name: "Only IMDb", imdb: "7.4" }, { id: "tt9200102", name: "No scores", imdb: null }, ...Array.from({ length: 8 }, (_, i) => widest(i + 1))];
const MAX = TITLES[0]!;

/** A one-pixel grey PNG: the addon's own poster. */
const PNG = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==", "base64");
const RPDB = (id: string) => `https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/${id}.jpg?fallback=true`;

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let keyed: Server | null = null;

const listen = (server: Server) => new Promise<Server>((ok) => server.listen(0, "127.0.0.1", () => ok(server)));

/** A movie catalog, its search and metas for the titles above. */
function localAddon(): Promise<Server> {
  const meta = (t: (typeof TITLES)[number]) => ({
    id: t.id,
    type: "movie",
    name: t.name,
    // One title with an addon poster of its own: the stand-in when RPDB's cover fails.
    poster: t.name === "Only IMDb" ? `http://127.0.0.1:${(addon?.address() as AddressInfo).port}/poster/only.png` : undefined,
    // A series' span of years: the longest release line a search hit carries.
    releaseInfo: "2011–2019",
    runtime: "148 min",
    background: t === MAX ? "https://images.metahub.space/background/medium/tt0111161/img" : undefined,
    links: [
      // The addon's own IMDb rating, as Cinemeta writes it.
      ...(t.imdb ? [{ name: t.imdb, category: "imdb", url: `https://imdb.com/title/${t.id}` }] : []),
      { name: "Drama", category: "Genres", url: "stremio:///discover" },
      { name: "Science Fiction", category: "Genres", url: "stremio:///discover" },
    ],
  });
  return listen(
    createServer((req, res) => {
      const json = (body: unknown) => {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify(body));
      };
      const url = decodeURIComponent(req.url ?? "");
      if (url === "/manifest.json") {
        const catalogs = [
          { type: "movie", id: "scores", name: "Scores" },
          { type: "movie", id: "find", name: "Find", extra: [{ name: "search", isRequired: true }] },
        ];
        return json({ id: "dev.marquee.ratings", version: "1.0.0", name: "Ratings Test", resources: ["catalog", "meta"], types: ["movie"], idPrefixes: ["tt920"], catalogs });
      }
      if (url.startsWith("/catalog/movie/")) return json({ metas: TITLES.map(meta) });
      if (url === "/poster/only.png") {
        res.writeHead(200, { "content-type": "image/png" });
        return res.end(PNG);
      }
      const asked = /^\/meta\/movie\/(tt\d+)\.json$/.exec(url);
      const title = TITLES.find((t) => t.id === asked?.[1]);
      if (title) return json({ meta: meta(title) });
      res.writeHead(404);
      res.end();
    }),
  );
}


/** An addon installed with an RPDB key in its link's config, as the Rotten Tomatoes default is:
 * the key every IMDb cover's band is drawn with. It lists nothing. */
function keyedAddon(): Promise<Server> {
  return listen(
    createServer((req, res) => {
      if (req.url?.endsWith("/manifest.json")) {
        res.writeHead(200, { "content-type": "application/json" });
        return res.end(JSON.stringify({ id: "pw.ers.rottentomatoes", version: "1.0.9", name: "Rotten Tomatoes", resources: ["catalog"], types: ["movie"], catalogs: [] }));
      }
      res.writeHead(404);
      res.end();
    }),
  );
}

test.beforeAll(async () => {
  addon = await localAddon();
  keyed = await keyedAddon();
  core = await startBridge(RATINGS_PORT);
  const c = new Client(`ws://127.0.0.1:${RATINGS_PORT}/ws`);
  try {
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    await c.req("installAddon", { url: `http://127.0.0.1:${(keyed.address() as AddressInfo).port}/%7B%22rpdb_key%22%3A%22t0-free-rpdb%22%7D/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1 } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  keyed?.close();
});

async function open(browser: Browser, form: Form): Promise<Page> {
  const { width, height, scale } = FORMS[form];
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  // RPDB knows none of these made-up ids: a stand-in cover, so nothing is asked of it.
  await context.route(/^https:\/\/api\.ratingposterdb\.com\//, (r) => r.fulfill({ status: 200, contentType: "image/png", body: PNG }));
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${RATINGS_PORT}/ws`);
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

/** Each score's box: one line, nothing cut, inside `within` (a selector of an ancestor). */
async function scores(page: Page, selector: string, within: string) {
  return page.locator(selector).evaluateAll(
    (els, within) =>
      els.map((el) => {
        const box = el.getBoundingClientRect();
        const outer = (el.closest(within) ?? document.body).getBoundingClientRect();
        return {
          text: (el.textContent ?? "").trim(),
          lines: el.getClientRects().length,
          cut: el.scrollWidth > el.clientWidth + 0.5,
          inside: box.left >= outer.left - 0.5 && box.right <= outer.right + 0.5,
          left: Math.round(box.left),
          top: Math.round(box.top),
          middle: Math.round(box.top + box.height / 2),
          width: Math.round(box.width * 10) / 10,
          outer: Math.round(outer.width * 10) / 10,
        };
      }),
    within,
  );
}

function measure(form: Form, where: string, values: unknown) {
  console.log(`ratings-measure ${JSON.stringify({ form, where, values })}`);
}

for (const form of Object.keys(FORMS) as Form[]) {
  test(`${form}: a poster tile shows the RPDB cover and no scores of its own; its label speaks the IMDb score`, async ({ browser }) => {
    const page = await open(browser, form);
    await page.goto(`/?form=${form}`);
    // On TV the tiles carry their names in the caption; elsewhere a shelf poster has none.
    const tile = (name: string) => page.locator(`.row .tile[aria-label="${name}"], .row .tile[aria-label^="${name},"]`).first();
    await expect(tile("Widest 2")).toHaveAttribute("aria-label", /^Widest 2(, [^,]+)?, IMDb 10\.0$/, { timeout: 30_000 });
    // The core's cover for an IMDb title, and it loads: every catalog row is 2:3 posters on every form.
    const cover = tile("Widest 1").locator(".pic img");
    await expect(cover).toHaveAttribute("src", RPDB(MAX.id));
    await expect.poll(() => cover.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
    // No score chip on the art and no ratings line in the caption (name and year): the cover carries them.
    await expect(page.locator(".row .tile .chip-rate")).toHaveCount(0);
    await expect(page.locator(".row .tile .cap .rate")).toHaveCount(0);
    // A tile with scores is the size of one without, so a late score moves nothing.
    const box = async (name: string) => (await tile(name).boundingBox())!;
    expect((await box("Widest 1")).height).toBe((await box("No scores")).height);
    await expect(tile("Only IMDb")).toHaveAttribute("aria-label", /IMDb 7\.4$/);

    // The hero: the IMDb score on one line; Rotten Tomatoes never as text.
    const hero = await scores(page, ".hero-body .meta .rate > [aria-hidden]", ".hero-body");
    expect(hero.map((h) => h.text)).toEqual(["IMDb 10.0"]);
    for (const h of hero) expect(h, JSON.stringify(h)).toMatchObject({ lines: 1, cut: false, inside: true });
    // A screen reader reads the hidden twin.
    await expect(page.locator(".hero-body .meta .rate .sr")).toHaveText(["IMDb 10.0"]);
    measure(form, "hero", hero.map((h) => h.width));
    await page.screenshot({ path: `${SHOTS}/ratings-home-${form}.jpg`, type: "jpeg", quality: 80 });

    // The title page: the IMDb score, linked off TV; no Rotten Tomatoes text.
    await page.goto(`/title/movie/${MAX.id}?form=${form}`);
    await expect(page.locator(".info .meta .rate > [aria-hidden]")).toHaveText(["IMDb 10.0"], { timeout: 30_000 });
    const title = await scores(page, ".info .meta .rate > [aria-hidden]", ".info");
    for (const t of title) expect(t, JSON.stringify(t)).toMatchObject({ lines: 1, cut: false, inside: true });
    await expect(page.locator(".info .meta")).not.toContainText(/RT |Rotten Tomatoes|Fresh/);
    const link = page.locator('.info .meta a[data-f="title:imdb"]');
    if (form === "tv") await expect(link).toHaveCount(0);
    else {
      await expect(link).toHaveAttribute("href", `https://www.imdb.com/title/${MAX.id}/`);
      await expect(link).toHaveAttribute("aria-label", "IMDb 10.0, on IMDb");
      if (form === "phone") expect((await link.boundingBox())!.height).toBeGreaterThanOrEqual(44);
    }
    await expect(page.locator(".info .meta span.rate .sr")).toHaveText(form === "tv" ? ["IMDb 10.0"] : []);
    // Where the scores came from is not said on the title page: no addon name outside the Addons pages.
    await expect(page.locator(".facts div", { hasText: "Ratings from" })).toHaveCount(0);
    measure(form, "title page", title.map((t) => t.width));
    await page.screenshot({ path: `${SHOTS}/ratings-title-${form}.jpg`, type: "jpeg", quality: 80 });


    // Discover's 2:3 posters: the cover, no chip, the label speaks the scores.
    await page.goto(`/discover?form=${form}`);
    const poster = page.locator(".grid .tile", { hasText: "Widest 2" }).first();
    await expect(poster).toHaveAttribute("aria-label", /^Widest 2(, [^,]+)?, IMDb 10\.0$/, { timeout: 30_000 });
    await expect(poster.locator(".pic img")).toHaveAttribute("src", RPDB(widest(1).id));
    await expect(page.locator(".grid .tile .chip-rate")).toHaveCount(0);
    // A 2:3 poster's caption has no ratings line, on TV either: the cover carries the scores.
    await expect(page.locator(".grid .tile .cap .rate")).toHaveCount(0);
    await page.screenshot({ path: `${SHOTS}/ratings-discover-${form}.jpg`, type: "jpeg", quality: 80 });

    // Search: the meta line keeps every score whole.
    await page.goto(`/search?form=${form}`);
    const field = page.locator('[data-f="search:input"]');
    await expect(field).toBeVisible({ timeout: 30_000 });
    await field.fill("widest");
    await expect(page.locator(".hit-meta", { hasText: "IMDb 10.0" }).first()).toBeVisible({ timeout: 30_000 });
    const meta = await page.locator(".hit-meta").first().evaluate((el) => ({ width: el.clientWidth, needs: el.scrollWidth }));
    expect(meta.needs, JSON.stringify(meta)).toBeLessThanOrEqual(meta.width);
    const hit = await scores(page, ".hit-meta .rate > [aria-hidden]", ".hit-meta");
    for (const h of hit.slice(0, 1)) expect(h, JSON.stringify(h)).toMatchObject({ lines: 1, cut: false, inside: true });
    await expect(page.locator(".hit-meta").first().locator(".rate .sr")).toHaveText(["IMDb 10.0"]);
    measure(form, "search hit", meta);
    await expect(page.locator(".hit-meta", { hasText: /RT |Fresh/ })).toHaveCount(0);
    await page.context().close();
  });
}

test("a cover that does not load gives way to the addon's poster, then to the name card", async ({ browser }) => {
  const page = await open(browser, "desktop");
  // RPDB answers 404 for an id it does not know.
  await page.context().route(/^https:\/\/api\.ratingposterdb\.com\//, (r) => r.fulfill({ status: 404 }));
  await page.goto("/discover?form=desktop");
  const tile = (name: string) => page.locator(".grid .tile", { hasText: name }).first();
  // Only IMDb has its addon's poster: it shows that one.
  await expect(tile("Only IMDb").locator(".pic img")).toHaveAttribute("src", /\/poster\/only\.png$/, { timeout: 30_000 });
  await expect.poll(() => tile("Only IMDb").locator(".pic img").evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
  // No addon poster: the name card, never a broken image.
  await expect(tile("No scores").locator(".pic img")).toHaveCount(0);
  await expect(tile("No scores").locator(".pic-name")).toHaveText("No scores");
  await page.context().close();
});
