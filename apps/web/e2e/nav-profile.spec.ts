import { type Browser, expect, type Page, test } from "@playwright/test";
import { Client } from "./core";

/** The profile in place of Settings (plans/nav-profile.md): the bar's far-right item is the current profile, its
 * menu lists the profiles (the current one checked), then Settings, and nothing else. */

const PIN = "2580";
const FORMS = { tv: [1920, 1080, 1], desktop: [1440, 900, 1], phone: [390, 844, 2] } as const;
type Form = keyof typeof FORMS;
type Snapshot = { active: string; profiles: { id: string; name: string }[] };

const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
const button = (page: Page) => page.locator('[data-f="nav:/profiles"]');
const rows = (page: Page) => page.locator('[role="menu"] [role^="menuitem"]');

let kid = "";
let locked = "";

test.beforeAll(async () => {
  const c = new Client();
  try {
    kid = (await c.req<Snapshot>("createProfile", { name: "Kid", avatar: "letter:sky" })).profiles.find((p) => p.name === "Kid")!.id;
    locked = (await c.req<Snapshot>("createProfile", { name: "Locked", avatar: "letter:mint" })).profiles.find((p) => p.name === "Locked")!.id;
    // A PIN is set from its own profile.
    await c.req("switchProfile", { profile: locked });
    await c.req("setPin", { profile: locked, pin: PIN });
    await c.req("switchProfile", { profile: "default" });
  } finally {
    c.close();
  }
});

test.afterAll(async () => {
  const c = new Client();
  try {
    await c.req("switchProfile", { profile: "default" }).catch(() => undefined);
    for (const id of [kid, locked]) if (id) await c.req("deleteProfile", { profile: id }).catch(() => undefined);
  } finally {
    c.close();
  }
});

/** A page of `form` at `path`, as Main (the start picker asks once, with three profiles). */
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
  await page.goto(`${path}?form=${form}`);
  const main = page.locator('[data-f="profile:default"]');
  await expect(main.or(button(page))).toBeVisible({ timeout: 30_000 });
  if (await main.isVisible()) await main.click();
  await expect(button(page)).toBeVisible();
  return { context, page };
}

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: no Settings in the bar; the profile's menu holds the profiles, then Settings`, async ({ browser }) => {
    const { context, page } = await open(browser, form, "/");
    await expect(page.locator('[data-f="nav:/settings"], nav a[href="/settings"]')).toHaveCount(0);
    await expect(button(page)).toHaveAttribute("aria-label", "Profile: Main");
    await expect(button(page)).toHaveAttribute("aria-haspopup", "menu");
    // A 48 px target, whatever the avatar's drawn size.
    const box = (await button(page).boundingBox())!;
    expect(Math.min(box.width, box.height)).toBeGreaterThanOrEqual(47.5);
    await button(page).click();
    await expect(button(page)).toHaveAttribute("aria-expanded", "true");
    await expect(rows(page)).toHaveText(["Main", "Kid", "Locked", "Settings"]);
    await expect(page.locator('[role="menu"] [aria-checked="true"]')).toHaveAttribute("aria-label", "Main, current profile");
    await expect(page.locator(`[data-f="pmenu:${locked}"]`)).toHaveAttribute("aria-label", "Locked, locked with a PIN");
    for (const r of await rows(page).all()) expect((await r.boundingBox())!.height).toBeGreaterThanOrEqual(47.5);
    // Settings opens Settings, and the profile is lit while it shows.
    await page.locator('[data-f="pmenu:settings"]').click();
    await expect(page).toHaveURL(/\/settings/);
    await expect(page.locator('[role="menu"]')).toHaveCount(0);
    await expect(button(page)).toHaveClass(/\bon\b/);
    await context.close();
  });
}

test("desktop: the menu by keyboard, Escape, Tab and a click outside close it", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/library");
  await button(page).focus();
  await page.keyboard.press("Enter");
  // Focus starts on the current profile; Up stops at the top, Down at the bottom.
  await expect.poll(() => focused(page)).toBe("pmenu:default");
  await page.keyboard.press("ArrowUp");
  expect(await focused(page)).toBe("pmenu:default");
  for (let i = 0; i < 5; i++) await page.keyboard.press("ArrowDown");
  expect(await focused(page)).toBe("pmenu:settings");
  await page.keyboard.press("ArrowLeft");
  expect(await focused(page)).toBe("pmenu:settings");
  // Escape closes, focus back on the profile, and the page stays.
  await page.keyboard.press("Escape");
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  expect(await focused(page)).toBe("nav:/profiles");
  await expect(page).toHaveURL(/\/library/);
  // Tab out of it closes it.
  await page.keyboard.press("Enter");
  await expect.poll(() => focused(page)).toBe("pmenu:default");
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Shift+Tab");
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  // A click outside closes it.
  await button(page).click();
  await expect(page.locator('[role="menu"]')).toHaveCount(1);
  await page.mouse.click(700, 600);
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  // The current profile closes it and changes nothing.
  await button(page).click();
  await page.locator('[data-f="pmenu:default"]').click();
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  await expect(page).toHaveURL(/\/library/);
  await context.close();
});

test("a profile without a PIN switches; one with a PIN opens its keypad, and Cancel goes back", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/library");
  // A PIN: its keypad, then Cancel leaves the picker for the page the menu was on.
  await button(page).click();
  await page.locator(`[data-f="pmenu:${locked}"]`).click();
  await expect(page.locator(".keypad h2")).toHaveText("Enter the PIN for Locked");
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page).toHaveURL(/\/library/);
  // The intent was used once: the picker's address opens no keypad again.
  await page.goForward();
  await expect(page.locator(".picker h1")).toHaveText("Who's watching?");
  await expect(page.locator(".keypad")).toHaveCount(0);
  await page.goBack();
  await expect(page).toHaveURL(/\/library/);
  // No PIN: the switch, then that profile's Home.
  await button(page).click();
  await page.locator(`[data-f="pmenu:${kid}"]`).click();
  await expect(button(page)).toHaveAttribute("aria-label", "Profile: Kid");
  await expect(page).toHaveURL(/\/(\?form=desktop)?$/);
  // Back to Main through the menu.
  await button(page).click();
  await expect(page.locator('[role="menu"] [aria-checked="true"]')).toHaveAttribute("aria-label", "Kid, current profile");
  await page.locator('[data-f="pmenu:default"]').click();
  await expect(button(page)).toHaveAttribute("aria-label", "Profile: Main");
  await context.close();
});

test("TV: the profile ends the bar, takes Up from Settings, and its menu keeps the remote", async ({ browser }) => {
  const { context, page } = await open(browser, "tv", "/library");
  // Off the content, up to the bar on Library, then Right to its end: Search, then the profile, which shows no place.
  for (let i = 0; i < 12 && !(await focused(page))?.startsWith("nav:"); i++) await page.keyboard.press("ArrowUp");
  expect(await focused(page)).toBe("nav:/library");
  for (let i = 0; i < 4; i++) await page.keyboard.press("ArrowRight");
  expect(await focused(page)).toBe("nav:/profiles");
  await page.waitForTimeout(600);
  await expect(page).toHaveURL(/\/library/);
  // Select opens the menu on the current profile; Left and Right stay; Back closes it on the profile.
  await page.keyboard.press("Enter");
  await expect.poll(() => focused(page)).toBe("pmenu:default");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowLeft");
  expect(await focused(page)).toBe("pmenu:default");
  await page.keyboard.press("Escape");
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  expect(await focused(page)).toBe("nav:/profiles");
  await expect(page).toHaveURL(/\/library/);
  // Settings: shown in the place of Library, the profile lit.
  await page.keyboard.press("Enter");
  await expect.poll(() => focused(page)).toBe("pmenu:default");
  for (let i = 0; i < 3; i++) await page.keyboard.press("ArrowDown");
  expect(await focused(page)).toBe("pmenu:settings");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/settings/);
  await expect(button(page)).toHaveClass(/\bon\b/);
  // Into the page and Up again: the bar takes it on the profile; Back there goes to the bar, then Home.
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).toMatch(/^settings:/);
  for (let i = 0; i < 12 && !(await focused(page))?.startsWith("nav:"); i++) await page.keyboard.press("ArrowUp");
  expect(await focused(page)).toBe("nav:/profiles");
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).toMatch(/^settings:/);
  await page.keyboard.press("Escape");
  await expect.poll(() => focused(page)).toBe("nav:/profiles");
  await page.keyboard.press("Escape");
  await expect.poll(() => new URL(page.url()).pathname).toBe("/");
  await context.close();
});
