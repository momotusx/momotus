/** Local stream addon for verification. Real catalogs and metadata come from Cinemeta;
 * this addon only answers `stream` for Sintel (tt1727587) with the real Blender files the
 * core's own live tests use, plus one dead URL to exercise failover.
 *
 * Run: bun scripts/sintel-addon.ts [port]   (default 7780, listens on all interfaces)
 * Install in the emulator: `adb reverse tcp:7780 tcp:7780`, then http://127.0.0.1:7780/manifest.json
 * Install on the host bridge: http://127.0.0.1:7780/manifest.json */

const port = Number(process.argv[2] ?? 7780);
const BLENDER = "https://download.blender.org/durian";

const manifest = {
  id: "app.marquee.localtest",
  version: "0.1.0",
  name: "Local Test",
  description: "Sintel sample streams for verification",
  resources: ["stream"],
  types: ["movie"],
  idPrefixes: ["tt1727587"],
  catalogs: [],
};

// The trailer is listed under the movie's file name so the title gate lets it through.
const streams = [
  {
    name: "Local Test",
    title: "Sintel.2010.1080p.mkv\nAC-3 5.1, 10 subtitle tracks, 1.18 GB",
    url: `${BLENDER}/movies/Sintel.2010.1080p.mkv`,
    behaviorHints: { filename: "Sintel.2010.1080p.mkv" },
  },
  {
    name: "Local Test",
    title: "Sintel.2010.1080p.mp4\nH.264 and AAC, 14.6 MB (the trailer)",
    url: `${BLENDER}/trailer/sintel_trailer-1080p.mp4`,
    behaviorHints: { filename: "Sintel.2010.1080p.mp4" },
  },
  {
    name: "Local Test",
    title: "Sintel.2010.720p.mp4\nH.264 and AAC (the 720p trailer)",
    url: `${BLENDER}/trailer/sintel_trailer-720p.mp4`,
    behaviorHints: { filename: "Sintel.2010.720p.mp4" },
  },
  {
    name: "Local Test",
    title: "Sintel.2010.480p.mp4\nH.264 and AAC (the 480p trailer)",
    url: `${BLENDER}/trailer/sintel_trailer-480p.mp4`,
    behaviorHints: { filename: "Sintel.2010.480p.mp4" },
  },
  {
    name: "Local Test",
    title: "Sintel.2010.1080p.missing.mkv\nDead mirror (404), for failover",
    url: `${BLENDER}/movies/Sintel.2010.1080p.missing.mkv`,
    behaviorHints: { filename: "Sintel.2010.1080p.missing.mkv" },
  },
];

const headers = { "content-type": "application/json", "access-control-allow-origin": "*" };

Bun.serve({
  port,
  hostname: "0.0.0.0",
  fetch(request) {
    const { pathname } = new URL(request.url);
    if (pathname === "/manifest.json") return Response.json(manifest, { headers });
    if (pathname === "/stream/movie/tt1727587.json") return Response.json({ streams }, { headers });
    return new Response("not found", { status: 404 });
  },
});

console.log(`local test addon on http://0.0.0.0:${port}/manifest.json`);
