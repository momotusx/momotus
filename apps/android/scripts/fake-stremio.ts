/** A local stand-in for Stremio's link service and account API, so Import from Stremio can be walked into the
 * recommended addons step without an account (the routes the core's mq-import reads: link create and read,
 * getUser, addonCollectionGet, datastoreGet). Nothing here is a real account: a made-up session, an empty
 * library, and two addons taken from the core's bundled defaults (Cinemeta and OpenSubtitles v3), so the step
 * shows them as "From Stremio".
 *
 * The app reaches it through a debug build: `build-core.sh dev`, `adb reverse tcp:<port> tcp:<port>`, and the
 * base URL in the app's `files/dev-stremio`. The code is linked after READY_AFTER reads (default 2).
 * Run: PORT=7783 bun scripts/fake-stremio.ts */

import { readFileSync } from "node:fs";
import { join } from "node:path";

const port = Number(process.env.PORT ?? 7783);
const readyAfter = Number(process.env.READY_AFTER ?? 2);
const base = `http://127.0.0.1:${port}`;
const session = "walk-made-up-session";

type Default = { key: string; url: string; manifest: unknown };
const defaults: Default[] = JSON.parse(
  readFileSync(join(import.meta.dir, "../../../crates/mq-runtime/src/defaults.json"), "utf8"),
);
const brought = defaults.filter((d) => d.key === "cinemeta" || d.key === "opensubtitles");

let reads = 0;

const ok = (result: unknown) => Response.json({ result });
const error = (code: number, message: string) => Response.json({ error: { code, message } });

Bun.serve({
  port,
  async fetch(request) {
    const url = new URL(request.url);
    console.log(new Date().toISOString(), request.method, url.pathname);
    switch (url.pathname) {
      case "/api/v2/create":
        reads = 0;
        return ok({ code: "WALK42", link: `${base}/WALK42`, qrcode: `${base}/WALK42` });
      case "/api/v2/read":
        reads += 1;
        return reads > readyAfter ? ok({ authKey: session }) : error(101, "the code is not linked yet");
    }
    const body = request.method === "POST" ? ((await request.json()) as { authKey?: string }) : {};
    if (body.authKey !== session) return error(1, "Session does not exist");
    switch (url.pathname) {
      case "/api/getUser":
        return ok({ _id: "walk-user", email: "walk@example.invalid", trakt: null });
      case "/api/addonCollectionGet":
        return ok({
          addons: brought.map((d) => ({ transportUrl: d.url, manifest: d.manifest, flags: {} })),
          lastModified: new Date().toISOString(),
        });
      case "/api/datastoreGet":
        return ok([]);
    }
    return new Response("not found", { status: 404 });
  },
});
console.log(`fake Stremio on ${base}: ${brought.map((d) => d.key).join(", ")} from Stremio`);
