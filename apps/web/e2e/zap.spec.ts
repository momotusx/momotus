/** Zap (plans/zap.md 2 and 3) on a core of its own: a local addon's one catalog lists "Zap Film" (ten minutes, one
 * link to the test film), and "Zap Nothing", which no catalog lists and no addon streams, is reached by its address
 * alone. The dial is Far, so every zap draws from that catalog. Muted, headless. */

import { type Browser, expect, type Page, test } from "./fixture";
import type { ChildProcess } from "node:child_process";
import { mkdirSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, PORTS, SHOTS, startBridge, startIfPaused, ZAP_PORT } from "./core";
import { patchedContext } from "./patch";
import { NAME, startSource } from "./source";

const FILM = "Zap Film";
let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let media: Server | null = null;

function zapAddon(film: string): Promise<Server> {
  const server = createServer((req, res) => {
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const url = decodeURIComponent(req.url ?? "");
    if (url === "/manifest.json")
      return json({
        id: "dev.marquee.zap",
        version: "1.0.0",
        name: "Zap Fixture",
        resources: ["catalog", "meta", "stream"],
        types: ["movie"],
        idPrefixes: ["mqz"],
        catalogs: [{ type: "movie", id: "zap", name: "Zap Films", extra: [{ name: "genre", options: ["Drama"] }] }],
      });
    if (url.startsWith("/catalog/movie/zap")) return json({ metas: [{ id: "mqzf", type: "movie", name: FILM }] });
    if (url === "/meta/movie/mqzf.json") return json({ meta: { id: "mqzf", type: "movie", name: FILM, runtime: "10 min", releaseInfo: "2008" } });
    if (url === "/meta/movie/mqzx.json") return json({ meta: { id: "mqzx", type: "movie", name: "Zap Nothing", runtime: "10 min" } });
    if (url === "/stream/movie/mqzf.json") return json({ streams: [{ name: "Zap Fixture\n1080p", url: film, behaviorHints: { filename: NAME } }] });
    if (url === "/stream/movie/mqzx.json") return json({ streams: [] });
    res.writeHead(404);
    res.end();
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok(server)));
}

test.beforeAll(async () => {
  media = await startSource(0);
  addon = await zapAddon(`http://127.0.0.1:${(media.address() as AddressInfo).port}/media/${NAME}`);
  core = await startBridge(ZAP_PORT);
  const c = new Client(`ws://127.0.0.1:${ZAP_PORT}/ws`);
  try {
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1, zapDial: "far" } });
  } finally {
    c.close();
  }
});

test.afterAll(async () => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  media?.close();
});

async function page(browser: Browser, path: string, form: "desktop" | "phone" | "tv" = "desktop") {
  const viewport = form === "phone" ? { width: 390, height: 844 } : { width: 1440, height: 900 };
  const { context } = await patchedContext(browser, ZAP_PORT, { viewport, reducedMotion: "reduce" }, (_, s) => s);
  const p = await context.newPage();
  await p.goto(`${path}${path.includes("?") ? "&" : "?"}form=${form}`);
  return { page: p, close: () => context.close() };
}

async function shot(p: Page, name: string) {
  mkdirSync(SHOTS, { recursive: true });
  await p.screenshot({ path: `${SHOTS}/zap-${name}.png` });
}

const ZAP_PLAY = /\/play\/movie\/mqzf\/mqzf\?zap=(\d+)/;

test("Zap from the header plays the film from its middle, its name hidden until asked, and Back returns", async ({ browser }) => {
  const { page: p, close } = await page(browser, "/");
  try {
    await p.locator('[data-f="nav:zap"]').click();
    await expect(p).toHaveURL(ZAP_PLAY, { timeout: 30_000 });
    const at = Number(ZAP_PLAY.exec(p.url())?.[1]);
    // 15 percent of ten minutes at least, and never so late the hold ends past 80 percent (3 minutes).
    expect(at).toBeGreaterThanOrEqual(90_000);
    expect(at).toBeLessThanOrEqual(180_000);
    await startIfPaused(p);
    await expect.poll(() => p.evaluate(() => document.querySelector("video")?.currentTime ?? 0), { timeout: 20_000 }).toBeGreaterThanOrEqual(at / 1000 - 3);
    // The name nowhere on the page, nor in its title.
    await expect(p).toHaveTitle("Zap · Momotus");
    await expect(p.locator(".ptitle h1")).toHaveText("Zap");
    expect(await p.locator("body").innerText()).not.toContain(FILM);
    expect(await p.locator("body").evaluate((b) => b.innerHTML.includes("Zap Film"))).toBe(false);
    // Versions gives way to Zap again, Episodes and Next to the dial (Far pressed).
    await expect(p.locator('[data-f="player:versionsact"]')).toHaveCount(0);
    await expect(p.locator('[data-f="player:zap"]')).toBeVisible();
    await expect(p.locator('[data-f="player:dial-far"]')).toHaveAttribute("aria-pressed", "true");
    await shot(p, "playing");
    // By the keyboard: the button goes with the answer and the focus moves on to its first way on.
    await p.locator('[data-f="player:what"]').focus();
    await p.keyboard.press("Enter");
    await expect(p.locator(".ptitle h1")).toHaveText(FILM);
    await expect(p).toHaveTitle(`${FILM} · Momotus`);
    await expect(p.locator('[data-f="player:fromstart"]')).toBeFocused();
    await expect(p.locator('[data-f="player:opentitle"]')).toBeVisible();
    await shot(p, "revealed");
    // Back goes where Zap was pressed: the zap page gave way to the play, so Home.
    await p.locator('[data-f="player:leave"]').click();
    await expect(p).toHaveURL(new RegExp(`127\\.0\\.0\\.1:${PORTS.vite}/(\\?.*)?$`));
  } finally {
    await close();
  }
});

test("a zap play opened with no history behind it goes Home on Back, not to the title it hides", async ({ browser }) => {
  const { page: p, close } = await page(browser, "/play/movie/mqzf/mqzf?zap=120000");
  try {
    await expect(p.locator('[data-f="player:leave"]')).toBeVisible({ timeout: 20_000 });
    await p.locator('[data-f="player:leave"]').click();
    await expect(p).toHaveURL(new RegExp(`127\\.0\\.0\\.1:${PORTS.vite}/(\\?.*)?$`));
  } finally {
    await close();
  }
});

test("a zap that finds nothing to play zaps on, and the third in a row ends on Nothing played", async ({ browser }) => {
  const first = await page(browser, "/play/movie/mqzx/mqzx?zap=60000");
  try {
    // No source: no Try again and no other version, the next zap at once, its try counted.
    await expect(first.page).toHaveURL(/tries=1/, { timeout: 30_000 });
  } finally {
    await first.close();
  }
  // On TV, where the card takes the focus by itself.
  const third = await page(browser, "/play/movie/mqzx/mqzx?zap=60000&tries=2", "tv");
  try {
    await expect(third.page.getByText("Nothing played.")).toBeVisible({ timeout: 30_000 });
    await expect(third.page.getByRole("button", { name: "Try again" })).toHaveCount(0);
    await expect(third.page.locator('[data-f="end:zap"]')).toBeFocused();
    await shot(third.page, "nothing-played");
    // Zap again starts over from 0 tries and plays.
    await third.page.locator('[data-f="end:zap"]').click();
    await expect(third.page).toHaveURL(ZAP_PLAY, { timeout: 30_000 });
    expect(third.page.url()).not.toContain("tries=");
  } finally {
    await third.close();
  }
});

test("turning the dial keeps it and zaps from there", async ({ browser }) => {
  const { page: p, close } = await page(browser, "/play/movie/mqzf/mqzf?zap=120000");
  try {
    await expect(p.locator('[data-f="player:dial-near"]')).toBeVisible({ timeout: 20_000 });
    await p.locator('[data-f="player:dial-near"]').click();
    // Near with no library falls to Far: the film again, the dial on Near.
    await expect(p).toHaveURL(ZAP_PLAY, { timeout: 30_000 });
    await expect(p.locator('[data-f="player:dial-near"]')).toHaveAttribute("aria-pressed", "true", { timeout: 20_000 });
  } finally {
    const c = new Client(`ws://127.0.0.1:${ZAP_PORT}/ws`);
    await c.req("setPrefs", { patch: { zapDial: "far" } }).finally(() => c.close());
    await close();
  }
});

test("the phone's Zap sits at Home's title; the zap page shows no title while it deals", async ({ browser }) => {
  const { page: p, close } = await page(browser, "/", "phone");
  try {
    const zap = p.locator('.ph-top [data-f="nav:zap"]');
    await expect(zap).toBeVisible({ timeout: 20_000 });
    const box = await zap.boundingBox();
    expect(box?.width).toBeGreaterThanOrEqual(44);
    await shot(p, "phone-home");
    await zap.click();
    await expect(p).toHaveURL(ZAP_PLAY, { timeout: 30_000 });
  } finally {
    await close();
  }
});
