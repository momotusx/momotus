/** TV: the bar's tabs show their place when the focus rests on them (Apple TV's tab bar, the androidx.tv
 * TabRow's 250 ms debounce), with no Enter and nothing opened on the way across; the focus stays on the
 * bar, Down goes in, Back from the content comes back to the bar and Back on the bar goes Home. */

import { expect, type Page, test } from "@playwright/test";

const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);

test("TV: Right from Home onto Discover shows Discover after the settle, with no Enter", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto("/?form=tv");
  const home = page.locator('[data-f="nav:/"]');
  await expect(home).toBeVisible({ timeout: 30_000 });
  // Home's first focus has landed (the hero's Play, or the first row); Up from it reaches the bar.
  await expect.poll(() => focused(page), { timeout: 30_000 }).not.toBeNull();
  await home.focus();
  await page.keyboard.press("ArrowRight");
  expect(await focused(page)).toBe("nav:/discover");
  // A sweep goes on before the settle: nothing opens on the way.
  await page.keyboard.press("ArrowRight");
  expect(await focused(page)).toBe("nav:/library");
  await page.waitForTimeout(100);
  expect(new URL(page.url()).pathname).toBe("/");
  await page.keyboard.press("ArrowLeft");
  expect(await focused(page)).toBe("nav:/discover");
  await expect(page).toHaveURL(/\/discover/, { timeout: 2_000 });
  // The focus stays on the tab, which is now the lit one.
  await expect(page.locator('[data-f="nav:/discover"].on')).toBeVisible();
  await page.waitForTimeout(300);
  expect(await focused(page)).toBe("nav:/discover");
  // Down goes into Discover; Back comes back to its tab, and Back on the bar goes Home.
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).not.toMatch(/^nav:/);
  await page.keyboard.press("Escape");
  expect(await focused(page)).toBe("nav:/discover");
  expect(new URL(page.url()).pathname).toBe("/discover");
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/(\?.*)?$/);
  await expect.poll(() => focused(page)).toBe("nav:/");
  await context.close();
});

const depth = (page: Page) => page.evaluate(() => (history.state as { _depth?: number } | null)?._depth ?? 0);
const path = (page: Page) => new URL(page.url()).pathname;

test("TV: Enter on a tab not shown switches at once, with the settle's history, and Back on the bar goes Home", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto("/?form=tv");
  const home = page.locator('[data-f="nav:/"]');
  await expect(home).toBeVisible({ timeout: 30_000 });
  await expect.poll(() => focused(page), { timeout: 30_000 }).not.toBeNull();
  await home.focus();
  const before = await depth(page);
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/discover/);
  // A replace, as the settle makes, and the settle that was running does not navigate again.
  expect(await depth(page)).toBe(before);
  await page.waitForTimeout(400);
  expect(path(page)).toBe("/discover");
  expect(await depth(page)).toBe(before);
  expect(await focused(page)).toBe("nav:/discover");
  await page.keyboard.press("Escape");
  await expect.poll(() => path(page)).toBe("/");
  await expect.poll(() => focused(page)).toBe("nav:/");
  await context.close();
});

test("TV: Back on the bar goes Home from under a pushed Settings page, past it", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto("/settings?form=tv");
  const addons = page.locator('[data-f="settings:addons"]');
  await expect(addons).toBeVisible({ timeout: 30_000 });
  await addons.focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/addons/);
  // The bar is there, lit as Settings: onto Discover, which shows after the settle in the pushed page's place.
  await page.locator('[data-f="nav:/settings"]').focus();
  for (let i = 0; i < 4; i++) await page.keyboard.press("ArrowLeft");
  expect(await focused(page)).toBe("nav:/discover");
  await expect(page).toHaveURL(/\/discover/, { timeout: 2_000 });
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).not.toMatch(/^nav:/);
  await page.keyboard.press("Escape");
  expect(await focused(page)).toBe("nav:/discover");
  // Back on the bar: Home, not the Settings page under it.
  await page.keyboard.press("Escape");
  await expect.poll(() => path(page)).toBe("/");
  await expect.poll(() => focused(page)).toBe("nav:/");
  await context.close();
});

test("TV: Back on the bar of a pushed Settings page goes Home", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto("/settings?form=tv");
  const addons = page.locator('[data-f="settings:addons"]');
  await expect(addons).toBeVisible({ timeout: 30_000 });
  await addons.focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/addons/);
  // Up to the bar, by key, once the page took its first focus.
  await expect.poll(() => focused(page)).toMatch(/^(?!settings:)/);
  for (let i = 0; i < 12 && !(await focused(page))?.startsWith("nav:"); i++) await page.keyboard.press("ArrowUp");
  expect(await focused(page)).toBe("nav:/settings");
  await page.keyboard.press("Escape");
  await expect.poll(() => path(page)).toBe("/");
  await expect.poll(() => focused(page)).toBe("nav:/");
  await context.close();
});
