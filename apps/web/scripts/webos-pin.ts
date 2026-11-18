/** The webOS bundle's storage pin (plans/webos.md section 4): served the way the TV's service serves it (the token
 * first on the socket, the keystore answered by the service itself), the built page never puts the socket token or a
 * keystore value in localStorage. Each run also checks itself: the same bundle with its token moved to localStorage
 * must fail the pin.
 *   bun scripts/webos-pin.ts [dist dir]      after `bun run build:webos`; headless, muted, no core behind it
 * `serveWebos` is the stand-in service, for other headless runs of the webOS bundle. */

import { chromium } from "@playwright/test";
import { cpSync, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";

/** What webOS 6's browser says it is (Chromium 79). */
export const WEBOS_UA = "Mozilla/5.0 (Web0S; Linux/SmartTV) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/79.0.3945.79 Safari/537.36 WebAppManager";

const KEYSTORE = '{"host":"keystore"';
const TYPES: Record<string, string> = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".png": "image/png", ".webp": "image/webp", ".woff2": "font/woff2", ".json": "application/json", ".ico": "image/x-icon", ".webmanifest": "application/manifest+json" };

type Store = Record<string, unknown>;

/** The service's half the page sees: the built UI with index.html for any other path, and `/ws`, which takes the
 * token first (else closes with 4401), answers keystore messages itself and passes the rest to `core` when given. */
export function serveWebos(dist: string, token: string, store: Store, core?: string) {
  const seen = { reads: 0, writes: 0 };
  type Socket = { authed: boolean; up?: WebSocket; queue: string[] };
  const server = Bun.serve<Socket>({
    port: 0,
    hostname: "127.0.0.1",
    fetch(req, srv) {
      const path = new URL(req.url).pathname;
      if (path === "/ws") return srv.upgrade(req, { data: { authed: false, queue: [] } }) ? undefined : new Response("no", { status: 400 });
      const file = join(dist, decodeURIComponent(path));
      const hit = path !== "/" && existsSync(file) && !file.endsWith("/") ? file : join(dist, "index.html");
      return new Response(Bun.file(hit), { headers: { "content-type": TYPES[extname(hit)] ?? "application/octet-stream" } });
    },
    websocket: {
      open(ws) {
        if (!core) return;
        const up = new WebSocket(core);
        ws.data.up = up;
        up.addEventListener("open", () => ws.data.queue.splice(0).forEach((t) => up.send(t)));
        up.addEventListener("message", (e) => ws.send(String(e.data)));
        up.addEventListener("close", () => ws.close());
      },
      message(ws, raw) {
        const text = String(raw);
        if (!ws.data.authed) {
          if (text !== token) return ws.close(4401, "bad token");
          ws.data.authed = true;
          return;
        }
        if (text.startsWith(KEYSTORE)) {
          const m = JSON.parse(text) as { id: number; op: string; key?: string; value?: unknown };
          if (m.op === "read") seen.reads++;
          if (m.op === "write" && m.key) {
            seen.writes++;
            if (m.value === null) delete store[m.key];
            else store[m.key] = m.value;
          }
          return void ws.send(JSON.stringify({ host: "keystore", id: m.id, ok: true, ...(m.op === "read" ? { value: store } : {}) }));
        }
        const up = ws.data.up;
        if (up?.readyState === WebSocket.OPEN) up.send(text);
        else if (up) ws.data.queue.push(text);
      },
      close(ws) {
        ws.data.up?.close();
      },
    },
  });
  return { url: `http://127.0.0.1:${server.port}`, seen, stop: () => server.stop(true) };
}

/** One run of the pin over `dist`; the problems found, empty when it holds. */
export async function pin(dist: string): Promise<string[]> {
  const mark = `pin${Math.random().toString(36).slice(2, 10)}`;
  const token = `${mark}-token`;
  const secrets = [token, `${mark}-sync`, `${mark}-debrid`, `${mark}-trakt`];
  const store: Store = {
    sync: { revision: 1, secret: `${mark}-sync` },
    debrid: { records: [{ service: "realdebrid", generation: "0".repeat(32), revision: 1, credential: { kind: "apiKey", key: `${mark}-debrid` } }], tombstones: [] },
    "trakt:p1": { access: `${mark}-trakt` },
  };
  const service = serveWebos(dist, token, store);
  const browser = await chromium.launch({ args: ["--mute-audio"] });
  const problems: string[] = [];
  try {
    const page = await (await browser.newContext({ userAgent: WEBOS_UA, viewport: { width: 1920, height: 1080 } })).newPage();
    page.on("pageerror", (e) => problems.push(`page error: ${e.message}`));
    await page.goto(`${service.url}/#t=${token}`);
    for (let i = 0; i < 100 && service.seen.reads === 0; i++) await page.waitForTimeout(100);
    if (!service.seen.reads) problems.push("the page never read the keystore");
    // What the start writes (auto-keep, a restore) lands in the next few seconds.
    await page.waitForTimeout(3000);
    const { local, session } = await page.evaluate(() => {
      const dump = (s: Storage) => Object.fromEntries(Array.from({ length: s.length }, (_, i) => s.key(i) ?? "").map((k) => [k, s.getItem(k)]));
      return { local: dump(localStorage), session: dump(sessionStorage) };
    });
    const localText = JSON.stringify(local);
    for (const s of secrets) if (localText.includes(s)) problems.push(`localStorage holds ${s === token ? "the socket token" : `the keystore's ${s.slice(mark.length + 1)} value`}`);
    if (!JSON.stringify(session).includes(token)) problems.push("the token is not in sessionStorage: the page did not take it");
    console.log(`  keystore reads ${service.seen.reads}, writes ${service.seen.writes}; localStorage keys: ${Object.keys(local).join(", ") || "none"}`);
  } finally {
    await browser.close();
    service.stop();
  }
  return problems;
}

async function main() {
  const dist = process.argv[2] ?? join(import.meta.dirname, "../dist-webos");
  console.log("pin, the built bundle:");
  const problems = await pin(dist);
  for (const p of problems) console.error(`FAIL ${p}`);
  // The mutation: the same bundle keeping its token in localStorage.
  const copy = mkdtempSync(join(tmpdir(), "mq-webos-pin-"));
  let caught = false;
  try {
    cpSync(dist, copy, { recursive: true });
    const assets = join(copy, "assets");
    for (const f of readdirSync(assets).filter((n) => n.endsWith(".js"))) {
      const code = readFileSync(join(assets, f), "utf8");
      if (code.includes("sessionStorage")) writeFileSync(join(assets, f), code.replaceAll("sessionStorage", "localStorage"));
    }
    console.log("pin, the mutation (token kept in localStorage):");
    caught = (await pin(copy)).some((p) => p.includes("socket token"));
    console.log(`${caught ? "ok  " : "MISS"} the mutation fails the pin`);
  } finally {
    rmSync(copy, { recursive: true, force: true });
  }
  const ok = !problems.length && caught;
  console.log(ok ? "webos pin passed, the mutation caught" : "webos pin FAILED");
  process.exit(ok ? 0 : 1);
}

if (import.meta.main) await main();
