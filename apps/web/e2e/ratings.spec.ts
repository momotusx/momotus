/** IMDb and Rotten Tomatoes side by side (plans/research/ratings.md) on a core of its own whose
 * MDBList is a local fake (MQ_DEV_MDBLIST): the widest scores there are ("IMDb 10.0", "RT 100%")
 * never wrap or get cut, on posters, the hero, the title page and search, in every form. On a
 * phone poster the two stack as two lines; everywhere else they sit side by side. The measured
 * widths are printed as `ratings-measure` lines for bench/core/ratings.jsonl. */

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
const widest = (i: number) => ({ id: `tt92000${String(i).padStart(2, "0")}`, name: `Widest ${i + 1}`, imdb: 10, tomatoes: 100 });
const TITLES = [
  widest(0),
  { id: "tt9200100", name: "Only IMDb", imdb: 7.4, tomatoes: null },
  { id: "tt9200101", name: "Only RT", imdb: null, tomatoes: 0 },
  { id: "tt9200102", name: "No scores", imdb: null, tomatoes: null },
  ...Array.from({ length: 8 }, (_, i) => widest(i + 1)),
];
const MAX = TITLES[0]!;

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let mdblist: Server | null = null;

const listen = (server: Server) => new Promise<Server>((ok) => server.listen(0, "127.0.0.1", () => ok(server)));

/** A movie catalog, its search and metas for the titles above. */
function localAddon(): Promise<Server> {
  const meta = (t: (typeof TITLES)[number]) => ({
    id: t.id,
    type: "movie",
    name: t.name,
    // A series' span of years: the longest release line a search hit carries.
    releaseInfo: "2011–2019",
    runtime: "148 min",
    background: t === MAX ? "https://images.metahub.space/background/medium/tt0111161/img" : undefined,
    links: [
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
      const asked = /^\/meta\/movie\/(tt\d+)\.json$/.exec(url);
      const title = TITLES.find((t) => t.id === asked?.[1]);
      if (title) return json({ meta: meta(title) });
      res.writeHead(404);
      res.end();
    }),
  );
}

/** MDBList's batch call, answering from TITLES. */
function fakeMdblist(): Promise<Server> {
  return listen(
    createServer((req, res) => {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        const source = /\/rating\/(?:movie|show)\/(imdb|tomatoes)/.exec(req.url ?? "")?.[1] as "imdb" | "tomatoes" | undefined;
        const ids: string[] = JSON.parse(body || "{}").ids ?? [];
        const ratings = ids.map((id) => ({ id, rating: source ? (TITLES.find((t) => t.id === id)?.[source] ?? null) : null }));
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ratings }));
      });
    }),
  );
}

test.beforeAll(async () => {
  addon = await localAddon();
  mdblist = await fakeMdblist();
  core = await startBridge(RATINGS_PORT, { MQ_DEV_MDBLIST: `http://127.0.0.1:${(mdblist.address() as AddressInfo).port}` });
  const c = new Client(`ws://127.0.0.1:${RATINGS_PORT}/ws`);
  try {
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1 } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  mdblist?.close();
});

async function open(browser: Browser, form: Form): Promise<Page> {
  const { width, height, scale } = FORMS[form];
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
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
  test(`${form}: the widest scores never wrap or get cut, stacked only on phone posters`, async ({ browser }) => {
    const page = await open(browser, form);
    await page.goto(`/?form=${form}`);
    const chips = page.locator(".tile .chip-rate");
    await expect(chips.filter({ hasText: "RT 100%" }).first()).toBeVisible({ timeout: 30_000 });

    // Posters: the chip and each of its scores.
    const chipBoxes = await scores(page, ".tile .chip-rate", ".pic");
    const parts = await scores(page, ".tile .chip-rate > span", ".chip-rate");
    expect(chipBoxes.length).toBeGreaterThan(0);
    for (const chip of chipBoxes) expect(chip, JSON.stringify(chip)).toMatchObject({ inside: true, cut: false });
    for (const part of parts) expect(part, JSON.stringify(part)).toMatchObject({ lines: 1, cut: false, inside: true });
    const widest = chipBoxes.find((c) => c.text.includes("RT 100%"))!;
    const pair = parts.filter((p) => ["IMDb 10.0", "RT 100%"].includes(p.text)).slice(0, 2);
    expect(pair.map((p) => p.text)).toEqual(["IMDb 10.0", "RT 100%"]);
    if (form === "phone") expect(pair[1]!.top).toBeGreaterThan(pair[0]!.top);
    else expect(Math.abs(pair[1]!.middle - pair[0]!.middle)).toBeLessThanOrEqual(1);
    // A score MDBList did not give has no slot.
    await expect(page.locator(".tile", { hasText: "No scores" }).locator(".chip-rate")).toHaveCount(0);
    await expect(page.locator(".tile", { hasText: "Only IMDb" }).locator(".chip-rate")).toHaveText("IMDb 7.4");
    measure(form, "poster chip", { chip: widest.width, poster: widest.outer, parts: pair.map((p) => p.width) });

    // The hero: side by side, one line each.
    const hero = await scores(page, ".hero-body .meta .rate", ".hero-body");
    expect(hero.map((h) => h.text)).toEqual(["IMDb 10.0", "RT 100%"]);
    for (const h of hero) expect(h, JSON.stringify(h)).toMatchObject({ lines: 1, cut: false, inside: true });
    expect(Math.abs(hero[1]!.middle - hero[0]!.middle)).toBeLessThanOrEqual(1);
    measure(form, "hero", hero.map((h) => h.width));
    await page.screenshot({ path: `${SHOTS}/ratings-home-${form}.jpg`, type: "jpeg", quality: 80 });

    // The title page: side by side, IMDb's links off TV, and the credit row.
    await page.goto(`/title/movie/${MAX.id}?form=${form}`);
    await expect(page.locator(".info .meta .rate").nth(1)).toHaveText("RT 100%", { timeout: 30_000 });
    const title = await scores(page, ".info .meta .rate", ".info");
    for (const t of title) expect(t, JSON.stringify(t)).toMatchObject({ lines: 1, cut: false, inside: true });
    // Side by side: one line through both (the phone's link is taller, a finger's size).
    expect(Math.abs(title[1]!.middle - title[0]!.middle)).toBeLessThanOrEqual(1);
    const link = page.locator('.info .meta a[data-f="title:imdb"]');
    if (form === "tv") await expect(link).toHaveCount(0);
    else {
      await expect(link).toHaveAttribute("href", `https://www.imdb.com/title/${MAX.id}/`);
      await expect(link).toHaveAttribute("aria-label", "IMDb 10.0, on IMDb");
      if (form === "phone") expect((await link.boundingBox())!.height).toBeGreaterThanOrEqual(44);
    }
    await expect(page.locator(".info .meta span.rate", { hasText: "RT 100%" })).toHaveAttribute("aria-label", "Rotten Tomatoes 100 percent");
    await expect(page.locator(".facts div", { hasText: "Ratings from" })).toContainText("IMDb and Rotten Tomatoes via MDBList");
    measure(form, "title page", title.map((t) => t.width));
    await page.screenshot({ path: `${SHOTS}/ratings-title-${form}.jpg`, type: "jpeg", quality: 80 });

    // Search: the meta line keeps every score whole.
    await page.goto(`/search?form=${form}`);
    const field = page.locator('[data-f="search:input"]');
    await expect(field).toBeVisible({ timeout: 30_000 });
    await field.fill("widest");
    await expect(page.locator(".hit-meta", { hasText: "RT 100%" }).first()).toBeVisible({ timeout: 30_000 });
    const meta = await page.locator(".hit-meta").first().evaluate((el) => ({ width: el.clientWidth, needs: el.scrollWidth }));
    expect(meta.needs, JSON.stringify(meta)).toBeLessThanOrEqual(meta.width);
    const hit = await scores(page, ".hit-meta .rate", ".hit-meta");
    for (const h of hit.slice(0, 2)) expect(h, JSON.stringify(h)).toMatchObject({ lines: 1, cut: false, inside: true });
    measure(form, "search hit", meta);
    await page.context().close();
  });
}
