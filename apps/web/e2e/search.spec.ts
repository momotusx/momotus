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
    await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
    await openFromNav(page, form);
    // Over Home, not instead of it: same address, the page behind is inert.
    expect(new URL(page.url()).pathname).toBe("/");
    await expect(page.locator(".screen")).toHaveAttribute("inert", "");
    await expect.poll(() => focused(page)).toBe("search:input");

    await page.locator('[data-f="search:input"]').fill("thrones");
    const hit = page.locator(".hit").first();
    await expect(hit).toBeVisible({ timeout: 30_000 });
    // A result row reads "Movie · 2019 · IMDb 8.0 · RT 87%" (kind, then the year and the scores there are).
    await expect(hit.locator(".hit-meta")).toHaveText(/^(Movie|Series)(\s*·\s*[^·\s][^·]*)*$/);

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
        // Back took the palette's history entry with it.
        expect(await page.evaluate(() => !!(history.state as { _palette?: boolean } | null)?._palette)).toBe(false);
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

test("desktop: a query nothing matches says No matches, the addons can search", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/");
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await openFromNav(page, "desktop");
  await page.locator('[data-f="search:input"]').fill("zqxjvkwpqzv");
  await expect(page.locator(".palette-layer .palette-note").last()).toHaveText('No matches for "zqxjvkwpqzv" in your catalogs.', { timeout: 30_000 });
  await expect(page.locator(".none-search")).toHaveCount(0);
  await context.close();
});

test("desktop: the scope pill steps through the kinds the results hold", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/");
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await openFromNav(page, "desktop");
  await page.locator('[data-f="search:input"]').fill("thrones");
  await expect(page.locator(".hit").first()).toBeVisible({ timeout: 30_000 });
  // A hit is known by its row and its title: Top results may repeat a title.
  expect(await page.locator(".hit").first().getAttribute("data-f")).toMatch(/^hit:.+:[^:]+$/);
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
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await page.locator('[data-f="nav:/search"]').click();
  // Typed at once, before the view has opened.
  await page.locator('[data-f="search:input"]').fill("thrones");
  await expect(page.locator(".hit").first()).toBeVisible({ timeout: 30_000 });
  await context.close();
});

const onPaletteEntry = (page: Page) => page.evaluate(() => !!(history.state as { _palette?: boolean } | null)?._palette);

test("desktop: browser Back closes the palette and stays on the screen under it", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/");
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await page.locator('[data-f="nav:/discover"]').click();
  await expect(page).toHaveURL(/\/discover/);
  await openFromNav(page, "desktop");
  expect(await onPaletteEntry(page)).toBe(true);
  await page.goBack();
  await expect(page.locator(".palette-layer")).toHaveCount(0);
  expect(new URL(page.url()).pathname).toBe("/discover");
  // Its entry is gone: the next Back leaves Discover for Home, as before.
  await page.goBack();
  await expect.poll(() => new URL(page.url()).pathname).toBe("/");
  await context.close();
});

test("phone: a labelled Close closes the palette and takes its history entry with it", async ({ browser }) => {
  const { context, page } = await open(browser, "phone", "/");
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await openFromNav(page, "phone");
  const close = page.getByRole("button", { name: "Close search" });
  await expect(close).toBeVisible();
  await close.tap();
  await expect(page.locator(".palette-layer")).toHaveCount(0);
  expect(new URL(page.url()).pathname).toBe("/");
  await expect.poll(() => onPaletteEntry(page)).toBe(false);
  await context.close();
});

test("phone: Back from a result returns to the results, Back again to the screen", async ({ browser }) => {
  const { context, page } = await open(browser, "phone", "/");
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await openFromNav(page, "phone");
  await page.locator('[data-f="search:input"]').fill("thrones");
  await page.locator(".hit").first().tap({ timeout: 30_000 });
  await expect(page).toHaveURL(/\/title\//);
  await expect(page.locator(".palette-layer")).toHaveCount(0);
  // The title went over the palette: Back opens it again on its results.
  await page.goBack();
  await expect.poll(() => new URL(page.url()).pathname).toBe("/");
  await expect(page.locator(".palette-layer .palette")).toBeVisible();
  await expect(page.locator('[data-f="search:input"]')).toHaveValue("thrones");
  await expect(page.locator(".palette-layer .hit").first()).toBeVisible({ timeout: 30_000 });
  expect(await onPaletteEntry(page)).toBe(true);
  // Back again closes it and stays on the screen under it.
  await page.goBack();
  await expect(page.locator(".palette-layer")).toHaveCount(0);
  expect(new URL(page.url()).pathname).toBe("/");
  expect(await onPaletteEntry(page)).toBe(false);
  await context.close();
});

test("tv: Back from a result reopens the palette on the result that was opened", async ({ browser }) => {
  const { context, page } = await open(browser, "tv", "/");
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await openFromNav(page, "tv");
  await page.keyboard.type("thrones");
  await expect(page.locator(".hit").first()).toBeVisible({ timeout: 30_000 });
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).toMatch(/^hit:/);
  const opened = await focused(page);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/title\//);
  await page.keyboard.press("Escape");
  await expect(page.locator(".palette-layer .palette")).toBeVisible();
  await expect.poll(() => focused(page), { timeout: 10_000 }).toBe(opened);
  // Back once more closes it, focus on the navigation's Search.
  await page.keyboard.press("Escape");
  await expect(page.locator(".palette-layer")).toHaveCount(0);
  await expect.poll(() => focused(page)).toBe("nav:/search");
  await context.close();
});

test("desktop: one status region carries Searching, the count and No matches, and Tab stays inside the palette", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/");
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await openFromNav(page, "desktop");
  // One region, there from the start and empty until there is something to say.
  const status = page.locator('.palette [role="status"]');
  await expect(status).toHaveCount(1);
  await expect(status).toHaveText("");
  // What it says follows the search: a query that finds titles ends on their count, one that finds none on No matches.
  const said: string[] = [];
  await page.exposeFunction("said", (t: string) => said.push(t));
  await page.evaluate(() => {
    const el = document.querySelector('.palette [role="status"]') as HTMLElement;
    new MutationObserver(() => (window as unknown as { said: (t: string) => void }).said(el.textContent ?? "")).observe(el, { childList: true, characterData: true, subtree: true });
  });
  await page.locator('[data-f="search:input"]').fill("thrones");
  await expect(status).toHaveText(/^\d+ results?$/, { timeout: 30_000 });
  expect(said).toContain("Searching");
  await page.locator('[data-f="search:input"]').fill("zzqxjkvv");
  await expect(status).toHaveText("No matches", { timeout: 30_000 });
  await page.locator('[data-f="search:input"]').fill("");
  await expect(status).toHaveText("");

  // Tab past the last control and Shift+Tab before the first wrap inside the dialog.
  await page.locator('[data-f="search:input"]').fill("thrones");
  await expect(status).toHaveText(/^\d+ results?$/, { timeout: 30_000 });
  const inside = () => page.evaluate(() => !!document.activeElement?.closest(".palette"));
  const first = () => page.evaluate(() => (document.activeElement as HTMLElement).dataset.f ?? "");
  await page.locator('[data-f="search:input"]').focus();
  await page.keyboard.press("Shift+Tab");
  expect(await inside()).toBe(true);
  const last = await first();
  expect(last).toMatch(/^hit:/);
  await page.keyboard.press("Tab");
  expect(await first()).toBe("search:input");
  // And all the way round, one Tab at a time, never out.
  const count = await page.locator(".palette a[href], .palette button:not(:disabled), .palette input").count();
  for (let i = 0; i < count + 2; i++) {
    await page.keyboard.press("Tab");
    expect(await inside()).toBe(true);
  }
  // Close gives the focus back to the control that opened it.
  // By key: a mouse takes no focus back (a ring nobody asked for), a key does.
  await page.locator('[data-f="search:close"]').focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".palette-layer")).toHaveCount(0);
  await expect.poll(() => focused(page)).toBe("nav:/search");
  await context.close();
});

test("tv: the search field keeps its height under a long list of results", async ({ browser }) => {
  const { context, page } = await open(browser, "tv", "/search");
  const field = page.locator(".palette .field");
  await expect(field).toBeVisible({ timeout: 30_000 });
  const before = (await field.boundingBox())?.height ?? 0;
  await page.locator('[data-f="search:input"]').fill("thrones");
  await expect(page.locator(".hit").nth(4)).toBeVisible({ timeout: 30_000 });
  expect((await field.boundingBox())?.height).toBe(before);
  expect(before).toBeGreaterThanOrEqual(84);
  await context.close();
});
