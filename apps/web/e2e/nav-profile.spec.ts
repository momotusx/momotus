import { type Browser, expect, type Page, test } from "./fixture";
import { Client, GOT, SHOTS, watch } from "./core";
import { BBB_ID } from "./source";

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
    // What each last watched, for the desktop panel: Kid an episode, Locked a film.
    await c.req("switchProfile", { profile: kid });
    await watch(c, { kind: "series", itemId: GOT.id, videoId: GOT.resume, title: "Game of Thrones", season: 2, episode: 3 }, 21 * 60_000, 57 * 60_000);
    // A PIN is set from its own profile.
    await c.req("switchProfile", { profile: locked });
    await watch(c, { kind: "movie", itemId: BBB_ID, videoId: BBB_ID, title: "Big Buck Bunny", via: "Sample Source" }, 150_000, 596_000);
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

for (const form of ["tv", "phone"] as const) {
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
    // Incognito sits after the profiles (plans/batch-10-incognito.md 5.1).
    await expect(rows(page)).toHaveText(["Main", "Kid", "Locked", "Incognito", "Settings"]);
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

/** The desktop profile panel (plans/profile-panel.md, the owner's pick 7). */
const panel = (page: Page) => page.locator('[role="dialog"][aria-labelledby="pp-title"]');
const shot = (page: Page, name: string) => page.screenshot({ path: `${SHOTS}/profile-panel-${name}.jpg`, type: "jpeg", quality: 80 });

test("desktop: the avatar opens the profile panel from the right; each profile with what it last watched", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/library");
  await expect(page.locator('[data-f="nav:/settings"], nav a[href="/settings"]')).toHaveCount(0);
  await expect(button(page)).toHaveAttribute("aria-label", "Profile: Main");
  await expect(button(page)).toHaveAttribute("aria-haspopup", "dialog");
  const box = (await button(page).boundingBox())!;
  expect(Math.min(box.width, box.height)).toBeGreaterThanOrEqual(47.5);
  await button(page).click();
  await expect(button(page)).toHaveAttribute("aria-expanded", "true");
  await expect(panel(page)).toBeVisible();
  await expect(panel(page).locator("h2")).toHaveText("Profiles");
  // 420 wide at the right edge, under the 52 high header, full height below it.
  const side = (await page.locator(".pp").boundingBox())!;
  expect(Math.round(side.width)).toBe(420);
  expect(Math.round(side.x + side.width)).toBe(1440);
  expect(Math.round(side.y)).toBe(52);
  expect(Math.round(side.y + side.height)).toBe(900);
  // The avatar and Search stay live above it; the page under it is inert.
  expect(await page.locator(".topnav").evaluate((el) => (el as HTMLElement).inert)).toBe(false);
  expect(await page.locator(".screen").evaluate((el) => (el as HTMLElement).inert)).toBe(true);
  // The current profile on the card, its last title from the seed; the others to switch to, then Incognito.
  const card = page.locator(".pp-card");
  await expect(card).toHaveAttribute("aria-label", "Main, current profile");
  await expect(card.locator(".pp-sub")).toHaveText("Watching now");
  await expect(card.locator(".pp-last")).toContainText("Last watched Big Buck Bunny", { timeout: 15_000 });
  const keys = await page.locator(".pp-row").evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.f));
  expect(keys).toEqual([`pmenu:${kid}`, `pmenu:${locked}`, "pmenu:incognito"]);
  await expect(page.locator(`[data-f="pmenu:${kid}"] small`)).toHaveText("Game of Thrones, S2:E3");
  await expect(page.locator(`[data-f="pmenu:${kid}"]`)).toHaveAttribute("aria-label", "Kid, last watched Game of Thrones, season 2, episode 3");
  // Main is the owner: a PIN profile's title shows to it.
  await expect(page.locator(`[data-f="pmenu:${locked}"] small`)).toHaveText("Big Buck Bunny");
  await expect(page.locator(`[data-f="pmenu:${locked}"]`)).toHaveAttribute("aria-label", "Locked, locked with a PIN, last watched Big Buck Bunny");
  await expect(page.locator('[data-f="pmenu:incognito"] small')).toHaveText("Watch without keeping anything");
  for (const r of await page.locator(".pp-row").all()) expect(Math.round((await r.boundingBox())!.height)).toBe(76);
  await expect(page.locator(".pp-foot .pp-pill")).toHaveText(["Manage profiles", "Settings"]);
  await shot(page, "1440");
  await context.close();
});

test("desktop: the panel by keyboard; Escape, Close, the dim layer and the avatar close it", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/library");
  await button(page).focus();
  await page.keyboard.press("Enter");
  // Focus starts on the first profile to switch to; Tab stays inside, wrapping at both ends.
  await expect.poll(() => focused(page)).toBe(`pmenu:${kid}`);
  await page.keyboard.press("Shift+Tab");
  expect(await focused(page)).toBe("pmenu:close");
  await page.keyboard.press("Shift+Tab");
  expect(await focused(page)).toBe("pmenu:settings");
  await page.keyboard.press("Tab");
  expect(await focused(page)).toBe("pmenu:close");
  await page.keyboard.press("ArrowDown");
  expect(await focused(page)).toBe(`pmenu:${kid}`);
  // Escape closes, focus back on the avatar, and the page stays, live again.
  await page.keyboard.press("Escape");
  await expect(panel(page)).toHaveCount(0);
  await expect.poll(() => focused(page)).toBe("nav:/profiles");
  await expect(page).toHaveURL(/\/library/);
  expect(await page.locator(".screen").evaluate((el) => (el as HTMLElement).inert)).toBe(false);
  // Space opens it too; the Close button closes it.
  await page.keyboard.press("Space");
  await expect(panel(page)).toBeVisible();
  await page.locator('[data-f="pmenu:close"]').click();
  await expect(panel(page)).toHaveCount(0);
  // A click on the dim layer closes it, and so does the avatar a second time.
  await button(page).click();
  await page.mouse.click(400, 600);
  await expect(panel(page)).toHaveCount(0);
  await button(page).click();
  await expect(panel(page)).toBeVisible();
  await button(page).click();
  await expect(panel(page)).toHaveCount(0);
  await expect(page).toHaveURL(/\/library/);
  // Manage profiles opens the profile manager; Settings opens Settings and lights the avatar.
  await button(page).click();
  await page.locator('[data-f="pmenu:manage"]').click();
  await expect(page).toHaveURL(/\/profiles\/manage/);
  await expect(panel(page)).toHaveCount(0);
  await page.goto("/library?form=desktop");
  await button(page).click();
  await page.locator('[data-f="pmenu:settings"]').click();
  await expect(page).toHaveURL(/\/settings/);
  await expect(panel(page)).toHaveCount(0);
  await expect(button(page)).toHaveClass(/\bon\b/);
  // A narrower window keeps the panel's sizes on the smaller scale, the foot pills side by side.
  await page.setViewportSize({ width: 1024, height: 768 });
  await button(page).click();
  await expect(page.locator(".pp-card .pp-last")).toBeVisible({ timeout: 15_000 });
  await shot(page, "1024");
  // A move to another page leaves nothing behind: no layer, nothing inert.
  await page.locator('[data-f="nav:/library"]').click();
  await expect(page).toHaveURL(/\/library/);
  await expect(panel(page)).toHaveCount(0);
  await expect(page.locator("[inert]")).toHaveCount(0);
  // Search from the header closes the panel, and the page under Search stays inert (the panel's cleanup never clears it).
  await button(page).click();
  await expect(panel(page)).toBeVisible();
  await page.locator('[data-f="nav:/search"]').click();
  await expect(panel(page)).toHaveCount(0);
  await expect(page.locator('[data-f="search:input"]')).toBeVisible();
  await expect.poll(() => page.locator(".screen").evaluate((el) => (el as HTMLElement).inert)).toBe(true);
  await context.close();
});

test("a profile without a PIN switches; one with a PIN opens its keypad, and Cancel goes back", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/library");
  // A PIN: its keypad, then Cancel leaves the picker for the page the panel was on.
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
  // Kid's panel: its own card and Main's title; Locked's PIN keeps its title off it; no Manage profiles.
  await button(page).click();
  await expect(page.locator(".pp-card")).toHaveAttribute("aria-label", "Kid, current profile");
  await expect(page.locator(".pp-card .pp-last")).toContainText("Game of Thrones, S2:E3", { timeout: 15_000 });
  await expect(page.locator('[data-f="pmenu:default"] small')).toHaveText("Big Buck Bunny");
  await expect(page.locator(`[data-f="pmenu:${locked}"] small`)).toHaveCount(0);
  await expect(page.locator('[data-f="pmenu:manage"]')).toHaveCount(0);
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
  // Past the profiles and Incognito.
  for (let i = 0; i < 4; i++) await page.keyboard.press("ArrowDown");
  expect(await focused(page)).toBe("pmenu:settings");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/settings/);
  await expect(button(page)).toHaveClass(/\bon\b/);
  // Focus stays on the profile, as it stays on a tab whose place Select showed: checked across 1.5 s, past the
  // page's own first focus.
  for (let t = 0; t < 15; t++) {
    expect(await focused(page)).toBe("nav:/profiles");
    await page.waitForTimeout(100);
  }
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
