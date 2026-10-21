/** The recorder behind MQ_DEV_REPLAY: every request a core makes in the e2e to
 * a host off this machine (Cinemeta, OpenSubtitles, Torrentio, their
 * subtitle files, any addon a test installs) comes here as
 * `/replay/<scheme>/<host><path>`, and so does every request the browser
 * makes off this machine (startBrowserRecorder). The first run asks the live
 * host once and keeps the answer in e2e/.cache/replay, which is never
 * committed: these are other services' data. Every later run is served from
 * there, so the whole e2e runs with the network off.
 *
 * MQ_E2E_OFFLINE=1: never ask a live host; a missing answer is a 504, and
 * the run lists every one at its end. MQ_E2E_RECORD=1: ask every live host
 * again and keep the new answers. */

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { createServer as createSecureServer } from "node:https";
import { fileURLToPath } from "node:url";

const DIR = fileURLToPath(new URL("./.cache/replay/", import.meta.url));
const OFFLINE = process.env.MQ_E2E_OFFLINE === "1";
const RECORD = process.env.MQ_E2E_RECORD === "1";
/** Request headers that change an answer, passed to the live host. */
const PASSED = ["accept", "accept-language", "content-type", "range", "user-agent"];
/** Answer headers kept: the type, and CORS (a page reads a logo's pixels only when allowed). */
const KEPT = ["content-type", "access-control-allow-origin", "access-control-expose-headers"];

/** An answer as kept, or a host that does not exist. */
type Kept = { url: string; status: number; headers: Record<string, string>; body: string } | { url: string; unknownHost: true };

/** Answers this run had to ask a live host for, and those it could not serve. */
export const replayed = { asked: 0, missed: new Set<string>() };

async function body(req: IncomingMessage): Promise<Buffer | undefined> {
  if (req.method === "GET" || req.method === "HEAD") return undefined;
  const chunks: Buffer[] = [];
  for await (const chunk of req) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks);
}

async function ask(url: string, req: IncomingMessage, sent: Buffer | undefined): Promise<Kept | null> {
  const headers: Record<string, string> = {};
  for (const name of PASSED) {
    const value = req.headers[name];
    if (typeof value === "string") headers[name] = value;
  }
  try {
    const res = await fetch(url, { method: req.method, headers, body: sent ? new Uint8Array(sent) : undefined, redirect: "follow", signal: AbortSignal.timeout(30_000) });
    const answer = Buffer.from(await res.arrayBuffer());
    const kept = Object.fromEntries(KEPT.flatMap((name) => (res.headers.get(name) === null ? [] : [[name, res.headers.get(name) as string]])));
    return { url, status: res.status, headers: kept, body: answer.toString("base64") };
  } catch (e) {
    // A host that does not exist stays so (a test's made-up addon); any other
    // failure is not kept, and the next run asks again.
    const code = (e as { cause?: { code?: string } }).cause?.code;
    return code === "ENOTFOUND" ? { url, unknownHost: true } : null;
  }
}

/** Serves `rest` (`<scheme>/<host><path>?<query>`) from the recording. */
export async function replay(req: IncomingMessage, res: ServerResponse, rest: string): Promise<void> {
  const slash = rest.indexOf("/");
  const url = `${rest.slice(0, slash)}://${rest.slice(slash + 1)}`;
  const host = new URL(url).host.replace(/[^\w.-]/g, "_");
  const sent = await body(req);
  const key = createHash("sha256").update(`${req.method} ${url} ${req.headers.range ?? ""} `).update(sent ?? Buffer.alloc(0)).digest("hex").slice(0, 40);
  const file = `${DIR}${host}/${key}.json`;
  let kept: Kept | null = !RECORD && existsSync(file) ? (JSON.parse(readFileSync(file, "utf8")) as Kept) : null;
  if (!kept && !OFFLINE) {
    replayed.asked++;
    kept = await ask(url, req, sent);
    if (kept) {
      mkdirSync(`${DIR}${host}`, { recursive: true });
      writeFileSync(file, JSON.stringify(kept));
    }
  }
  if (!kept) {
    if (OFFLINE) replayed.missed.add(`${req.method} ${url}`);
    res.writeHead(504, { "content-type": "text/plain" });
    res.end("not recorded");
    return;
  }
  // As the core would see a name that does not resolve: no answer at all.
  if ("unknownHost" in kept) {
    req.socket.destroy();
    return;
  }
  res.writeHead(kept.status, kept.headers);
  res.end(Buffer.from(kept.body, "base64"));
}

/** A certificate made for this checkout's runs (e2e/.cache/tls): the browser
 * is told to ignore its errors, so any name may be served with it. */
function certificate(): { key: Buffer; cert: Buffer } {
  const dir = fileURLToPath(new URL("./.cache/tls/", import.meta.url));
  const [key, cert] = [`${dir}key.pem`, `${dir}cert.pem`];
  if (!existsSync(cert)) {
    mkdirSync(dir, { recursive: true });
    execFileSync("openssl", ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", cert, "-subj", "/CN=marquee-e2e", "-days", "3650"], { stdio: "ignore" });
  }
  return { key: readFileSync(key), cert: readFileSync(cert) };
}

/** The browser's side of the same recording: playwright.config maps every
 * name but this machine's to these two ports (443 to `https`, 80 to `http`),
 * so the images, logos and frames a page loads are recorded and served like
 * the cores' requests, and a run with the network off still shows them. */
export async function startBrowserRecorder(ports: { https: number; http: number }): Promise<Server[]> {
  const handle = (scheme: string) => (req: IncomingMessage, res: ServerResponse) => void replay(req, res, `${scheme}/${req.headers.host ?? ""}${req.url ?? "/"}`);
  const servers: Server[] = [createSecureServer(certificate(), handle("https")), createServer(handle("http"))];
  await Promise.all(servers.map((s, i) => new Promise<void>((ok) => s.listen(i ? ports.http : ports.https, "127.0.0.1", ok))));
  return servers;
}
