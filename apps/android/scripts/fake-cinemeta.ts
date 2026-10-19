/** A local stand-in for Cinemeta's two Find addons lists, for walks of the states the live lists never
 * show: Official empty ("Nothing to show from Cinemeta here.") and Community with one subtitles addon only
 * (so the Sources filter is empty). The app reads it through a debug build: `build-core.sh dev`,
 * `adb reverse tcp:<port> tcp:<port>`, and `http://127.0.0.1:<port>/manifest.json` in the app's
 * `files/dev-cinemeta`. `/_down` makes every list answer 503, `/_up` brings them back.
 * Run: PORT=17921 bun scripts/fake-cinemeta.ts */

const port = Number(process.env.PORT ?? 17921);
let down = false;

const subtitles = {
  transportUrl: "https://subtitles.invalid/manifest.json",
  transportName: "http",
  manifest: {
    id: "walk.subtitles",
    version: "1.0.0",
    name: "Walk Subtitles",
    description: "A subtitles addon that exists only in this walk.",
    types: ["movie", "series"],
    resources: ["subtitles"],
    catalogs: [],
  },
};

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

Bun.serve({
  port,
  hostname: "127.0.0.1",
  fetch(req) {
    const path = new URL(req.url).pathname;
    console.log(`${new Date().toISOString()} ${req.method} ${path}${down ? " (down)" : ""}`);
    if (path === "/_down" || path === "/_up") {
      down = path === "/_down";
      return json(200, { down });
    }
    if (down) return json(503, {});
    if (path === "/manifest.json") {
      return json(200, {
        id: "walk.cinemeta",
        version: "1.0.0",
        name: "Walk Cinemeta",
        resources: ["addon_catalog"],
        types: ["all"],
        catalogs: [],
        addonCatalogs: [],
      });
    }
    if (path === "/addon_catalog/all/official.json") return json(200, { addons: [] });
    if (path === "/addon_catalog/all/community.json") return json(200, { addons: [subtitles] });
    return json(404, {});
  },
});
console.log(`fake Cinemeta lists on http://127.0.0.1:${port}`);
