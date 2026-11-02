/** An urgent account alert is seen without a scroll (QA r1, Q2 row 31; the owner, 2026-11-02): a new device, a
 * deletion on its way or the account gone comes first on Home, above the hero, compact and wholly on the first screen,
 * on every form, over the hero's art. With no alert nothing is kept for one, and one that lands late moves neither the
 * hero nor the focus (QA r2, Q2 row 33). The second-way
 * nudge stays after the first row. ALERT_SHOTS=<dir> keeps the shots (alerts-<label>-<form>.png). */

import { expect, type Page, test } from "./fixture";

const FORMS = {
  desktop: { viewport: { width: 1440, height: 900 } },
  phone: { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true },
  tv: { viewport: { width: 1920, height: 1080 } },
} as const;

const JOINED = { id: "7.qa-b", kind: "device", member: "qa-b", kit: false, passkey: false, by: "qa-a", name: "QA device B", count: null };

/** A round's report with one announcement, as the relay would send it (sync.ts's development door). */
const announce = (page: Page) =>
  page.evaluate((a) => {
    const dev = (window as unknown as { mqDevSync: { setReport: (r: object) => void } }).mqDevSync;
    dev.setReport({ deletion: null, announcements: [a] });
  }, JOINED);

/** OK's ack is answered here: the shared core has no account to tell. */
const ackHere = (page: Page) =>
  page.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((raw) => {
      const m = JSON.parse(String(raw));
      if (m.op === "ackAnnouncements") ws.send(JSON.stringify({ id: m.id, ok: true, result: null }));
      else server.send(raw);
    });
  });

const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");

for (const [form, options] of Object.entries(FORMS)) {
  test(`${form}: a new-device alert comes first on Home, whole on the first screen`, async ({ browser }) => {
    const context = await browser.newContext({ ...options, reducedMotion: "reduce" });
    const page = await context.newPage();
    await ackHere(page);
    await page.goto(`/?form=${form}`);
    await expect(page.locator('main a[href^="/title/"]').first()).toBeAttached({ timeout: 30_000 });
    await expect(page.locator(".home .hero")).toBeVisible();
    // No alert: nothing above the hero, nothing kept for one.
    await expect(page.locator(".account-card.urgent")).toHaveCount(0);
    expect(await page.locator(".home main > *").first().evaluate((el) => el.classList.contains("hero"))).toBe(true);
    const heroTop = (await page.locator(".home .hero").boundingBox())?.y ?? 0;
    const play = page.locator('.hero-page.on [data-f="hero:play"]');
    const playTop = (await play.boundingBox())?.y ?? -1;
    if (form === "tv") await expect.poll(() => focused(page)).toMatch(/^hero:/);
    const before = await focused(page);

    await announce(page);
    const card = page.locator('.account-card.urgent[data-card="announcement"]');
    await expect(card).toContainText("QA device B");
    expect(await page.locator(".home main > *").first().evaluate((el) => el.classList.contains("urgent"))).toBe(true);
    const box = await card.boundingBox();
    const view = page.viewportSize() as { width: number; height: number };
    expect(box && box.y >= 0 && box.y + box.height <= view.height, JSON.stringify(box)).toBe(true);
    // Compact: a band, not a panel.
    expect(box?.height ?? 0).toBeLessThan(form === "phone" ? 100 : 160);
    // It lands over the hero's art: the hero, its Play and its title stay where they were (QA r2, Q2 row 33).
    expect((await page.locator(".home .hero").boundingBox())?.y).toBe(heroTop);
    expect((await play.boundingBox())?.y).toBe(playTop);
    const title = await page.locator(".hero-page.on .hero-logo:not(.sr), .hero-page.on .logo-text").first().boundingBox();
    expect((box?.y ?? 0) + (box?.height ?? 0), JSON.stringify({ box, title })).toBeLessThanOrEqual(title?.y ?? 0);
    // An alert that lands after Home opened takes no focus from where the viewer is.
    expect(await focused(page)).toBe(before);
    if (process.env.ALERT_SHOTS) await page.screenshot({ path: `${process.env.ALERT_SHOTS}/alerts-${process.env.ALERT_LABEL ?? "after"}-${form}.png` });
    // OK clears it, and the hero has not moved.
    await card.locator('[data-f="home:account:ok"]').click();
    await expect(card).toHaveCount(0);
    expect((await play.boundingBox())?.y).toBe(playTop);
    await context.close();
  });
}

test("desktop: a deletion on its way comes first and keeps Cancel, with nothing to wave it away", async ({ browser }) => {
  const context = await browser.newContext({ ...FORMS.desktop, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto("/?form=desktop");
  await expect(page.locator(".home .hero")).toBeVisible({ timeout: 30_000 });
  await page.evaluate(() => {
    const dev = (window as unknown as { mqDevSync: { setReport: (r: object) => void } }).mqDevSync;
    dev.setReport({ deletion: { dueMs: Date.now() + 7 * 86_400_000, provider: null }, announcements: [] });
  });
  const card = page.locator('.account-card.urgent[data-card="deletion"]');
  await expect(card).toBeVisible();
  await expect(card.locator('[data-f="home:account:cancel"]')).toBeVisible();
  await expect(card.locator('[data-f="home:account:ok"]')).toHaveCount(0);
  await context.close();
});
