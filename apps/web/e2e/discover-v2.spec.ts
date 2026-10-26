/** Discover v2 (plans/discover-v2.md, the owner's pick: variant 1) on a core of its own with the
 * default addons and the real catalogs behind them: Movies, Series and Anime as a segmented
 * control, the services as name tiles with All first, Genre and Year menus, Clear, the sort menu
 * at the right, and no addon's name anywhere. A TV walks it with keys only. Shots go to SHOTS
 * (e2e/core.ts) at desktop, phone and TV sizes. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, DISCOVER_PORT, PORTS, SHOTS, startBridge } from "./core";

type Form = "tv" | "desktop" | "phone";
const FORMS = {
  tv: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 },
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
};
const ADDON_NAMES = ["Cinemeta", "Kitsu", "Trakt", "Streaming Catalogs", "Torrent", "Rotten Tomatoes", "RT:"];

let core: { proc: ChildProcess; data: string } | null = null;
let client: Client;
let tmdb: Server;
/** How long the core's answers with titles wait on their way to the page: a slow list, for the dimmed one before it. */
let lag = 0;

/** TMDB's watch providers as /3/watch/providers/movie listed them on 2026-10-27, served by a local fake (the e2e core
 * never holds the owner's token); the logos themselves load from TMDB's image host through the recorder. */
const PROVIDERS = [
  { provider_id: 8, provider_name: "Netflix", logo_path: "/rK1KljqmbvO9HQa1PBFLILWah72.png" },
  { provider_id: 9, provider_name: "Amazon Prime Video", logo_path: "/gMZdpavHmxFNnLpMHwVxfqeux2g.png" },
  { provider_id: 11, provider_name: "MUBI", logo_path: "/k7iSlvgWzZuO4zU5PcBjhABMuia.png" },
  { provider_id: 15, provider_name: "Hulu", logo_path: "/44uAnmSqvA4yBOdbPWN8YgQHjWm.png" },
  { provider_id: 283, provider_name: "Crunchyroll", logo_path: "/uFL3c4Cq8M6WoLymlC5Y8bmGytV.png" },
  { provider_id: 337, provider_name: "Disney Plus", logo_path: "/5eZ872CghnHFLB1j8grszbrx0dx.png" },
  { provider_id: 350, provider_name: "Apple TV", logo_path: "/9icYBfYFcwgCbky5VdGUIKJ4C5i.png" },
  { provider_id: 386, provider_name: "Peacock Premium", logo_path: "/a1UIdq5BrkcAxnxcUhFsNbXnxeu.png" },
  { provider_id: 531, provider_name: "Paramount Plus", logo_path: "/pkx3klJlwW5JdtaulvDx6hDNtch.png" },
  { provider_id: 1899, provider_name: "HBO Max", logo_path: "/skypuy7SXuugIQeYg0IglmzoKaS.png" },
];

test.beforeAll(async () => {
  tmdb = createServer((req, res) => {
    const body = req.url?.startsWith("/3/watch/providers/movie") ? JSON.stringify({ results: PROVIDERS }) : "{}";
    res.writeHead(req.url?.startsWith("/3/watch/providers/movie") ? 200 : 404, { "content-type": "application/json" }).end(body);
  });
  await new Promise<void>((ok) => tmdb.listen(0, "127.0.0.1", ok));
  core = await startBridge(DISCOVER_PORT, { MQ_DEV_TMDB: `http://127.0.0.1:${(tmdb.address() as AddressInfo).port}` });
  client = new Client(`ws://127.0.0.1:${DISCOVER_PORT}/ws`);
  await client.req("installDefaults", {
    choices: ["cinemeta", "trakt", "rottentomatoes", "torrentcatalogs", "kitsu", "streamingcatalogs"].map((key) => ({ key, on: true })),
  });
  await client.req("setPrefs", { patch: { defaultsSeen: 99 } });
});

test.afterAll(async () => {
  client.close();
  tmdb.close();
  if (core) cleanup(core.proc, core.data);
  core = null;
});

async function device(browser: Browser, form: Form): Promise<Page> {
  const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${DISCOVER_PORT}/ws`);
    const early: string[] = [];
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => {
      const data = String(e.data);
      // Only what carries titles waits: the loading answer comes at once.
      if (lag > 0 && data.includes('"items":[{')) setTimeout(() => ws.send(data), lag);
      else ws.send(data);
    });
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((m) => (upstream.readyState === WebSocket.OPEN ? upstream.send(String(m)) : early.push(String(m))));
  });
  const page = await context.newPage();
  await page.goto(`/discover?form=${form}`);
  await expect(page.locator(".grid .tile").first()).toBeVisible({ timeout: 60_000 });
  return page;
}

/** A shot after the covers have come and the list has settled. */
async function shot(page: Page, name: string) {
  await expect(page.locator(".grid-foot")).toBeVisible({ timeout: 60_000 });
  await expect(page.locator(".grid .tile img.in").first()).toBeVisible({ timeout: 60_000 });
  if (await page.locator(".dv-rail").count()) await expect(page.locator(".dv-rail .svc-tile.logo").first()).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(600);
  await page.screenshot({ path: `${SHOTS}/${name}.png` });
}

async function noAddonNames(page: Page) {
  const text = await page.locator("main").innerText();
  for (const name of ADDON_NAMES) expect(text, name).not.toContain(name);
}

async function pick(page: Page, group: string, label: RegExp | string) {
  await page.locator(`[data-f="${group}:menu"]`).click();
  await page.locator(".pull-opt", { hasText: label }).first().click();
}

test("the picked design: kinds, services, menus and Clear on desktop", async ({ browser }) => {
  const page = await device(browser, "desktop");
  const kinds = page.locator('[data-fgroup="kinds"] .chip');
  await expect(kinds).toHaveText(["Movies", "Series", "Anime"]);
  const rail = page.locator(".dv-rail .svc-tile");
  await expect(rail.first()).toHaveText("All");
  await expect(rail.first()).toHaveAttribute("aria-checked", "true");
  for (const service of ["Netflix", "Prime Video", "Disney+", "Apple TV+", "HBO Max", "Paramount+", "Hulu"]) {
    await expect(rail.filter({ hasText: new RegExp(`^${service.replace("+", "\\+")}$`) })).toHaveCount(1);
  }
  // The services show TMDB's logos, each named for a screen reader.
  await expect(rail.filter({ hasText: /^Netflix$/ }).locator("img")).toHaveAttribute("src", "https://image.tmdb.org/t/p/w154/rK1KljqmbvO9HQa1PBFLILWah72.png");
  await expect(rail.filter({ hasText: /^Netflix$/ })).toHaveAttribute("aria-label", "Netflix");
  await expect(page.locator(".dv-rail .svc-tile.logo").first()).toBeVisible({ timeout: 30_000 });
  await shot(page, "discover-v2-desktop-all");
  await noAddonNames(page);

  // A service, then the IMDb sort: the score sits on the poster's corner.
  await rail.filter({ hasText: /^Netflix$/ }).click();
  await expect(rail.filter({ hasText: /^Netflix$/ })).toHaveAttribute("aria-checked", "true");
  await pick(page, "sorts", /^IMDb rating/);
  await expect(page.locator(".grid .tile .corner").first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".grid-foot")).toContainText("titles");
  await shot(page, "discover-v2-desktop-netflix-imdb");

  // Clear shows with a filter and puts the service back to All, keeping the sort.
  await expect(page.locator('[data-f="discover:clear"]')).toBeVisible();
  await page.locator('[data-f="discover:clear"]').click();
  await expect(rail.first()).toHaveAttribute("aria-checked", "true");
  await expect(page.locator('[data-f="discover:clear"]')).toHaveCount(0);
  await expect(page.locator('[data-f="sorts:menu"]')).toContainText("IMDb rating");
  await expect(page.locator(".grid-foot")).toContainText("on your services");

  // Anime: Crunchyroll is its service; Movies and Series do not list it.
  await kinds.filter({ hasText: "Anime" }).click();
  await expect(rail.filter({ hasText: /^Crunchyroll$/ })).toHaveCount(1);
  await shot(page, "discover-v2-desktop-anime");
  await noAddonNames(page);
  await kinds.filter({ hasText: "Movies" }).click();
  await expect(rail.filter({ hasText: /^Crunchyroll$/ })).toHaveCount(0);

  // The choice is remembered: a new page opens on it.
  await rail.filter({ hasText: /^Hulu$/ }).click();
  await expect(rail.filter({ hasText: /^Hulu$/ })).toHaveAttribute("aria-checked", "true");
  await page.waitForTimeout(800);
  await page.context().close();
  const again = await device(browser, "desktop");
  await expect(again.locator(".dv-rail .svc-tile", { hasText: /^Hulu$/ })).toHaveAttribute("aria-checked", "true");
  await again.locator(".dv-rail .svc-tile").first().click();
  await again.context().close();
});

test("a change keeps the list before, dimmed, until the next one comes", async ({ browser }) => {
  const page = await device(browser, "desktop");
  await shot(page, "discover-v2-desktop-before-change");
  lag = 1500;
  try {
    await page.locator('[data-fgroup="kinds"] .chip', { hasText: "Series" }).click();
    await expect(page.locator(".dv-dim .grid .tile").first()).toBeVisible({ timeout: 5_000 });
    await expect(page.locator(".dv-dim")).toHaveAttribute("inert", "");
  } finally {
    lag = 0;
  }
  await expect(page.locator(".dv-dim")).toHaveCount(0, { timeout: 30_000 });
  await page.locator('[data-fgroup="kinds"] .chip', { hasText: "Movies" }).click();
  await page.context().close();
});

test("the phone shows the same controls in one column", async ({ browser }) => {
  const page = await device(browser, "phone");
  await expect(page.locator('[data-fgroup="kinds"] .chip')).toHaveText(["Movies", "Series", "Anime"]);
  await page.locator(".dv-rail .svc-tile", { hasText: /^Prime Video$/ }).click();
  await pick(page, "sorts", /^Newest$/);
  await expect(page.locator(".grid .tile .corner").first()).toBeVisible({ timeout: 30_000 });
  await shot(page, "discover-v2-phone-prime-newest");
  await noAddonNames(page);
  await page.locator('[data-f="discover:clear"]').click();
  await page.context().close();
});

test("a TV walks it row by row with keys only", async ({ browser }) => {
  const page = await device(browser, "tv");
  const focused = () => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");
  await expect.poll(focused).toMatch(/^kinds:/);
  await page.keyboard.press("ArrowDown");
  await expect.poll(focused).toMatch(/^discover:rail:/);
  // Entering the rail lands on the chosen service; Left walks to its start and stays there.
  for (let i = 0; i < 12; i++) await page.keyboard.press("ArrowLeft");
  await expect.poll(focused).toBe("discover:rail:all");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await expect(page.locator(".dv-rail .svc-tile").nth(1)).toHaveAttribute("aria-checked", "true");
  await expect.poll(focused).toBe(await page.locator(".dv-rail .svc-tile").nth(1).getAttribute("data-f"));
  await shot(page, "discover-v2-tv-rail");
  await page.keyboard.press("ArrowDown");
  await expect.poll(focused).toMatch(/^(genres|decades|sorts|discover):/);
  await page.keyboard.press("ArrowDown");
  await expect.poll(focused).toMatch(/^g:discover:/);
  await noAddonNames(page);
  await page.context().close();
});
