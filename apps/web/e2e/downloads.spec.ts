/** Offline downloads on the web (plans/offline-downloads.md 3, D6), on a core of its own: Big Buck
 * Bunny from a local addon whose one stream is a local HTTP origin that serves the test film and holds
 * after its first part until the test lets it go. Download it from the title page, see the ring,
 * pause, resume, done; go offline (the browser's offline event, which tells the core `none`, and the
 * origin and addon gone), see Home's banner, play it from Downloads; delete it. Then the version to
 * keep through Versions, Settings' section, and the TV form, which shows none of it. The film's English
 * subtitle, kept with the download, shows while offline; a series' seasons queue their aired episodes
 * and its page groups them by season; a download held for Wi-Fi goes on over mobile data when asked; a
 * kids profile sees Play only. */

import { type Browser, type BrowserContextOptions, expect, type Page, test } from "@playwright/test";
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
let series: Server | null = null;
const SRT = "1\n00:00:00,500 --> 00:10:00,000\nA kept line\n";
const SERIES = "mqd1";
const AIRED = "2020-01-01T00:00:00.000Z";
// Two seasons: three and three episodes, the last not out yet, so every season holds five aired.
const EPISODES = [
  [1, 1, AIRED],
  [1, 2, AIRED],
  [1, 3, AIRED],
  [2, 1, AIRED],
  [2, 2, AIRED],
  [2, 3, "2099-01-01T00:00:00.000Z"],
] as const;
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
    const url = req.url ?? "";
    if (url === "/manifest.json") return json({ id: "dev.marquee.downloads", version: "1.0.0", name: "Local Film", resources: ["stream", "subtitles"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] });
    if (url === `/stream/movie/${BBB_ID}.json`) return json({ streams: [{ name: "Local Film\n240p", url: film, behaviorHints: { filename: NAME, videoSize: SIZE } }] });
    // Asked as a playback asks, with the file's hash and name; kept as WebVTT beside the download.
    if (url.startsWith(`/subtitles/movie/${BBB_ID}`)) return json({ subtitles: [{ id: "kept-en", lang: "eng", url: `${addonUrl}/kept.srt` }] });
    if (url === "/kept.srt") {
      res.writeHead(200, { "content-type": "text/plain" });
      return res.end(SRT);
    }
    json({ streams: [] });
  });
  const addonUrl = await listen(addon);
  series = createServer((req, res) => {
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const url = decodeURIComponent(req.url ?? "");
    if (url === "/manifest.json") return json({ id: "dev.marquee.download-series", version: "1.0.0", name: "Local Series", resources: ["meta", "stream"], types: ["series"], idPrefixes: ["mqd"], catalogs: [] });
    if (url === `/meta/series/${SERIES}.json`) {
      const videos = EPISODES.map(([season, episode, released]) => ({ id: `${SERIES}:${season}:${episode}`, title: `Part ${season}.${episode}`, season, episode, released }));
      return json({ meta: { id: SERIES, type: "series", name: "Download Series", videos } });
    }
    // An episode's file names its episode, or the core would hide it as another one.
    const ep = /^\/stream\/series\/mqd1:(\d):(\d)\.json$/.exec(url);
    if (ep) {
      const filename = `Download.Series.S0${ep[1]}E0${ep[2]}.720p.WEB.VP9.webm`;
      return json({ streams: [{ name: "Local Series\n720p", url: film, behaviorHints: { filename, videoSize: SIZE } }] });
    }
    res.writeHead(404);
    res.end();
  });
  const seriesUrl = await listen(series);
  core = await startBridge(DOWNLOADS_PORT);
  const c = new Client(`ws://127.0.0.1:${DOWNLOADS_PORT}/ws`);
  try {
    // Cinemeta names the title (answered from the recording offline).
    await c.req("installAddon", { url: "https://v3-cinemeta.strem.io/manifest.json" });
    await c.req("installAddon", { url: `${addonUrl}/manifest.json` });
    await c.req("installAddon", { url: `${seriesUrl}/manifest.json` });
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
  series?.close();
});

/** A page on the spec's core (the app's socket goes to it, never to the shared one). */
async function openPage(browser: Browser, path: string, options: BrowserContextOptions = { viewport: { width: 1440, height: 900 } }) {
  const { context } = await patchedContext(browser, DOWNLOADS_PORT, options, (_view, s) => s);
  const page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(path);
  return { context, page, errors };
}

const control = (page: Page) => page.locator('[data-f="title:download"]');
const item = (page: Page, name: string) => page.getByRole("menuitem", { name, exact: true });
const banner = (page: Page) => page.locator(".offline-bar");
/** The browser's own offline or online event, without Playwright's offline (which also cuts the page
 * from the core's loopback stream server): the app tells the core and shows or hides its banner. */
const goOnline = (page: Page, on: boolean) =>
  page.evaluate((on) => {
    Object.defineProperty(navigator, "onLine", { configurable: true, get: () => on });
    window.dispatchEvent(new Event(on ? "online" : "offline"));
  }, on);
/** One request to the spec's core, as the app or the platform sends it. */
async function ask<T = unknown>(op: string, fields: Record<string, unknown>): Promise<T> {
  const c = new Client(`ws://127.0.0.1:${DOWNLOADS_PORT}/ws`);
  try {
    return await c.req<T>(op, fields);
  } finally {
    c.close();
  }
}

/** The bridge told the network, as the app tells it on the browser's offline and online events. */
const network = (type: "none" | "other" | "wifi" | "cellular", metered = false) => ask("lifecycle", { event: "networkChanged", network: type, metered, lowData: false });

type Row = { videoId: string; state: string };
type Downloads = { downloading: Row[]; titles: { rows: Row[] }[] };
/** Every download on the device, as the `downloads` view lists it, after `until` holds. */
async function downloads(until: (rows: Row[]) => boolean): Promise<Row[]> {
  const c = new Client(`ws://127.0.0.1:${DOWNLOADS_PORT}/ws`);
  try {
    const view = await c.subscribe<Downloads>("downloads");
    const rows = () => [...view.now().downloading, ...view.now().titles.flatMap((t) => t.rows)];
    const end = Date.now() + 30_000;
    while (!until(rows())) {
      if (Date.now() > end) throw new Error(`downloads never settled: ${JSON.stringify(rows())}`);
      await new Promise((r) => setTimeout(r, 200));
    }
    return rows();
  } finally {
    c.close();
  }
}

test("a film downloads from the title page with its ring, pauses and resumes until it is done", async ({ browser }) => {
  const { context, page, errors } = await openPage(browser, `/title/movie/${BBB_ID}?form=desktop`);
  // Download, last in the action row, starts it at once: the ring shows its value under the held origin.
  // Its name starts with what it shows, then its state, its value and the title (M3).
  await expect(control(page)).toHaveAttribute("aria-label", "Download, Big Buck Bunny");
  await control(page).click();
  await expect(control(page)).toHaveAttribute("aria-label", /^\d+%, Downloading, Big Buck Bunny$/, { timeout: 30_000 });
  await expect(control(page).locator(".dl-mark.ring")).toBeVisible();
  await expect(control(page)).not.toHaveAttribute("aria-valuetext", /.*/);
  await expect(control(page)).toHaveAttribute("aria-haspopup", "menu");
  const held = Number((await control(page).getAttribute("aria-label"))?.split("%")[0]);
  expect(held).toBeGreaterThan(0);
  expect(held).toBeLessThan(100);
  await expect(control(page)).toContainText(`${held}%`);

  // Its menu: Pause, then Resume.
  await control(page).click();
  await expect(page.getByRole("dialog")).toContainText("Big Buck Bunny");
  await item(page, "Pause download").click();
  await expect(control(page)).toHaveAttribute("aria-label", /^Paused, \d+ percent, Big Buck Bunny$/);
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
  await expect(control(page)).toHaveAttribute("aria-label", "Downloaded, Big Buck Bunny", { timeout: 30_000 });
  await expect(control(page).locator(".dl-mark.ring")).toHaveCount(0);

  // Library's way in, then the row with its size and its control.
  await page.goto("/library?form=desktop");
  await page.locator('[data-f="library:downloads"]').click();
  await expect(page).toHaveURL(/\/library\/downloads$/);
  // Under Library, but the tab and history name Downloads.
  await expect(page).toHaveTitle("Downloads · Momotus");
  // The e2e core is built without the torrent engine, whose disk figure gives the free space.
  await expect(page.locator(".dl-storage-line")).toHaveText(/^Downloads use 922 KB( · .+ free on this device)?$/);
  const row = page.locator(".dlrow", { hasText: "Big Buck Bunny" });
  await expect(row).toContainText("922 KB");
  await expect(row.getByRole("button", { name: "Downloaded, Big Buck Bunny" })).toBeVisible();
  // The film's English subtitle, kept with it (the device's language, which the add carries).
  await expect(row.locator(".dlrow-subs")).toHaveText("Subtitles: English", { timeout: 30_000 });
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
  // Downloads' own pages give way to Home on TV, even by address.
  for (const path of ["/library/downloads", `/library/downloads/${SERIES}`, `/title/movie/${BBB_ID}/versions/${BBB_ID}?purpose=download`]) {
    await page.goto(path);
    await expect(page, path).toHaveURL(/\/$/);
  }
  // Offline with a film kept: the desktop's shell says so (the browser's own offline event), the TV's
  // does not.
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

test("offline, every page says so, Home once, a download plays from Downloads, then it is deleted", async ({ browser }) => {
  // The origin and the addon are gone. The page goes offline with the browser's own event (`goOnline`),
  // which tells the core `none` as the app does, and moves within the app.
  down = true;
  for (const s of sockets) s.destroy();
  const { context, page, errors } = await openPage(browser, "/?form=desktop");
  await expect(page.locator('[data-f="nav:/"]')).toBeVisible({ timeout: 30_000 });
  await goOnline(page, false);
  // Home: one banner, the shell's, with the way to Downloads; it sits above the nav, not over it.
  await expect(banner(page)).toHaveText(/You're offline\. Your downloads still play\.\s*Go to Downloads/, { timeout: 30_000 });
  await expect(page.getByText("You're offline.", { exact: false })).toHaveCount(1);
  await expect(banner(page)).toHaveAttribute("role", "status");
  const bar = await banner(page).boundingBox();
  const nav = await page.locator(".topnav").boundingBox();
  expect(bar && nav && bar.y + bar.height <= nav.y + 1).toBe(true);
  // Another page shows it too.
  await page.locator('[data-f="nav:/discover"]').click();
  await expect(page).toHaveURL(/\/discover/);
  await expect(banner(page)).toContainText("You're offline. Your downloads still play.");
  await banner(page).getByRole("link", { name: "Go to Downloads" }).click();
  await expect(page).toHaveURL(/\/library\/downloads$/);
  // On Downloads it says so with no way to where it is.
  await expect(banner(page)).toContainText("You're offline. Your downloads still play.");
  await expect(banner(page).getByRole("link")).toHaveCount(0);

  // Played from Downloads: the normal play path, from disk; the player has no banner.
  const started = Date.now();
  await page.locator(".dlrow-main", { hasText: "Big Buck Bunny" }).click();
  await expect(page).toHaveURL(new RegExp(`/play/movie/${BBB_ID}/${BBB_ID}`));
  await expect(banner(page)).toHaveCount(0);
  await page.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
  await page.waitForFunction(() => (document.querySelector("video")?.currentTime ?? 0) > 0.5, null, { timeout: 30_000 });
  console.log(`offline start: ${Date.now() - started} ms from the row's press to 0.5 s played`);
  // The kept track is listed under its language and, chosen, its cue shows, with no source to ask.
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  const english = page.locator('[data-f="sublang:en"]');
  await expect(english).toBeVisible();
  await english.dispatchEvent("click");
  await expect(page.locator('[data-f="player:subs"] small')).toHaveText("English");
  await expect(page.locator(".cues .cue")).toHaveText("A kept line", { timeout: 15_000 });
  await page.goBack();

  // Deleted with the row's trash button: the empty state, and the banner says only that it is offline.
  await expect(page).toHaveURL(/\/library\/downloads$/);
  await page.getByRole("button", { name: "Delete Big Buck Bunny" }).click();
  await expect(page.getByRole("heading", { name: "Nothing downloaded yet" })).toBeVisible();
  await expect(page.getByText("Download movies and episodes to watch them without a connection.")).toBeVisible();
  await expect(page.locator(".dlrow")).toHaveCount(0);
  await page.locator('[data-f="nav:/"]').click();
  await expect(banner(page)).toHaveText("You're offline.");
  // Back online, it goes.
  await goOnline(page, true);
  await expect(banner(page)).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});

test("Choose a version to download keeps the version picked on Versions, and Settings has its section", async ({ browser }) => {
  down = false;
  await network("other");
  const { context, page, errors } = await openPage(browser, `/title/movie/${BBB_ID}?form=desktop`);
  await expect(control(page)).toHaveAttribute("aria-label", "Download, Big Buck Bunny", { timeout: 30_000 });
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
  await expect(control(page)).toHaveAttribute("aria-label", "Downloaded, Big Buck Bunny", { timeout: 30_000 });

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

test("on touch, a tap on Download offers the version too, since no secondary press reaches it", async ({ browser }) => {
  const phone = { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true };
  const { context, page, errors } = await openPage(browser, `/title/movie/${BBB_ID}?form=phone`, phone);
  await expect(control(page)).toHaveAttribute("aria-label", "Download, Big Buck Bunny", { timeout: 30_000 });
  await control(page).tap();
  await expect(item(page, "Choose a version to download")).toBeVisible();
  await item(page, "Download").tap();
  await expect(control(page)).toHaveAttribute("aria-label", "Downloaded, Big Buck Bunny", { timeout: 30_000 });
  await control(page).tap();
  await item(page, "Delete download").tap();
  await expect(control(page)).toHaveAttribute("aria-label", "Download, Big Buck Bunny");
  expect(errors).toEqual([]);
  await context.close();
});

test("a download held for Wi-Fi says so, and Download now on mobile data lets it go on", async ({ browser }) => {
  // Download on Wi-Fi only (no switch on the web; a phone sets it), on a metered network.
  // The page tells its own network at load, so the metered one is told once it is up.
  await ask("setDownloadSettings", { wifiOnly: true });
  const { context, page, errors } = await openPage(browser, `/title/movie/${BBB_ID}?form=desktop`);
  await expect(control(page)).toHaveAttribute("aria-label", "Download, Big Buck Bunny", { timeout: 30_000 });
  await network("cellular", true);
  await control(page).click();
  await expect(control(page)).toHaveAttribute("aria-label", /^Waiting for Wi-Fi(, \d+ percent)?, Big Buck Bunny$/, { timeout: 30_000 });
  await expect(control(page)).toContainText("Waiting for Wi-Fi");
  await control(page).click();
  await expect(page.getByRole("dialog")).toContainText("Download on Wi-Fi only is on, and this network uses mobile data.");
  await item(page, "Download now on mobile data").click();
  await expect(control(page)).toHaveAttribute("aria-label", "Downloaded, Big Buck Bunny", { timeout: 30_000 });
  await control(page).click();
  await item(page, "Delete download").click();
  await expect(control(page)).toHaveAttribute("aria-label", "Download, Big Buck Bunny");
  await ask("setDownloadSettings", { wifiOnly: false });
  await network("wifi");
  expect(errors).toEqual([]);
  await context.close();
});

test("a series queues a season's aired episodes, then every season's; its page groups them and deletes them all", async ({ browser }) => {
  const { context, page, errors } = await openPage(browser, `/title/series/${SERIES}?form=desktop`);
  await expect(control(page)).toHaveAttribute("aria-label", "Download, Download Series", { timeout: 30_000 });
  await control(page).click();
  await expect(item(page, "Download every season (2 seasons, 5 episodes)")).toBeVisible();
  await item(page, "Download Season 1 (3 episodes)").click();
  const ids = (rows: Row[]) => rows.map((r) => r.videoId).sort();
  expect(ids(await downloads((rows) => rows.length >= 3 && rows.every((r) => r.state === "done")))).toEqual([`${SERIES}:1:1`, `${SERIES}:1:2`, `${SERIES}:1:3`]);
  // Three of five kept is not the series downloaded: the control still reads Download.
  await expect(control(page)).toHaveAttribute("aria-label", "Download, Download Series");
  // Deleting a season asks first; Cancel, where the focus waits, keeps it.
  await control(page).click();
  await item(page, "Delete Season 1 downloads").click();
  await expect(page.getByRole("dialog")).toContainText("Delete 3 downloaded episodes of Download Series?");
  await expect(item(page, "Cancel")).toBeFocused();
  await item(page, "Cancel").click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  expect(await downloads((rows) => rows.length === 3)).toHaveLength(3);
  // Every season adds the rest that aired, never the one to come.
  await control(page).click();
  await item(page, "Download every season (2 seasons, 5 episodes)").click();
  const all = await downloads((rows) => rows.length >= 5 && rows.every((r) => r.state === "done"));
  expect(ids(all)).toEqual([`${SERIES}:1:1`, `${SERIES}:1:2`, `${SERIES}:1:3`, `${SERIES}:2:1`, `${SERIES}:2:2`]);
  await expect(control(page)).toHaveAttribute("aria-label", "Downloaded, Download Series");

  // Downloads lists the series with a chevron; its page holds the episodes by season.
  await page.goto("/library/downloads?form=desktop");
  const row = page.locator(".dlrow", { hasText: "Download Series" });
  await expect(row).toContainText("5 episodes · ");
  await row.locator(".dlrow-main").click();
  await expect(page).toHaveURL(new RegExp(`/library/downloads/${SERIES}$`));
  await expect(page).toHaveTitle("Download Series · Downloads · Momotus");
  const season = (n: number) => page.locator(".dl-sec", { has: page.getByRole("heading", { name: `Season ${n}`, exact: true }) });
  await expect(season(1).locator(".dlrow")).toHaveText([/^E1 Part 1\.1/, /^E2 Part 1\.2/, /^E3 Part 1\.3/]);
  await expect(season(2).locator(".dlrow")).toHaveText([/^E1 Part 2\.1/, /^E2 Part 2\.2/]);

  // Delete all asks first, inline; Cancel keeps them, Delete removes every one and leaves the page.
  await page.getByRole("button", { name: "Delete all" }).click();
  await expect(page.getByText("Delete 5 downloaded episodes of Download Series?")).toBeVisible();
  await expect(page.locator('[data-f="downloads:delete-all-cancel"]')).toBeFocused();
  await page.locator('[data-f="downloads:delete-all-cancel"]').click();
  await expect(page.locator(".dlrow")).toHaveCount(5);
  await page.getByRole("button", { name: "Delete all" }).click();
  await page.locator('[data-f="downloads:delete-all-confirm"]').click();
  await expect(page).toHaveURL(/\/library\/downloads$/);
  await expect(page.getByRole("heading", { name: "Nothing downloaded yet" })).toBeVisible();
  expect(await downloads((rows) => rows.length === 0)).toEqual([]);
  expect(errors).toEqual([]);
  await context.close();
});

test("a kids profile sees its downloaded title with Play only, and no control on the title page", async ({ browser }) => {
  // The film kept on this device.
  await ask("downloadAdd", { kind: "movie", itemId: BBB_ID, scope: "video", videoId: BBB_ID, params: { language: "en" } });
  await downloads((rows) => rows.length === 1 && rows[0]?.state === "done");
  // A kids profile allowed the film (the owner needs a PIN first), all in one session: a new one opens
  // locked once the owner has a PIN. The device's sessions then open as the kid's.
  const c = new Client(`ws://127.0.0.1:${DOWNLOADS_PORT}/ws`);
  try {
    await c.req("setPin", { profile: "default", pin: "4826" });
    type Profiles = { profiles: { id: string; kids: boolean }[] };
    const kid = (await c.req<Profiles>("createProfile", { name: "Kid", avatar: "letter:sky", kids: true })).profiles.find((p) => p.kids)?.id as string;
    await c.req("allowTitle", { profile: kid, kind: "movie", itemId: BBB_ID });
    await c.req("switchProfile", { profile: kid });
  } finally {
    c.close();
  }
  const { context, page, errors } = await openPage(browser, "/library/downloads?form=desktop");
  // The picker asks who is watching on this browser's first open: the kid.
  await page.getByRole("button", { name: "Kid, current profile" }).click();
  await page.goto("/library/downloads?form=desktop");
  const row = page.locator(".dlrow", { hasText: "Big Buck Bunny" });
  await expect(row).toBeVisible({ timeout: 30_000 });
  await expect(row.locator("button")).toHaveCount(0);
  await expect(page.locator(".dl-storage")).toHaveCount(0);
  await expect(page.getByRole("button", { name: /Pause all|Resume all|Delete/ })).toHaveCount(0);
  await expect(row.getByRole("link", { name: "Play Big Buck Bunny" })).toBeVisible();

  await page.goto(`/title/movie/${BBB_ID}?form=desktop`);
  await expect(page.locator('[data-f="title:play"], .btn.primary').first()).toBeVisible({ timeout: 30_000 });
  await expect(control(page)).toHaveCount(0);
  // A plain word, nothing to press.
  await expect(page.locator(".info .acts span.dl")).toHaveText("Downloaded");
  await expect(page.locator(".info .acts button.dl")).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});

test("a downloads view open across a profile switch lets the switch through and lists the new profile's rows", async ({ browser }) => {
  // Left by the kids run: the device's sessions open as the kid, the owner has a PIN.
  const c = new Client(`ws://127.0.0.1:${DOWNLOADS_PORT}/ws`);
  try {
    type Profiles = { profiles: { id: string; kids: boolean }[] };
    const kid = (await c.subscribe<Profiles>("profiles")).now().profiles.find((p) => p.kids)?.id as string;
    // The owner keeps an episode the kid is not allowed, so the two lists differ.
    await c.req("switchProfile", { profile: "default", pin: "4826" });
    await c.req("downloadAdd", { kind: "series", itemId: SERIES, scope: "video", videoId: `${SERIES}:1:1`, params: { language: "en" } });
    type Snap = { titles: { itemId: string }[]; downloading: unknown[] };
    const view = await c.subscribe<Snap>("downloads");
    const names = () => view.now().titles.map((t) => t.itemId).sort();
    const end = Date.now() + 30_000;
    while (names().length < 2 || view.now().downloading.length > 0) {
      if (Date.now() > end) throw new Error(`the owner's downloads never settled: ${JSON.stringify(view.now())}`);
      await new Promise((r) => setTimeout(r, 200));
    }
    expect(names()).toEqual([BBB_ID, SERIES].sort());
    // With the view still open, the switch goes through, and the same view now lists the kid's rows.
    await c.req("switchProfile", { profile: kid });
    await expect.poll(names, { timeout: 15_000 }).toEqual([BBB_ID]);
  } finally {
    c.close();
  }
  // The web app, opened as the kid, lists the kid's rows only.
  const { context, page, errors } = await openPage(browser, "/library/downloads?form=desktop");
  await page.getByRole("button", { name: "Kid, current profile" }).click();
  await page.goto("/library/downloads?form=desktop");
  await expect(page.locator(".dlrow", { hasText: "Big Buck Bunny" })).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".dlrow", { hasText: "Download Series" })).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});
