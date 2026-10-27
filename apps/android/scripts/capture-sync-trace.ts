/** Records the sync golden trace from two real cores and a real sync server: device A starts an
 * account, B shows a pairing code, A reads it and approves it from a scan, B joins, both sync, A lists the devices, previews B lost,
 * makes a recovery kit, removes B, reads both device lists (forkPreview), then approves B's next code
 * and ends that approval at once (cancelPairing). Writes core/src/test/resources/traces/sync.jsonl for the strict
 * decode test. Records, kits and pairing keys are replaced by placeholders before writing, so the
 * file holds no secret of the (throwaway) account.
 *
 * Needs: a sync server (`mq-sync-server --listen 127.0.0.1:<port>`) and two bridges on empty data
 * directories (`MQ_DATA=<empty dir> MQ_BRIDGE_PORT=<port> mq-bridge`).
 * Run: SYNC_SERVER=http://127.0.0.1:<port> MQ_BRIDGE=ws://127.0.0.1:<a>/ws MQ_BRIDGE2=ws://127.0.0.1:<b>/ws \
 *   bun scripts/capture-sync-trace.ts */

import { writeFileSync } from "node:fs";
import { join } from "node:path";

type Json = Record<string, any>;
const server = process.env.SYNC_SERVER;
const bridges = [process.env.MQ_BRIDGE, process.env.MQ_BRIDGE2];
if (!server || !bridges[0] || !bridges[1]) throw new Error("set SYNC_SERVER, MQ_BRIDGE and MQ_BRIDGE2");

const lines: { t: number; device: string; dir: "out" | "in"; msg: Json }[] = [];
const started = performance.now();
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function device(name: string, url: string, firstId: number) {
  const socket = new WebSocket(url);
  await new Promise<void>((ok, fail) => {
    socket.addEventListener("open", () => ok());
    socket.addEventListener("error", () => fail(new Error(`cannot reach ${url}`)));
  });
  const waiting = new Map<number, (m: Json) => void>();
  const pushes: Json[] = [];
  const log = (dir: "out" | "in", msg: Json) =>
    lines.push({ t: Math.round(performance.now() - started), device: name, dir, msg });
  socket.addEventListener("message", (event) => {
    const msg = JSON.parse(String(event.data)) as Json;
    log("in", msg);
    if (typeof msg.id === "number") waiting.get(msg.id)?.(msg);
    if (Array.isArray(msg.updates)) pushes.push(...msg.updates);
  });
  let nextId = firstId;
  const request = async (op: string, fields: Json = {}) => {
    const msg = { id: nextId++, op, ...fields };
    log("out", msg);
    const reply = await new Promise<Json>((ok) => {
      waiting.set(msg.id, ok);
      socket.send(JSON.stringify(msg));
    });
    if (!reply.ok) throw new Error(`${name} ${op}: ${reply.error}`);
    return reply.result;
  };
  return { request, pushes, close: () => socket.close() };
}

async function until<T>(what: string, find: () => T | undefined): Promise<T> {
  for (let i = 0; i < 300; i++) {
    const found = find();
    if (found !== undefined) return found;
    await sleep(100);
  }
  throw new Error(`gave up waiting for ${what}`);
}

const a = await device("a", bridges[0], 1);
const b = await device("b", bridges[1], 1001);

let recordA: Json = await a.request("createSyncAccount", { server, name: "Trace A" });
const pairing = await b.request("subscribe", { view: "pairing", params: { server, name: "Trace B" } });
const qr = await until("B's code", () => b.pushes.find((p) => p.phase === "waiting")?.qr as string | undefined);
await a.request("readPairingCode", { qr });
const approved = await a.request("approvePairing", { record: recordA, qr, via: "scan", unlocked: true });
recordA = approved.record;
const recordB = await until("B joined", () => b.pushes.find((p) => p.phase === "joined")?.record as Json | undefined);
await b.request("unsubscribe", { sub: pairing.sub });

const round = async (d: typeof a, record: Json) => {
  const report = await d.request("syncNow", { record });
  return report.record ?? record;
};
recordA = await round(a, recordA);
await round(b, recordB);
const devices = await a.request("syncDevices", { record: recordA });
recordA = devices.record ?? recordA;
const idB = (devices.members as Json[]).find((m) => m.name === "Trace B")?.id as string;
await a.request("lostPreview", { record: recordA, lost: idB });
recordA = (await a.request("createRecoveryKit", { record: recordA })).record;
recordA = await a.request("removeDevice", { record: recordA, device: idB });
await a.request("syncDevices", { record: recordA });
await a.request("forkPreview", { record: recordA });
// B shows a new code; A approves it, then ends that approval at once.
b.pushes.length = 0;
const again = await b.request("subscribe", { view: "pairing", params: { server, name: "Trace B" } });
const qr2 = await until("B's new code", () => b.pushes.find((p) => p.phase === "waiting")?.qr as string | undefined);
recordA = (await a.request("approvePairing", { record: recordA, qr: qr2, via: "scan", unlocked: true })).record;
recordA = await a.request("cancelPairing", { record: recordA, qr: qr2 });
await b.request("unsubscribe", { sub: again.sub });
a.close();
b.close();

// Placeholders keep each field's type: a record stays an object, a kit and a code stay strings.
const secret = (key: string, value: unknown): unknown => {
  if (key === "record" && value && typeof value === "object") return { revision: (value as Json).revision ?? 0 };
  if (key === "kit" && typeof value === "string") return "AAAA-BBBB-CCCC-DDDD-EEEE-FFFF-GGGG-HHHH";
  if (key === "qr" && typeof value === "string") return "https://momotus.example/pair#v=3&s=SERVER&p=PAIRING&k=KEY&g=SIG&n=Trace%20B";
  if (key === "server" && typeof value === "string") return "http://127.0.0.1:PORT";
  return value;
};
const opOf = new Map(lines.filter((l) => l.dir === "out").map((l) => [`${l.device}:${l.msg.id}`, l.msg.op as string]));
const body = lines.map((l) => {
  const op = opOf.get(`${l.device}:${l.msg.id}`);
  // Ops whose whole result or argument is the record itself.
  const whole = ["createSyncAccount", "removeDevice", "reportLost", "cancelPairing"].includes(op ?? "");
  const msg = JSON.parse(JSON.stringify(l.msg), secret);
  if (whole && l.dir === "in" && msg.result) msg.result = { revision: msg.result.revision ?? 0 };
  return JSON.stringify({ ...l, msg });
});
const out = join(import.meta.dir, "../core/src/test/resources/traces/sync.jsonl");
writeFileSync(out, body.join("\n") + "\n");
console.log(`sync: ${body.length} lines`);
