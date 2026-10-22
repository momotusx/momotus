/** No addon's name outside the Addons pages and Customize Home (the owner, 2026-10-22;
 * design/DECISION.md, Shelves), on a core of its own: two local addons list a "Popular" movie
 * catalog each, one of them also a catalog that fails, one that answers late and one that is
 * empty, and both a movie search. Home, the hero, its row states, Search and Discover never say
 * either addon's name; the later twin row waits off Home; Customize Home names each row's addon
 * on a quiet second line. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, NAMES_PORT, PORTS, startBridge } from "./core";

let core: { proc: ChildProcess; data: string } | null = null;
const addons: Server[] = [];

const NAMES = ["Alpha Catalogs", "Beta Catalogs"];
// A 1x1 PNG: the hero is picked from a tile with a backdrop.
const PNG = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=", "base64");

function addon(n: 0 | 1): Promise<Server> {
  const prefix = `tt93${n}`;
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const metas = (tag: string) =>
      [1, 2, 3].map((i) => ({ id: `${prefix}00${i}`, type: "movie", name: `${tag} ${n ? "Beta" : "Alpha"} ${i}`, background: `${base}/bg.png` }));
    const url = decodeURIComponent(req.url ?? "");
    if (url === "/manifest.json") {
      const catalogs = [
        { type: "movie", id: "pop", name: "Popular" },
        ...(n === 0
          ? [
              { type: "movie", id: "broken", name: "Broken" },
              { type: "movie", id: "late", name: "Late" },
              { type: "movie", id: "quiet", name: "Quiet" },
            ]
          : []),
        { type: "movie", id: "find", name: "Find", extra: [{ name: "search", isRequired: true }] },
        // Search groups that fail (Alpha's series search) and answer late (Beta's channel search).
        n === 0 ? { type: "series", id: "lost", name: "Lost", extra: [{ name: "search", isRequired: true }] } : { type: "tv", id: "late", name: "Late", extra: [{ name: "search", isRequired: true }] },
      ];
      return json({ id: `dev.marquee.names${n}`, version: "1.0.0", name: NAMES[n], resources: ["catalog"], types: ["movie", "series", "tv"], idPrefixes: [prefix], catalogs });
    }
    if (url === "/bg.png") {
      res.writeHead(200, { "content-type": "image/png" });
      return res.end(PNG);
    }
    if (url === "/catalog/movie/pop.json") return json({ metas: metas("Pop") });
    if (url === "/catalog/movie/quiet.json") return json({ metas: [] });
    // Past the core's soft deadline (2.5 s) and inside its hard one (12 s): slow, then ready.
    if (url === "/catalog/movie/late.json") return void setTimeout(() => json({ metas: metas("Late") }), 8000);
    if (url.startsWith("/catalog/movie/find/")) return json({ metas: metas("Pop") });
    if (url.startsWith("/catalog/tv/late/")) return void setTimeout(() => json({ metas: [] }), 8000);
    res.writeHead(500);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

test.beforeAll(async () => {
  addons.push(await addon(0), await addon(1));
  // Trakt's Top results at a closed port: this spec asks no service off the machine.
  core = await startBridge(NAMES_PORT, { MQ_DEV_TRAKT: "http://127.0.0.1:9" });
  const c = new Client(`ws://127.0.0.1:${NAMES_PORT}/ws`);
  try {
    for (const a of addons) await c.req("installAddon", { url: `http://127.0.0.1:${(a.address() as AddressInfo).port}/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1 } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  for (const a of addons) a.close();
});

async function open(browser: Browser, path: string): Promise<Page> {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: "dark", reducedMotion: "reduce", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${NAMES_PORT}/ws`);
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
  await page.goto(`${path}?form=desktop`);
  return page;
}

const noAddonName = async (page: Page) => {
  const text = await page.evaluate(() => document.body.innerText);
  for (const name of NAMES) expect(text).not.toContain(name);
  expect(text).not.toMatch(/\b(Alpha|Beta) Catalogs\b/);
};

test("Home, the hero and every row state name no addon, and the later twin waits off Home", async ({ browser }) => {
  const page = await open(browser, "/");
  const row = (title: string) => page.locator(".row", { has: page.locator(".row-head h2", { hasText: new RegExp(`^${title}$`) }) });
  // The late row is slow first, in words of its own.
  await expect(row("Late Movies").locator(".row-note")).toHaveText("Slow to answer, still trying", { timeout: 30_000 });
  await noAddonName(page);
  await expect(page.locator(".hero .reason")).toHaveText("#1 in Popular Movies", { timeout: 30_000 });
  await expect(row("Broken Movies").locator(".row-msg p").first()).toHaveText("This row did not load.");
  await expect(row("Quiet Movies").locator(".row-msg p")).toHaveText("Nothing in this row right now.");
  await expect(row("Late Movies").locator(".tile").first()).toBeVisible({ timeout: 30_000 });
  // One "Popular Movies": Alpha's, first in addon order; Beta's is off Home.
  await expect(row("Popular Movies")).toHaveCount(1);
  await expect(row("Popular Movies").locator(".tile").first()).toHaveAttribute("aria-label", /^Pop Alpha 1/);
  await expect(page.locator(".row-src")).toHaveCount(0);
  await noAddonName(page);
  await page.context().close();
});

test("Search merges both addons' movie search into one Movies group, and no group's state names an addon", async ({ browser }) => {
  const page = await open(browser, "/search");
  await page.locator('[data-f="search:input"]').fill("pop");
  await expect(page.locator(".hits:not(.top) .hit").first()).toBeVisible({ timeout: 30_000 });
  // The late channel search is slow first, in words of its own.
  await expect(page.locator(".hits:not(.top) .palette-note.warn")).toHaveText("Slow to answer, still trying", { timeout: 30_000 });
  await noAddonName(page);
  const movies = page.locator(".hits:not(.top)", { has: page.locator(".hits-head .kicker", { hasText: /^Movies$/ }) });
  await expect(movies).toHaveCount(1);
  await expect(movies).toHaveAttribute("aria-label", "Movies");
  // The series search failed: its group says so without naming the addon.
  await expect(page.locator(".hits:not(.top) .palette-note.bad")).toContainText("These results did not load.", { timeout: 30_000 });
  await noAddonName(page);
  await page.context().close();
});

test("Discover's catalogs name no addon; Customize Home names each row's on a quiet second line", async ({ browser }) => {
  const page = await open(browser, "/discover");
  await expect(page.locator('[data-fgroup="catalogs"] [data-f]').first()).toBeVisible({ timeout: 30_000 });
  await noAddonName(page);
  await page.goto("/home-rows?form=desktop");
  const popular = page.locator(".hrow", { has: page.locator(".hrow-text b", { hasText: /^Popular Movies$/ }) });
  await expect(popular).toHaveCount(2, { timeout: 30_000 });
  await expect(popular.locator(".hrow-text small")).toHaveText(["Alpha Catalogs", "Beta Catalogs · Hidden from Home"]);
  await expect(popular.nth(1).locator(".hrow-main")).toHaveAttribute("aria-label", /^Popular Movies, Beta Catalogs, hidden, row \d+ of \d+$/);
  // Quiet: the 64 percent secondary line.
  expect(await popular.first().locator(".hrow-text small").evaluate((el) => getComputedStyle(el).color)).toBe("rgba(255, 255, 255, 0.64)");
  await page.context().close();
});
