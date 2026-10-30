import { expect, test } from "@playwright/test";

/** Debrid services on the web (plans/c10-handoff.md, phase 3a): a typed key
 * connects as a record the tab keeps, Settings counts it, a connected account
 * can take a new key, and Disconnect goes through the helper. The key never
 * reaches the console. Sign-in by code talks to the real services, so it is
 * walked live, not here. */

test.use({ viewport: { width: 1920, height: 1080 } });

const KEY = "MQDUMMYE2EKEY1";

test("debrid: a key connects, is counted, can be replaced and disconnects", async ({ page }) => {
  const console: string[] = [];
  page.on("console", (m) => console.push(m.text()));
  await page.goto("/settings/sources?form=tv");
  await expect(page.locator('[data-f="settings:debrid"]')).toContainText("Play torrents as fast direct links");
  await page.locator('[data-f="settings:debrid"]').click();
  await expect(page).toHaveURL(/\/debrid/);

  const card = page.locator('.dcard[aria-label="Premiumize"]');
  await page.locator('[data-f="debrid:premiumize:field"]').fill(KEY);
  await page.locator('[data-f="debrid:premiumize:save"]').click();
  await expect(card.locator(".dcard-head .st")).toHaveText("Connected");
  // Connected: a new key without a disconnect first; the field takes focus.
  await expect(page.locator('[data-f="debrid:premiumize:key"]')).toHaveText("Replace the key");
  await page.locator('[data-f="debrid:premiumize:key"]').click();
  await expect.poll(() => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f)).toBe("debrid:premiumize:field");
  await page.keyboard.type(`${KEY}B`);
  await page.locator('[data-f="debrid:premiumize:save"]').click();
  await expect(page.locator('[data-f="debrid:premiumize:field"]')).toHaveCount(0);
  await expect(card.locator(".dcard-head .st")).toHaveText("Connected");

  await page.locator('[data-f="nav:/profiles"]').click();
  await page.locator('[data-f="pmenu:settings"]').click();
  await expect(page.locator('[data-f="settings:debrid"]')).toContainText("1 debrid service in use.");

  await page.locator('[data-f="settings:debrid"]').click();
  // Two presses: the first only asks, and the account stays.
  const disconnect = page.locator('[data-f="debrid:premiumize:disconnect"]');
  await disconnect.click();
  await expect(disconnect).toHaveText("Press again to disconnect");
  await page.waitForTimeout(300);
  await expect(card.locator(".dcard-head .st")).toHaveText("Connected");
  await disconnect.click();
  await expect(card.getByText("Disconnected")).toBeVisible();
  await expect(page.locator('[data-f="debrid:premiumize:field"]')).toBeVisible();
  expect(console.filter((line) => line.includes(KEY))).toEqual([]);
});
