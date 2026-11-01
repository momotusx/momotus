/** Search's "Top results" on a core of its own: a fake Trakt (MQ_DEV_TRAKT)
 * names titles that a local search addon also lists, so Top results repeats
 * titles of the row below it, as it does for a franchise. The remote walks
 * Down through every hit of both rows; on a phone the row under Top results
 * stays where it is while Top results answers, fails or finds nothing. */

import { type Browser, expect, type Page, test } from "./fixture";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, PORTS, SEARCH_TOP_PORT, startBridge } from "./core";

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let trakt: Server | null = null;

const TITLES = ["Journey", "Journey Home", "Journey Away", "Journey North", "Journey South"].map((name, i) => ({ id: `tt91000${i + 1}0`, name }));

/** Search catalog and meta for the titles above. */
function searchAddon(): Promise<Server> {
  const server = createServer((req, res) => {
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const url = decodeURIComponent(req.url ?? "");
    if (url === "/manifest.json") {
      const catalogs = [{ type: "movie", id: "jsearch", name: "Journey Search", extra: [{ name: "search", isRequired: true }] }];
      return json({ id: "dev.marquee.search", version: "1.0.0", name: "Journey Search", resources: ["catalog", "meta"], types: ["movie"], idPrefixes: ["tt910"], catalogs });
    }
    if (url.startsWith("/catalog/movie/jsearch/")) return json({ metas: TITLES.map((t) => ({ id: t.id, type: "movie", name: t.name })) });
    const meta = /^\/meta\/movie\/(tt\d+)\.json$/.exec(url);
    const title = TITLES.find((t) => t.id === meta?.[1]);
    if (title) return json({ meta: { id: title.id, type: "movie", name: title.name } });
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

/** Trakt's search, a moment late: an error for "journeyfail", nothing for
 * "journeynone", two of the titles for anything else. */
function fakeTrakt(): Promise<Server> {
  const server = createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://x");
    if (!url.pathname.startsWith("/search/")) {
      res.writeHead(404);
      return res.end();
    }
    const query = url.searchParams.get("query") ?? "";
    const hit = (t: { id: string; name: string }) => ({ type: "movie", score: 1, movie: { title: t.name, year: 2020, ids: { imdb: t.id } } });
    setTimeout(() => {
      if (query === "journeyfail") {
        res.writeHead(500);
        return res.end();
      }
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(query === "journeynone" ? [] : [hit(TITLES[1] ?? TITLES[0]!), hit(TITLES[0]!)]));
    }, 1500);
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

test.beforeAll(async () => {
  addon = await searchAddon();
  trakt = await fakeTrakt();
  core = await startBridge(SEARCH_TOP_PORT, { MQ_DEV_TRAKT: `http://127.0.0.1:${(trakt.address() as AddressInfo).port}` });
  const c = new Client(`ws://127.0.0.1:${SEARCH_TOP_PORT}/ws`);
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
  trakt?.close();
});

async function page(browser: Browser, viewport: { width: number; height: number }): Promise<Page> {
  const context = await browser.newContext({ viewport, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${SEARCH_TOP_PORT}/ws`);
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

const focused = (p: Page) => p.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);

test("TV: the remote walks Down through Top results and the row under it, past the titles they share", async ({ browser }) => {
  const p = await page(browser, { width: 1920, height: 1080 });
  await p.goto("/search?form=tv");
  const field = p.locator('[data-f="search:input"]');
  await expect(field).toBeVisible({ timeout: 30_000 });
  await field.focus();
  await p.keyboard.type("journey");
  const top = p.locator(".hits.top .hit");
  await expect(top).toHaveCount(2, { timeout: 30_000 });
  await expect(p.locator(".hits:not(.top) .hit")).toHaveCount(5);
  // Down from the field, the scope pill, then every hit in order.
  const all = await p.locator(".hit").evaluateAll((els) => els.map((e) => e.getAttribute("data-f")));
  expect(new Set(all).size).toBe(all.length);
  await p.keyboard.press("ArrowDown");
  await expect.poll(() => focused(p)).toBe("search:scope");
  const walked: (string | null)[] = [];
  for (let i = 0; i < all.length; i++) {
    await p.keyboard.press("ArrowDown");
    await expect.poll(() => focused(p)).toMatch(/^hit:/);
    walked.push(await focused(p));
  }
  expect(walked).toEqual(all);
  await p.context().close();
});

test("phone: the row under Top results stays put while Top results answers, fails or finds nothing", async ({ browser }) => {
  const p = await page(browser, { width: 390, height: 844 });
  await p.goto("/search?form=phone");
  const field = p.locator('[data-f="search:input"]');
  await expect(field).toBeVisible({ timeout: 30_000 });
  const row = p.locator(".hits:not(.top)").first();
  const y = () => row.evaluate((el) => Math.round(el.getBoundingClientRect().top));
  // Queries of its own: the core keeps a fresh Trakt answer, and a kept one
  // fills Top results without a wait.
  for (const [query, ends] of [
    ["journeys", ".hits.top .hit"],
    ["journeyfail", ".hits.top .palette-note"],
    ["journeynone", ".hits.top .palette-note"],
  ] as const) {
    await field.fill(query);
    // Top results still asking: its placeholders, and the row under it.
    await expect(p.locator(".hits.top .hit-skel").first()).toBeVisible({ timeout: 30_000 });
    await expect(row.locator(".hit")).toHaveCount(5, { timeout: 30_000 });
    const before = await y();
    await expect(p.locator(ends).first()).toBeVisible({ timeout: 30_000 });
    expect(await y(), query).toBe(before);
  }
  await p.context().close();
});
