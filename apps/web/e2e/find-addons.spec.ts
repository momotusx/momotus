import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { ADDON_LISTS_PORT, Client, cleanup, PORTS, SHOTS, startBridge } from "./core";

// The live Official list (plans/m12-addon-discovery.md). The setup installed
// Cinemeta and OpenSubtitles v3; WatchHub is not installed.
const card = (page: Page, name: string) => page.locator(".fa-card").filter({ has: page.getByRole("heading", { name, exact: true }) });
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);

test("desktop: cards name their action, Community shows its note", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/find-addons?form=desktop");
  await expect(page.getByRole("radio", { name: /Official/ })).toHaveAttribute("aria-checked", "true");
  await expect(card(page, "OpenSubtitles v3").locator(".fa-state.on")).toHaveText("Installed");
  await expect(card(page, "OpenSubtitles v3").getByRole("button")).toHaveCount(0);
  await expect(card(page, "WatchHub").getByRole("button", { name: "Install WatchHub, streams" })).toBeVisible();

  await page.getByRole("radio", { name: /Community/ }).click();
  await expect(page.locator(".fa-note")).toContainText("Community addons are made by other people");
  // Every Configure link says which addon it sets up.
  const configure = page.locator('a[data-f$=":configure"]');
  await expect(configure.first()).toBeVisible();
  for (const label of await configure.evaluateAll((links) => links.map((a) => a.getAttribute("aria-label") ?? ""))) expect(label).toMatch(/^Configure \S/);
  // No link of an entry is ever in the page: they may hold an addon's keys.
  const hrefs = await page.locator(".fa-card a").evaluateAll((links) => links.map((a) => a.getAttribute("href") ?? ""));
  for (const href of hrefs) expect(href.endsWith("manifest.json")).toBe(false);
});

test("a late install result lands on its own card after the page closed and opened again", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  // Hold installListed on its way to the core, then answer it from here once
  // the page has been left and opened again: nothing is installed.
  let answer = (_reply: object) => {};
  await page.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m));
      if (msg.op === "installListed") answer = (reply) => ws.send(JSON.stringify({ id: msg.id, ...reply }));
      else server.send(m);
    });
    server.onMessage((m) => ws.send(m));
  });
  await page.goto("/find-addons?form=desktop");
  const install = () => card(page, "WatchHub").getByRole("button", { name: /WatchHub/ });
  await install().focus();
  await page.keyboard.press("Enter");
  // The pressed button stays, busy, and keeps keyboard focus.
  await expect(install()).toHaveAttribute("aria-busy", "true");
  await expect(install()).toHaveText("Installing");
  await expect(install()).toBeFocused();

  // Leave through the app (the page closes its view), then come back.
  await page.locator('a[href="/settings"]').first().click();
  await expect(page).toHaveURL(/\/settings/);
  await page.locator('[data-f="settings:find-addons"]').click();
  await expect(install()).toHaveText("Installing");

  answer({ ok: false, error: "This addon's server did not answer in time", code: "timeout" });
  await expect(card(page, "WatchHub").locator(".st.bad")).toHaveText("This addon's server did not answer in time");
  await expect(install()).toHaveText("Install");
  await expect(card(page, "YouTube").locator(".st.bad")).toHaveCount(0);
  await expect(card(page, "YouTube").getByRole("button", { name: /YouTube/ })).toHaveText("Install");
});

test("TV: a card is one focus stop, Back goes to the chips, and there is no Configure", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.goto("/find-addons?form=tv");
  await expect.poll(() => focused(page)).toBe("fa-lists:official");
  await expect(page.locator(".fa-card[data-f]").first()).toBeVisible();
  // Down past the Show chips and Refresh to the first card.
  for (let i = 0; i < 4 && !/^fa:official:\d+$/.test((await focused(page)) ?? ""); i++) await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).toMatch(/^fa:official:\d+$/);
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${SHOTS}/find-addons-tv-focus.jpg`, type: "jpeg", quality: 80 });
  await page.keyboard.press("Escape");
  await expect.poll(() => focused(page)).toBe("fa-lists:official");
  await expect(page).toHaveURL(/\/find-addons/);

  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await expect(page.locator(".fa-note")).toBeVisible();
  await expect(page.locator(".fa-help").first()).toContainText("on your phone or computer");
  await expect(page.locator('a[data-f$=":configure"]')).toHaveCount(0);
});

// Lists from an addon the user installed (plans/plugins-and-marketplace.md 7.1),
// on a core of its own: stremio-addons.net's directory, as recorded.
test.describe("a directory addon's list", () => {
  let core: { proc: ChildProcess; data: string } | null = null;
  const DIRECTORY = "https://stremio-addons.net/api/manifest.json";

  test.beforeAll(async () => {
    core = await startBridge(ADDON_LISTS_PORT);
    const c = new Client(`ws://127.0.0.1:${ADDON_LISTS_PORT}/ws`);
    try {
      await c.req("installAddon", { url: DIRECTORY });
      await c.req("setPrefs", { patch: { defaultsSeen: 1 } });
    } finally {
      c.close();
    }
  });
  test.afterAll(() => {
    if (core) cleanup(core.proc, core.data);
  });

  async function open(browser: Browser, form: string, size: { width: number; height: number }): Promise<Page> {
    const context = await browser.newContext({ viewport: size, colorScheme: "dark", reducedMotion: "reduce", baseURL: `http://127.0.0.1:${PORTS.vite}` });
    await context.routeWebSocket(/\/ws$/, (ws) => {
      const upstream = new WebSocket(`ws://127.0.0.1:${ADDON_LISTS_PORT}/ws`);
      const early: string[] = [];
      upstream.addEventListener("open", () => {
        for (const m of early.splice(0)) upstream.send(m);
      });
      upstream.addEventListener("message", (e) => ws.send(String(e.data)));
      upstream.addEventListener("close", () => ws.close());
      ws.onMessage((m) => (upstream.readyState === WebSocket.OPEN ? upstream.send(String(m)) : early.push(String(m))));
      ws.onClose(() => upstream.close());
    });
    const page = await context.newPage();
    await page.goto(`/find-addons?form=${form}`);
    return page;
  }

  test("desktop: its own chip, the host it comes from, and a search over the list", async ({ browser }) => {
    const page = await open(browser, "desktop", { width: 1440, height: 900 });
    const chip = page.getByRole("radio", { name: /^stremio-addons\.net/ });
    await expect(chip).toBeVisible({ timeout: 20_000 });
    // Cinemeta's lists keep their slots.
    await expect(page.getByRole("radio").first()).toHaveAccessibleName(/^Official/);
    await chip.click();
    const from = page.locator(".fa-from a");
    await expect(from).toHaveText("From stremio-addons.net");
    await expect(from).toHaveAttribute("href", "https://stremio-addons.net/");
    await expect(page.locator(".fa-note")).toContainText("Community addons are made by other people");
    await expect(page.locator(".fa-card:not(.skel)").first()).toBeVisible({ timeout: 20_000 });
    // The chip counts what the list shows: hundreds of addons.
    const count = async () => Number((await chip.textContent())?.match(/(\d+)\s*$/)?.[1] ?? 0);
    await expect.poll(count).toBeGreaterThan(100);
    const shown = await count();
    await page.screenshot({ path: `${SHOTS}/find-addons-directory-desktop.jpg`, type: "jpeg", quality: 80 });

    const search = page.getByRole("searchbox", { name: "Search this list" });
    await search.fill("OpenSubtitles");
    await expect.poll(async () => page.locator(".fa-card").count()).toBeLessThan(Math.min(shown, 24));
    for (const text of await page.locator(".fa-card").allInnerTexts()) expect(text.toLowerCase()).toContain("opensubtitles");
    await search.fill("zzzz no such addon zzzz");
    await expect(page.locator(".note")).toHaveText("Nothing in this list matches.");
    // No link of an entry is ever in the page: they may hold an addon's keys.
    await search.fill("");
    const hrefs = await page.locator(".fa-card a").evaluateAll((links) => links.map((a) => a.getAttribute("href") ?? ""));
    for (const href of hrefs) expect(href.endsWith("manifest.json")).toBe(false);
    await page.context().close();
  });

  test("TV: the list's chip and its host, with no link to follow", async ({ browser }) => {
    const page = await open(browser, "tv", { width: 1920, height: 1080 });
    const chip = page.getByRole("radio", { name: /^stremio-addons\.net/ });
    await expect(chip).toBeVisible({ timeout: 20_000 });
    await chip.click();
    await expect(page.locator(".fa-from")).toHaveText("From stremio-addons.net");
    await expect(page.locator(".fa-from a")).toHaveCount(0);
    await expect(page.locator(".fa-card:not(.skel)").first()).toBeVisible({ timeout: 20_000 });
    await expect(page.getByRole("searchbox")).toHaveCount(0);
    await page.screenshot({ path: `${SHOTS}/find-addons-directory-tv.jpg`, type: "jpeg", quality: 80 });
    await page.context().close();
  });
});
