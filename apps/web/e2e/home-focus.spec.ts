/** Home's first focus on TV (plans/design-v3-phase2-web.md, rule 1): the hero's Play, as tvOS
 * and Android TV, with the page at its top; Continue Watching only when no hero is there or
 * coming. The slow and late cases are made at the socket: the home view's first answer is
 * held back or stripped of its hero, then the real one follows. */

import { type Browser, expect, type Page, test } from "./fixture";
import { BBB_ID } from "./source";

const cw = (id: string) => `tile:home:continue-watching:${id}`;
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
const top = (page: Page) => page.evaluate(() => document.querySelector("[data-vscroll]")?.scrollTop ?? -1);

type Row = { rowId: string; state: { state: string }; tiles: unknown[] };
type Diff = { op: string; value?: Row; values?: Row[]; index?: number; length?: number };
type Update = { kind: string; header?: { heroes: unknown[] }; diffs?: Diff[] };

/** The core's list diff (src/core/diff.ts), to follow Home's real rows at the socket. */
function apply(list: Row[], d: Diff): Row[] {
  const i = d.index ?? 0;
  switch (d.op) {
    case "append":
      return [...list, ...(d.values ?? [])];
    case "clear":
      return [];
    case "pushFront":
      return [d.value as Row, ...list];
    case "pushBack":
      return [...list, d.value as Row];
    case "popFront":
      return list.slice(1);
    case "popBack":
      return list.slice(0, -1);
    case "insert":
      return [...list.slice(0, i), d.value as Row, ...list.slice(i)];
    case "set":
      return list.map((r, j) => (j === i ? (d.value as Row) : r));
    case "remove":
      return [...list.slice(0, i), ...list.slice(i + 1)];
    case "truncate":
      return list.slice(0, d.length);
    default:
      return (d.values ?? []).slice();
  }
}

const loading = (r: Row): Row => (r.rowId === "continue-watching" ? r : { ...r, state: { state: "loading" }, tiles: [] });
const failing = (r: Row): Row => (r.rowId === "continue-watching" ? r : { ...r, state: { state: "failed", reason: "offline" } as Row["state"], tiles: [] });

/** A TV page whose Home is held at the socket for `ms` from its subscription, then shown as it
 * really is (a reset to the real rows and the real header); `ms` Infinity never lets go. "catalogs":
 * the page sees Continue Watching and every catalog still loading, no hero. "hero": every row as it
 * comes, only the hero held back (with Infinity: a Home with no hero by design, as a kids profile's).
 * "failed": Continue Watching and every catalog failed, so no hero. */
async function held(browser: Browser, mode: "catalogs" | "hero" | "failed", ms: number) {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const core = ws.connectToServer();
    let homeId: number | null = null;
    let sub: number | null = null;
    let holding = false;
    let rows: Row[] = [];
    let header: { heroes: unknown[] } = { heroes: [] };
    const shown = (list: Row[]) => (mode === "catalogs" ? list.map(loading) : mode === "failed" ? list.map(failing) : list);
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m));
      if (msg.op === "subscribe" && msg.view === "home") homeId = msg.id;
      core.send(m);
    });
    core.onMessage((m) => {
      const msg = JSON.parse(String(m));
      if (homeId !== null && msg.id === homeId && msg.ok) {
        sub = msg.result.sub;
        holding = true;
        ({ rows, header } = msg.result.snapshot);
        if (ms !== Infinity)
          setTimeout(() => {
            holding = false;
            ws.send(JSON.stringify({ sub, updates: [{ kind: "rows", diffs: [{ op: "reset", values: rows }] }, { kind: "header", header }] }));
          }, ms);
        return void ws.send(JSON.stringify({ ...msg, result: { ...msg.result, snapshot: { header: { heroes: [] }, rows: shown(rows) } } }));
      }
      if (!holding || msg.sub !== sub) return void ws.send(m);
      const updates = msg.updates as Update[];
      const out: Update[] = [];
      for (const u of updates) {
        if (u.kind === "header") header = u.header ?? header;
        else {
          rows = (u.diffs ?? []).reduce(apply, rows);
          // The page keeps the same rows, each catalog shown loading while held: its diffs carry them so.
          out.push({ ...u, diffs: (u.diffs ?? []).map((d) => ({ ...d, value: d.value && shown([d.value])[0], values: d.values && shown(d.values) })) });
        }
      }
      if (out.length) ws.send(JSON.stringify({ ...msg, updates: out }));
    });
  });
  const page = await context.newPage();
  await page.goto("/?form=tv");
  return page;
}

test("TV: Home opens on Play with the page at its top, and Down and Up go to the row and back", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto("/?form=tv");
  await expect(page.locator(`[data-f="${cw(BBB_ID)}"]`)).toBeVisible({ timeout: 30_000 });
  await expect.poll(() => focused(page)).toBe("hero:play");
  // Settled: still Play and still at the top half a second on.
  await page.waitForTimeout(500);
  expect(await focused(page)).toBe("hero:play");
  expect(await top(page)).toBe(0);
  await expect(page.locator('[data-f="hero:play"]')).toBeInViewport({ ratio: 0.99 });
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).toBe(cw(BBB_ID));
  await page.waitForTimeout(400);
  await expect(page.locator(`[data-f="${cw(BBB_ID)}"]`)).toBeInViewport({ ratio: 0.99 });
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => focused(page)).toBe("hero:play");
  await expect.poll(() => top(page)).toBe(0);
  await context.close();
});

test("TV: while the catalogs are on their way nothing takes the focus, then Play does", async ({ browser }) => {
  // Continue Watching answers at once; every catalog is still loading and no hero has come.
  const page = await held(browser, "catalogs", 3000);
  await expect(page.locator(`[data-f="${cw(BBB_ID)}"]`)).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-f="hero:play"]')).toHaveCount(0);
  await page.waitForTimeout(1000);
  expect(await focused(page)).toBe(null);
  expect(await top(page)).toBe(0);
  // A movie row still loading already has its final height (16:9 stills, the 84 caption): nothing below jumps.
  const movies = page.locator('.row[data-fgroup*=":movie:"] .row-body').first();
  const loadingHeight = await movies.evaluate((el) => (el as HTMLElement).offsetHeight);
  await expect.poll(() => focused(page), { timeout: 10_000 }).toBe("hero:play");
  await expect(page.locator('.row[data-fgroup*=":movie:"] .tile').first()).toBeAttached({ timeout: 10_000 });
  expect(await movies.evaluate((el) => (el as HTMLElement).offsetHeight)).toBe(loadingHeight);
  await page.waitForTimeout(500);
  expect(await top(page)).toBe(0);
  await page.context().close();
});

test("TV: a hero that lands after the row took the focus takes it back, with the page at its top", async ({ browser }) => {
  // Every row answered, the hero late: the row takes the focus, then the hero's arrival returns it.
  const page = await held(browser, "hero", 5000);
  await expect.poll(() => focused(page), { timeout: 30_000 }).toBe(cw(BBB_ID));
  await expect.poll(() => focused(page), { timeout: 15_000 }).toBe("hero:play");
  await expect.poll(() => top(page)).toBe(0);
  await page.context().close();
});

test("TV: with no hero and none coming, Continue Watching takes the first focus at once", async ({ browser }) => {
  // Every row answers and no hero comes (a kids profile's Home, or posters only).
  const page = await held(browser, "hero", Infinity);
  // "At once" counts from the rows' first paint, as the other Home specs wait for it, not from a cold page load.
  await expect(page.locator(`[data-f="${cw(BBB_ID)}"]`)).toBeVisible({ timeout: 30_000 });
  await expect.poll(() => focused(page), { timeout: 2000 }).toBe(cw(BBB_ID));
  await expect(page.locator(".hero.closed")).toHaveCount(1);
  expect(await top(page)).toBe(0);
  await page.context().close();
});

test("TV: with every catalog failed, Continue Watching takes the first focus at once", async ({ browser }) => {
  const page = await held(browser, "failed", Infinity);
  await expect(page.locator(`[data-f="${cw(BBB_ID)}"]`)).toBeVisible({ timeout: 30_000 });
  await expect.poll(() => focused(page), { timeout: 2000 }).toBe(cw(BBB_ID));
  await page.context().close();
});

test("desktop: a key on a fresh Home focuses Play with the page at its top", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto("/?form=desktop");
  await expect(page.locator(`[data-f="${cw(BBB_ID)}"]`)).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-f="hero:play"]')).toBeVisible();
  // Off the TV nothing is focused until a key says the viewer uses the keyboard.
  expect(await focused(page)).toBe(null);
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).toBe("hero:play");
  expect(await top(page)).toBe(0);
  await context.close();
});

/** QA r5, R5-13: Tab onto a row's tile that is only partly in view (Top 10's ninth, 16 px of 153 on a 1440 window)
 * scrolls the row until the whole tile shows. */
test("desktop: Tab onto a Top 10 tile at the row's right edge brings the whole tile into view", async ({ browser }) => {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.goto("/?form=desktop");
  await expect(page.locator(".row .tile img.in").first()).toBeVisible({ timeout: 30_000 });
  const topRow = '[data-fgroup*=":movie:top"] [data-f^="tile:"]';
  await expect(page.locator(topRow)).not.toHaveCount(0);
  const visible = () =>
    page.evaluate(() => {
      const r = (document.activeElement as HTMLElement).getBoundingClientRect();
      return r.left >= 0 && r.right <= innerWidth;
    });
  let seen = 0;
  for (let i = 0; i < 60 && seen < 10; i++) {
    await page.keyboard.press("Tab");
    if (!(await page.evaluate(() => (document.activeElement as HTMLElement).dataset.f?.includes(":movie:top:") ?? false))) continue;
    seen++;
    await expect.poll(visible, { message: `Top 10 tile ${seen} fully in view` }).toBe(true);
  }
  expect(seen).toBe(10);
  await page.close();
});

/** QA r5, R5-12: the phone's page scrolls the document, so its rows were all "near" and mounted every tile at once (361
 * poster requests in 15 s on a slow link, starving the hero's art). A Home of about 500 tiles mounts only the rows near
 * the screen while the first hero's art loads, asks for no other poster before it, and mounts the rest once it is in,
 * so a later fling meets rows already there (home-perf's phone fling). */
test("phone: a Home of 500 tiles mounts only the near rows until the hero's art is in, then the rest", async ({ browser }) => {
  type Tile = { poster?: string | null; posterFallback?: string | null };
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  const tag = (u: unknown, n: number) => (typeof u === "string" ? `${u}${u.includes("?") ? "&" : "?"}q=${n}` : u);
  const expand = (rows: Row[]) =>
    rows.flatMap((r) =>
      r.rowId === "continue-watching" || r.tiles.length === 0
        ? [r]
        : Array.from({ length: 12 }, (_, n) => ({ ...r, rowId: `${r.rowId}~${n}`, title: `${(r as Row & { title: string }).title} ${n}`, tiles: (r.tiles as Tile[]).map((t) => ({ ...t, poster: tag(t.poster, n), posterFallback: tag(t.posterFallback, n) })) })),
    );
  // The heroes' art, as the socket names it: held below, a slow link's first picture.
  const heroArt = new Set<string>();
  const noteHeroes = (heroes: unknown[] | undefined) =>
    heroes?.forEach((h) => {
      const art = (h as { tile: { background: string | null } }).tile.background;
      if (art) heroArt.add(art);
    });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const core = ws.connectToServer();
    let homeId: number | null = null;
    let sub: number | null = null;
    let rows: Row[] = [];
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m));
      if (msg.op === "subscribe" && msg.view === "home") homeId = msg.id;
      core.send(m);
    });
    core.onMessage((m) => {
      const msg = JSON.parse(String(m));
      if (homeId !== null && msg.id === homeId && msg.ok) {
        sub = msg.result.sub;
        rows = msg.result.snapshot.rows;
        noteHeroes(msg.result.snapshot.header?.heroes);
        return void ws.send(JSON.stringify({ ...msg, result: { ...msg.result, snapshot: { ...msg.result.snapshot, rows: expand(rows) } } }));
      }
      if (msg.sub !== sub || !msg.updates) return void ws.send(m);
      const updates = (msg.updates as Update[]).map((u) => {
        noteHeroes(u.header?.heroes);
        if (u.kind !== "rows") return u;
        rows = (u.diffs ?? []).reduce(apply, rows);
        return { ...u, diffs: [{ op: "reset", values: expand(rows) }] };
      });
      ws.send(JSON.stringify({ ...msg, updates }));
    });
  });
  // Every image request in order; the heroes' art waits until the test lets it in.
  const images: string[] = [];
  let letArtIn = () => {};
  const artHeld = new Promise<void>((done) => (letArtIn = done));
  await context.route("**/*", async (route) => {
    const r = route.request();
    if (r.resourceType() === "image") {
      images.push(r.url());
      if (heroArt.has(r.url())) await artHeld;
    }
    await route.fallback();
  });
  const page = await context.newPage();
  await page.goto("/?form=phone");
  const art = page.locator(".hero-bd img").first();
  await expect(art).toHaveAttribute("fetchpriority", "high", { timeout: 30_000 });
  await expect.poll(() => page.locator("[data-row]").count(), { timeout: 30_000 }).toBeGreaterThan(40);
  await page.waitForTimeout(500);
  // While the art loads: only the rows near the screen have tiles, and no poster but theirs was asked for before it.
  const src = await art.evaluate((e) => (e as HTMLImageElement).src);
  const first = images.indexOf(src);
  expect(first, `the hero's art ${src} was not asked for`).toBeGreaterThanOrEqual(0);
  // A copied row's posters carry its copy (`q=n` for row `~n`): the copies with tiles now are the near ones.
  const keys = await page.locator('.row [data-f^="tile:"]').evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.f ?? ""));
  const near = new Set(keys.map((f) => /~(\d+):[^:]+$/.exec(f)?.[1]));
  const mounted = await page.locator(".row .tile").count();
  expect(mounted, `${mounted} tiles mounted while the hero's art loads`).toBeLessThan(120);
  const early = images.slice(0, first).filter((u) => {
    const copy = /[?&]q=(\d+)/.exec(u)?.[1];
    return copy !== undefined && !near.has(copy);
  });
  expect(early, "posters of far rows asked for before the hero's art").toEqual([]);
  // Once the art is in, the far rows mount too, ready for a fling.
  letArtIn();
  await expect.poll(() => page.locator(".row .tile").count(), { timeout: 15_000 }).toBeGreaterThan(200);
  await context.close();
});
