/** A version that cannot play here, on a core of its own: a local addon
 * lists Big Buck Bunny four times. The best named is an HEVC MP4 (the
 * fixture e2e/fixtures/hevc-160x90.mp4), which headless Chromium cannot
 * play at all (MEDIA_ERR_SRC_NOT_SUPPORTED); the next is the real film's
 * head with every frame scrambled, which opens and fails on its first
 * frame (MEDIA_ERR_DECODE); the other two are the real film. The
 * recommendation moves on by itself and says so, and the device remembers
 * what did not play; a version picked by hand ends on the card, which
 * offers the next one first; a hand-picked key rides in the history
 * entry's state, never the address. */

import { type Browser, expect, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createReadStream, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, NEXT_PORT, PORTS, startBridge } from "./core";
import { BBB_ID, FILM_FILE } from "./source";

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let slow: Server | null = null;
let SCRAMBLED: Buffer | undefined;

/** The film's first `bytes` with the frames of every cluster from `fromMs`
 * on scrambled and its structure kept: the metadata loads, the frames
 * before `fromMs` play, then the decoder fails (the element's
 * MEDIA_ERR_DECODE). Walks the clusters' Timecode and SimpleBlocks. */
function undecodable(fromMs = 0, bytes = 1 << 20): Buffer {
  const b = Buffer.from(readFileSync(FILM_FILE).subarray(0, bytes));
  // An EBML size: its length from the leading bits, its value from the rest.
  const size = (at: number) => {
    let len = 1;
    while (len < 8 && !((b[at] ?? 0) & (0x80 >> (len - 1)))) len++;
    let value = (b[at] ?? 0) & (0xff >> len);
    for (let i = 1; i < len; i++) value = value * 256 + (b[at + i] ?? 0);
    return { len, value, unknown: value === 2 ** (7 * len) - 1 };
  };
  const idLength = (at: number) => {
    let len = 1;
    while (len < 4 && !((b[at] ?? 0) & (0x80 >> (len - 1)))) len++;
    return len;
  };
  let at = b.indexOf(Buffer.from([0x1f, 0x43, 0xb6, 0x75]));
  let blocks = 0;
  while (at >= 0 && at < b.length - 12) {
    // A cluster: its children up to its end (or the next cluster).
    const s = size(at + 4);
    const end = s.unknown ? b.length : Math.min(b.length, at + 4 + s.len + s.value);
    let child = at + 4 + s.len;
    // The cluster's Timecode (0xE7), in milliseconds at the default scale.
    let clusterMs = 0;
    while (child < end - 4) {
      if (b[child] === 0x1f && b[child + 1] === 0x43 && b[child + 2] === 0xb6 && b[child + 3] === 0x75) break;
      const id = b[child];
      const il = idLength(child);
      const cs = size(child + il);
      const data = child + il + cs.len;
      if (id === 0xe7) {
        clusterMs = 0;
        for (let i = 0; i < cs.value; i++) clusterMs = clusterMs * 256 + (b[data + i] ?? 0);
      }
      // A SimpleBlock: track number (1), timecode (2), flags (1), then the frame.
      if (id === 0xa3 && clusterMs >= fromMs) {
        for (let i = data + 4; i < Math.min(end, data + cs.value); i++) b[i] = (b[i] ?? 0) ^ 0x5a;
        blocks++;
      }
      child = data + cs.value;
    }
    at = b.indexOf(Buffer.from([0x1f, 0x43, 0xb6, 0x75]), Math.max(child, at + 4));
  }
  if (!blocks) throw new Error("no SimpleBlock found to scramble");
  return b;
}

// Two names that rank above the real files, which play: an HEVC MP4 this
// browser cannot play, and a WebM whose frames do not decode.
const BROKEN = ["Big.Buck.Bunny.2008.1080p.WEB.mp4", "Big.Buck.Bunny.2008.720p.WEB.VP9.webm"];
const HEVC = fileURLToPath(new URL("fixtures/hevc-160x90.mp4", import.meta.url));
const REAL = ["Big.Buck.Bunny.2008.480p.WEB.VP9.webm", "Big.Buck.Bunny.2008.360p.WEB.VP9.webm"];
// A second title, each version with an English subtitle of its own: the best
// plays a few seconds and then fails to decode; the next one plays.
const SINTEL = "tt1727587";
const LATE = "Sintel.2010.1080p.WEB.VP9.webm";
const GOOD = "Sintel.2010.720p.WEB.VP9.webm";
const LINES: Record<string, string> = { [LATE]: "From the first version", [GOOD]: "From the second version" };
let LATE_BODY: Buffer | undefined;
// A second addon that answers for the second title only, and late, once a
// test turns it on: two 1080p versions of release groups of their own (so
// each is its own row) that rank above the row the viewer is on, so they join
// the list before it while the viewer is moving in it.
const SLOW = ["Sintel.2010.1080p.WEB.VP9-SLOWA.webm", "Sintel.2010.1080p.WEB.VP9-SLOWB.webm"];
let slowOn = false;

function slowAddon(): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    if (req.url === "/manifest.json") {
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify({ id: "dev.marquee.slow", version: "1.0.0", name: "Slow", resources: ["stream"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] }));
    }
    if (req.url === `/stream/movie/${SINTEL}.json` && slowOn) {
      setTimeout(() => {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ streams: SLOW.map((file) => ({ name: "Slow", title: file, url: `${base}/media/${file}`, behaviorHints: { filename: file } })) }));
      }, 4000);
      return;
    }
    if (req.url?.startsWith("/stream/")) {
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify({ streams: [] }));
    }
    const file = decodeURIComponent(req.url?.replace("/media/", "") ?? "");
    if (SLOW.includes(file)) return ranges(req, res, FILM_FILE, "video/webm");
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

/** `path` with Range support, as a file host answers. */
function ranges(req: import("node:http").IncomingMessage, res: import("node:http").ServerResponse, path: string, type: string) {
  const size = statSync(path).size;
  const m = /bytes=(\d*)-(\d*)/.exec(req.headers.range ?? "");
  const start = m?.[1] ? Number(m[1]) : 0;
  const end = m?.[2] ? Math.min(Number(m[2]), size - 1) : size - 1;
  const head = { "content-type": type, "accept-ranges": "bytes", "content-length": String(end - start + 1) };
  if (m) res.writeHead(206, { ...head, "content-range": `bytes ${start}-${end}/${size}` });
  else res.writeHead(200, head);
  if (req.method === "HEAD") return void res.end();
  createReadStream(path, { start, end }).pipe(res);
}

function versions(): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    if (req.url === "/manifest.json") {
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify({ id: "dev.marquee.next", version: "1.0.0", name: "Next", resources: ["stream"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] }));
    }
    if (req.url === `/stream/movie/${BBB_ID}.json`) {
      // A moment late, as an addon is: a new Versions view lists nothing at first.
      const stream = (file: string) => ({ name: "Next", title: file, url: `${base}/media/${file}`, behaviorHints: { filename: file } });
      setTimeout(() => {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ streams: [...BROKEN, ...REAL].map(stream) }));
      }, 800);
      return;
    }
    if (req.url === `/stream/movie/${SINTEL}.json`) {
      const stream = (file: string) => ({
        name: "Next",
        title: file,
        url: `${base}/media/${file}`,
        behaviorHints: { filename: file },
        subtitles: [{ id: file, lang: "eng", url: `${base}/subs/${encodeURIComponent(file)}.srt` }],
      });
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify({ streams: [LATE, GOOD].map(stream) }));
    }
    if (req.url?.startsWith("/subs/")) {
      const of = decodeURIComponent(req.url.slice("/subs/".length).replace(/\.srt$/, ""));
      res.writeHead(200, { "content-type": "text/plain" });
      return res.end(`1\n00:00:00,000 --> 00:10:00,000\n${LINES[of] ?? "?"}\n`);
    }
    const file = decodeURIComponent(req.url?.replace("/media/", "") ?? "");
    if (file === LATE) {
      const body = LATE_BODY ??= undecodable(6000, 3 << 20);
      res.writeHead(200, { "content-type": "video/webm", "content-length": String(body.length) });
      return res.end(body);
    }
    if (file === GOOD) return ranges(req, res, FILM_FILE, "video/webm");
    if (file === BROKEN[1]) {
      // Opens, then no frame decodes; slow to answer, so "Trying the next
      // version" stays long enough to be read.
      const body = SCRAMBLED ??= undecodable();
      setTimeout(() => {
        res.writeHead(200, { "content-type": "video/webm", "content-length": String(body.length) });
        res.end(body);
      }, 1500);
      return;
    }
    if (file === BROKEN[0]) return ranges(req, res, HEVC, "video/mp4");
    if (REAL.includes(file)) return ranges(req, res, FILM_FILE, "video/webm");
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

test.beforeAll(async () => {
  addon = await versions();
  core = await startBridge(NEXT_PORT);
  const c = new Client(`ws://127.0.0.1:${NEXT_PORT}/ws`);
  try {
    for (let attempt = 1; ; attempt++) {
      try {
        await c.req("installAddon", { url: "https://v3-cinemeta.strem.io/manifest.json" });
        break;
      } catch (e) {
        if (attempt === 3) throw e;
      }
    }
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    slow = await slowAddon();
    await c.req("installAddon", { url: `http://127.0.0.1:${(slow.address() as AddressInfo).port}/manifest.json` });
    // English subtitles turn on at start (only the second title has any).
    await c.req("setPrefs", { patch: { defaultsSeen: 1, subtitleLanguages: ["eng"] } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  slow?.close();
});

type Sent = { op?: string; view?: string; command?: { type?: string; kind?: string; status?: number } };

async function page(browser: Browser, viewport = { width: 1440, height: 900 }, opts: { failTitle?: () => boolean } = {}) {
  // What the page asked the core, in order.
  const sent: Sent[] = [];
  const context = await browser.newContext({ viewport, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${NEXT_PORT}/ws`);
    const early: string[] = [];
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => ws.send(String(e.data)));
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m));
      sent.push(msg);
      // A title view the meta addon cannot answer for, made up at the socket.
      if (msg.op === "subscribe" && msg.view === "title" && opts.failTitle?.()) {
        return ws.send(JSON.stringify({ id: msg.id, ok: false, error: "the meta addon did not answer" }));
      }
      if (upstream.readyState === WebSocket.OPEN) upstream.send(String(m));
      else early.push(String(m));
    });
    ws.onClose(() => upstream.close());
  });
  return { context, page: await context.newPage(), sent };
}

const playing = (p: import("@playwright/test").Page) => p.evaluate(() => document.querySelector("video")?.currentTime ?? 0);
const source = (p: import("@playwright/test").Page) => p.evaluate(() => document.querySelector("video")?.getAttribute("src") ?? "");
const mute = (p: import("@playwright/test").Page) =>
  p.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
// The key the history entry carries for the player.
const chosen = (p: import("@playwright/test").Page) => p.evaluate(() => (history.state as { key?: string } | null)?.key ?? null);
const slowKey = (file: string) => `url:http://127.0.0.1:${(slow?.address() as AddressInfo).port}/media/${file}`;
const keyOf = (file: string) => `url:http://127.0.0.1:${(addon?.address() as AddressInfo).port}/media/${file}`;

test("the recommendation that cannot play here moves on and says so", async ({ browser }) => {
  const { context, page: p, sent } = await page(browser);
  await p.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=desktop`);
  await expect(p.locator(".ld.on .ld-cap")).toContainText("Trying the next version: ", { timeout: 60_000 });
  // It plays the real file in the end, with no card on the way.
  await mute(p);
  await expect.poll(() => playing(p), { timeout: 60_000 }).toBeGreaterThan(1);
  await expect(p.locator(".endcard")).toHaveCount(0);
  // Both were the files' own: one this browser cannot play (its source
  // answered), one whose frames did not decode. Each was told as decode.
  const kinds = sent.filter((m) => m.command?.type === "failOver").map((m) => m.command?.kind);
  expect(kinds).toEqual(["decode", "decode"]);
  // This device remembers them: on Versions each says it did not play here.
  await p.goto(`/title/movie/${BBB_ID}/versions/${BBB_ID}?form=desktop`);
  await expect(p.locator(".vrow").first()).toBeVisible({ timeout: 60_000 });
  for (const fold of await p.locator('.fold[aria-expanded="false"]').all()) await fold.click();
  for (const file of BROKEN) await expect(p.locator(`[data-f="v:${keyOf(file)}"] .lt`)).toHaveText("Did not play on this device");
  await context.close();
});

test("a version picked by hand that cannot play offers the next one first", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  const key = keyOf(BROKEN[1] ?? "");
  // Picked by hand on Versions, as a viewer does, on a TV (where focus
  // moves by itself): its row, once listed.
  await p.goto(`/title/movie/${BBB_ID}/versions/${BBB_ID}?form=tv`);
  const row = p.locator(`[data-f="v:${key}"]`);
  await expect(row).toBeVisible({ timeout: 60_000 });
  await row.dispatchEvent("click");
  await expect(p).toHaveURL(/\/play\/movie\//);
  // The key rides in the history entry, never in the address.
  expect(p.url()).not.toContain("key");
  expect(await chosen(p)).toBe(key);
  const offer = p.locator('.endcard [data-f="player:offer"]');
  await expect(offer).toBeVisible({ timeout: 60_000 });
  await expect(offer).toHaveText(/^Play .+/);
  // One version failed, and another is offered: the headline says so.
  await expect(p.locator(".endcard .st")).toHaveText("This version could not be played");
  // The offer is the card's first action and has the focus.
  await expect(p.locator(".endcard .acts > *").first()).toHaveAttribute("data-f", "player:offer");
  await expect(p.locator(".endcard .acts > *").nth(1)).toHaveAttribute("data-f", "player:retry");
  await expect.poll(() => p.evaluate(() => document.activeElement?.getAttribute("data-f"))).toBe("player:offer");
  await p.keyboard.press("Enter");
  // The offer is a new play at its own address, in place of this one: the
  // card goes, the picked file leaves the player, and the real file plays in
  // the end (an offer that fails too is a recommendation and moves on by itself).
  // Which version is offered depends on what this device already failed to
  // decode (the core remembers it); its key replaces the entry's, and the
  // address still carries none.
  await expect.poll(() => chosen(p), { timeout: 30_000 }).not.toBe(key);
  expect(await chosen(p)).toMatch(/^url:http:\/\/127\.0\.0\.1:\d+\/media\//);
  expect(p.url()).not.toContain("key");
  await expect(p.locator(".endcard")).toHaveCount(0, { timeout: 30_000 });
  await expect.poll(() => source(p), { timeout: 30_000 }).not.toContain(encodeURIComponent(BROKEN[1] ?? ""));
  await mute(p);
  await expect.poll(() => playing(p), { timeout: 60_000 }).toBeGreaterThan(1);
  await context.close();
});

test("a new version at the same play address replaces the one playing", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto(`/title/movie/${BBB_ID}/versions/${BBB_ID}?form=desktop`);
  // One real file may be the recommendation by now (the core remembers what
  // this device failed to decode): the other is a row. Play a row, then the other.
  const rows = REAL.map((f) => p.locator(`[data-f="v:${keyOf(f)}"]`));
  const listed = async () => Promise.all(rows.map((r) => r.count()));
  await expect.poll(async () => (await listed()).some((n) => n > 0), { timeout: 60_000 }).toBe(true);
  const i = (await listed())[0] ? 0 : 1;
  const [from, to] = [REAL[i] ?? "", REAL[1 - i] ?? ""];
  await rows[i]?.dispatchEvent("click");
  await expect.poll(() => source(p), { timeout: 60_000 }).toContain(from);
  await mute(p);
  await expect.poll(() => playing(p), { timeout: 60_000 }).toBeGreaterThan(1);
  // Another entry of this address with another key, reached as Back and
  // Forward reach it: the router reads the entry's state.
  const address = p.url();
  await p.evaluate((key) => {
    history.pushState({ ...history.state, key }, "", location.href);
    dispatchEvent(new PopStateEvent("popstate", { state: history.state }));
  }, keyOf(to));
  expect(p.url()).toBe(address);
  expect(await chosen(p)).toBe(keyOf(to));
  await expect.poll(() => source(p), { timeout: 30_000 }).toContain(to);
  await expect(p.locator("video")).toHaveCount(1);
  await mute(p);
  await expect.poll(() => playing(p), { timeout: 60_000 }).toBeGreaterThan(1);
  await context.close();
});

test("a source that fails before its metadata is not a decode failure, and Try again starts over", async ({ browser }) => {
  const { context, page: p, sent } = await page(browser);
  // Every answer of the core's stream server fails for this page: the
  // player and its question to the source get a 500.
  const server = /\/s\/[0-9a-f]+\//;
  await p.route(server, (r) => r.fulfill({ status: 500, body: "" }));
  await p.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=tv`);
  const card = p.locator(".endcard");
  await expect(card).toBeVisible({ timeout: 60_000 });
  const failOvers = () => sent.filter((m) => m.command?.type === "failOver").map((m) => m.command);
  expect(failOvers().length).toBeGreaterThan(0);
  for (const f of failOvers()) expect(f).toMatchObject({ kind: "http", status: 500 });
  await expect(card.locator(".attempts")).toContainText("The source had an error");
  // In words, never the status or the addon's name (the owner's rule).
  await expect(card.locator(".attempts")).not.toContainText("500");
  await expect(card.locator(".attempts b")).toHaveCount(0);
  // Nothing offered: every source failed, and the core says none could be reached.
  await expect(card.locator(".st")).toHaveText("Can't reach any source");
  await expect(card.locator(".attempts")).not.toContainText("decode");
  // No offer here: Try again is the first action and has the focus.
  await expect(p.locator(".endcard .acts > *").first()).toHaveAttribute("data-f", "player:retry");
  await expect(p.locator('[data-f="player:retry"]')).toHaveText("Try again");
  await expect.poll(() => p.evaluate(() => document.activeElement?.getAttribute("data-f"))).toBe("player:retry");
  // Try again while the server still fails: the play fails again at once, and
  // the card stays with "Trying again" under its headline for a moment
  // (`retries`), as on Android and Apple, never a spinner in its place.
  await p.keyboard.press("Enter");
  await expect(card.locator(".end-note")).toHaveText("Trying again");
  await expect(card).toBeVisible();
  await expect(p.locator(".ld.on")).toHaveCount(0);
  await expect(card.locator(".end-note")).toHaveCount(0, { timeout: 10_000 });
  await expect.poll(() => p.evaluate(() => document.activeElement?.getAttribute("data-f"))).toBe("player:retry");
  // The server answers again; Try again asks the core to start over.
  await p.unroute(server);
  await p.keyboard.press("Enter");
  await expect.poll(() => sent.filter((m) => m.op === "command" && m.command?.type === "retry").length).toBe(2);
  await expect(card).toHaveCount(0, { timeout: 30_000 });
  await mute(p);
  // It plays: once started (at a resume point, maybe), the playhead moves on.
  const started = () => p.evaluate(() => {
    const v = document.querySelector("video");
    return !!v && !v.paused && v.readyState >= 3 && v.currentTime > 0;
  });
  await expect.poll(started, { timeout: 60_000 }).toBe(true);
  const from = await playing(p);
  await expect.poll(() => playing(p), { timeout: 30_000 }).toBeGreaterThan(from + 1);
  await context.close();
});

test("a reload after a hand pick plays that version once Versions lists it again", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto(`/title/movie/${BBB_ID}/versions/${BBB_ID}?form=desktop`);
  // A real file that is a row (the other may be the recommendation by now).
  const rows = REAL.map((f) => p.locator(`[data-f="v:${keyOf(f)}"]`));
  await expect.poll(async () => (await Promise.all(rows.map((r) => r.count()))).some((n) => n > 0), { timeout: 60_000 }).toBe(true);
  const i = (await rows[0]?.count()) ? 0 : 1;
  const file = REAL[i] ?? "";
  await rows[i]?.dispatchEvent("click");
  await expect.poll(() => source(p), { timeout: 60_000 }).toContain(file);
  // A reload opens a new Versions view, which lists nothing at first; the
  // entry still holds the key, and the play waits for it to be listed.
  await p.reload();
  // Until Versions lists it again, the pick is being found (as on Android).
  await expect(p.locator(".ld.on .ld-cap")).toHaveText("Finding this version");
  expect(p.url()).not.toContain("key");
  expect(await chosen(p)).toBe(keyOf(file));
  await expect.poll(() => source(p), { timeout: 60_000 }).toContain(file);
  await expect(p.locator(".endcard")).toHaveCount(0);
  await mute(p);
  await expect.poll(() => playing(p), { timeout: 60_000 }).toBeGreaterThan(0.5);
  await context.close();
});

test("a playback that cannot open says so with the error: Choose a version first, Try again reopens it, then Back", async ({ browser }) => {
  const { context, page: p, sent } = await page(browser);
  await p.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=tv`);
  await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 60_000 });
  // An entry whose key the addon no longer lists, opened again.
  await p.evaluate((key) => history.replaceState({ ...history.state, key }, ""), keyOf("Big.Buck.Bunny.2008.Gone.webm"));
  await p.reload();
  const card = p.locator(".endcard");
  await expect(card).toBeVisible({ timeout: 60_000 });
  // Apple's words: the headline, then what the core said under it.
  await expect(card.locator(".st")).toHaveText("This version could not be opened");
  await expect(card.locator(".end-detail")).toContainText("no longer listed");
  // Mostly a version no longer listed, which Try again cannot bring back:
  // Choose a version first and focused, then Try again, then Back.
  const acts = card.locator(".acts > *");
  await expect(acts).toHaveCount(3);
  await expect(acts.nth(0)).toHaveAttribute("data-f", "player:versions");
  await expect(acts.nth(1)).toHaveAttribute("data-f", "player:retry");
  await expect(acts.nth(2)).toHaveAttribute("data-f", "player:back");
  await expect.poll(() => p.evaluate(() => document.activeElement?.getAttribute("data-f"))).toBe("player:versions");
  // Try again opens the playback once more; still not listed, the same card comes back.
  const opens = () => sent.filter((m) => m.op === "subscribe" && m.view === "playback").length;
  const before = opens();
  await acts.nth(1).click();
  await expect.poll(opens).toBe(before + 1);
  await expect(card.locator(".st")).toHaveText("This version could not be opened");
  await context.close();
});

test("moving on drops the old version's subtitle and turns on the new one's", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto(`/play/movie/${SINTEL}/${SINTEL}?form=desktop`);
  const cue = () => p.evaluate(() => {
    const t = document.querySelector<HTMLTrackElement>('video track[data-line="1"]')?.track;
    return t && t.mode === "hidden" ? (t.cues?.[0] as VTTCue | undefined)?.text ?? null : null;
  });
  await expect.poll(() => p.evaluate(() => !!document.querySelector("video")), { timeout: 30_000 }).toBe(true);
  await mute(p);
  // The best version plays, with its English subtitle on from the start.
  await expect.poll(cue, { timeout: 60_000 }).toBe(LINES[LATE]);
  // A few seconds in it fails to decode, and the core moves on.
  await expect.poll(() => source(p), { timeout: 60_000 }).toContain(GOOD);
  // The new version's own subtitle, not the old one's cues.
  await expect.poll(cue, { timeout: 30_000 }).toBe(LINES[GOOD]);
  await expect(p.locator("video track")).toHaveCount(1);
  await expect.poll(() => playing(p), { timeout: 60_000 }).toBeGreaterThan(0.5);
  await context.close();
});

test("TV: Right from Play reaches a short list, and the focus stays on its row when a late addon adds rows above it", async ({ browser }) => {
  // The TV canvas: the recommended card on the left, the list on its right.
  slowOn = true;
  const { context, page: p } = await page(browser, { width: 1920, height: 1080 });
  await p.goto(`/title/movie/${SINTEL}/versions/${SINTEL}?form=tv`);
  // The first addon answered (which of its versions is the pick depends on
  // what this device already failed to play); the slow one has not.
  await expect(p.locator(".vrow").first()).toBeVisible({ timeout: 30_000 });
  await expect(p.locator(`[data-f="v:${slowKey(SLOW[0] ?? "")}"]`)).toHaveCount(0);
  const focused = () => p.evaluate(() => document.activeElement?.getAttribute("data-f") ?? null);
  await expect.poll(focused).toBe("versions:play");
  // Right from Play reaches the list, though it is short and high up on the
  // right (the press also freezes the pick). And what the row says.
  await p.keyboard.press("ArrowRight");
  await expect.poll(focused).toMatch(/^v:/);
  const row = await focused();
  const says = () => p.evaluate(() => document.activeElement?.querySelector(".rel")?.textContent ?? null);
  const before = await says();
  // The slow addon answers: its rows join the list, above the focused one.
  await expect(p.locator(`[data-f="v:${slowKey(SLOW[0] ?? "")}"]`)).toHaveCount(1, { timeout: 30_000 });
  const order = await p.evaluate(() => [...document.querySelectorAll(".vrow")].map((e) => e.getAttribute("data-f")));
  expect(order.indexOf(`v:${slowKey(SLOW[0] ?? "")}`)).toBeLessThan(order.indexOf(row ?? ""));
  // The focus is where the viewer put it, on the same release.
  expect(await focused()).toBe(row);
  expect(await says()).toBe(before);
  // And Left from the list goes back to Play.
  await p.keyboard.press("ArrowLeft");
  await expect.poll(focused).toBe("versions:play");
  await context.close();
});

test("a version changed from the player replaces that play in history: Back goes to Versions, then the title", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto(`/title/movie/${BBB_ID}?form=desktop`);
  await p.locator('[data-f="title:versions"]').click({ timeout: 60_000 });
  await expect(p).toHaveURL(/\/versions\//);
  // A real file that is a row (the other may be the recommendation by now).
  const rows = REAL.map((f) => p.locator(`[data-f="v:${keyOf(f)}"]`));
  const playRow = async () => {
    // On every Versions page: once a real file is listed (the list may still be filling).
    await expect.poll(async () => (await Promise.all(rows.map((r) => r.count()))).some((n) => n > 0), { timeout: 60_000 }).toBe(true);
    const i = (await rows[0]?.count()) ? 0 : 1;
    await rows[i]?.dispatchEvent("click");
    await expect(p).toHaveURL(/\/play\//);
    await mute(p);
  };
  await playRow();
  // The player's own Versions, and another (or the same) version from there.
  await p.locator('[data-f="player:versionsact"]').dispatchEvent("click");
  await expect(p).toHaveURL(/\/versions\//);
  await playRow();
  // Back: Versions, then the title; never the first play again.
  await p.goBack();
  await expect(p).toHaveURL(/\/versions\//);
  await p.goBack();
  await expect(p).toHaveURL(new RegExp(`/title/movie/${BBB_ID}(\\?|$)`));
  // From the title's Play straight to the film, then its Versions: the
  // play is replaced, so Back from the next play is Versions, then the title.
  await p.locator('[data-f="title:play"]').click({ timeout: 60_000 });
  await expect(p).toHaveURL(/\/play\//);
  await mute(p);
  await p.locator('[data-f="player:versionsact"]').dispatchEvent("click");
  await expect(p).toHaveURL(/\/versions\//);
  await playRow();
  await p.goBack();
  await expect(p).toHaveURL(/\/versions\//);
  await p.goBack();
  await expect(p).toHaveURL(new RegExp(`/title/movie/${BBB_ID}(\\?|$)`));
  await context.close();
});

test("a title that cannot be opened: the player and Versions say so, with Try again and Back", async ({ browser }) => {
  let failing = true;
  const { context, page: p, sent } = await page(browser, undefined, { failTitle: () => failing });
  const titles = () => sent.filter((m) => m.op === "subscribe" && m.view === "title").length;
  const focused = () => p.evaluate(() => document.activeElement?.getAttribute("data-f") ?? null);
  // The player: a card, never "Opening" for good.
  await p.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=tv`);
  const card = p.locator(".endcard");
  await expect(card).toBeVisible({ timeout: 30_000 });
  await expect(card.locator(".st")).toHaveText("This title could not be opened");
  await expect(card.locator(".end-detail")).toHaveText("the meta addon did not answer");
  const acts = card.locator(".acts > *");
  await expect(acts).toHaveCount(2);
  await expect(acts.nth(0)).toHaveAttribute("data-f", "player:retry");
  await expect(acts.nth(1)).toHaveAttribute("data-f", "player:back");
  await expect.poll(focused).toBe("player:retry");
  // Try again opens the title anew: still failing, the same card.
  let before = titles();
  await p.keyboard.press("Enter");
  await expect.poll(titles).toBe(before + 1);
  await expect(card.locator(".st")).toHaveText("This title could not be opened");
  // The addon answers again: Try again, and the film plays.
  failing = false;
  await expect.poll(focused).toBe("player:retry");
  await p.keyboard.press("Enter");
  await expect(card).toHaveCount(0, { timeout: 30_000 });
  await mute(p);
  await expect.poll(() => playing(p), { timeout: 60_000 }).toBeGreaterThan(0.5);
  // Versions: the same, in its own block.
  failing = true;
  await p.goto(`/title/movie/${BBB_ID}/versions/${BBB_ID}?form=tv`);
  const block = p.locator(".vempty.problem");
  await expect(block.locator("h2")).toHaveText("This title could not be opened", { timeout: 30_000 });
  await expect(block.locator("p")).toHaveText("the meta addon did not answer");
  await expect.poll(focused).toBe("versions:retry");
  await expect(p.locator('[data-f="versions:back"]')).toBeVisible();
  failing = false;
  before = titles();
  await p.keyboard.press("Enter");
  await expect.poll(titles).toBe(before + 1);
  await expect(p.locator(".vrow").first()).toBeVisible({ timeout: 30_000 });
  await context.close();
});
