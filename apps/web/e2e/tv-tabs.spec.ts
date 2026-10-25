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
