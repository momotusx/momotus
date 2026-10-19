/** A local stand-in for Trakt's API and auth hosts, for emulator walks of the Trakt card and the player mark
 * (the routes and answers of the core's own fake in crates/mq-runtime/tests/trakt.rs). Nothing here is real
 * Trakt data: one user, two movies, three shows whose watched rows are specials (listed as not matched).
 *
 * The app reaches it through a debug build: `build-core.sh dev`, `adb reverse tcp:<port> tcp:<port>`, and the
 * base URL in the app's `files/dev-trakt`. Controls (GET):
 *   /_login?next=approve|deny|expire|pending   how the next device-code polls answer (default pending)
 *   /_api?mode=ok|reconnect                    reconnect: every call 401 and a refresh 400 invalid_grant
 *   /_seen                                     the requests so far, one per line
 *   /_delay?path=/oauth/device/code&ms=8000    answer that path this late (a screen reader walk turns
 *                                              TalkBack on meanwhile); ms=0 ends it
 * Run: PORT=17920 bun scripts/fake-trakt.ts */

const port = Number(process.env.PORT ?? 17920);
// Each watched show's history answers this late, so "importing" shows for a while.
const showDelayMs = Number(process.env.SHOW_DELAY_MS ?? 2500);

let login: "approve" | "deny" | "expire" | "pending" = "pending";
let api: "ok" | "reconnect" = "ok";
let codes = 0;
const delays = new Map<string, number>();
const seen: string[] = [];

const at = "2026-10-01T20:00:00.000Z";
const activities = {
  all: at,
  movies: { watched_at: at, paused_at: at },
  episodes: { watched_at: at, paused_at: at },
};
const watchedMovies = [
  { plays: 1, last_watched_at: at, movie: { title: "Sintel", year: 2010, ids: { trakt: 11, imdb: "tt1727587" } } },
  { plays: 1, last_watched_at: at, movie: { title: "Home video", year: 2019, ids: { trakt: 12 } } },
];
const shows = [
  { trakt: 21, imdb: "tt0903747", title: "Breaking Bad" },
  { trakt: 22, imdb: "tt0944947", title: "Game of Thrones" },
  { trakt: 23, imdb: "tt2560140", title: "Attack on Titan" },
];
const watchedShows = shows.map((s) => ({
  plays: 1,
  last_watched_at: at,
  last_updated_at: at,
  show: { title: s.title, ids: { trakt: s.trakt, imdb: s.imdb } },
}));
// Specials only (season 0): listed as not matched without asking any addon.
const showHistory = (s: (typeof shows)[number]) => [
  {
    id: s.trakt * 100,
    watched_at: at,
    action: "watch",
    type: "episode",
    episode: { season: 0, number: 1, title: "Special", ids: { trakt: s.trakt * 100, tvdb: s.trakt * 1000 } },
    show: { title: s.title, ids: { trakt: s.trakt, imdb: s.imdb } },
  },
];

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

const tokens = () => ({
  access_token: `walk-access-${Date.now()}`,
  refresh_token: `walk-refresh-${Date.now()}`,
  expires_in: 604_800,
  created_at: Math.floor(Date.now() / 1000),
  scope: "public",
  token_type: "bearer",
});

/** A list read page by page (`limit`, `page`): past the end, an empty page. */
function paged(url: URL, items: unknown[]) {
  const limit = Number(url.searchParams.get("limit") ?? 100);
  const page = Number(url.searchParams.get("page") ?? 1);
  return json(200, items.slice((page - 1) * limit, page * limit));
}

async function answer(req: Request, url: URL, body: any): Promise<Response> {
  const route = `${req.method} ${url.pathname}`;
  if (url.pathname.startsWith("/_")) {
    if (url.pathname === "/_login") login = (url.searchParams.get("next") as typeof login) ?? "pending";
    if (url.pathname === "/_api") api = (url.searchParams.get("mode") as typeof api) ?? "ok";
    if (url.pathname === "/_seen") return new Response(seen.join("\n") + "\n");
    if (url.pathname === "/_delay") delays.set(url.searchParams.get("path") ?? "", Number(url.searchParams.get("ms") ?? 0));
    return json(200, { login, api });
  }
  switch (route) {
    case "POST /oauth/device/code":
      codes++;
      return json(200, {
        device_code: `walk-device-${codes}`,
        user_code: `WALK${String(1000 + codes)}`,
        verification_url: "https://trakt.tv/activate",
        expires_in: Number(process.env.CODE_SECONDS ?? 600),
        interval: 1,
      });
    case "POST /oauth/device/token":
      if (login === "approve") return json(200, tokens());
      if (login === "deny") return json(418, {});
      if (login === "expire") return json(410, {});
      return json(400, {});
    case "POST /oauth/token":
      return api === "reconnect" ? json(400, { error: "invalid_grant" }) : json(200, tokens());
    case "POST /oauth/revoke":
      return json(200, {});
  }
  if (api === "reconnect") return json(401, {});
  if (route === "GET /users/settings") {
    return json(200, { user: { username: "walker", ids: { slug: "walker", uuid: "walk-account-1" } } });
  }
  if (route === "GET /sync/last_activities") return json(200, activities);
  if (route === "GET /sync/watched/movies") return paged(url, watchedMovies);
  if (route === "GET /sync/watched/shows") return paged(url, watchedShows);
  if (route === "GET /sync/playback" || route === "GET /sync/history") return paged(url, []);
  const show = url.pathname.match(/^\/sync\/history\/shows\/(\d+)$/);
  if (req.method === "GET" && show) {
    const s = shows.find((x) => x.trakt === Number(show[1]));
    if (Number(url.searchParams.get("page") ?? 1) === 1) await Bun.sleep(showDelayMs);
    return paged(url, s ? showHistory(s) : []);
  }
  if (req.method === "POST" && url.pathname.startsWith("/scrobble/")) {
    const progress = Number(body?.progress ?? 0);
    const kind = url.pathname.slice("/scrobble/".length);
    const action = kind === "stop" ? (progress >= 80 ? "scrobble" : "pause") : kind;
    const out: Record<string, unknown> = { id: 1, action, progress };
    if (body?.movie) out.movie = { title: body.movie.title ?? "Film", ids: body.movie.ids };
    if (body?.episode) {
      out.episode = { season: 1, number: 1, ids: body.episode.ids };
      out.show = { title: "Show", ids: body.show?.ids ?? {} };
    }
    return json(201, out);
  }
  if (req.method === "POST" && (url.pathname === "/sync/history" || url.pathname === "/sync/history/remove")) {
    const count = (k: string) => (Array.isArray(body?.[k]) ? body[k].length : 0);
    const counts = { movies: count("movies"), episodes: count("episodes") };
    const field = url.pathname.endsWith("remove") ? "deleted" : "added";
    return json(field === "added" ? 201 : 200, { [field]: counts, not_found: { movies: [], episodes: [] } });
  }
  return json(404, {});
}

Bun.serve({
  port,
  hostname: "127.0.0.1",
  idleTimeout: 30,
  async fetch(req) {
    const url = new URL(req.url);
    const text = req.method === "POST" ? await req.text() : "";
    let body: any = null;
    try {
      body = text ? JSON.parse(text) : null;
    } catch {}
    const late = delays.get(url.pathname) ?? 0;
    if (late > 0) await Bun.sleep(late);
    const res = await answer(req, url, body);
    if (!url.pathname.startsWith("/_")) {
      // The progress and action of a scrobble say what the player sent; tokens are never written out.
      const extra = url.pathname.startsWith("/scrobble/") ? ` progress=${body?.progress}` : "";
      const line = `${new Date().toISOString()} ${req.method} ${url.pathname} ${res.status}${extra}`;
      seen.push(line);
      console.log(line);
    }
    return res;
  },
});
console.log(`fake Trakt on http://127.0.0.1:${port}`);
