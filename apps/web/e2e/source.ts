/** A minimal Stremio stream addon for tests: direct HTTP streams of our own
 * test film, served from here with Range support, so playback runs through
 * the real core and stream server without a remote media host. The film
 * (e2e/fixtures/test-film.webm, under 1 MB) is generated, not downloaded:
 * colour bars with a moving square and silent audio, as long as the release
 * it stands in for (10:34.6, VP9 and Opus in WebM, a key frame every 2 s):
 *
 *   ffmpeg -f lavfi -i "smptebars=size=320x180:rate=12,drawbox=x='mod(t*40,300)':y=150:w=20:h=20:color=white:t=fill,format=yuv420p" \
 *     -f lavfi -i "anullsrc=channel_layout=stereo:sample_rate=48000" -t 634.571 -c:v libvpx-vp9 -b:v 0 -crf 55 -g 24 \
 *     -deadline realtime -cpu-used 8 -row-mt 1 -c:a libopus -b:a 6k -shortest test-film.webm
 *
 * The addon names the film as the releases the tests play (Big Buck Bunny,
 * two Game of Thrones episodes): the web bridge has no torrent engine and
 * the e2e cores no streaming service account, so Torrentio's rows are
 * blocked (needsAccount) and these direct links are what plays, with no
 * BitTorrent in any run. The same server answers the core's skip lookups
 * (MQ_DEV_SEGMENTS) and its requests to hosts off this machine
 * (MQ_DEV_REPLAY, e2e/replay.ts). */

import { createReadStream, readFileSync, statSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { fileURLToPath } from "node:url";
import { replay } from "./replay";

export const BBB_ID = "tt1254207";
/** The test film, standing in for every release the tests play. */
export const FILM_FILE = fileURLToPath(new URL("./fixtures/test-film.webm", import.meta.url));
export const NAME = "Big.Buck.Bunny.2008.240p.WEB.VP9.webm";
// A real addon names the release it serves; the stand-in film carries the
// episode's name so the core's title and episode checks accept it.
const EPISODE_NAME = "Game.of.Thrones.S01E05.Sample.240p.WEB.VP9.webm";
// The resume episode gets two, so Versions has a pick and one row besides.
const RESUME_NAMES = ["Game.of.Thrones.S02E03.Sample.240p.WEB.VP9-OPENFILM.webm", "Game.of.Thrones.S02E03.Sample.240p.WEB.VP9-BLENDER.webm"];

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

/** The episode whose one stream is DASH: the first 40 s of the test film as
 * ffmpeg's `-f dash` writes it (e2e/fixtures/dash, VP9 and Opus in fMP4, as
 * MovieBox's rows are shaped), which the core serves a browser as HLS:
 *
 *   ffmpeg -t 40 -i test-film.webm -map 0:v -map 0:a -c copy -f dash -dash_segment_type mp4 -seg_duration 4 \
 *     -use_timeline 0 -use_template 1 -init_seg_name 'init-$RepresentationID$.m4s' \
 *     -media_seg_name 'chunk-$RepresentationID$-$Number%05d$.m4s' film.mpd */
export const DASH_EPISODE = "tt0944947:1:6";
const DASH_NAME = "Game.of.Thrones.S01E06.Sample.240p.WEB.VP9.mpd";
const DASH_DIR = fileURLToPath(new URL("./fixtures/dash/", import.meta.url));

/** Skip segments the core asks for (MQ_DEV_SEGMENTS points it here), as
 * synthetic answers in TheIntroDB's and IntroDB's shapes (their terms allow no stored copy):
 * `bun run e2e:live` checks that the live answers still have those shapes. */
const fixture = (name: string) => JSON.parse(readFileSync(fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url)), "utf8")) as unknown;
const SEGMENTS: Record<string, unknown> = {
  "/segments/theintrodb/v3/media": fixture(`theintrodb-${BBB_ID}.json`),
  "/segments/introdb/segments": fixture(`introdb-${BBB_ID}.json`),
};

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
    // The raw path: a recorded request is kept as the core sent it.
    if (req.url?.startsWith("/replay/")) {
      void replay(req, res, req.url.slice("/replay/".length));
      return;
    }
    const path = decodeURIComponent(req.url ?? "");
    if (path === "/manifest.json") return json(200, manifest);
    const asked = new URL(req.url ?? "/", "http://source");
    if (asked.pathname.startsWith("/segments/")) {
      const recorded = asked.searchParams.get("imdb_id") === BBB_ID ? SEGMENTS[asked.pathname] : undefined;
      return recorded ? json(200, recorded) : json(404, { error: "not found" });
    }
    const files = LISTED[path];
    if (files) {
      const size = statSync(FILM_FILE).size;
      return json(200, {
        streams: files.map((file) => ({
          name: "Sample Source\n240p",
          title: file,
          url: `http://127.0.0.1:${port}/media/${file}`,
          behaviorHints: { filename: file, videoSize: size },
        })),
      });
    }
    if (path === `/stream/series/${DASH_EPISODE}.json`) {
      const url = `http://127.0.0.1:${port}/dash/film.mpd`;
      return json(200, { streams: [{ name: "Sample Source\n240p", title: DASH_NAME, url, behaviorHints: { filename: DASH_NAME } }] });
    }
    const dash = /^\/dash\/([\w.-]+\.(?:mpd|m4s))$/.exec(asked.pathname);
    if (dash) {
      const file = DASH_DIR + dash[1];
      let body: Buffer;
      try {
        body = readFileSync(file);
      } catch {
        return json(404, { error: "not found" });
      }
      res.writeHead(200, { "content-type": file.endsWith(".mpd") ? "application/dash+xml" : "video/mp4", "content-length": String(body.length) });
      return res.end(body);
    }
    if (SERVED.has(req.url ?? "")) {
      const size = statSync(FILM_FILE).size;
      const m = /bytes=(\d*)-(\d*)/.exec(req.headers.range ?? "");
      const start = m?.[1] ? Number(m[1]) : 0;
      const end = m?.[2] ? Math.min(Number(m[2]), size - 1) : size - 1;
      const head = { "content-type": "video/webm", "accept-ranges": "bytes", "content-length": String(end - start + 1) };
      if (m) res.writeHead(206, { ...head, "content-range": `bytes ${start}-${end}/${size}` });
      else res.writeHead(200, head);
      if (req.method === "HEAD") return res.end();
      createReadStream(FILM_FILE, { start, end }).pipe(res);
      return;
    }
    if (req.url?.startsWith("/stream/")) return json(200, { streams: [] });
    json(404, { error: "not found" });
  });
  return new Promise((resolve) => server.listen(port, "127.0.0.1", () => resolve(server)));
}
