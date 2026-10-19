import { type Browser, expect, type Page, test } from "@playwright/test";

/** Search is a palette over the screen you are on (design/DECISION.md):
 * the navigation opens it on every form, Back or the scrim closes it, the
 * keyboard and remote walk from the field to the scope pill to the results,
 * and /search still works as a deep link. */

const FORMS = { tv: [1920, 1080, 1], desktop: [1440, 900, 1], phone: [390, 844, 2] } as const;
type Form = keyof typeof FORMS;

async function open(browser: Browser, form: Form, path: string) {
  const [width, height, scale] = FORMS[form];
  const context = await browser.newContext({
    viewport: { width, height },
    deviceScaleFactor: scale,
    isMobile: form === "phone",
    hasTouch: form === "phone",
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(`${path}${path.includes("?") ? "&" : "?"}form=${form}`);
  return { context, page, errors };
}

const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");

/** Opens the palette from the navigation, as a person on that form would. */
async function openFromNav(page: Page, form: Form) {
  if (form === "phone") await page.locator('.tabs a[href="/search"]').tap();
  else if (form === "tv") {
    await page.locator('[data-f="nav:/search"]').focus();
    await page.keyboard.press("Enter");
  } else await page.locator('[data-f="nav:/search"]').click();
  await expect(page.locator(".palette-layer .palette")).toBeVisible();
}

for (const form of Object.keys(FORMS) as Form[]) {
  test(`${form}: the navigation opens the palette over the screen, and it closes again`, async ({ browser }) => {
    const { context, page, errors } = await open(browser, form, "/");
    await expect(page.locator(".hero .reason")).toBeVisible({ timeout: 30_000 });
    await openFromNav(page, form);
    // Over Home, not instead of it: same address, the page behind is inert.
    expect(new URL(page.url()).pathname).toBe("/");
    await expect(page.locator(".screen")).toHaveAttribute("inert", "");
    await expect.poll(() => focused(page)).toBe("search:input");

    await page.locator('[data-f="search:input"]').fill("thrones");
    const hit = page.locator(".hit").first();
    await expect(hit).toBeVisible({ timeout: 30_000 });
    // A result row reads "Movie · 2019 · ★ 8.0" (kind, then year and rating when the addon gave them).
    await expect(hit.locator(".hit-meta")).toHaveText(/^(Movie|Series)(\s*·\s*\S+)*$/);

    if (form === "phone") {
      // Touch: a result opens its title, and the palette goes with the screen.
      await hit.tap();
      await expect(page).toHaveURL(/\/title\//);
      await expect(page.locator(".palette-layer")).toHaveCount(0);
    } else {
      // Remote and keyboard: Down from the field reaches the scope pill, then the results.
      await page.keyboard.press("ArrowDown");
      await expect.poll(() => focused(page)).toBe("search:scope");
      await page.keyboard.press("ArrowDown");
      await expect.poll(() => focused(page)).toMatch(/^hit:/);
      if (form === "tv") {
        // Back closes it and gives focus back where it was.
        await page.keyboard.press("Escape");
        await expect(page.locator(".palette-layer")).toHaveCount(0);
        await expect.poll(() => focused(page)).toBe("nav:/search");
      } else {
        // A pointer closes it on the scrim.
        await page.locator(".palette-scrim").click({ position: { x: 20, y: 600 } });
        await expect(page.locator(".palette-layer")).toHaveCount(0);
      }
      expect(new URL(page.url()).pathname).toBe("/");
      await expect(page.locator(".screen")).not.toHaveAttribute("inert", "");
    }
    expect(errors).toEqual([]);
    await context.close();
  });
}

test("desktop: the scope pill steps through the kinds the results hold", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/");
  await expect(page.locator(".hero .reason")).toBeVisible({ timeout: 30_000 });
  await openFromNav(page, "desktop");
  await page.locator('[data-f="search:input"]').fill("thrones");
  await expect(page.locator(".hit").first()).toBeVisible({ timeout: 30_000 });
  const scope = page.locator('[data-f="search:scope"]');
  const kinds = async () => [...new Set((await page.locator(".hit-meta").allInnerTexts()).map((t) => t.split("·")[0]?.trim()))];
  await expect(scope).toHaveText("All");
  const all = await kinds();
  expect(all.length).toBeGreaterThan(1);
  await scope.click();
  const first = (await scope.innerText()).trim();
  expect(await kinds()).toEqual([first]);
  for (let i = 1; i < all.length; i++) await scope.click();
  await scope.click();
  await expect(scope).toHaveText("All");
  await context.close();
});

test("tv: /search is still a deep link to the same panel", async ({ browser }) => {
  const { context, page } = await open(browser, "tv", "/search");
  await expect(page.locator(".search-page .palette")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".palette-layer")).toHaveCount(0);
  await page.locator('[data-f="search:input"]').fill("thrones");
  await expect(page.locator(".hit").first()).toBeVisible({ timeout: 30_000 });
  await context.close();
});

test("tv: a query typed while the search view is still opening is not lost", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  // The core's answer to the search subscription arrives late, as on a slow link.
  await page.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    const held = new Set<number>();
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m)) as { id?: number; op?: string; view?: string };
      if (msg.op === "subscribe" && msg.view === "search" && msg.id !== undefined) held.add(msg.id);
      server.send(m);
    });
    server.onMessage((m) => {
      const msg = JSON.parse(String(m)) as { id?: number };
      if (msg.id !== undefined && held.has(msg.id)) setTimeout(() => ws.send(m), 1500);
      else ws.send(m);
    });
  });
  await page.goto("/?form=tv");
  await expect(page.locator(".hero .reason")).toBeVisible({ timeout: 30_000 });
  await page.locator('[data-f="nav:/search"]').click();
  // Typed at once, before the view has opened.
  await page.locator('[data-f="search:input"]').fill("thrones");
  await expect(page.locator(".hit").first()).toBeVisible({ timeout: 30_000 });
  await context.close();
});
