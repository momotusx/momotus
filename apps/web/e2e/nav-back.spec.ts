/** A pushed page has a way back that needs no browser button (design/DECISION.md, Navigation; an installed web app is
 * `standalone` and has none): the title page and Versions on the phone and the desktop, and the profile switcher at
 * /profiles. TV draws none (the remote's Back), and nothing that is not a pushed page gets one: the start picker, the
 * player. */

import { expect, type Page, test } from "@playwright/test";
import { Client, GOT } from "./core";

const forms = [
  { form: "desktop", viewport: { width: 1440, height: 900 }, minSide: 24 },
  { form: "phone", viewport: { width: 390, height: 844 }, minSide: 44 },
] as const;

const path = (page: Page) => new URL(page.url()).pathname;
const box = (page: Page, selector: string) => page.locator(selector).first().boundingBox();
type Box = { x: number; y: number; width: number; height: number };
const meets = (a: Box, b: Box) => a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

/** The channels of a computed color: `rgb(...)`, `rgba(...)` or `color(srgb r g b / a)`. */
function channels(css: string): [number, number, number, number] {
  const nums = css.match(/-?\d*\.?\d+/g)?.map(Number) ?? [];
  const srgb = css.startsWith("color(");
  const [r = 0, g = 0, b = 0, a = 1] = nums;
  return srgb ? [r * 255, g * 255, b * 255, a] : [r, g, b, a];
}
const lum = ([r, g, b]: number[]) => {
  const lin = (c: number) => (c / 255 <= 0.04045 ? c / 255 / 12.92 : ((c / 255 + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r ?? 0) + 0.7152 * lin(g ?? 0) + 0.0722 * lin(b ?? 0);
};
const ratio = (a: number, b: number) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);

for (const { form, viewport, minSide } of forms) {
  test(`${form}: the title page and Versions show Back, which returns to the opener; a deep link lands on Home`, async ({ browser }) => {
    const context = await browser.newContext({ viewport, reducedMotion: "reduce", hasTouch: form === "phone" });
    const page = await context.newPage();
    await page.goto(`/library?form=${form}`);
    await page.locator(`a[href^="/title/series/${GOT.id}"]`).first().click();
    await expect(page).toHaveURL(new RegExp(`/title/series/${GOT.id}$`));

    // The link is there from the first frame, a real target, and clear of the logo slot.
    const back = page.locator(".back");
    await expect(back).toHaveText("Back");
    await expect(back).toHaveAttribute("href", "/");
    // The page is there once its meta line is; a phone's cover prints the title itself, so it has no logo slot.
    await expect(page.locator(".info .meta")).toBeVisible({ timeout: 30_000 });
    const b = (await back.boundingBox()) as Box;
    expect(b.height).toBeGreaterThanOrEqual(minSide - 0.5);
    expect(b.width).toBeGreaterThanOrEqual(minSide);
    const slot = page.locator(".logo-slot");
    // The desktop title has its logo's slot (a phone's cover prints the title itself and has none).
    if (form === "desktop") await expect(slot).toBeVisible();
    if (await slot.count()) {
      const s = (await slot.boundingBox()) as Box;
      expect(meets(b, s), `Back ${JSON.stringify(b)} against the logo slot ${JSON.stringify(s)}`).toBe(false);
    }

    // Key blue over the art: the pill behind it keeps 4.5:1 even over white art (WCAG, worst case).
    const { fg, bg } = await back.evaluate((el) => ({ fg: getComputedStyle(el).color, bg: getComputedStyle(el).backgroundColor }));
    const [pr, pg, pb, pa] = channels(bg);
    const under = [pr, pg, pb].map((c) => c * pa + 255 * (1 - pa));
    const contrast = ratio(lum(channels(fg)), lum(under));
    console.log(`${form} title: Back ${fg} on ${bg} over white art = ${contrast.toFixed(2)}:1`);
    expect(contrast).toBeGreaterThanOrEqual(4.5);

    // The phone's link stays while the page scrolls.
    if (form === "phone") {
      await page.evaluate(() => window.scrollTo(0, 700));
      expect((await back.boundingBox())?.y).toBeLessThan(60);
    }

    // Versions, from the title: its own bar, clear of the heading.
    await page.evaluate(() => window.scrollTo(0, 0));
    await page.locator('[data-f="title:versions"]').click();
    await expect(page).toHaveURL(/\/versions\//);
    await expect(back).toHaveText("Back");
    const head = (await box(page, ".vhead h1")) as Box;
    const vb = (await back.boundingBox()) as Box;
    expect(vb.height).toBeGreaterThanOrEqual(minSide - 0.5);
    expect(meets(vb, head), `Back ${JSON.stringify(vb)} against the heading ${JSON.stringify(head)}`).toBe(false);
    await page.screenshot({ path: `${process.env.SHOTS_DIR ?? "test-results/shots"}/nav-back-versions-${form}.png` });
    await back.click();
    await expect(page).toHaveURL(new RegExp(`/title/series/${GOT.id}$`));
    await back.click();
    await expect.poll(() => path(page)).toBe("/library");

    // A deep link has no history to go back in: Back lands on Home.
    await page.goto(`/title/series/${GOT.id}?form=${form}`);
    await page.locator(".back").click();
    await expect.poll(() => path(page)).toBe("/");
    await page.goto(`/title/series/${GOT.id}/versions/${GOT.resume}?form=${form}`);
    await page.locator(".back").click();
    await expect.poll(() => path(page)).toBe(`/title/series/${GOT.id}`);
    await context.close();
  });
}

test("TV draws no back link on the title page or Versions (the remote's Back is the way)", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto(`/title/series/${GOT.id}?form=tv`);
  await expect(page.locator(".logo-slot")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".backbar")).toHaveCount(0);
  await page.goto(`/title/series/${GOT.id}/versions/${GOT.resume}?form=tv`);
  await expect(page.locator(".versions-scr")).toBeVisible();
  await expect(page.locator(".backbar")).toHaveCount(0);
  await context.close();
});

test("the profile switcher shows Back, the start picker does not", async ({ browser }) => {
  const c = new Client();
  try {
    await c.req("createProfile", { name: "Back Kid", avatar: "letter:sky" });
    for (const { form, viewport } of forms) {
      const context = await browser.newContext({ viewport, reducedMotion: "reduce" });
      const page = await context.newPage();
      // A cold start with two profiles asks who is watching: nothing to go back to.
      await page.goto(`/?form=${form}`);
      await expect(page.locator(".picker h1")).toHaveText("Who's watching?");
      await expect(page.locator(".back")).toHaveCount(0);
      await page.locator('[data-f="profile:default"]').click();
      await expect(page.getByRole("region", { name: "Featured" }).first()).toBeVisible({ timeout: 30_000 });
      // Switching profiles is a pushed page: no tab bar, no lit Settings, and Back.
      await page.goto(`/profiles?form=${form}`);
      await expect(page.locator(".picker h1")).toHaveText("Who's watching?");
      const back = page.locator(".back");
      await expect(back).toHaveText("Back");
      await expect(page.locator(".tabs, .topnav")).toHaveCount(0);
      const h = (await box(page, ".picker h1")) as Box;
      expect(meets((await back.boundingBox()) as Box, h)).toBe(false);
      await page.screenshot({ path: `${process.env.SHOTS_DIR ?? "test-results/shots"}/nav-back-profiles-${form}.png` });
      await back.click();
      await expect.poll(() => path(page)).toBe("/");
      await context.close();
    }
  } finally {
    const s = await c.subscribe<{ profiles: { id: string; name: string }[] }>("profiles");
    for (const p of s.now().profiles) if (p.name === "Back Kid") await c.req("deleteProfile", { profile: p.id }).catch(() => undefined);
    c.close();
  }
});
