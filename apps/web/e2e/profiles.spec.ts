import { expect, type Page, test } from "@playwright/test";
import { Client } from "./core";

/** Profiles on the remote (plans/m11-profiles.md, P1 apps): add, the start
 * picker, a PIN set twice, a switch both ways, a wrong PIN, the locked cold
 * start, then everything put back so later tests see one unlocked profile. */

test.use({ viewport: { width: 1920, height: 1080 } });

const PIN = "2580";
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
type Snapshot = { active: string; locked: boolean; profiles: { id: string; name: string; hasPin: boolean }[] };

async function picker(): Promise<Snapshot> {
  const c = new Client();
  try {
    const s = await c.subscribe<Snapshot>("profiles");
    return s.now();
  } finally {
    c.close();
  }
}

test.afterAll(async () => {
  // Whatever a failure left: Main without a PIN, active, and no other profile.
  const c = new Client();
  try {
    const s = await c.subscribe<Snapshot>("profiles");
    if (s.now().locked) await c.req("unlockProfile", { pin: PIN }).catch(() => undefined);
    if (s.now().active !== "default") await c.req("switchProfile", { profile: "default", pin: PIN }).catch(() => undefined);
    await c.req("setPin", { profile: "default", pin: null, currentPin: PIN }).catch(() => undefined);
    for (const p of s.now().profiles) if (p.id !== "default") await c.req("deleteProfile", { profile: p.id }).catch(() => undefined);
  } finally {
    c.close();
  }
});

test("profiles: add, pick, PIN, switch both ways, locked start", async ({ page }) => {
  // One profile: Home opens directly, and Settings names it.
  await page.goto("/settings?form=tv");
  await expect(page.locator('[data-f="settings:profiles"]')).toContainText("Main");

  // Add Kid with a cat on sky blue, from the picker's Add tile.
  await page.goto("/profiles?form=tv");
  await expect.poll(() => focused(page)).toBe("profile:default");
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => focused(page)).toBe("profile:add");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/profiles\/new/);
  await page.locator('[data-f="pedit:name"]').fill("Kid");
  await page.locator('[data-f="pedit:glyph:cat"]').click();
  await page.locator('[data-f="pedit:color:sky"]').click();
  await page.locator('[data-f="pedit:save"]').click();
  await expect(page).toHaveURL(/\/profiles\/manage/);
  await expect(page.locator(".prow", { hasText: "Kid" })).toBeVisible();
  const kid = (await picker()).profiles.find((p) => p.name === "Kid")?.id as string;
  expect(kid).toBeTruthy();

  // A cold start with two profiles asks who is watching; Main opens Home.
  await page.goto("/?form=tv");
  await expect(page.locator(".picker h1")).toHaveText("Who's watching?");
  await expect.poll(() => focused(page)).toBe("profile:default");
  // The start picker only chooses: managing waits for the open app.
  await expect(page.locator('[data-f="profile:add"], [data-f="profiles:manage"]')).toHaveCount(0);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("region", { name: "Featured" }).first()).toBeVisible();

  // Main sets a PIN: entered twice; a mismatch starts over from the first entry.
  await page.goto("/profiles/edit/default?form=tv");
  await page.locator('[data-f="pedit:pin:set"]').click();
  await expect(page.locator(".keypad h2")).toHaveText("Choose a PIN");
  await page.keyboard.type(PIN);
  await expect(page.locator(".keypad h2")).toHaveText("Enter it again");
  await page.keyboard.type("1111");
  await expect(page.locator(".pin-said")).toHaveText("The PINs did not match. Choose one again.");
  await expect(page.locator(".keypad h2")).toHaveText("Choose a PIN");
  await page.keyboard.type(PIN);
  await expect(page.locator(".keypad h2")).toHaveText("Enter it again");
  await page.keyboard.type(PIN);
  await expect(page.locator(".pedit .st")).toContainText("PIN saved");
  expect((await picker()).profiles.find((p) => p.id === "default")?.hasPin).toBe(true);

  // Switch to Kid (no PIN): Home opens as Kid, and the pill shows Kid.
  await page.goto("/profiles?form=tv");
  await page.locator(`[data-f="profile:${kid}"]`).focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/(\?form=tv)?$/);
  await expect(page.locator('[data-f="nav:/profiles"]')).toHaveAttribute("aria-label", "Profile: Kid, switch profile");
  expect((await picker()).active).toBe(kid);

  // The browser's own Back never reopens Main's pages under Kid, across a reload too, and the app
  // goes on from there without bouncing back Home.
  await page.goBack();
  await expect(page.locator('[data-f="nav:/profiles"]')).toHaveAttribute("aria-label", "Profile: Kid, switch profile");
  await expect(page).not.toHaveURL(/\/profiles\/edit\/default/);
  await page.locator('[data-f="nav:/library"]').click();
  await expect(page).toHaveURL(/\/library/);
  await page.locator('[data-f="nav:/settings"]').click();
  await expect(page).toHaveURL(/\/settings/);
  await expect(page.locator('[data-f="settings:profiles"]')).toContainText("Kid");
  // Kits are Main's to make (D17): Kid reaching the kit page by its address lands on the sync page.
  await page.goto("/sync/kit?form=tv");
  await expect(page).toHaveURL(/\/sync$/);

  // Back to Main needs its PIN: a wrong one says so, the right one switches.
  await page.goto("/profiles?form=tv");
  await page.locator('[data-f="profile:default"]').focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".keypad h2")).toHaveText("Enter the PIN for Main");
  await expect.poll(() => focused(page)).toBe("pin:5");
  await page.keyboard.type("1111");
  await expect(page.locator(".pin-said")).toHaveText("Wrong PIN");
  await page.keyboard.type(PIN);
  await expect(page.locator('[data-f="nav:/profiles"]')).toHaveAttribute("aria-label", "Profile: Main, switch profile");

  // A cold start on a protected profile is locked until its PIN.
  await page.goto("/library?form=tv");
  await expect(page.locator(".picker .lede")).toContainText("This profile is locked");
  await page.locator('[data-f="profile:default"]').focus();
  await page.keyboard.press("Enter");
  await page.keyboard.type(PIN);
  // Unlocked, it opens where it was asked to.
  await expect(page.locator('[data-f="nav:/profiles"]')).toBeVisible();
  await expect(page).toHaveURL(/\/library/);

  // Put it back, inside the app (a reload would lock it again): Main's PIN
  // removed with the current one, then Kid deleted.
  await page.locator('[data-f="nav:/settings"]').click();
  await page.locator('[data-f="settings:profile-edit"]').click();
  await page.locator('[data-f="pedit:pin:remove"]').click();
  await page.keyboard.type(PIN);
  await expect(page.locator(".pedit .st")).toContainText("PIN removed");
  await page.goto(`/profiles/edit/${kid}?form=tv`);
  // Deleting asks first; Escape keeps the profile, and the page behind waits meanwhile.
  await page.locator('[data-f="pedit:delete"]').click();
  await expect(page.locator(".topnav")).toHaveJSProperty("inert", true);
  // It opens on Keep: a held OK can never delete.
  await expect.poll(() => focused(page)).toBe("pedit:delete:no");
  await page.keyboard.press("Escape");
  await expect.poll(() => focused(page)).toBe("pedit:delete");
  await expect(page.locator('[data-f="pedit:delete:yes"]')).toHaveCount(0);
  await expect(page.locator(".topnav")).toHaveJSProperty("inert", false);
  await page.locator('[data-f="pedit:delete"]').click();
  await page.locator('[data-f="pedit:delete:yes"]').click();
  await expect(page).toHaveURL(/\/profiles\/manage/);
  const end = await picker();
  expect(end.profiles.map((p) => p.id)).toEqual(["default"]);
  expect(end.profiles[0]?.hasPin).toBe(false);
});

test("profiles: the browser's Forward never reopens another profile's pages", async ({ page }) => {
  const c = new Client();
  let kid = "";
  try {
    const s = await c.req<Snapshot>("createProfile", { name: "Kid", avatar: "letter:sky" });
    kid = s.profiles.find((p) => p.name === "Kid")?.id as string;
    // Main browses: Home, Library, Calendar, then Back twice to Home, with Main's pages ahead.
    await page.goto("/?form=tv");
    await page.locator('[data-f="profile:default"]').click();
    await page.locator('[data-f="nav:/library"]').click();
    await expect(page).toHaveURL(/\/library/);
    await page.locator('[data-f="nav:/calendar"]').click();
    await expect(page).toHaveURL(/\/calendar/);
    await page.goBack();
    await page.goBack();
    await expect(page.locator('[data-f="nav:/profiles"]')).toHaveAttribute("aria-label", "Profile: Main, switch profile");
    // Another device or tab makes Kid the device's profile; this tab reloads and picks Kid.
    await c.req("switchProfile", { profile: kid });
    await page.reload();
    await page.locator(`[data-f="profile:${kid}"]`).click();
    await expect(page.locator('[data-f="nav:/profiles"]')).toHaveAttribute("aria-label", "Profile: Kid, switch profile");
    // Forward reaches Main's Library: it opens as Kid's Home instead.
    await page.goForward();
    await expect(page).not.toHaveURL(/\/library/);
    await expect(page.locator('[data-f="nav:/profiles"]')).toHaveAttribute("aria-label", "Profile: Kid, switch profile");
  } finally {
    await c.req("switchProfile", { profile: "default" }).catch(() => undefined);
    if (kid) await c.req("deleteProfile", { profile: kid }).catch(() => undefined);
    c.close();
  }
});
