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
    // The newest socket is the live one; an older one is closed and would only wait out its answer.
    await line.ask("leaveIncognito").catch(() => undefined);
    await line.ask("ackIncognitoEnded").catch(() => undefined);
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

/** Every in-app link's address on screen that names a page of a title (its route rides in `data-route` instead). */
const routesShown = (page: Page) =>
  page.evaluate(() =>
    [...document.querySelectorAll("a[href]")]
      .map((a) => a.getAttribute("href") ?? "")
      .filter((h) => h.startsWith("/") && (/^\/(title|play)\b|^\/library\/downloads\/./.test(h) || /tt\d|%3A/.test(h))),
  );
/** On this screen: nothing the browser keeps names it, no link shows a title's route, no input keeps its words. */
async function namesNothing(page: Page, where: string) {
  expect(await kept(page), where).not.toMatch(/tt\d|\/title|\/play|thrones/);
  expect(await routesShown(page), where).toEqual([]);
  expect(await page.locator('input:not([autocomplete="off"])').count(), where).toBe(0);
}

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
    if (form === "desktop") {
      // The panel (plans/profile-panel.md): Incognito the last row to switch to, Settings at the foot.
      await expect(page.locator(".pp-row").last()).toHaveAttribute("data-f", "pmenu:incognito");
      await expect(page.locator(".pp-foot .pp-pill").last()).toHaveText("Settings");
    } else {
      // The same rows in both states: the profiles, Incognito, then Settings.
      await expect(page.locator('[role="menu"] [role^="menuitem"]').last()).toHaveText("Settings");
      await expect(page.locator('[data-f="pmenu:incognito"]')).toHaveAttribute("aria-checked", "false");
    }
    await page.locator('[data-f="pmenu:incognito"]').click();
    // The intro, every entry: the two groups, this form's end line, Continue first.
    const intro = page.locator(".incog-intro");
    await expect(intro.locator("h1")).toHaveText("You're Incognito");
    await expect(intro.locator(".incog-group h2")).toHaveText(["Not kept", "Still kept or seen"]);
    await expect(intro).toContainText("Downloads stay on this device, and every profile can see them.");
    await expect(intro).toContainText("Your browser may keep images it showed.");
    const ends = { tv: "It ends when you leave, or after 30 minutes away.", desktop: "It ends when you leave, close this tab, or after 30 minutes without use.", phone: "It ends when you leave, close the app, or after 30 minutes away." };
    await expect(intro.locator(".incog-ends")).toHaveText(ends[form]);
    // TV starts on Continue; elsewhere the heading is read first.
    if (form === "tv") await expect.poll(() => focused(page)).toBe("incognito:continue");
    else await expect(intro.locator("h1")).toBeFocused();
    await page.locator('[data-f="incognito:continue"]').click();
    await expect(menuButton(page)).toHaveAttribute("aria-label", "Profile: Incognito");
    await expect(menuButton(page).locator(".incog-mark")).toBeVisible();
    if (form === "phone") await expect(menuButton(page)).toContainText("Incognito");
    expect(await live()).toBe(1);
    await menuButton(page).click();
    if (form === "desktop") {
      // The panel: Incognito's card with Leave, no Incognito row, the host by name only (no title, no art), the focus
      // on the first profile to switch to.
      const card = page.locator(".pp-card");
      await expect(card).toHaveAttribute("aria-label", "Incognito, current profile");
      await expect(card.locator(".pp-sub")).toHaveText("Nothing you watch here is kept.");
      await expect(card.locator('[data-f="pmenu:leave-incognito"]')).toBeVisible();
      await expect(page.locator('[data-f="pmenu:incognito"]')).toHaveCount(0);
      await expect.poll(() => focused(page)).toBe("pmenu:default");
      await expect(page.locator(".pp-last, .pp-row small, .pp img")).toHaveCount(0);
    } else {
      // The menu: Incognito checked, Leave right under it, the focus on the current row.
      await expect.poll(() => focused(page)).toBe("pmenu:incognito");
      const rows = await page.locator('[role="menu"] [role^="menuitem"]').evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.f));
      expect(rows.slice(-3)).toEqual(["pmenu:incognito", "pmenu:leave-incognito", "pmenu:settings"]);
    }
    await page.locator('[data-f="pmenu:settings"]').click();
    // Settings: the Incognito header, Leave its button, nothing of the account, Downloads, Trakt or Import; This
    // device stays.
    await expect(page.locator(".set-head")).toContainText("Nothing you watch here is kept.");
    await expect(page.locator('[data-f="settings:leave-incognito"]')).toBeVisible();
    await expect(page.locator('[data-f="settings-nav:account"], [data-f="settings-sec:account"]')).toHaveCount(0);
    await expect(page.locator('[data-f="settings-nav:downloads"], [data-f="settings-sec:downloads"]')).toHaveCount(0);
    await expect(page.locator('[data-f="settings-nav:device"], [data-f="settings-sec:device"]')).toHaveCount(1);
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
  // Tab on from the button moves on and the warning stays; the next Tab reaches it again.
  await page.keyboard.press("Tab");
  await expect(stay).not.toBeFocused();
  await expect(alert).toHaveCount(1);
  expect(actives()).toBe(before);
  await page.keyboard.press("Tab");
  await expect(stay).toBeFocused();
  // 3. The button keeps Incognito, by its own press, and the focus goes back to where it was.
  await stay.click();
  await expect(alert).toHaveCount(0);
  await expect.poll(actives).toBe(before + 1);
  expect(await live()).toBe(1);
  // Pressed with Enter from the keyboard, the same, and the focus goes back to where Tab took it from.
  await page.locator('[data-f="nav:/search"]').focus();
  await page.clock.fastForward(61_000);
  await expect.poll(idles).toBe(2);
  line.push({ incognito: "endsSoon" });
  await expect(alert).toBeVisible();
  await page.keyboard.press("Tab");
  await expect(stay).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(alert).toHaveCount(0);
  await expect.poll(actives).toBe(before + 2);
  expect(await focused(page)).toBe("nav:/search");
  // In a third run, any key keeps it too.
  await page.clock.fastForward(61_000);
  await expect.poll(idles).toBe(3);
  line.push({ incognito: "endsSoon" });
  await expect(alert).toBeVisible();
  await page.keyboard.press("Shift");
  await expect(alert).toHaveCount(0);
  await expect.poll(actives).toBe(before + 3);
  // 4. Left alone, the end (here the core's own loud end, a lock) shows the chooser and the ended line before any input.
  await page.clock.fastForward(61_000);
  await expect.poll(idles).toBe(4);
  await line.ask("lockSession");
  await expect(page.locator(".picker h1")).toHaveText("Who's watching?");
  await expect(page.locator(".picker .st")).toHaveText(ENDED);
  expect(await live()).toBe(0);
});

test("a reload ends Incognito, and the next page says so", async ({ browser }) => {
  const { page } = await open(browser, "desktop");
  await enterFromMenu(page);
  // The tab closing is an end the person did not ask for: the line shows, on the chooser or over Home.
  await page.reload();
  await expect(page.getByText(ENDED)).toBeVisible({ timeout: 30_000 });
  expect(await live()).toBe(0);
});

test("over a single profile's Home the ended notice survives reloads and Sync now, and OK reads it", async ({ browser }) => {
  const { page, line } = await open(browser, "desktop");
  await enterFromMenu(page);
  // A loud end; the scope is gone before the reload, so the next session opens the host's Home, not the chooser.
  await line.ask("lockSession");
  await expect(page.locator(".picker .st")).toHaveText(ENDED);
  for (let i = 0; i < 2; i++) {
    await page.reload();
    await expect(page.locator(".incog-notice[role=status]")).toContainText(ENDED, { timeout: 30_000 });
    await expect(page.locator(".picker")).toHaveCount(0);
    // Requests the app sends on its own never clear it.
    await line.ask("syncNow");
    await line.ask("ping");
  }
  await page.locator('[data-f="incognito:ended-ok"]').click();
  await expect(page.getByText(ENDED)).toHaveCount(0);
  await page.reload();
  await expect(menuButton(page)).toBeVisible({ timeout: 30_000 });
  await expect(page.getByText(ENDED)).toHaveCount(0);
});

test("on the chooser the ended line stays across a reload and goes once a profile is chosen", async ({ browser }) => {
  const c = new Client();
  await c.ready;
  // Two profiles: a cold start asks who is watching.
  const other = (await c.req<{ profiles: { id: string; name: string }[] }>("createProfile", { name: "Other", avatar: "letter:sky" })).profiles.find((p) => p.name === "Other")?.id as string;
  try {
    const { page, line } = await open(browser, "desktop");
    await page.locator('[data-f="profile:incognito"]').click();
    await page.locator('[data-f="incognito:continue"]').click();
    await line.ask("lockSession");
    await expect(page.locator(".picker .st")).toHaveText(ENDED);
    await page.reload();
    await expect(page.locator(".picker .st")).toHaveText(ENDED, { timeout: 30_000 });
    await line.ask("syncNow");
    await page.locator('[data-f="profile:default"]').click();
    await expect(menuButton(page)).toHaveAttribute("aria-label", "Profile: Main");
    await expect(page.getByText(ENDED)).toHaveCount(0);
  } finally {
    await c.req("deleteProfile", { profile: other }).catch(() => undefined);
    c.close();
  }
});

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: every screen in Incognito names nothing; Back and Forward work with the address at "/"`, async ({ browser }) => {
    const { page } = await open(browser, form);
    await enterFromMenu(page);
    await expect(page.locator(".row a.tile").first()).toBeVisible({ timeout: 30_000 });
    await namesNothing(page, "home");
    const places = ["/discover", "/library", "/calendar", "/settings", "/settings/sources", "/settings/subtitles", "/addons", ...(form === "tv" ? [] : ["/library/downloads"])];
    for (const place of places) {
      await go(page, place);
      await expect(page.locator("main h1, .set-page-title, .set-title").first()).toBeAttached({ timeout: 30_000 });
      await page.waitForTimeout(500);
      await namesNothing(page, place);
    }
    // A title, then its Versions, each naming nothing; the phone's page starts at its top.
    await go(page, "/");
    const tile = page.locator(".row a.tile[data-route]").first();
    await expect(tile).toBeVisible({ timeout: 30_000 });
    if (form === "phone") {
      await page.evaluate(() => window.scrollTo(0, 600));
      expect(await page.evaluate(() => window.scrollY)).toBeGreaterThan(0);
    }
    await tile.click();
    await expect(page.locator('[data-f="title:versions"]')).toBeVisible({ timeout: 30_000 });
    if (form === "phone") expect(await page.evaluate(() => window.scrollY)).toBe(0);
    const name = await page.locator(".title-scr h1").first().textContent({ timeout: 10_000 });
    await namesNothing(page, "title");
    await page.locator('[data-f="title:versions"]').click();
    await expect(page.locator('[data-f="title:versions"]')).toHaveCount(0, { timeout: 30_000 });
    await page.waitForTimeout(1000);
    await namesNothing(page, "versions");
    // The browser's Back and Forward: the title, then Versions again.
    await page.evaluate(() => history.back());
    await expect(page.locator(".title-scr h1").first()).toHaveText(name ?? "", { timeout: 30_000 });
    await page.evaluate(() => history.forward());
    await expect(page.locator('[data-f="title:versions"]')).toHaveCount(0, { timeout: 30_000 });
    await namesNothing(page, "versions again");
    expect(new URL(page.url()).pathname).toBe("/");
  });
}

test("tv: Back from a title opened in Incognito puts the focus back on its tile", async ({ browser }) => {
  const { page } = await open(browser, "tv");
  await enterFromMenu(page);
  const tile = page.locator(".row a.tile[data-route]").first();
  await expect(tile).toBeVisible({ timeout: 30_000 });
  await tile.focus();
  const key = await tile.getAttribute("data-f");
  await page.keyboard.press("Enter");
  await expect(page.locator('[data-f="title:versions"]')).toBeVisible({ timeout: 30_000 });
  // A move of the viewer's own on the title page: remembered for that page, not for Home.
  await expect.poll(() => focused(page)).not.toBe(key);
  await page.keyboard.press("ArrowRight");
  await page.waitForTimeout(300);
  await page.keyboard.press("Escape");
  await expect.poll(() => focused(page), { timeout: 1500 }).toBe(key);
});
