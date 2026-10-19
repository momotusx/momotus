/** The subtitle menu, one section per language, on a core of its own: Big
 * Buck Bunny from a local addon whose stream names subtitles in many
 * languages and spellings (OpenSubtitles' specials among them), with a
 * profile that prefers Turkish. Off comes first, then Turkish, then the
 * browser's language, then the rest by name; files of one language are
 * told apart by name. Turkish turns on at start, until the viewer says
 * otherwise. */

import { type Browser, expect, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, LANGS_PORT, PORTS, startBridge, startIfPaused } from "./core";
import { BBB_ID, ensureMedia, NAME, startSource } from "./source";

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let media: Server | null = null;

const SRT = "1\n00:00:01,000 --> 00:00:30,000\nA line\n";

function manySubs(film: string): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    const sub = (id: string, lang: string, label?: string) => ({ id, lang, url: `${base}/${id}.srt`, ...(label ? { label } : {}) });
    if (req.url === "/manifest.json") {
      return json({ id: "dev.marquee.many-subs", version: "1.0.0", name: "Many Subs", resources: ["stream"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] });
    }
    if (req.url === `/stream/movie/${BBB_ID}.json`) {
      return json({
        streams: [
          {
            name: "Many Subs\n240p",
            url: film,
            behaviorHints: { filename: NAME },
            subtitles: [sub("1", "pob"), sub("2", "eng"), sub("3", "tur", "TOPAZ"), sub("4", "ger"), sub("5", "zht"), sub("6", "tur", "TOPAZ"), sub("7", "deu"), sub("8", "eng", "Big.Buck.Bunny.2008.WEB")],
          },
        ],
      });
    }
    if (req.url?.endsWith(".srt")) {
      res.writeHead(200, { "content-type": "text/plain" });
      return res.end(SRT);
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

test.beforeAll(async () => {
  await ensureMedia();
  media = await startSource(0);
  addon = await manySubs(`http://127.0.0.1:${(media.address() as AddressInfo).port}/media/${NAME}`);
  core = await startBridge(LANGS_PORT);
  const c = new Client(`ws://127.0.0.1:${LANGS_PORT}/ws`);
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
    await c.req("setPrefs", { patch: { defaultsSeen: 1, subtitleLanguages: ["tur"] } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  media?.close();
});

async function playerPage(browser: Browser) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${LANGS_PORT}/ws`);
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
  const open = async () => {
    await page.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=desktop`);
    await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
    await page.evaluate(() => {
      const v = document.querySelector("video");
      if (v) v.muted = true;
    });
    await startIfPaused(page);
  };
  return { context, page, open };
}

// First in the file: it leaves the title's remembered choice at Off.
test("the profile's language turns on at start; Off wins and is remembered", async ({ browser }) => {
  const { context, page, open } = await playerPage(browser);
  await open();
  const summary = page.locator('[data-f="player:subs"] small');
  // Turkish is the profile's subtitle language, the audio is not Turkish.
  await expect(summary).toHaveText("Turkish", { timeout: 30_000 });
  await expect.poll(() => page.evaluate(() => document.querySelector("video")?.textTracks[0]?.mode)).toBe("showing");
  // The viewer turns it off: off now, and on the next play of the title.
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  await page.locator('[data-f="sub:off"]').dispatchEvent("click");
  await expect(summary).toHaveText(/^Off, \d+ found$/);
  await open();
  await expect(summary).toHaveText(/^Off, \d+ found$/, { timeout: 30_000 });
  await page.waitForTimeout(1000);
  await expect(summary).toHaveText(/^Off, \d+ found$/);
  await context.close();
});

test("the subtitle menu is one section per language, preferred first", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${LANGS_PORT}/ws`);
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
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await page.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
  await startIfPaused(page);
  await page.locator('[data-f="player:subs"]').dispatchEvent("click");
  const panel = page.locator('aside[aria-label="Subtitles"]');
  // Turkish, the profile's; English, the browser's language standing in
  // for audio languages the profile never set; then the rest by name.
  await expect(panel.locator(".sub-lang h3")).toHaveText(["Turkish", "English", "Chinese (Traditional)", "German", "Portuguese (Brazil)"]);
  // Off first, then the sections.
  await expect(panel.locator(".opt").first()).toHaveAttribute("data-f", "sub:off");
  const names = (language: string) => panel.locator(`section.sub-lang[aria-label="${language}"] .opt b`);
  await expect(names("Turkish")).toHaveText(["TOPAZ 1", "TOPAZ 2"]);
  await expect(names("English")).toHaveText(["Many Subs", "Big.Buck.Bunny.2008.WEB"]);
  // "ger" and "deu" are one language, its two files numbered.
  await expect(names("German")).toHaveText(["Many Subs 1", "Many Subs 2"]);
  await page.screenshot({ path: `${process.env.SHOTS_DIR ?? "e2e/shots"}/subtitle-languages-desktop.png` });

  // Picking one shows its language, as the core names it, on the
  // Subtitles button ("pob" is not "Brazilian Portuguese" here).
  await names("Portuguese (Brazil)").first().dispatchEvent("click");
  await expect(page.locator('[data-f="player:subs"] small')).toHaveText("Portuguese (Brazil)");
  expect(errors).toEqual([]);
  await context.close();
});
