/** Offline downloads on the web (plans/offline-downloads.md 3, D6), on a core of its own: Big Buck
 * Bunny from a local addon whose one stream is a local HTTP origin that serves the test film and holds
 * after its first part until the test lets it go. Download it from the title page, see the ring,
 * pause, resume, done; go offline (the browser's offline event, which tells the core `none`, and the
 * origin and addon gone), see Home's banner, play it from Downloads; delete it. Then the version to
 * keep through Versions, Settings' section, and the TV form, which shows none of it. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createReadStream, statSync } from "node:fs";
import { createServer, type Server, type ServerResponse } from "node:http";
import type { AddressInfo, Socket } from "node:net";
import { Client, cleanup, DOWNLOADS_PORT, startBridge } from "./core";
import { patchedContext } from "./patch";
import { BBB_ID, FILM_FILE, NAME } from "./source";

const SIZE = statSync(FILM_FILE).size;
// What the origin sends before it holds: the ring shows a value under 100 until the test lets go.
const HELD_AT = 256 * 1024;

let core: { proc: ChildProcess; data: string } | null = null;
let origin: Server | null = null;
let addon: Server | null = null;
const sockets = new Set<Socket>();
let open = false;
const waiting: (() => void)[] = [];
const letGo = () => {
  open = true;
  for (const go of waiting.splice(0)) go();
};

/** The film with ranges and a validator, written in 64 KiB steps; past HELD_AT it waits for `letGo`. */
function serveFilm(range: string | undefined, ifRange: string | undefined, res: ServerResponse) {
  const m = /bytes=(\d*)-(\d*)/.exec(range ?? "");
  const ranged = !!m && (!ifRange || ifRange === '"film-1"');
  const start = ranged && m?.[1] ? Number(m[1]) : 0;
  const end = ranged && m?.[2] ? Math.min(Number(m[2]), SIZE - 1) : SIZE - 1;
  const head = { "content-type": "video/webm", "accept-ranges": "bytes", etag: '"film-1"', "content-length": String(end - start + 1) };
  res.writeHead(ranged ? 206 : 200, ranged ? { ...head, "content-range": `bytes ${start}-${end}/${SIZE}` } : head);
  const file = createReadStream(FILM_FILE, { start, end, highWaterMark: 64 * 1024 });
  let sent = start;
  file.on("data", (chunk) => {
    sent += chunk.length;
    res.write(chunk);
    if (sent >= HELD_AT && !open) {
      file.pause();
      waiting.push(() => file.resume());
    }
  });
  file.on("end", () => res.end());
  res.on("close", () => file.destroy());
}

// The network gone, for the core: the origin and the addon drop every connection.
let down = false;

function listen(server: Server): Promise<string> {
  server.on("connection", (s) => {
    if (down) return void s.destroy();
    sockets.add(s);
    s.on("close", () => sockets.delete(s));
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok(`http://127.0.0.1:${(server.address() as AddressInfo).port}`)));
}

test.beforeAll(async () => {
  origin = createServer((req, res) => {
    if (req.url === `/${NAME}`) return serveFilm(req.headers.range, req.headers["if-range"] as string | undefined, res);
    res.writeHead(404);
    res.end();
  });
  const film = `${await listen(origin)}/${NAME}`;
  addon = createServer((req, res) => {
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    if (req.url === "/manifest.json") return json({ id: "dev.marquee.downloads", version: "1.0.0", name: "Local Film", resources: ["stream"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] });
    if (req.url === `/stream/movie/${BBB_ID}.json`) return json({ streams: [{ name: "Local Film\n240p", url: film, behaviorHints: { filename: NAME, videoSize: SIZE } }] });
    json({ streams: [] });
  });
  const addonUrl = await listen(addon);
  core = await startBridge(DOWNLOADS_PORT);
  const c = new Client(`ws://127.0.0.1:${DOWNLOADS_PORT}/ws`);
  try {
    // Cinemeta names the title (answered from the recording offline).
    await c.req("installAddon", { url: "https://v3-cinemeta.strem.io/manifest.json" });
    await c.req("installAddon", { url: `${addonUrl}/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1 } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  for (const s of sockets) s.destroy();
  origin?.close();
  addon?.close();
});

/** A page on the spec's core (the app's socket goes to it, never to the shared one). */
async function openPage(browser: Browser, path: string) {
  const { context } = await patchedContext(browser, DOWNLOADS_PORT, { viewport: { width: 1440, height: 900 } }, (_view, s) => s);
  const page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(path);
  return { context, page, errors };
}

const control = (page: Page) => page.locator('[data-f="title:download"]');
const item = (page: Page, name: string) => page.getByRole("menuitem", { name, exact: true });
const banner = (page: Page) => page.locator(".dl-banner");
/** The bridge told the network, as the app tells it on the browser's offline and online events. */
async function network(type: "none" | "other") {
  const c = new Client(`ws://127.0.0.1:${DOWNLOADS_PORT}/ws`);
  try {
    await c.req("lifecycle", { event: "networkChanged", network: type, metered: false, lowData: false });
  } finally {
    c.close();
  }
}

test("a film downloads from the title page with its ring, pauses and resumes until it is done", async ({ browser }) => {
  const { context, page, errors } = await openPage(browser, `/title/movie/${BBB_ID}?form=desktop`);
  // Download, last in the action row, starts it at once: the ring shows its value under the held origin.
  await expect(control(page)).toHaveAttribute("aria-label", "Download");
  await control(page).click();
  await expect(control(page)).toHaveAttribute("aria-label", "Downloading", { timeout: 30_000 });
  await expect(control(page).locator(".dl-mark.ring")).toBeVisible();
  await expect(control(page)).toHaveAttribute("aria-valuetext", /^\d+ percent$/);
  await expect(control(page)).toHaveAttribute("aria-haspopup", "menu");
  const held = Number((await control(page).getAttribute("aria-valuetext"))?.split(" ")[0]);
  expect(held).toBeGreaterThan(0);
  expect(held).toBeLessThan(100);
  await expect(control(page)).toContainText(`${held}%`);

  // Its menu: Pause, then Resume.
  await control(page).click();
  await expect(page.getByRole("dialog")).toContainText("Big Buck Bunny");
  await item(page, "Pause download").click();
  await expect(control(page)).toHaveAttribute("aria-label", "Paused");
  await expect(control(page).locator(".dl-mark.ring")).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await control(page).click();
  await expect(item(page, "Cancel download")).toBeVisible();
  // Back closes the menu and gives the focus back to the control.
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(control(page)).toBeFocused();
  await control(page).click();
  await item(page, "Resume download").click();
  letGo();
  await expect(control(page)).toHaveAttribute("aria-label", "Downloaded", { timeout: 30_000 });
  await expect(control(page)).not.toHaveAttribute("aria-valuetext", /.*/);
  await expect(control(page).locator(".dl-mark.ring")).toHaveCount(0);

  // Library's way in, then the row with its size and its control.
  await page.goto("/library?form=desktop");
  await page.locator('[data-f="library:downloads"]').click();
  await expect(page).toHaveURL(/\/library\/downloads$/);
  // The e2e core is built without the torrent engine, whose disk figure gives the free space.
  await expect(page.locator(".dl-storage-line")).toHaveText(/^Downloads use 922 KB( · .+ free on this device)?$/);
  const row = page.locator(".dlrow", { hasText: "Big Buck Bunny" });
  await expect(row).toContainText("922 KB");
  await expect(row.getByRole("button", { name: "Downloaded" })).toBeVisible();
  expect(errors).toEqual([]);
  await context.close();
});

test("the TV form shows no control, no Downloads entry, no section and no offline banner", async ({ browser }) => {
  const tv = await openPage(browser, `/title/movie/${BBB_ID}?form=tv`);
  const page = tv.page;
  await expect(page.locator('[data-f="title:library"]')).toBeVisible({ timeout: 30_000 });
  await expect(control(page)).toHaveCount(0);
  await expect(page.locator(".dl-mark")).toHaveCount(0);
  await page.goto("/library");
  await expect(page.getByRole("heading", { name: "Library", exact: true })).toBeVisible();
  await expect(page.locator('[data-f="library:downloads"]')).toHaveCount(0);
  await page.goto("/settings");
  await expect(page.getByRole("heading", { name: "Playback", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Downloads", exact: true })).toHaveCount(0);
  // Offline with a film kept: the desktop says so on Home (the browser's own offline event tells the
  // core), the TV does not.
  const desk = await openPage(browser, "/?form=desktop");
  await page.goto("/");
  await expect(page.locator('[data-f="nav:/"]')).toBeVisible();
  await desk.context.setOffline(true);
  await expect(banner(desk.page)).toContainText("You're offline. Your downloads still play.", { timeout: 30_000 });
  await tv.context.setOffline(true);
  await expect(banner(page)).toHaveCount(0);
  await page.waitForTimeout(500);
  await expect(banner(page)).toHaveCount(0);
  expect([...tv.errors, ...desk.errors]).toEqual([]);
  await tv.context.close();
  await desk.context.close();
});

test("offline, Home says the downloads play, one plays from Downloads, then it is deleted", async ({ browser }) => {
  // The origin and the addon are gone. Playwright's offline also cuts the page from the core's loopback
  // stream server, which a browser without a network still reaches, so the bridge is told `none` here,
  // as the app tells it on the browser's offline event (the previous test).
  down = true;
  for (const s of sockets) s.destroy();
  await network("none");
  const { context, page, errors } = await openPage(browser, "/?form=desktop");
  await expect(banner(page)).toContainText("You're offline. Your downloads still play.", { timeout: 30_000 });
  await banner(page).getByRole("link", { name: "Go to Downloads" }).click();
  await expect(page).toHaveURL(/\/library\/downloads$/);

  // Played from Downloads: the normal play path, from disk.
  const started = Date.now();
  await page.locator(".dlrow-main", { hasText: "Big Buck Bunny" }).click();
  await expect(page).toHaveURL(new RegExp(`/play/movie/${BBB_ID}/${BBB_ID}`));
  await page.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
  await page.waitForFunction(() => (document.querySelector("video")?.currentTime ?? 0) > 0.5, null, { timeout: 30_000 });
  console.log(`offline start: ${Date.now() - started} ms from the row's press to 0.5 s played`);
  await page.goBack();

  // Deleted with the row's trash button: the empty state, and Home has no banner left.
  await expect(page).toHaveURL(/\/library\/downloads$/);
  await page.getByRole("button", { name: "Delete Big Buck Bunny" }).click();
  await expect(page.getByRole("heading", { name: "Nothing downloaded yet" })).toBeVisible();
  await expect(page.getByText("Download movies and episodes to watch them without a connection.")).toBeVisible();
  await expect(page.locator(".dlrow")).toHaveCount(0);
  await page.locator('[data-f="nav:/"]').click();
  await expect(banner(page)).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});

test("Choose a version to download keeps the version picked on Versions, and Settings has its section", async ({ browser }) => {
  down = false;
  await network("other");
  const { context, page, errors } = await openPage(browser, `/title/movie/${BBB_ID}?form=desktop`);
  await expect(control(page)).toHaveAttribute("aria-label", "Download", { timeout: 30_000 });
  // A secondary press on Download (a right click, a long press, the menu key) offers the version too.
  await control(page).click({ button: "right" });
  await expect(item(page, "Download")).toBeVisible();
  await item(page, "Choose a version to download").click();
  await expect(page).toHaveURL(new RegExp(`/versions/${BBB_ID}\\?purpose=download$`));
  await expect(page.locator(".pick .best")).toHaveText("Pick the version to keep on this device");
  const keep = page.locator('[data-f="versions:play"]');
  await expect(keep).toHaveText("Download this version");
  await expect(page.getByText("Play this version")).toHaveCount(0);
  await keep.click();
  // Back on the title page, the version picked is on its way, then kept.
  await expect(page).toHaveURL(new RegExp(`/title/movie/${BBB_ID}`));
  await expect(control(page)).toHaveAttribute("aria-label", "Downloaded", { timeout: 30_000 });

  // Settings: Smart downloads with its line, the storage line, Delete all downloads with its confirmation;
  // no Wi-Fi switch on the web.
  await page.goto("/settings?form=desktop");
  await expect(page.getByRole("heading", { name: "Downloads", exact: true })).toBeVisible();
  const smart = page.getByRole("switch", { name: /Smart downloads/ });
  await expect(smart).toHaveAttribute("aria-checked", "true");
  await expect(smart).toContainText("When you finish a downloaded episode, the next one downloads and the one you watched is deleted.");
  await smart.click();
  await expect(smart).toHaveAttribute("aria-checked", "false");
  await smart.click();
  await expect(smart).toHaveAttribute("aria-checked", "true");
  await expect(page.getByText("Download on Wi-Fi only")).toHaveCount(0);
  await expect(page.locator(".dl-settings .dl-storage-line")).toHaveText(/^Downloads use 922 KB/);
  await page.getByRole("button", { name: "Delete all downloads" }).click();
  await expect(page.getByText("Delete all downloads on this device? 922 KB")).toBeVisible();
  await expect(page.locator('[data-f="settings:delete-downloads-cancel"]')).toBeFocused();
  await page.locator('[data-f="settings:delete-downloads-confirm"]').click();
  await expect(page.getByRole("button", { name: "Delete all downloads" })).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});
