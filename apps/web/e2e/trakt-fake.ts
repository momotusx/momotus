/** A stand-in for Trakt's API and sign-in for the web e2e, on loopback: the
 * device code flow (approved or refused by the test), the user, empty lists,
 * scrobbles and history writes answered as Trakt counts them. The dev bridge
 * reaches it through MQ_DEV_TRAKT (debug builds only). Its tokens are made up
 * per run; the spec looks for them wherever the page could keep or show one. */

import { createServer } from "node:http";

export interface Seen {
  method: string;
  path: string;
  body: Record<string, unknown>;
}

export interface FakeTrakt {
  url: string;
  /** The tokens it handed out, to look for in storage, the URL and the page. */
  secrets: string[];
  seen: Seen[];
  approve(): void;
  /** The next answers to `route` ("POST /scrobble/pause"), as statuses. */
  answer(route: string, statuses: number[]): void;
  close(): Promise<void>;
}

export const USER_CODE = "MQ7K2P4D";
export const USER = "marquee-tester";

export function startFakeTrakt(port: number): Promise<FakeTrakt> {
  const run = Math.random().toString(36).slice(2, 10);
  const access = `fake-access-${run}`;
  const refresh = `fake-refresh-${run}`;
  const seen: Seen[] = [];
  const queued = new Map<string, number[]>();
  let approved = false;
  const at = "2026-01-01T00:00:00.000Z";

  const server = createServer((req, res) => {
    let raw = "";
    req.on("data", (c: Buffer) => (raw += c.toString()));
    req.on("end", () => {
      const [path = ""] = (req.url ?? "").split("?");
      let body: Record<string, unknown> = {};
      try {
        body = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
      } catch {
        // Not JSON: nothing in it is used.
      }
      seen.push({ method: req.method ?? "", path, body });
      const send = (status: number, value: unknown) => {
        res.writeHead(status, { "content-type": "application/json" });
        res.end(JSON.stringify(value));
      };
      const route = `${req.method} ${path}`;
      const next = queued.get(route)?.shift();
      if (next !== undefined) return send(next, {});
      if (route === "POST /oauth/device/code")
        return send(200, { device_code: `device-${run}`, user_code: USER_CODE, verification_url: "https://trakt.tv/activate", expires_in: 600, interval: 1 });
      if (route === "POST /oauth/device/token")
        return approved
          ? send(200, { access_token: access, refresh_token: refresh, expires_in: 7_776_000, created_at: Math.floor(Date.now() / 1000), scope: "public", token_type: "bearer" })
          : send(400, {});
      if (route === "POST /oauth/revoke") return send(200, {});
      if (route === "GET /users/settings") return send(200, { user: { username: USER, ids: { slug: USER, uuid: `uuid-${run}` } } });
      if (route === "GET /sync/last_activities") return send(200, { all: at, movies: { watched_at: at, paused_at: at }, episodes: { watched_at: at, paused_at: at } });
      if (req.method === "POST" && path.startsWith("/scrobble/")) {
        const progress = Number(body.progress ?? 0);
        const action = path.endsWith("stop") ? (progress >= 80 ? "scrobble" : "pause") : path.slice("/scrobble/".length);
        return send(201, { id: 1, action, progress, ...(body.movie ? { movie: { title: "Film", ids: (body.movie as { ids: unknown }).ids } } : {}) });
      }
      if (req.method === "POST" && (path === "/sync/history" || path === "/sync/history/remove")) {
        const count = (k: string) => (Array.isArray(body[k]) ? (body[k] as unknown[]).length : 0);
        const field = path.endsWith("remove") ? "deleted" : "added";
        return send(field === "added" ? 201 : 200, { [field]: { movies: count("movies"), episodes: count("episodes") }, not_found: { movies: [], episodes: [] } });
      }
      // Every list is empty.
      if (req.method === "GET") return send(200, []);
      send(404, {});
    });
  });

  return new Promise((ok) =>
    server.listen(port, "127.0.0.1", () =>
      ok({
        url: `http://127.0.0.1:${port}`,
        secrets: [access, refresh, `device-${run}`],
        seen,
        approve: () => void (approved = true),
        answer: (route, statuses) => void queued.set(route, [...(queued.get(route) ?? []), ...statuses]),
        close: () => new Promise((done) => server.close(() => done())),
      }),
    ),
  );
}
