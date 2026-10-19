/** A second sync device for Android walks: a host `mq-bridge` core driven over a small local HTTP
 * API, so a walk can pair the emulator with it, sync an addon across, make and redeem a kit and
 * report devices lost, step by step with curl. The account record stays in this process's memory
 * and is never printed: answers show codes, counts, ids and names only.
 *
 * Needs: a sync server (crates/mq-sync-server, `--listen 127.0.0.1:<port>`) and a bridge
 * (`MQ_DATA=<empty dir> MQ_BRIDGE_PORT=<port> mq-bridge`).
 * Run: bun scripts/sync-peer.ts <bridge port> <control port>
 *
 * POST /create {server, name}           start an account here (the first device)
 * POST /pair {server, name}             show a code to join an existing account; GET /pairing reads its state
 * POST /confirm                         the digits match: join
 * POST /approve {qr}                    approve another device's code; answers {code}
 * POST /recover {server, kit, name}     join with a recovery kit
 * POST /kit                             make a recovery kit; answers {kit} (a throwaway walk account's)
 * POST /sync                            one round (going again while the core asks); answers the report's facts
 * GET  /devices                         the device list without the record
 * POST /lost {lost, keep?}              report a device lost; POST /preview {lost} shows what goes
 * POST /install {url}, GET /addons      an addon here, and the addons this device has
 */

const [bridgePort, controlPort] = process.argv.slice(2).map(Number);
if (!bridgePort || !controlPort) throw new Error("usage: bun scripts/sync-peer.ts <bridge port> <control port>");

type Json = Record<string, any>;
const socket = new WebSocket(`ws://127.0.0.1:${bridgePort}/ws`);
await new Promise<void>((ok, fail) => {
  socket.addEventListener("open", () => ok());
  socket.addEventListener("error", () => fail(new Error("cannot reach the bridge")));
});

let nextId = 1;
const waiting = new Map<number, (m: Json) => void>();
const latest = new Map<number, Json>();
socket.addEventListener("message", (event) => {
  const msg = JSON.parse(String(event.data)) as Json;
  if (typeof msg.sub === "number" && Array.isArray(msg.updates)) latest.set(msg.sub, msg.updates[msg.updates.length - 1]);
  else if (typeof msg.id === "number") waiting.get(msg.id)?.(msg);
});

async function request(op: string, fields: Json = {}): Promise<any> {
  const id = nextId++;
  const reply = await new Promise<Json>((ok) => {
    waiting.set(id, ok);
    socket.send(JSON.stringify({ id, op, ...fields }));
  });
  waiting.delete(id);
  if (!reply.ok) throw Object.assign(new Error(reply.error), { code: reply.code });
  return reply.result;
}

/** The keystore record: memory only. */
let record: Json | null = null;
const revision = (r: Json | null) => (typeof r?.revision === "number" ? r.revision : 0);
const keep = (next: Json | null | undefined) => {
  if (next && revision(next) >= revision(record)) record = next;
};

let pairing: { sub: number; snapshot: Json } | null = null;
const pairingNow = () => (pairing ? (latest.get(pairing.sub) ?? pairing.snapshot) : null);

async function sync() {
  let last: Json = {};
  for (let i = 0; i < 5; i++) {
    last = await request("syncNow", { record });
    keep(last.record);
    if (!last.again) break;
  }
  const { record: _, ...facts } = last;
  return facts;
}

const routes: Record<string, (body: Json) => Promise<unknown>> = {
  "POST /create": async (b) => {
    record = await request("createSyncAccount", { server: b.server, name: b.name });
    return { joined: true };
  },
  "POST /pair": async (b) => {
    // Joining replaces the account here: the old record's revision must not outrank the new one's.
    record = null;
    pairing = await request("subscribe", { view: "pairing", params: { server: b.server, name: b.name } });
    return { started: true };
  },
  "GET /pairing": async () => {
    const s = pairingNow();
    if (s?.phase === "joined") {
      keep(s.record);
      return { phase: "joined" };
    }
    return s;
  },
  "POST /confirm": async () => request("command", { sub: pairing?.sub, command: { type: "confirm" } }),
  "POST /approve": async (b) => {
    const r = await request("approvePairing", { record, qr: b.qr });
    keep(r.record);
    return { code: r.code };
  },
  "POST /recover": async (b) => {
    record = await request("recover", { server: b.server, kit: b.kit, name: b.name });
    return { joined: true };
  },
  "POST /kit": async () => {
    const r = await request("createRecoveryKit", { record });
    keep(r.record);
    return { kit: r.kit };
  },
  "POST /sync": sync,
  "GET /devices": async () => {
    const d = await request("syncDevices", { record });
    keep(d.record);
    const { record: _, ...list } = d;
    return list;
  },
  "POST /preview": async (b) => request("lostPreview", { record, lost: b.lost }),
  "POST /lost": async (b) => {
    keep(await request("reportLost", { record, lost: b.lost, keep: b.keep ?? [] }));
    return { done: true };
  },
  "POST /install": async (b) => request("installAddon", { url: b.url }),
  "GET /addons": async () => {
    const view = await request("subscribe", { view: "addons", params: {} });
    await request("unsubscribe", { sub: view.sub }).catch(() => undefined);
    return view.snapshot.addons.map((a: Json) => ({ name: a.name, host: a.host, enabled: a.enabled }));
  },
};

Bun.serve({
  hostname: "127.0.0.1",
  port: controlPort,
  async fetch(req) {
    const route = routes[`${req.method} ${new URL(req.url).pathname}`];
    if (!route) return new Response("no such step\n", { status: 404 });
    try {
      const body = req.method === "POST" ? await req.json().catch(() => ({})) : {};
      return Response.json(await route(body));
    } catch (e) {
      const err = e as Error & { code?: string };
      return Response.json({ error: err.message, code: err.code }, { status: 500 });
    }
  },
});
console.log(`sync_peer_ready ${controlPort}`);
