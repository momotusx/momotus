/** A minimal Stremio stream addon for tests: one direct HTTP stream for Big
 * Buck Bunny (2008, CC BY 3.0, Blender Foundation) hosted by Wikimedia
 * Commons, so playback runs through the real core and stream server. */

import { createServer, type Server } from "node:http";

export const BBB_ID = "tt1254207";
export const BBB_URL =
  "https://upload.wikimedia.org/wikipedia/commons/transcoded/c/c0/Big_Buck_Bunny_4K.webm/Big_Buck_Bunny_4K.webm.480p.vp9.webm";

const manifest = {
  id: "dev.marquee.sample-source",
  version: "1.0.0",
  name: "Sample Source",
  description: "Open movies for testing playback",
  resources: ["stream"],
  types: ["movie"],
  idPrefixes: ["tt"],
  catalogs: [],
};

export function startSource(port: number): Promise<Server> {
  const server = createServer((req, res) => {
    const json = (status: number, body: unknown) => {
      res.writeHead(status, { "content-type": "application/json", "access-control-allow-origin": "*" });
      res.end(JSON.stringify(body));
    };
    if (req.url === "/manifest.json") return json(200, manifest);
    if (req.url === `/stream/movie/${BBB_ID}.json`) {
      return json(200, {
        streams: [
          {
            name: "Sample Source\n480p",
            title: "Big.Buck.Bunny.2008.480p.WEB.VP9.webm",
            url: BBB_URL,
            behaviorHints: { filename: "Big.Buck.Bunny.2008.480p.WEB.VP9.webm", videoSize: 96645673 },
          },
        ],
      });
    }
    if (req.url?.startsWith("/stream/")) return json(200, { streams: [] });
    json(404, { error: "not found" });
  });
  return new Promise((resolve) => server.listen(port, "127.0.0.1", () => resolve(server)));
}

if (import.meta.main) {
  const port = Number(process.argv[2] ?? 17890);
  await startSource(port);
  console.log(`sample source on http://127.0.0.1:${port}/manifest.json`);
}
