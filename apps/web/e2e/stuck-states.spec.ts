/** A page that cannot open says so in one plain line with Try again, and opens by itself once the network is back
 * (QA r1, Q4 rows 36 and 40, Q5 row 39): Discover and a title wait in grey only while there is still reason to,
 * Search says once that its lists did not load, and a lost core is named in words, never by its address. */

import type { WebSocketRoute } from "@playwright/test";
import { expect, type Page, test } from "./fixture";
import { PORTS } from "./core";
import { patchedContext } from "./patch";

const DESKTOP = { viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" } as const;

/** The core's messages to the page, held while `hold()` and sent in order once it ends. */
async function holdCore(page: Page, hold: () => boolean) {
  const held: { ws: WebSocketRoute; m: string | Buffer }[] = [];
  const flush = () => {
    for (const { ws, m } of held.splice(0)) ws.send(m);
  };
  await page.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    server.onMessage((m) => (hold() || held.length ? held.push({ ws, m }) : ws.send(m)));
  });
  return flush;
}

test("desktop: a title that cannot open offline says so, then opens when the network is back", async ({ browser }) => {
  const context = await browser.newContext(DESKTOP);
  const page = await context.newPage();
  let hold = false;
  const flush = await holdCore(page, () => hold);
  await page.goto("/?form=desktop");
  await expect(page.locator('main a[href^="/title/"]').first()).toBeVisible({ timeout: 30_000 });
  hold = true;
  await context.setOffline(true);
  await page.evaluate(() => {
    history.pushState(null, "", "/title/movie/tt0056923");
    dispatchEvent(new PopStateEvent("popstate"));
  });
  await expect(page.locator('[data-f="title:stalled"]')).toBeVisible();
  await expect(page.locator(".stalled p")).toHaveText("You're offline. This page opens when the network is back.");
  await context.setOffline(false);
  hold = false;
  flush();
  await expect(page.locator(".stalled")).toHaveCount(0);
  await expect(page.locator(".info .meta")).toBeVisible({ timeout: 30_000 });
  await context.close();
});

test("desktop: Discover that hangs says so after a while, and Try again opens it", async ({ browser }) => {
  test.setTimeout(60_000);
  const context = await browser.newContext(DESKTOP);
  const page = await context.newPage();
  let hold = false;
  const flush = await holdCore(page, () => hold);
  await page.goto("/?form=desktop");
  await expect(page.locator('main a[href^="/title/"]').first()).toBeVisible({ timeout: 30_000 });
  hold = true;
  await page.locator('[data-f="nav:/discover"]').click();
  await expect(page.locator(".discover .skel").first()).toBeVisible();
  await expect(page.locator('[data-f="discover:stalled"]')).toBeVisible({ timeout: 15_000 });
  await expect(page.locator(".stalled p")).toHaveText("This is taking longer than it should.");
  // Try again starts the page anew: the skeleton, and the page once the core answers.
  await page.locator('[data-f="discover:stalled"]').click();
  await expect(page.locator(".discover .skel").first()).toBeVisible();
  hold = false;
  flush();
  await expect(page.locator('main a[href^="/title/"]').first()).toBeVisible({ timeout: 30_000 });
  await context.close();
});

test("desktop: Search whose every list failed says it once, with Try again, and asks again", async ({ browser }) => {
  const { context, commands } = await patchedContext(browser, PORTS.bridge, DESKTOP, (view, s) =>
    view === "search" && s.query
      ? { ...s, rows: s.rows.length ? s.rows.map((r: object) => ({ ...r, tiles: [], state: { state: "failed", reason: "timeout" } })) : [] }
      : s,
  );
  const page = await context.newPage();
  const sent = () => commands.filter((c) => c.command?.type === "setQuery");
  await page.goto("/?form=desktop");
  await page.locator('[data-f="nav:/search"]').click();
  await page.locator('[data-f="search:input"]').fill("thrones");
  const again = page.locator('[data-f="search:again"]');
  await expect(again).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".palette .st.bad")).toHaveCount(1);
  await expect(page.locator(".palette")).not.toContainText("No top results");
  const before = sent().length;
  await again.click();
  await expect.poll(() => sent().length).toBeGreaterThan(before);
  expect(sent().at(-1)?.command.query).toBe("thrones");
  await context.close();
});

test("desktop: a lost core is said in words, never by its address", async ({ browser }) => {
  const context = await browser.newContext(DESKTOP);
  const page = await context.newPage();
  let socket: WebSocketRoute | null = null;
  await page.routeWebSocket(/\/ws$/, (ws) => {
    socket = ws;
    ws.connectToServer();
  });
  await page.goto("/?form=desktop");
  await expect(page.locator('main a[href^="/title/"]').first()).toBeVisible({ timeout: 30_000 });
  await (socket as WebSocketRoute | null)?.close();
  const banner = page.locator(".banner.bad");
  await expect(banner).toContainText("Lost the connection to Momotus. Start it again, then reload.");
  await expect(banner).not.toContainText(/ws:|127\.0\.0\.1|core/);
  await context.close();
});
