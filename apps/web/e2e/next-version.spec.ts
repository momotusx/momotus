/** A version that cannot play here, on a core of its own: a local addon
 * lists Big Buck Bunny three times, the two best named files holding no
 * picture this browser can decode and the third the real film. The
 * recommendation moves on by itself and says so; a version picked by hand
 * ends on the card, which offers the next one first. */

import { type Browser, expect, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createReadStream, statSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, NEXT_PORT, PORTS, startBridge } from "./core";
import { BBB_FILE, BBB_ID, ensureMedia } from "./source";

let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;

// Two names that rank above the real file, which plays.
const BROKEN = ["Big.Buck.Bunny.2008.1080p.WEB.VP9.webm", "Big.Buck.Bunny.2008.720p.WEB.VP9.webm"];
const REAL = "Big.Buck.Bunny.2008.480p.WEB.VP9.webm";

function versions(): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    if (req.url === "/manifest.json") {
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify({ id: "dev.marquee.next", version: "1.0.0", name: "Next", resources: ["stream"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] }));
    }
    if (req.url === `/stream/movie/${BBB_ID}.json`) {
      res.writeHead(200, { "content-type": "application/json" });
      const stream = (file: string) => ({ name: "Next", title: file, url: `${base}/media/${file}`, behaviorHints: { filename: file } });
      return res.end(JSON.stringify({ streams: [...BROKEN, REAL].map(stream) }));
    }
    const file = decodeURIComponent(req.url?.replace("/media/", "") ?? "");
    if (BROKEN.includes(file)) {
      // No picture anyone can decode; the second one is slow to answer, so
      // "Trying the next version" stays long enough to be read.
      const body = Buffer.alloc(64 * 1024, 7);
      setTimeout(() => {
        res.writeHead(200, { "content-type": "video/webm", "content-length": String(body.length) });
        res.end(body);
      }, file === BROKEN[1] ? 1500 : 0);
      return;
    }
    if (file === REAL) {
      const size = statSync(BBB_FILE).size;
      const m = /bytes=(\d*)-(\d*)/.exec(req.headers.range ?? "");
      const start = m?.[1] ? Number(m[1]) : 0;
      const end = m?.[2] ? Math.min(Number(m[2]), size - 1) : size - 1;
      const head = { "content-type": "video/webm", "accept-ranges": "bytes", "content-length": String(end - start + 1) };
      if (m) res.writeHead(206, { ...head, "content-range": `bytes ${start}-${end}/${size}` });
      else res.writeHead(200, head);
      if (req.method === "HEAD") return res.end();
      createReadStream(BBB_FILE, { start, end }).pipe(res);
      return;
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

test.beforeAll(async () => {
  await ensureMedia();
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
    await c.req("setPrefs", { patch: { defaultsSeen: 1 } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
});

async function page(browser: Browser) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${NEXT_PORT}/ws`);
    const early: string[] = [];
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => ws.send(String(e.data)));
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((m) => (upstream.readyState === WebSocket.OPEN ? upstream.send(String(m)) : early.push(String(m))));
    ws.onClose(() => upstream.close());
  });
  return { context, page: await context.newPage() };
}

const playing = (p: import("@playwright/test").Page) => p.evaluate(() => document.querySelector("video")?.currentTime ?? 0);

test("the recommendation that cannot play here moves on and says so", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  await p.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=desktop`);
  await expect(p.locator(".pstate .st")).toContainText("Trying the next version: ", { timeout: 60_000 });
  // It plays the real file in the end, with no card on the way.
  await p.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
  await expect.poll(() => playing(p), { timeout: 60_000 }).toBeGreaterThan(1);
  await expect(p.locator(".endcard")).toHaveCount(0);
  await context.close();
});

test("a version picked by hand that cannot play offers the next one first", async ({ browser }) => {
  const { context, page: p } = await page(browser);
  const key = `url:http://127.0.0.1:${(addon?.address() as AddressInfo).port}/media/${BROKEN[1]}`;
  // Picked by hand on Versions, as a viewer does, on a TV (where focus
  // moves by itself): its row, once listed.
  await p.goto(`/title/movie/${BBB_ID}/versions/${BBB_ID}?form=tv`);
  const row = p.locator(`[data-f="v:${key}"]`);
  await expect(row).toBeVisible({ timeout: 60_000 });
  await row.dispatchEvent("click");
  await expect(p).toHaveURL(/\/play\/movie\//);
  const offer = p.locator('.endcard [data-f="player:offer"]');
  await expect(offer).toBeVisible({ timeout: 60_000 });
  await expect(offer).toHaveText(/^Play .+/);
  // The offer is the card's first action and has the focus.
  await expect(p.locator(".endcard .acts > *").first()).toHaveAttribute("data-f", "player:offer");
  await expect.poll(() => p.evaluate(() => document.activeElement?.getAttribute("data-f"))).toBe("player:offer");
  await p.keyboard.press("Enter");
  // The offer plays here: the card goes, and the real file plays in the end
  // (an offer that fails too is a recommendation and moves on by itself).
  await expect(p.locator(".endcard")).toHaveCount(0, { timeout: 30_000 });
  await p.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
  await expect.poll(() => playing(p), { timeout: 60_000 }).toBeGreaterThan(1);
  await context.close();
});
