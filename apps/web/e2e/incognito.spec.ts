/** Incognito on the web (plans/batch-10-incognito.md 7.4), against the shared core: entry from the chooser and the
 * bar's menu on every form, an address and history that name nothing, links that carry their route out of sight,
 * trailers and outside links off, what Leave leaves behind, the desktop's idle warning and the ended line across
 * reloads. Each page reaches the core through a routed socket, which the teardown uses to end any scope, pass or
 * fail, so the next spec's page never starts behind the chooser (the guard in e2e/fixture.ts checks). */

import { type Browser, type BrowserContext, expect, type Page, type WebSocketRoute, test } from "./fixture";
import { BBB_ID } from "./source";
import { Client, startIfPaused } from "./core";
import { fakeYouTube, ytLog } from "./yt-fake";

const FORMS = { tv: [1920, 1080, 1], desktop: [1440, 900, 1], phone: [390, 844, 2] } as const;
type Form = keyof typeof FORMS;
const ENDED = "Incognito ended. Its history and addons were cleared.";

/** One page's sockets to the core: what the page sent, and requests or pushes the test adds. */
class Line {
  servers: WebSocketRoute[] = [];
  pages: WebSocketRoute[] = [];
  sent: { op?: string; event?: string; view?: string }[] = [];
  private waiters = new Map<number, () => void>();
  private next = 9_000_000;
  /** Sends `op` to the core on the newest socket, as if the page had, and waits for the answer (the page drops it). */
  ask(op: string, fields: Record<string, unknown> = {}, server = this.servers.at(-1)) {
    const id = this.next++;
    return new Promise<void>((done) => {
      this.waiters.set(id, done);
      server?.send(JSON.stringify({ id, op, ...fields }));
      setTimeout(done, 5000);
    });
  }
  /** A push from the core to the page, as the core would send it. */
  push(message: object) {
    this.pages.at(-1)?.send(JSON.stringify(message));
  }
  heard(text: string) {
    const id = (JSON.parse(text) as { id?: number }).id;
    if (id !== undefined && this.waiters.has(id)) {
      this.waiters.get(id)?.();
      this.waiters.delete(id);
    }
  }
}

const lines = new Map<BrowserContext, Line>();

async function open(browser: Browser, form: Form, opts: { clock?: boolean; trailers?: boolean } = {}) {
  const [width, height, scale] = FORMS[form];
  // With trailers allowed, motion is too, so only Incognito keeps them off.
  const reducedMotion = opts.trailers ? "no-preference" : "reduce";
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, isMobile: form === "phone", hasTouch: form === "phone", reducedMotion });
  const line = new Line();
  lines.set(context, line);
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    line.servers.push(server);
    line.pages.push(ws);
    ws.onMessage((m) => {
      line.sent.push(JSON.parse(String(m)));
      server.send(m);
    });
    server.onMessage((m) => {
      line.heard(String(m));
      ws.send(m);
    });
  });
  // Trailers allowed in this run (lib/trailers.ts `walkNow`), so their absence is Incognito's doing.
  if (opts.trailers) {
    await context.addInitScript(() => localStorage.setItem("mq.trailerPreviews", "e2e"));
    await fakeYouTube(context);
  }
  const page = await context.newPage();
  if (opts.clock) await page.clock.install();
  await page.goto(`/?form=${form}`);
  await expect(page.locator('[data-f="nav:/profiles"]').or(page.locator(".picker"))).toBeVisible({ timeout: 30_000 });
  return { context, page, line };
}

// Pass or fail, every scope this spec opened ends, quietly, and an end not yet read is read.
test.afterEach(async () => {
  for (const [context, line] of lines) {
    for (const server of line.servers) {
      await line.ask("leaveIncognito", {}, server).catch(() => undefined);
      await line.ask("ackIncognitoEnded", {}, server).catch(() => undefined);
    }
    await context.close();
  }
  lines.clear();
});

const menuButton = (page: Page) => page.locator('[data-f="nav:/profiles"]');
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
/** What the browser itself keeps: the address, the tab title and the entry's state (never the app's memory). */
const kept = (page: Page) =>
  page.evaluate(() => {
    const get = Object.getOwnPropertyDescriptor(History.prototype, "state")?.get;
    return JSON.stringify([location.href, document.title, get?.call(history)]);
  });
const live = async () => {
  const c = new Client();
  try {
    await c.ready;
    return (await c.req<{ liveScopes: number }>("ping")).liveScopes;
  } finally {
    c.close();
  }
};

/** Opens `route` as an in-app link does in Incognito: an anchor saying "/" with its route in `data-route`. */
const go = (page: Page, route: string) =>
  page.evaluate((to) => {
    const a = document.createElement("a");
    a.setAttribute("link", "");
    a.href = "/";
    a.dataset.route = to;
    document.body.append(a);
    a.click();
    a.remove();
  }, route);

/** Enters from the bar's menu and passes the intro. */
async function enterFromMenu(page: Page) {
  await menuButton(page).click();
  await page.locator('[data-f="pmenu:incognito"]').click();
  await expect(page.locator(".incog-intro h1")).toHaveText("You're Incognito");
  await page.locator('[data-f="incognito:continue"]').click();
  await expect(menuButton(page)).toHaveAttribute("aria-label", "Profile: Incognito");
}

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: the menu enters Incognito; its mark takes the bar's slot, which opens the menu and Settings`, async ({ browser }) => {
    const { page } = await open(browser, form);
    await menuButton(page).click();
    // The same rows in both states: the profiles, Incognito, then Settings.
    await expect(page.locator('[role="menu"] [role^="menuitem"]').last()).toHaveText("Settings");
    await expect(page.locator('[data-f="pmenu:incognito"]')).toHaveAttribute("aria-checked", "false");
    await page.locator('[data-f="pmenu:incognito"]').click();
    // The intro, every entry: the two groups, this form's end line, Continue first.
    const intro = page.locator(".incog-intro");
    await expect(intro.locator("h1")).toHaveText("You're Incognito");
    await expect(intro.locator(".incog-group h2")).toHaveText(["Not kept", "Still kept or seen"]);
    await expect(intro).toContainText("Downloads stay on this device, and every profile can see them.");
    await expect(intro).toContainText("Your browser may keep images it showed.");
    const ends = { tv: "It ends when you leave, or after 30 minutes away.", desktop: "It ends when you leave, close this tab, or after 30 minutes without use.", phone: "It ends when you leave, close the app, or after 30 minutes away." };
    await expect(intro.locator(".incog-ends")).toHaveText(ends[form]);
    if (form === "tv") await expect.poll(() => focused(page)).toBe("incognito:continue");
    await page.locator('[data-f="incognito:continue"]').click();
    await expect(menuButton(page)).toHaveAttribute("aria-label", "Profile: Incognito");
    await expect(menuButton(page).locator(".incog-mark")).toBeVisible();
    if (form === "phone") await expect(menuButton(page)).toContainText("Incognito");
    expect(await live()).toBe(1);
    // The menu: Incognito checked, Leave right under it, the focus on the current row.
    await menuButton(page).click();
    await expect.poll(() => focused(page)).toBe("pmenu:incognito");
    const rows = await page.locator('[role="menu"] [role^="menuitem"]').evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.f));
    expect(rows.slice(-3)).toEqual(["pmenu:incognito", "pmenu:leave-incognito", "pmenu:settings"]);
    await page.locator('[data-f="pmenu:settings"]').click();
    // Settings: the Incognito header, Leave its button, and nothing of the account, Downloads or this device.
    await expect(page.locator(".set-head")).toContainText("Nothing you watch here is kept.");
    await expect(page.locator('[data-f="settings:leave-incognito"]')).toBeVisible();
    await expect(page.locator('[data-f="settings-nav:account"], [data-f="settings-sec:account"]')).toHaveCount(0);
    await expect(page.locator('[data-f="settings-nav:device"], [data-f="settings-sec:device"]')).toHaveCount(0);
    await expect(page.locator('[data-f="settings:trakt"], [data-f="settings:import"]')).toHaveCount(0);
    await page.locator('[data-f="settings:leave-incognito"]').click();
    await expect(menuButton(page)).toHaveAttribute("aria-label", "Profile: Main");
    expect(await live()).toBe(0);
  });
}

test("desktop: in Incognito the address, the title and history name nothing, and Back and Forward still work", async ({ browser }) => {
  const { page, context } = await open(browser, "desktop", { trailers: true });
  // What the host holds before: Continue Watching, recent searches and its addons, read from the core.
  await expect(page.locator(".row").first()).toHaveAttribute("aria-label", "Continue Watching", { timeout: 30_000 });
  const hostHolds = async () => {
    const c = new Client();
    await c.ready;
    try {
      type Row = { rowId: string; tiles: { id: string; progress: number | null; videoId: string | null }[] };
      const home = (await c.subscribe<{ rows: Row[] }>("home")).now();
      const search = (await c.subscribe<{ recent: string[] }>("search")).now();
      const addons = (await c.subscribe<{ addons: unknown[] }>("addons")).now();
      const cw = home.rows.find((r) => r.rowId === "continue-watching")?.tiles.map((t) => [t.id, t.progress, t.videoId]);
      return JSON.stringify({ cw, recent: search.recent, addons: addons.addons });
    } finally {
      c.close();
    }
  };
  const before = await hostHolds();
  await enterFromMenu(page);
  // Home holds no Continue Watching row, and its hero plays no trailer.
  await expect(page.locator(".row").first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('.row[aria-label="Continue Watching"]')).toHaveCount(0);
  await page.waitForTimeout(4000);
  await expect(page.locator("iframe")).toHaveCount(0);
  expect(await kept(page)).not.toMatch(/tt\d|title|search/);
  // A title link says "/" and carries its route; a click opens the title, not Home.
  const tile = page.locator('.row a.tile[data-route^="/title/"]').first();
  await expect(tile).toHaveAttribute("href", "/");
  // A middle click opens "/" in a new tab, which shows the chooser: a new session behind a live scope starts locked.
  const [tab] = await Promise.all([context.waitForEvent("page", { timeout: 10_000 }), tile.click({ button: "middle" })]);
  await tab.waitForLoadState();
  expect(new URL(tab.url()).pathname).toBe("/");
  await expect(tab.locator(".picker h1")).toHaveText("Who's watching?", { timeout: 30_000 });
  await tab.close();
  const route = (await tile.getAttribute("data-route")) as string;
  const id = decodeURIComponent(route.split("/")[3] ?? "");
  await tile.click();
  await expect(page.locator('[data-f="title:versions"]')).toBeVisible({ timeout: 30_000 });
  const heading = () => page.locator(".title-scr h1").first().textContent({ timeout: 10_000 });
  const onHome = () => page.locator(".row").first().isVisible();
  await expect(page).toHaveTitle("Incognito · Momotus");
  expect(await kept(page)).not.toContain(id);
  expect(await kept(page)).not.toContain("/title");
  expect(page.url()).toMatch(/\/(\?form=desktop)?$/);
  // No trailer plays and no Trailer button; scores are plain text.
  await page.waitForTimeout(3000);
  await expect(page.locator("iframe")).toHaveCount(0);
  expect(await ytLog(page)).toEqual([]);
  await expect(page.locator('[data-f="title:trailer"]')).toHaveCount(0);
  await expect(page.locator(".scores a")).toHaveCount(0);
  // The app's Back to Home, then Search: the palette, a word, a result.
  await page.locator('[data-f="back"]').click();
  await expect.poll(onHome).toBe(true);
  await page.locator('[data-f="nav:/search"]').click();
  await page.locator('[data-f="search:input"]').fill("thrones");
  await expect(page.locator(".hit").first()).toBeVisible({ timeout: 30_000 });
  expect(await kept(page)).not.toContain("thrones");
  // Every input on the way keeps nothing for the browser to suggest.
  expect(await page.locator('input:not([autocomplete="off"])').count()).toBe(0);
  await page.locator(".hit").first().click({ timeout: 15_000 });
  await expect(page.locator('[data-f="title:versions"]')).toBeVisible({ timeout: 30_000 });
  const result = await heading();
  // The browser's Back: the palette's entry (it opens again over Home), then Home; Forward twice: the search, then
  // the result. The address stays "/" throughout.
  await page.evaluate(() => history.back());
  await expect(page.locator(".palette-layer")).toBeVisible();
  await page.evaluate(() => history.back());
  await expect(page.locator(".palette-layer")).toHaveCount(0);
  await expect.poll(onHome).toBe(true);
  expect(page.url()).toMatch(/\/(\?form=desktop)?$/);
  await page.evaluate(() => history.forward());
  await expect(page.locator(".palette-layer")).toBeVisible();
  await page.evaluate(() => history.forward());
  await expect.poll(heading).toBe(result);
  expect(page.url()).toMatch(/\/(\?form=desktop)?$/);
  expect(await kept(page)).not.toMatch(/tt\d|\/title|thrones/);
  // Play a film for a while (headless, so silent): its progress is the scope's.
  await go(page, `/title/movie/${BBB_ID}`);
  await page.locator('[data-f="title:play"]').click({ timeout: 30_000 });
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await startIfPaused(page);
  const time = () => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0);
  const from = await time();
  await expect.poll(time, { timeout: 30_000 }).toBeGreaterThan(from + 6);
  expect(await kept(page)).not.toContain(BBB_ID);
  // An addon turned off in Incognito, from Settings > Addons.
  await go(page, "/settings/sources");
  await page.locator('[data-f="settings:addons"]').click();
  await expect(page.locator(".lede + .note")).toHaveText("Changes here last until you leave Incognito.");
  expect(await page.locator('input:not([autocomplete="off"])').count()).toBe(0);
  await page.locator('[data-f="addon:0:enabled"]').click();
  // Leave: the host's Continue Watching, recent searches and addons are as they were.
  await menuButton(page).click();
  await page.locator('[data-f="pmenu:leave-incognito"]').click();
  await expect(menuButton(page)).toHaveAttribute("aria-label", "Profile: Main");
  expect(await hostHolds()).toBe(before);
  await expect(page.locator(".row").first()).toHaveAttribute("aria-label", "Continue Watching", { timeout: 30_000 });
});

test("desktop: a download asked in Incognito says it stays on this device", async ({ browser }) => {
  const { page } = await open(browser, "desktop");
  await enterFromMenu(page);
  await go(page, `/title/movie/${BBB_ID}`);
  const download = page.locator('[data-f="title:download"]');
  await expect(download).toBeVisible({ timeout: 30_000 });
  await download.click();
  await expect(page.locator(".dl-card")).toContainText("This download stays on this device. Every profile can see it in Downloads.");
  await page.keyboard.press("Escape");
});

test("the chooser enters Incognito, and Leave goes back to it without a lock line", async ({ browser }) => {
  const c = new Client();
  await c.ready;
  const other = (await c.req<{ profiles: { id: string; name: string }[] }>("createProfile", { name: "Other", avatar: "letter:sky" })).profiles.find((p) => p.name === "Other")?.id as string;
  try {
    const { page } = await open(browser, "desktop");
    // The tile sits after the profiles, before Add profile.
    const tiles = await page.locator(".pgrid > *").evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.f));
    expect(tiles.indexOf("profile:incognito")).toBe(tiles.filter((t) => t?.startsWith("profile:") && t !== "profile:add").length - 1);
    await page.locator('[data-f="profile:incognito"]').click();
    await page.locator('[data-f="incognito:continue"]').click();
    await expect(menuButton(page)).toHaveAttribute("aria-label", "Profile: Incognito");
    await menuButton(page).click();
    await page.locator('[data-f="pmenu:leave-incognito"]').click();
    // Back to the chooser over the host, which has no PIN: no "This profile is locked".
    await expect(page.locator(".picker h1")).toHaveText("Who's watching?");
    await expect(page.locator(".picker .lede")).toHaveCount(0);
    await expect(page.locator(".picker .st")).toHaveText("");
    expect(await live()).toBe(0);
  } finally {
    await c.req("deleteProfile", { profile: other }).catch(() => undefined);
    c.close();
  }
});

test("desktop: a minute idle sends idle; the warning's button is one Tab away and keeps Incognito; left alone it ends", async ({ browser }) => {
  const { page, line } = await open(browser, "desktop", { clock: true });
  await enterFromMenu(page);
  const idles = () => line.sent.filter((m) => m.op === "lifecycle" && m.event === "idle").length;
  const actives = () => line.sent.filter((m) => m.op === "lifecycle" && m.event === "active").length;
  // 1. A minute without input sends idle, once.
  await page.clock.fastForward(61_000);
  await expect.poll(idles).toBe(1);
  // 2. The core's warning: announced, its button not focused, and one Tab reaches it without sending active.
  const before = actives();
  line.push({ incognito: "endsSoon" });
  const alert = page.getByRole("alert");
  await expect(alert).toHaveText(/Incognito ends in 1 minute\./);
  await expect(page.getByRole("alert")).toHaveCount(1);
  const stay = page.getByRole("button", { name: "Stay in Incognito" });
  await expect(stay).not.toBeFocused();
  await page.keyboard.press("Tab");
  await expect(stay).toBeFocused();
  expect(actives()).toBe(before);
  // 3. The button keeps Incognito.
  await page.keyboard.press("Enter");
  await expect(alert).toHaveCount(0);
  await expect.poll(actives).toBe(before + 1);
  expect(await live()).toBe(1);
  // In a second run, any key keeps it too.
  await page.clock.fastForward(61_000);
  await expect.poll(idles).toBe(2);
  line.push({ incognito: "endsSoon" });
  await expect(alert).toBeVisible();
  await page.keyboard.press("Shift");
  await expect(alert).toHaveCount(0);
  await expect.poll(actives).toBe(before + 2);
  // 4. Left alone, the end (here the core's own loud end, a lock) shows the chooser and the ended line before any input.
  await page.clock.fastForward(61_000);
  await expect.poll(idles).toBe(3);
  await line.ask("lockSession");
  await expect(page.locator(".picker h1")).toHaveText("Who's watching?");
  await expect(page.locator(".picker .st")).toHaveText(ENDED);
  expect(await live()).toBe(0);
});

test("the ended line survives two reloads before any click, and clears only after one", async ({ browser }) => {
  const { page, line } = await open(browser, "desktop");
  await enterFromMenu(page);
  // A reload ends it: the tab closing is an end the person did not ask for.
  await page.reload();
  await expect(page.getByText(ENDED)).toBeVisible({ timeout: 30_000 });
  expect(await live()).toBe(0);
  await page.reload();
  await expect(page.getByText(ENDED)).toBeVisible({ timeout: 30_000 });
  // Requests the app sends on its own never clear it.
  await line.ask("syncNow");
  await line.ask("ping");
  await page.reload();
  await expect(page.getByText(ENDED)).toBeVisible({ timeout: 30_000 });
  // Read: OK over Home, or a profile on the chooser.
  const ok = page.locator('[data-f="incognito:ended-ok"]');
  if (await ok.isVisible()) await ok.click();
  else await page.locator('[data-f="profile:default"]').click();
  await expect(page.getByText(ENDED)).toHaveCount(0);
  await page.reload();
  await expect(menuButton(page)).toBeVisible({ timeout: 30_000 });
  await expect(page.getByText(ENDED)).toHaveCount(0);
});
