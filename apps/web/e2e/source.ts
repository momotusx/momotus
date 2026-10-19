/** A minimal Stremio stream addon for tests: direct HTTP streams of Big Buck
 * Bunny (2008, CC BY 3.0, Blender Foundation). The file comes from Wikimedia
 * Commons once, is kept in e2e/.cache, and is served from here with Range
 * support, so playback runs through the real core and stream server without
 * a remote media host in every run. The same file stands in for two Game of
 * Thrones episodes: the web bridge has no torrent engine and the e2e cores
 * no streaming service account, so Torrentio's rows are blocked
 * (needsAccount) and these direct links are what plays, with no BitTorrent
 * in any run. */

import { createReadStream, createWriteStream, existsSync, mkdirSync, renameSync, statSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

export const BBB_ID = "tt1254207";
const BBB_REMOTE =
  "https://upload.wikimedia.org/wikipedia/commons/transcoded/c/c0/Big_Buck_Bunny_4K.webm/Big_Buck_Bunny_4K.webm.240p.vp9.webm";
const CACHE = fileURLToPath(new URL("./.cache/", import.meta.url));
export const BBB_FILE = `${CACHE}bbb-240p-vp9.webm`;
export const NAME = "Big.Buck.Bunny.2008.240p.WEB.VP9.webm";
// A real addon names the release it serves; the stand-in film carries the
// episode's name so the core's title and episode checks accept it.
const EPISODE_NAME = "Game.of.Thrones.S01E05.Sample.240p.WEB.VP9.webm";
// The resume episode gets two, so Versions has a pick and one row besides.
const RESUME_NAMES = ["Game.of.Thrones.S02E03.Sample.240p.WEB.VP9-OPENFILM.webm", "Game.of.Thrones.S02E03.Sample.240p.WEB.VP9-BLENDER.webm"];

/** Downloads the test film once (about 30 MB). */
export async function ensureMedia(): Promise<void> {
  if (existsSync(BBB_FILE)) return;
  mkdirSync(CACHE, { recursive: true });
  const res = await fetch(BBB_REMOTE, { headers: { "user-agent": "Marquee e2e (https://github.com/marquee)" } });
  if (!res.ok || !res.body) throw new Error(`downloading the test film failed: ${res.status}`);
  const part = `${BBB_FILE}.part`;
  await pipeline(Readable.fromWeb(res.body as never), createWriteStream(part));
  renameSync(part, BBB_FILE);
}

/** The one episode the harness can play: the same open film stands in for
 * it, so a test can pick an episode that is not the resume target and see
 * playback and progress land on that episode. */
export const SAMPLE_EPISODE = "tt0944947:1:5";
/** Game of Thrones S2:E3, the shared core's resume target (`GOT.resume`). */
const RESUME_EPISODE = "tt0944947:2:3";

/** What each stream request lists, by path. */
const LISTED: Record<string, string[]> = {
  [`/stream/movie/${BBB_ID}.json`]: [NAME],
  [`/stream/series/${SAMPLE_EPISODE}.json`]: [EPISODE_NAME],
  [`/stream/series/${RESUME_EPISODE}.json`]: RESUME_NAMES,
};
const SERVED = new Set(Object.values(LISTED).flat().map((n) => `/media/${n}`));

const manifest = {
  id: "dev.marquee.sample-source",
  version: "1.0.0",
  name: "Sample Source",
  description: "Open movies for testing playback",
  resources: ["stream"],
  types: ["movie", "series"],
  idPrefixes: ["tt"],
  catalogs: [],
};

export function startSource(port: number): Promise<Server> {
  const server = createServer((req, res) => {
    const json = (status: number, body: unknown) => {
      res.writeHead(status, { "content-type": "application/json", "access-control-allow-origin": "*" });
      res.end(JSON.stringify(body));
    };
    const path = decodeURIComponent(req.url ?? "");
    if (path === "/manifest.json") return json(200, manifest);
    const files = LISTED[path];
    if (files) {
      const size = statSync(BBB_FILE).size;
      return json(200, {
        streams: files.map((file) => ({
          name: "Sample Source\n240p",
          title: file,
          url: `http://127.0.0.1:${port}/media/${file}`,
          behaviorHints: { filename: file, videoSize: size },
        })),
      });
    }
    if (SERVED.has(req.url ?? "")) {
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
    if (req.url?.startsWith("/stream/")) return json(200, { streams: [] });
    json(404, { error: "not found" });
  });
  return new Promise((resolve) => server.listen(port, "127.0.0.1", () => resolve(server)));
}
