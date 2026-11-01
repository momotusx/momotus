/** The start subtitle moves on when its file cannot be read, on a core of its own: Big Buck Bunny from
 * a local addon whose stream names three English subtitle files, the first two holding no subtitle and
 * the third a good one, and the profile's subtitle language is English. The core turns on the first;
 * each one it cannot read leaves the list and the next becomes `autoSubtitle`, with no note; the player
 * follows, and the good one shows. With all three broken, nothing is left: the player says so quietly,
 * naming no source. The film comes from a sample source of its own. */

import { expect, type Page, test } from "./fixture";
import type { ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { Client, cleanup, startBridge, startIfPaused, SUB_AUTO_PORT } from "./core";
import { patchedContext } from "./patch";
import { BBB_ID, NAME, startSource } from "./source";

const GOOD = "WEBVTT\n\n00:00:00.000 --> 00:10:00.000\nA good line\n";
let core: { proc: ChildProcess; data: string } | null = null;
let addon: Server | null = null;
let media: Server | null = null;
// Whether the third file is good too; each stream answer names fresh URLs, so no file is cached.
let goodWorks = true;
let round = 0;

function subsAddon(film: string): Promise<Server> {
  const server = createServer((req, res) => {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const url = req.url ?? "";
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    if (url === "/manifest.json") {
      return json({ id: "dev.marquee.sub-auto", version: "1.0.0", name: "Sub Auto", resources: ["stream"], types: ["movie"], idPrefixes: ["tt"], catalogs: [] });
    }
    if (url === `/stream/movie/${BBB_ID}.json`) {
      round++;
      const file = (id: string) => ({ id, lang: "eng", label: id, url: `${base}/${id}.vtt?round=${round}` });
      return json({ streams: [{ name: "Sub Auto\n240p", url: film, behaviorHints: { filename: NAME }, subtitles: [file("broken-a"), file("broken-b"), file("good")] }] });
    }
    if (url.startsWith("/broken-") || (url.startsWith("/good") && !goodWorks)) {
      res.writeHead(200, { "content-type": "text/plain" });
      return res.end("no cues in here");
    }
    if (url.startsWith("/good")) {
      res.writeHead(200, { "content-type": "text/vtt" });
      return res.end(GOOD);
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

test.beforeAll(async () => {
  media = await startSource(0);
  addon = await subsAddon(`http://127.0.0.1:${(media.address() as AddressInfo).port}/media/${NAME}`);
  core = await startBridge(SUB_AUTO_PORT);
  const c = new Client(`ws://127.0.0.1:${SUB_AUTO_PORT}/ws`);
  try {
    // Cinemeta names the title (answered from the recording offline).
    await c.req("installAddon", { url: "https://v3-cinemeta.strem.io/manifest.json" });
    await c.req("installAddon", { url: `http://127.0.0.1:${(addon.address() as AddressInfo).port}/manifest.json` });
    await c.req("setPrefs", { patch: { defaultsSeen: 1, subtitleLanguages: ["eng"] } });
  } finally {
    c.close();
  }
});

test.afterAll(() => {
  if (core) cleanup(core.proc, core.data);
  addon?.close();
  media?.close();
});

type Snap = { autoSubtitle: string | null; startSubtitleFailed: boolean; subtitles: { url: string; label: string | null }[]; subtitleNotes: unknown[] };

/** A player page on the spec's core; `seen` is every playback snapshot the core sent, as sent. */
async function open(browser: Parameters<typeof patchedContext>[0]) {
  const seen: Snap[] = [];
  const { context } = await patchedContext(browser, SUB_AUTO_PORT, { viewport: { width: 1440, height: 900 } }, (view, s) => {
    if (view === "playback" && s) seen.push(s as Snap);
    return s;
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
  return { context, page, seen, errors };
}

const labelOf = (seen: Snap[], url: string | null | undefined) => seen.flatMap((s) => s.subtitles).find((t) => t.url === url)?.label ?? null;
const shownLabel = (page: Page, seen: Snap[]) => page.evaluate(() => document.querySelector("video track")?.getAttribute("src") ?? null).then((src) => labelOf(seen, src));

test("two broken start subtitles hand over to the next best, quietly, and the good one shows", async ({ browser }) => {
  goodWorks = true;
  const { context, page, seen, errors } = await open(browser);
  // The core's start pick moved on twice, from file to file, and settled on the good one.
  await expect.poll(() => labelOf(seen, seen.at(-1)?.autoSubtitle), { timeout: 30_000 }).toBe("good");
  const picks = seen.map((s) => labelOf(seen, s.autoSubtitle)).filter((l, i, all) => l && l !== all[i - 1]);
  expect(picks).toEqual(["broken-a", "broken-b", "good"]);
  // The player follows: the good file is the one on, with its line, and nothing alarms the viewer.
  await expect.poll(() => shownLabel(page, seen)).toBe("good");
  await expect.poll(() => page.evaluate(() => document.querySelector<HTMLTrackElement>('video track[data-line="1"]')?.track?.mode)).toBe("hidden");
  await expect(page.locator(".toast")).toHaveCount(0);
  expect(seen.at(-1)?.subtitleNotes).toEqual([]);
  expect(seen.at(-1)?.startSubtitleFailed).toBe(false);
  expect(errors).toEqual([]);
  await context.close();
});

test("with every start subtitle broken, the player says so quietly and names no source", async ({ browser }) => {
  goodWorks = false;
  const { context, page, seen, errors } = await open(browser);
  await expect.poll(() => seen.at(-1)?.startSubtitleFailed, { timeout: 30_000 }).toBe(true);
  const quiet = page.locator(".toast.quiet");
  // The shared words, as Android and Apple say them.
  await expect(quiet).toHaveText("Subtitles could not be loaded");
  await expect(quiet).not.toContainText("Sub Auto");
  await expect(page.locator(".toast.bad")).toHaveCount(0);
  // Nothing stays on that the core could not read.
  await expect.poll(() => page.evaluate(() => document.querySelector("video track")?.getAttribute("src") ?? null)).toBe(null);
  expect(errors).toEqual([]);
  await context.close();
});
