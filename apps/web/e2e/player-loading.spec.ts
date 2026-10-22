/** The player's loading layer (plans/player-polish.md 1.2), on a core of its
 * own: a local addon names three films, "Loading Logo" with a logo that has
 * transparent margins (so the trim shows) and "Loading Name" with none (its
 * name fills), each with one link that holds its first byte until the test
 * lets it go (under the core's 8 s probe limit), and "Loading Stall", whose
 * link sends its first bytes and then holds the rest: the core is ready, the
 * player has the URL, and no frame comes. While it holds, the core's
 * own `start` is what the page shows; the proxy only watches it
 * (e2e/patch.ts). The proxy writes `start` only for what this bridge cannot
 * reach: a torrent's stages and their words (the e2e bridge has no torrent
 * engine, and a link's start says no words: its one `opening` sample comes
 * before 3 s) and a fill that would go back (the core never sends one). A
 * stall is the element's own `waiting` event. Muted, headless. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createServer, get, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, LOADING_PORT, SHOTS, startBridge, startIfPaused } from "./core";
import { patchedContext } from "./patch";
import { NAME, startSource } from "./source";

// 800x310 with its drawn box at (196, 9), 417x292, as the frames' title logo (design/exploration/v3/src/data.ts).
const LOGO =
  '<svg xmlns="http://www.w3.org/2000/svg" width="800" height="310"><rect x="196" y="9" width="417" height="292" rx="36" fill="#3ddc5a"/><text x="404.5" y="186" font-family="sans-serif" font-size="92" font-weight="700" text-anchor="middle" fill="#0d3b18">LOGO</text></svg>';
const BACKDROP =
  '<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#3b5b7a"/><stop offset="1" stop-color="#7a5b3b"/></linearGradient></defs><rect width="1280" height="720" fill="url(#g)"/><circle cx="900" cy="300" r="180" fill="#d8c7a0"/></svg>';
const TITLES = { mqlo: "Loading Logo", mqln: "Loading Name", mqls: "Loading Stall" } as const;
// What "Loading Stall" sends before it holds: the file's first bytes, too few for a frame.
const FIRST_BYTES = 3000;

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let media: Server | null = null;
// The links wait on this until the test lets them go.
let release = () => {};
let held = Promise.resolve();
const hold = () => {
  held = new Promise((go) => {
    release = go;
  });
};

function loadingAddon(film: string): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const url = req.url ?? "";
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const svg = (body: string) => {
      res.writeHead(200, { "content-type": "image/svg+xml", "access-control-allow-origin": "*" });
      res.end(body);
    };
    if (url === "/manifest.json") {
      return json({ id: "dev.marquee.loading", version: "1.0.0", name: "Loading Fixture", resources: ["meta", "stream"], types: ["movie"], idPrefixes: ["mql"], catalogs: [] });
    }
    const meta = /^\/meta\/movie\/(mql[ons])\.json$/.exec(url);
    if (meta) {
      const id = meta[1] as keyof typeof TITLES;
      return json({ meta: { id, type: "movie", name: TITLES[id], releaseInfo: "2008", background: `${base}/backdrop.svg`, ...(id === "mqlo" ? { logo: `${base}/logo.svg` } : {}) } });
    }
    const stream = /^\/stream\/movie\/(mql[ons])\.json$/.exec(url);
    if (stream) {
      const file = `${TITLES[stream[1] as keyof typeof TITLES].replace(/ /g, ".")}.2008.240p.WEB.VP9.webm`;
      const path = stream[1] === "mqls" ? "stall" : "held";
      return json({ streams: [{ name: "Loading Fixture\n240p", url: `${base}/${path}/${file}`, behaviorHints: { filename: file } }] });
    }
    if (url.startsWith("/stall/")) {
      // The film, its first bytes at once and the rest once the test lets it go.
      get(film, { headers: req.headers.range ? { range: req.headers.range } : {} }, (up) => {
        const parts: Buffer[] = [];
        up.on("data", (c: Buffer) => parts.push(c));
        up.on("end", () => {
          const body = Buffer.concat(parts);
          res.writeHead(up.statusCode ?? 200, up.headers);
          res.write(body.subarray(0, FIRST_BYTES));
          void held.then(() => res.end(body.subarray(FIRST_BYTES)));
        });
      });
      return;
    }
    if (url.startsWith("/held/")) {
      void held.then(() => {
        res.writeHead(302, { location: film });
        res.end();
      });
      return;
    }
    if (url === "/logo.svg") return svg(LOGO);
    if (url === "/backdrop.svg") return svg(BACKDROP);
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

test.beforeAll(async () => {
  media = await startSource(0);
  addon = await loadingAddon(`http://127.0.0.1:${(media.address() as AddressInfo).port}/media/${NAME}`);
  core = await startBridge(LOADING_PORT);
  const c = new Client(`ws://127.0.0.1:${LOADING_PORT}/ws`);
  try {
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1 } });
  } finally {
    c.close();
  }
});

test.afterEach(() => release());
test.afterAll(() => {
  release();
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  media?.close();
});

type Start = { stage: string; progress: number; caption: string | null };
type Form = "tv" | "desktop" | "phone";
const VIEWPORTS = { tv: { width: 1920, height: 1080 }, desktop: { width: 1440, height: 900 }, phone: { width: 844, height: 390 } };

/** A player page for `id` while its link holds; `starts` is every `start` the core sent, as sent. */
async function open(browser: Browser, id: keyof typeof TITLES, form: Form, more: { reducedMotion?: "reduce"; patch?: (view: string, s: Record<string, unknown>) => unknown } = {}) {
  const starts: Start[] = [];
  const { context, repush } = await patchedContext(browser, LOADING_PORT, { viewport: VIEWPORTS[form], reducedMotion: more.reducedMotion ?? "no-preference" }, (view, s) => {
    if (view === "playback" && s?.start) starts.push(s.start as Start);
    return more.patch ? more.patch(view, s) : s;
  });
  const page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(`/play/movie/${id}/${id}?form=${form}`);
  await mute(page);
  await expect(page.locator(".ld.on")).toBeVisible({ timeout: 30_000 });
  return { context, page, repush, errors, starts };
}

const mute = (page: Page) =>
  page.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
const fillOf = (page: Page) => page.locator(".ld.on .ld-mark").evaluate((el) => (el as HTMLElement).style.getPropertyValue("--p"));
/** Waits for the fill to ease to where it was asked to (1 s), then shoots. */
const shoot = async (page: Page, name: string) => {
  await page.waitForTimeout(1300);
  await page.screenshot({ path: `${SHOTS}/${name}.png` });
};

test("desktop: the core's start fills the logo under the pick's words; the picture takes over; a stall brings it back", async ({ browser }) => {
  hold();
  const { context, page, errors, starts } = await open(browser, "mqlo", "desktop");
  const layer = page.locator(".ld.on");
  // The logo trimmed to its drawn box, about 40 percent of the width, its faint copy whole under it.
  const solid = layer.locator("img.ld-logo.ld-solid");
  await expect(solid).toBeVisible();
  expect(await solid.getAttribute("src")).toMatch(/^data:image\/png/);
  expect(await solid.evaluate((img: HTMLImageElement) => img.naturalWidth / img.naturalHeight)).toBeCloseTo(417 / 292, 1);
  expect(((await solid.boundingBox())?.width ?? 0) / 1440).toBeCloseTo(0.4, 1);
  await expect(layer.locator("img.ld-logo.ld-faint")).toHaveCSS("opacity", "0.2");
  await expect(layer.locator(".ld-bd")).toHaveCSS("opacity", "0.3");
  // Back and nothing else.
  await expect(page.locator('[data-f="player:leave"]')).toBeVisible();
  await expect(page.locator('[data-f="player:toggle"]')).toHaveCount(0);
  await expect(page.locator(".ptitle h1")).toBeHidden();

  // The core's own start while the link holds (`opening`: the source is open, its first byte on
  // its way): its progress is the fill, its words (none yet) under the pick's line.
  await expect.poll(() => starts.length, { timeout: 5_000 }).toBeGreaterThan(0);
  const last = starts.at(-1) as Start;
  expect(last.stage).toBe("opening");
  expect(last.progress).toBeGreaterThan(0);
  await expect.poll(() => fillOf(page)).toBe(String(last.progress));
  await expect(layer.locator(".ld-cap span")).toHaveText([/^Starting .+ from Loading Fixture$/, ...(last.caption ? [last.caption] : [])]);
  // The words carry a halo over a bright backdrop, and a live region that is always there reads them.
  await expect(layer.locator(".ld-cap")).not.toHaveCSS("text-shadow", "none");
  const status = page.locator('.player-scr > p.sr[role="status"]');
  await expect(status).toHaveText(/^Starting .+ from Loading Fixture/);
  await expect(page.locator('.ld [role="status"]')).toHaveCount(0);
  await shoot(page, "player-loading-desktop");

  // The link answers: the first frame takes over.
  release();
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await mute(page);
  await startIfPaused(page);
  await expect(page.locator(".ld.on")).toHaveCount(0);
  await expect(page.locator(".ptitle h1")).toBeVisible();
  await expect(status).toHaveText("");

  // A stall after the first frame: the layer over the paused frame, no backdrop, no words, the controls usable.
  await page.evaluate(() => document.querySelector("video")?.dispatchEvent(new Event("waiting")));
  await expect(page.locator(".ld.on.stall")).toBeVisible({ timeout: 5_000 });
  await expect(page.locator(".ld.on .ld-bd")).toBeHidden();
  await expect(page.locator(".ld.on .ld-cap span")).toHaveCount(0);
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible();
  await page.evaluate(() => document.querySelector("video")?.dispatchEvent(new Event("playing")));
  await expect(page.locator(".ld.on")).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});

test("desktop: with the URL in and no frame yet, Back stays visible and works past the controls' hide time", async ({ browser }) => {
  hold();
  const { context, page, errors } = await open(browser, "mqls", "desktop");
  // The core is ready and the player has the URL; the frame waits for bytes that do not come.
  await expect.poll(() => page.evaluate(() => !!document.querySelector("video")?.currentSrc), { timeout: 30_000 }).toBe(true);
  await page.waitForTimeout(5_000);
  await expect(page.locator(".ld.on")).toBeVisible();
  expect(await page.evaluate(() => document.querySelector("video")?.readyState)).toBeLessThan(2);
  await expect(page.locator(".player-scr")).not.toHaveClass(/\bidle\b/);
  await expect(page.locator(".ptitle")).toHaveCSS("opacity", "1");
  await expect(page.locator(".ptitle")).toHaveCSS("pointer-events", "auto");
  const back = page.locator('[data-f="player:leave"]');
  await expect(back).toBeVisible();
  await back.click();
  await expect(page).not.toHaveURL(/\/play\//);
  expect(errors).toEqual([]);
  await context.close();
});

test("TV: nothing is focused while it loads, and a title with no logo fills its name", async ({ browser }) => {
  hold();
  const { context, page, errors, starts } = await open(browser, "mqln", "tv");
  const layer = page.locator(".ld.on");
  await expect(layer.locator(".ld-name.ld-solid")).toHaveText("Loading Name");
  await expect(layer.locator(".ld-name.ld-faint")).toHaveCSS("opacity", "0.2");
  await expect(layer.locator("img.ld-logo")).toHaveCount(0);
  await expect.poll(() => starts.length, { timeout: 15_000 }).toBeGreaterThan(0);
  await expect.poll(() => fillOf(page)).toBe(String(starts.at(-1)?.progress));
  // No Back to focus: the remote's Back cancels.
  await expect(page.locator('[data-f="player:leave"]')).toHaveCount(0);
  expect(await page.evaluate(() => document.activeElement?.getAttribute("data-f") ?? null)).toBeNull();
  await shoot(page, "player-loading-name-tv");
  await page.keyboard.press("Escape");
  await expect(page).not.toHaveURL(/\/play\//);
  expect(errors).toEqual([]);
  await context.close();
});

test("TV and phone: the logo at the form's size, Back on the phone", async ({ browser }) => {
  for (const form of ["tv", "phone"] as const) {
    hold();
    const { context, page, errors } = await open(browser, "mqlo", form);
    const solid = page.locator(".ld.on img.ld-logo.ld-solid");
    await expect(solid).toBeVisible();
    const width = (await solid.boundingBox())?.width ?? 0;
    // 40 percent of the width on TV; on the landscape phone the height binds (60 percent of it).
    if (form === "tv") expect(width / 1920).toBeCloseTo(0.4, 1);
    else expect(width).toBeCloseTo(390 * 0.6 * (417 / 292), -1);
    await expect(page.locator('[data-f="player:leave"]')).toHaveCount(form === "phone" ? 1 : 0);
    await shoot(page, `player-loading-${form}`);
    expect(errors).toEqual([]);
    await context.close();
    release();
  }
});

test("reduced motion: no pulse, the fill stays", async ({ browser }) => {
  hold();
  const still = await open(browser, "mqlo", "desktop", { reducedMotion: "reduce" });
  await expect(still.page.locator(".ld.on .ld-mark")).toHaveCSS("animation-name", "none");
  await still.context.close();
  const moving = await open(browser, "mqlo", "desktop");
  await expect(moving.page.locator(".ld.on .ld-mark")).toHaveCSS("animation-name", "ld-pulse");
  await moving.context.close();
});

// What this bridge cannot send: a torrent's start (no torrent engine in the e2e bridge), and a
// progress lower than the last (the core never goes back). The proxy writes `start` for these.
test("a torrent's start: the fill steps with it, its words come under the pick's, and it never goes back", async ({ browser }) => {
  // The link answers at once; the page is told the playback still resolves, with these numbers.
  let start: Start & Record<string, unknown> = { stage: "buffering", peers: 3, haveBytes: 1_700_000, needBytes: 8_400_000, bytesPerS: 900_000, source: 1, sources: 1, progress: 0.2, caption: null };
  const { context, page, repush, errors } = await open(browser, "mqlo", "desktop", { patch: (view, s) => (view === "playback" ? { ...s, phase: "resolving", url: null, start } : s) });
  const layer = page.locator(".ld.on");
  await expect.poll(() => fillOf(page)).toBe("0.2");
  start = { ...start, progress: 0.4 };
  repush("playback");
  await expect.poll(() => fillOf(page)).toBe("0.4");
  await shoot(page, "player-loading-torrent-desktop");
  start = { ...start, progress: 0.6, caption: "Finding peers, 3 so far" };
  repush("playback");
  await expect.poll(() => fillOf(page)).toBe("0.6");
  await expect(layer.locator(".ld-cap span")).toHaveText([/^Starting .+ from Loading Fixture$/, "Finding peers, 3 so far"]);
  await shoot(page, "player-loading-wait-desktop");
  start = { ...start, progress: 0.45, caption: "Buffering, 3.8 of 8.4 MB" };
  repush("playback");
  await expect(layer.locator(".ld-cap span").last()).toHaveText("Buffering, 3.8 of 8.4 MB");
  expect(await fillOf(page)).toBe("0.6");
  expect(errors).toEqual([]);
  await context.close();
});
