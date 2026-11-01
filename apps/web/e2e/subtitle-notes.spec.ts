/** A subtitle file the core cannot serve plays as an empty track and comes
 * back as a note, on a core of its own: Big Buck Bunny from a local addon
 * whose own two subtitle files fail, one with a 502 and one that holds no
 * subtitle. The player says so under the list and in a toast, keeps the
 * first track (it may load later), drops the second, and the video plays
 * on. The film comes from a sample source of its own. */

import { expect, type Page, test } from "./fixture";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, NOTES_PORT, PORTS, startBridge, startIfPaused } from "./core";
import { BBB_ID, NAME, startSource } from "./source";

const CINEMETA = "https://v3-cinemeta.strem.io/manifest.json";
// Never the source's name ("Flaky Subs"): the owner's rule.
const GONE = "A subtitle source did not answer. Try its subtitles again in a moment.";
const BROKEN = "This subtitle could not be read.";

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let media: Server | null = null;

/** A stream addon for Big Buck Bunny whose stream names two subtitle files
 * of its own that never load. */
function flakySubs(film: string): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    if (req.url === "/manifest.json") {
      return json({ id: "dev.marquee.flaky-subs", version: "1.0.0", name: "Flaky Subs", resources: ["stream"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] });
    }
    if (req.url === `/stream/movie/${BBB_ID}.json`) {
      return json({
        streams: [
          {
            name: "Flaky Subs\n240p",
            url: film,
            behaviorHints: { filename: NAME },
            subtitles: [
              { id: "gone", lang: "eng", label: "Gone", url: `${base}/gone.srt` },
              { id: "broken", lang: "eng", label: "Broken", url: `${base}/broken.srt` },
            ],
          },
        ],
      });
    }
    if (req.url === "/gone.srt") {
      res.writeHead(502);
      return res.end();
    }
    if (req.url === "/broken.srt") {
      res.writeHead(200, { "content-type": "text/plain" });
      return res.end("no cues in here");
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

test.beforeAll(async () => {
  media = await startSource(0);
  addon = await flakySubs(`http://127.0.0.1:${(media.address() as AddressInfo).port}/media/${NAME}`);
  core = await startBridge(NOTES_PORT);
  const c = new Client(`ws://127.0.0.1:${NOTES_PORT}/ws`);
  try {
    // Cinemeta names the title; public addons sometimes miss the core's 10 s fetch limit.
    for (let attempt = 1; ; attempt++) {
      try {
        await c.req("installAddon", { url: CINEMETA });
        break;
      } catch (e) {
        if (attempt === 3) throw e;
      }
    }
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    // A subtitle language the film lacks: nothing starts on its own, so each file is asked for only when picked.
    await c.req("setPrefs", { patch: { defaultsSeen: 1, subtitleLanguages: ["ja"] } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  media?.close();
});

/** Opens the subtitle picker, then English's options (both files are English), and picks the track labelled `label`. */
async function pick(page: Page, label: string) {
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  await page.locator('[data-f="subopen:en"]').dispatchEvent("click");
  await page.locator('aside[aria-label="Subtitles"] .opt', { hasText: label }).dispatchEvent("click");
}

test("a subtitle that does not load leaves a note and the video plays on", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${NOTES_PORT}/ws`);
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
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));

  await page.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=desktop`);
  await expect(page.locator(".ptitle h1")).toHaveText("Big Buck Bunny");
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await startIfPaused(page);
  const time = () => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0);
  await expect.poll(time, { timeout: 30_000 }).toBeGreaterThan(1);
  const panel = page.locator('aside[aria-label="Subtitles"]');

  // A file that did not answer: a toast and a note; the track stays.
  await pick(page, "Gone");
  await expect(page.locator(".toast.bad")).toHaveText(GONE);
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  await expect(panel.locator(".opt-note")).toHaveText([GONE]);
  await panel.locator('[data-f="subopen:en"]').dispatchEvent("click");
  await expect(panel.locator(".opt", { hasText: "Gone" })).toBeVisible();
  await expect(panel.locator(".opt.on", { hasText: "Gone" })).toBeVisible();
  // Back from English's options steps to the languages, then closes.
  await page.keyboard.press("Escape");
  await expect(panel.locator('[data-f="sub:off"]')).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);

  // A file that holds no subtitle: it leaves the list and subtitles go off.
  await pick(page, "Broken");
  await expect(page.locator(".toast.bad")).toHaveText(BROKEN);
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  await expect(panel.locator(".opt-note")).toHaveText([GONE, BROKEN]);
  await expect(panel.locator('[data-f="sub:off"]')).toHaveClass(/\bon\b/);
  await panel.locator('[data-f="subopen:en"]').dispatchEvent("click");
  await expect(panel.locator(".opt", { hasText: "Gone" })).toBeVisible();
  await expect(panel.locator(".opt", { hasText: "Broken" })).toHaveCount(0);

  // The video never stopped and no source failed over.
  const at = await time();
  await expect.poll(time, { timeout: 15_000 }).toBeGreaterThan(at + 1);
  await expect(page.locator(".endcard")).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});
