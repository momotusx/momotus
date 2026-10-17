// A minimal Stremio stream addon for checking playback on Apple devices: one
// direct HTTP stream of Big Buck Bunny (2008, CC BY 3.0, Blender Foundation),
// an H.264 and AAC MP4 that AVPlayer opens, served from archive.org.
// Run: bun apps/apple/scripts/sample-source.ts [port] [local copy of the file]
// With a local copy the stream is served from here with Range support, which
// takes the origin's latency (about 2 s per request) out of measurements.
// SAMPLE_BROKEN=1 adds a "1080p" stream that answers 404 and ranks first, to
// check that playback fails over to the next source.
// With a local copy it also answers for Breaking Bad season 1 (tt0903747),
// each episode a file named for it with the same picture and one binge group,
// to check Up Next from one episode to the next. Nothing else is claimed.
// Install in a debug build: -MQInstallAddons http://127.0.0.1:7792/manifest.json

const port = Number(process.argv[2] ?? 7792);
const local = process.argv[3];
const BBB_ID = "tt1254207";
const SERIES_ID = "tt0903747";
const NAME = "Big.Buck.Bunny.2008.360p.WEB.H264.AAC.mp4";
const ORIGIN = "https://archive.org/download/BigBuckBunny_124/Content/big_buck_bunny_720p_surround.mp4";
const URL_MP4 = local ? `http://127.0.0.1:${port}/media/${NAME}` : ORIGIN;
const SIZE = 61878609;

const manifest = {
  id: "dev.marquee.apple-sample-source",
  version: "1.1.0",
  name: "Sample Source",
  description: "Open movies for checking playback",
  resources: ["stream"],
  types: local ? ["movie", "series"] : ["movie"],
  idPrefixes: ["tt"],
  catalogs: [],
};

const json = (body: unknown) =>
  new Response(JSON.stringify(body), {
    headers: { "content-type": "application/json", "access-control-allow-origin": "*" },
  });

/** Range-capable response for the local file. */
function serveFile(req: Request): Response {
  const file = Bun.file(local!);
  const m = /bytes=(\d*)-(\d*)/.exec(req.headers.get("range") ?? "");
  if (!m) return new Response(file, { headers: { "content-type": "video/mp4", "accept-ranges": "bytes" } });
  const start = m[1] ? Number(m[1]) : 0;
  const end = m[2] ? Math.min(Number(m[2]), file.size - 1) : file.size - 1;
  return new Response(file.slice(start, end + 1), {
    status: 206,
    headers: {
      "content-type": "video/mp4",
      "accept-ranges": "bytes",
      "content-range": `bytes ${start}-${end}/${file.size}`,
      "content-length": String(end - start + 1),
    },
  });
}

Bun.serve({
  port,
  hostname: "127.0.0.1",
  fetch(req) {
    const path = decodeURIComponent(new URL(req.url).pathname);
    if (process.env.SAMPLE_LOG) console.log(req.method, path);
    if (path === "/manifest.json") return json(manifest);
    if (path === `/stream/movie/${BBB_ID}.json`) {
      const broken = {
        name: "Sample Source\n1080p",
        title: "Big.Buck.Bunny.2008.1080p.WEB.H264.AAC.mp4",
        url: `http://127.0.0.1:${port}/media/missing.mp4`,
        behaviorHints: { filename: "Big.Buck.Bunny.2008.1080p.WEB.H264.AAC.mp4", videoSize: 412000000 },
      };
      return json({
        streams: [
          ...(process.env.SAMPLE_BROKEN ? [broken] : []),
          { name: "Sample Source\n360p", title: NAME, url: URL_MP4, behaviorHints: { filename: NAME, videoSize: SIZE } },
        ],
      });
    }
    const episode = new RegExp(`^/stream/series/${SERIES_ID}:1:(\\d+)\\.json$`).exec(path);
    if (local && episode) {
      const name = `Breaking.Bad.S01E${episode[1].padStart(2, "0")}.360p.WEB.H264.AAC.mp4`;
      return json({
        streams: [
          {
            name: "Sample Source\n360p",
            title: name,
            url: `http://127.0.0.1:${port}/media/${name}`,
            behaviorHints: { filename: name, videoSize: SIZE, bingeGroup: "sample-360p" },
          },
        ],
      });
    }
    if (local && path.startsWith("/media/") && path.endsWith(".mp4") && !path.endsWith("missing.mp4")) return serveFile(req);
    if (path.startsWith("/stream/")) return json({ streams: [] });
    return new Response("not found", { status: 404 });
  },
});
console.log(`sample source on http://127.0.0.1:${port}/manifest.json`);
