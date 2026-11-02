/** Kids profiles on the owner's side (plans/m11-kids.md 6 to 8, K5), driven through the real UI on the shared core:
 * Add profile's switch stays closed until the owner has a PIN, then makes a kids profile; Allow from the title page
 * (one press) and from a tile's menu (two); the list's Remove; Search's add mode; View as the kid, who sees only its
 * list; a title taken off while its page is open; leaving with the owner's PIN; Make standard behind its question.
 * Then the phone's 48 px reach and the TV with arrows, OK (held for a tile's menu) and Back alone. Everything is put
 * back after: no kids profile, no PIN, Main active. */

import { type Page } from "@playwright/test";
import { expect, test } from "./fixture";
import { Client, GOT, SHOTS } from "./core";
import { BBB_ID } from "./source";

const PIN = "4826";
type Profile = { id: string; name: string; kids: boolean; hasPin: boolean; isOwner: boolean };
type Snapshot = { active: string; locked: boolean; kids: boolean; profiles: Profile[] };

/** A protocol session as the owner, whatever the device's last profile was (a kids one included). */
async function asOwner<T>(run: (c: Client, now: () => Snapshot) => Promise<T>): Promise<T> {
  const c = new Client();
  try {
    const view = await c.subscribe<Snapshot>("profiles");
    const s = view.now();
    const pin = s.profiles.find((p) => p.isOwner)?.hasPin ? PIN : undefined;
    if (s.active !== "default" || s.kids) await c.req("switchProfile", { profile: "default", pin });
    else if (s.locked) await c.req("unlockProfile", { pin });
    return await run(c, view.now);
  } finally {
    c.close();
  }
}

/** No kids profile, no PIN, Main active: what every other spec expects of the shared core. */
async function putBack() {
  await asOwner(async (c, now) => {
    for (const p of now().profiles) if (!p.isOwner) await c.req("deleteProfile", { profile: p.id }).catch(() => undefined);
    await c.req("setPin", { profile: "default", pin: null, currentPin: PIN }).catch(() => undefined);
  });
}

/** A kids profile "Kid" allowed Big Buck Bunny, the owner's PIN set: its id. */
async function kidReady(): Promise<string> {
  return asOwner(async (c, now) => {
    if (!now().profiles.find((p) => p.isOwner)?.hasPin) await c.req("setPin", { profile: "default", pin: PIN });
    const made = now().profiles.find((p) => p.kids)?.id ?? (await c.req<Snapshot>("createProfile", { name: "Kid", avatar: "letter:sky", kids: true })).profiles.find((p) => p.kids)?.id;
    await c.req("allowTitle", { profile: made, kind: "movie", itemId: BBB_ID });
    return made as string;
  });
}

test.beforeAll(putBack);
test.afterAll(putBack);

/** Opens `path`; a session the owner's PIN locks shows the picker first, which the owner answers. */
async function open(page: Page, path: string) {
  await page.goto(path);
  const picker = page.locator(".picker, .keypad");
  const app = page.locator(".screen");
  await expect(picker.or(app).first()).toBeVisible({ timeout: 30_000 });
  if (!(await picker.first().isVisible())) return;
  await page.locator('[data-f="profile:default"]').click();
  await expect(page.locator(".keypad h2")).toHaveText("Enter the PIN for Main");
  await page.keyboard.type(PIN);
  await expect(app).toBeVisible({ timeout: 30_000 });
}

const shot = (page: Page, name: string) => page.screenshot({ path: `${SHOTS}/kids-${name}.jpg`, type: "jpeg", quality: 80 });
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);

test("desktop: a kids profile made, filled, watched as the kid, left with the PIN and made standard", async ({ page }) => {
  test.setTimeout(240_000);
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));

  // No PIN on Main: the switch is off and closed, and says why.
  await open(page, "/profiles/new?form=desktop");
  const kidsSwitch = page.locator('[data-f="pedit:kids"]');
  await expect(kidsSwitch).toHaveAttribute("aria-disabled", "true");
  await expect(kidsSwitch).toContainText("Set a PIN on Main first. A kids profile is only as safe as that PIN.");
  await kidsSwitch.click({ force: true });
  await expect(kidsSwitch).toHaveAttribute("aria-checked", "false");

  // Main sets a PIN; the switch opens.
  await open(page, "/profiles/edit/default?form=desktop");
  await page.locator('[data-f="pedit:pin:set"]').click();
  await page.keyboard.type(PIN);
  await page.keyboard.type(PIN);
  await expect(page.locator(".st.ok")).toHaveText("PIN saved");
  await open(page, "/profiles/new?form=desktop");
  await expect(kidsSwitch).toHaveAttribute("aria-disabled", "false");
  await expect(kidsSwitch).toContainText("Shows only the titles you allow. Leaving it needs your PIN.");
  await page.locator('[data-f="pedit:name"]').fill("Kid");
  await kidsSwitch.click();
  await expect(kidsSwitch).toHaveAttribute("aria-checked", "true");
  await expect(page.getByText("Videos come from your addons. Momotus checks titles and names, not what is inside a file.")).toBeVisible();
  await shot(page, "add-desktop");
  await page.locator('[data-f="pedit:save"]').click();
  await expect(page).toHaveURL(/\/profiles\/manage/);
  const row = page.locator(".prow", { hasText: "Kid" });
  await expect(row.locator("small")).toHaveText("Kids · 0 titles");
  const kid = (await asOwner(async (_c, now) => now().profiles.find((p) => p.kids)?.id)) as string;
  expect(kid).toBeTruthy();

  // Allow from the title page: one press.
  await open(page, `/title/movie/${BBB_ID}?form=desktop`);
  const allow = page.locator('[data-f="title:kids"]');
  await expect(allow).toHaveText("Allow for Kid", { timeout: 30_000 });
  await allow.click();
  await expect(allow).toHaveText("Allowed for Kid", { timeout: 15_000 });
  await expect(allow).toHaveAttribute("aria-label", "Allowed for Kid, select to remove");
  await shot(page, "title-desktop");

  // Allow from a tile's menu: a right click, then the item.
  await open(page, "/library?form=desktop");
  const got = page.locator(`a.tile[href$="/series/${GOT.id}"]`).first();
  await got.click({ button: "right" });
  const menu = page.getByRole("dialog", { name: "Game of Thrones" });
  await expect(menu).toBeVisible();
  await shot(page, "tile-menu-desktop");
  await menu.getByRole("menuitem", { name: "Allow for Kid" }).click();
  await expect(menu).toBeHidden();
  await got.click({ button: "right" });
  await expect(menu.getByRole("menuitem", { name: "Remove from Kid's list" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();

  // The kid's list: both, newest first; Remove takes Game of Thrones off.
  await open(page, `/profiles/edit/${kid}?form=desktop`);
  await expect(page.getByRole("heading", { name: "What Kid can watch" })).toBeVisible();
  const rows = page.locator(".kids-row b");
  await expect(rows).toHaveText(["Game of Thrones", "Big Buck Bunny"], { timeout: 15_000 });
  await shot(page, "list-desktop");
  await page.getByRole("button", { name: "Remove Game of Thrones from Kid's list" }).click();
  await expect(rows).toHaveText(["Big Buck Bunny"]);

  // Add titles: Search with an Allow switch on each result; on, then off again.
  await page.locator('[data-f="pedit:kids:add"]').click();
  await expect(page).toHaveURL(/\/search\?add=/);
  await expect(page.getByRole("heading", { name: "Add titles for Kid" })).toBeVisible();
  await page.locator('[data-f="search:input"]').fill("Game of Thrones");
  const addSwitch = page.getByRole("switch", { name: "Allow Game of Thrones" }).first();
  await expect(addSwitch).toHaveAttribute("aria-checked", "false", { timeout: 30_000 });
  await addSwitch.click();
  await expect(addSwitch).toHaveAttribute("aria-checked", "true", { timeout: 15_000 });
  await shot(page, "add-titles-desktop");
  await addSwitch.click();
  await expect(addSwitch).toHaveAttribute("aria-checked", "false", { timeout: 15_000 });
  await page.goBack();
  await expect(rows).toHaveText(["Big Buck Bunny"]);

  // View as Kid: free, and the kid sees only its list.
  await page.locator('[data-f="pedit:kids:view"]').click();
  await expect(page).toHaveURL(/\/(\?form=desktop)?$/, { timeout: 15_000 });
  await expect(page.getByRole("region", { name: "Your shows and movies" }).locator("a.tile")).toHaveCount(1, { timeout: 30_000 });
  await expect(page.locator('a.tile[href$="/movie/' + BBB_ID + '"]').first()).toBeVisible();
  await expect(page.locator('[data-f="nav:/discover"], [data-f="nav:/calendar"]')).toHaveCount(0);
  await expect(page.locator('[data-f="nav:/profiles"]')).toHaveAttribute("aria-label", "Profile: Kid. Switching needs a grown-up.");
  await page.locator('[data-f="nav:/library"]').click();
  await expect(page.locator("a.tile")).toHaveCount(1, { timeout: 15_000 });

  // Taken off while its page is open (here by the owner on another session): the page leaves for Home and says why.
  await page.locator("a.tile").first().click();
  await expect(page).toHaveURL(new RegExp(`/title/movie/${BBB_ID}`));
  await expect(page.locator('[data-f="title:play"]')).toBeVisible({ timeout: 30_000 });
  await asOwner((c) => c.req("removeTitle", { profile: kid, itemId: BBB_ID }));
  await expect(page.getByRole("status").filter({ hasText: "A grown-up took this off your list." })).toBeVisible({ timeout: 15_000 });
  await expect(page).not.toHaveURL(/\/title\//);
  await shot(page, "taken-off-desktop");
  await asOwner((c) => c.req("allowTitle", { profile: kid, kind: "movie", itemId: BBB_ID }));

  // Leaving asks Main's PIN, whatever profile is picked: a wrong one, then the right one.
  await page.locator('[data-f="nav:/profiles"]').click();
  await page.locator('[data-f="pmenu:default"]').click();
  await expect(page.locator(".keypad h2")).toHaveText("Ask a grown-up to enter Main's PIN");
  await page.keyboard.type("2580");
  await expect(page.locator(".pin-said")).toHaveText("Wrong PIN");
  await shot(page, "exit-desktop");
  await page.keyboard.type(PIN);
  await expect(page.locator('[data-f="nav:/profiles"]')).toHaveAttribute("aria-label", "Profile: Main", { timeout: 15_000 });
  await expect(page.locator('[data-f="nav:/discover"]')).toBeVisible();

  // Make standard: asked first, the safe choice focused; Back keeps it, then Make standard.
  await open(page, `/profiles/edit/${kid}?form=desktop`);
  await page.locator('[data-f="pedit:kids:standard"]').click();
  const ask = page.getByRole("alertdialog", { name: "Make Kid a standard profile?" });
  await expect(ask).toContainText("Kid will see everything your addons show and can leave without your PIN.");
  await expect.poll(() => focused(page)).toBe("pedit:kids:standard:no");
  await shot(page, "standard-desktop");
  await page.keyboard.press("Escape");
  await expect(ask).toBeHidden();
  await page.locator('[data-f="pedit:kids:standard"]').click();
  await page.locator('[data-f="pedit:kids:standard:yes"]').click();
  await expect(page.getByRole("heading", { name: "What Kid can watch" })).toBeHidden({ timeout: 15_000 });
  await expect(page.locator(".pedit-pin")).toContainText("Kid has no PIN.");
  expect((await asOwner(async (_c, now) => now().profiles.find((p) => p.id === kid)))?.kids).toBe(false);
  expect(errors).toEqual([]);
});

/** Each control's reach around its centre, as a finger finds it (e2e/phone-targets.spec.ts's measure). */
async function reaches(page: Page, selector: string) {
  return page.evaluate((selector) => {
    const out: { name: string; w: number; h: number }[] = [];
    for (const el of document.querySelectorAll<HTMLElement>(selector)) {
      el.scrollIntoView({ block: "center", inline: "center" });
      const r = el.getBoundingClientRect();
      const cx = r.left + r.width / 2;
      const cy = r.top + r.height / 2;
      const mine = (x: number, y: number) => document.elementFromPoint(x, y)?.closest("button, a[href], [role=switch], [role=menuitem]") === el;
      const run = (dx: number, dy: number) => {
        let d = 0;
        while (d < 40 && mine(cx + dx * (d + 0.5), cy + dy * (d + 0.5))) d += 0.5;
        return d;
      };
      out.push({ name: el.dataset.f ?? el.className, w: run(1, 0) + run(-1, 0), h: run(0, 1) + run(0, -1) });
    }
    return out;
  }, selector);
}
const short = (rows: { name: string; w: number; h: number }[]) => rows.filter((r) => r.w < 47.5 || r.h < 47.5).map((r) => `${r.name} ${r.w}x${r.h}`);

test("phone: the kids controls reach 48 x 48", async ({ browser }) => {
  test.setTimeout(180_000);
  const kid = await kidReady();
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
  const page = await context.newPage();
  const found: string[] = [];

  await open(page, "/profiles/new?form=phone");
  await expect(page.locator('[data-f="pedit:kids"]')).toBeVisible();
  found.push(...short(await reaches(page, '[data-f="pedit:kids"]')));

  await open(page, `/title/movie/${BBB_ID}?form=phone`);
  await expect(page.locator('[data-f="title:kids"]')).toHaveText("Allowed for Kid", { timeout: 30_000 });
  found.push(...short(await reaches(page, '[data-f="title:kids"]')));
  await shot(page, "title-phone");

  // A long press on a tile opens its menu (the browser's contextmenu); its items are full rows.
  await open(page, "/library?form=phone");
  await page.locator(`a.tile[href$="/series/${GOT.id}"]`).first().dispatchEvent("contextmenu");
  await expect(page.getByRole("menuitem", { name: "Allow for Kid" })).toBeVisible();
  found.push(...short(await reaches(page, "[role=menuitem]")));
  await shot(page, "tile-menu-phone");
  await page.locator(".dl-scrim").click({ position: { x: 10, y: 10 } });

  await open(page, `/profiles/edit/${kid}?form=phone`);
  await expect(page.locator(".kids-row button")).toHaveCount(1, { timeout: 15_000 });
  found.push(...short(await reaches(page, '.kids-row button, [data-f="pedit:kids:add"], [data-f="pedit:kids:view"], [data-f="pedit:kids:standard"]')));
  await shot(page, "list-phone");

  await open(page, `/search?add=${kid}&form=phone`);
  await page.locator('[data-f="search:input"]').fill("Game of Thrones");
  await expect(page.getByRole("switch", { name: "Allow Game of Thrones" }).first()).toBeVisible({ timeout: 30_000 });
  found.push(...short(await reaches(page, ".hit-allow")));
  await shot(page, "add-titles-phone");
  expect(found).toEqual([]);
  await context.close();
});

test("TV: arrows, OK and Back alone allow from the title page and a held OK's tile menu, and keep a kids profile", async ({ browser }) => {
  test.setTimeout(180_000);
  const kid = await kidReady();
  await asOwner((c) => c.req("removeTitle", { profile: kid, itemId: BBB_ID }));
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 } });
  const page = await context.newPage();

  // The title page: Right until the kids control, OK allows; OK again removes.
  await open(page, `/title/movie/${BBB_ID}?form=tv`);
  await expect(page.locator('[data-f="title:kids"]')).toHaveText("Allow for Kid", { timeout: 30_000 });
  await expect.poll(() => focused(page), { timeout: 15_000 }).not.toBe(null);
  for (let i = 0; i < 8 && (await focused(page)) !== "title:kids"; i++) await page.keyboard.press(i < 1 ? "ArrowDown" : "ArrowRight");
  expect(await focused(page)).toBe("title:kids");
  await page.keyboard.press("Enter");
  await expect(page.locator('[data-f="title:kids"]')).toHaveText("Allowed for Kid", { timeout: 15_000 });
  await shot(page, "title-tv");

  // A tile with OK held: its menu, focus on its one item, a single focus on screen; Back closes it on the tile.
  await open(page, "/library?form=tv");
  const tile = page.locator(`a.tile[href$="/series/${GOT.id}"]`).first();
  await tile.focus();
  await page.keyboard.down("Enter");
  await page.waitForTimeout(700);
  await page.keyboard.up("Enter");
  await expect(page).toHaveURL(/\/library/);
  await expect.poll(() => focused(page)).toBe(`kidsmenu:${kid}`);
  expect(await page.evaluate(() => document.activeElement?.closest("[role=dialog]") !== null)).toBe(true);
  // The focus shows: the item turns white, as every TV control does.
  expect(await page.evaluate(() => getComputedStyle(document.activeElement as Element).backgroundColor)).toMatch(/^rgba?\(255, 255, 255/);
  await shot(page, "tile-menu-tv");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toBeHidden();
  await expect.poll(() => page.evaluate(() => (document.activeElement as HTMLElement | null)?.getAttribute("href") ?? "")).toContain(GOT.id);
  // Held again, OK on the item allows; a short OK still opens the title.
  await page.keyboard.down("Enter");
  await page.waitForTimeout(700);
  await page.keyboard.up("Enter");
  await expect.poll(() => focused(page)).toBe(`kidsmenu:${kid}`);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog")).toBeHidden();
  expect(await asOwner(async (c) => (await c.subscribe<{ profiles: { keys: string[] }[] }>("kidsKeys")).now().profiles[0]?.keys.sort())).toEqual([`movie:${BBB_ID}`, `series:${GOT.id}`]);
  await tile.focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(new RegExp(`/title/series/${GOT.id}`));

  // Make standard on the remote: the question opens on its safe choice, Back keeps the kids profile.
  await open(page, `/profiles/edit/${kid}?form=tv`);
  await page.locator('[data-f="pedit:kids:standard"]').focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => focused(page)).toBe("pedit:kids:standard:no");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("alertdialog")).toBeHidden();
  await expect(page.locator('[data-f="pedit:kids:standard"]')).toBeVisible();
  await context.close();
});

test("desktop: made standard from another session, the kid's open app goes on as a standard profile from Home", async ({ page }) => {
  const kid = await kidReady();
  // The device's sessions open as the kid from here on.
  await asOwner((c) => c.req("switchProfile", { profile: kid }));
  await page.goto("/?form=desktop");
  await page.getByRole("button", { name: "Kid, kids profile, current profile" }).click();
  await expect(page.getByRole("region", { name: "Your shows and movies" })).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-f="nav:/discover"]')).toHaveCount(0);
  await asOwner((c) => c.req("makeStandard", { profile: kid }));
  // The core closed the kid's views: Home opens anew with a standard profile's rows and the whole bar.
  await expect(page.locator('[data-f="nav:/discover"]')).toBeVisible({ timeout: 15_000 });
  await expect(page.locator('[data-f="nav:/profiles"]')).toHaveAttribute("aria-label", "Profile: Kid");
  await expect(page.getByRole("region", { name: "Your shows and movies" })).toHaveCount(0, { timeout: 15_000 });
  await expect(page.locator("section.row").first()).toBeVisible({ timeout: 30_000 });
});

test("desktop: with two kids profiles the title page's Kids opens Who can watch this?, a switch for each", async ({ page }) => {
  const kid = await kidReady();
  const ada = await asOwner(async (c) => (await c.req<Snapshot>("createProfile", { name: "Ada", avatar: "letter:sea", kids: true })).profiles.find((p) => p.name === "Ada")?.id as string);
  await open(page, `/title/movie/${BBB_ID}?form=desktop`);
  const kidsButton = page.locator('[data-f="title:kids"]');
  await expect(kidsButton).toHaveText("Kids", { timeout: 30_000 });
  await kidsButton.click();
  const sheet = page.getByRole("dialog", { name: "Who can watch this?" });
  await expect(sheet.getByRole("switch", { name: "Kid" })).toHaveAttribute("aria-checked", "true");
  await expect(sheet.getByRole("switch", { name: "Ada" })).toHaveAttribute("aria-checked", "false");
  await expect.poll(() => focused(page)).toBe(`kidsmenu:${kid}`);
  await sheet.getByRole("switch", { name: "Ada" }).click();
  await expect(sheet.getByRole("switch", { name: "Ada" })).toHaveAttribute("aria-checked", "true", { timeout: 15_000 });
  await shot(page, "sheet-desktop");
  // The sheet stays for the next switch; Done closes it, back on Kids.
  await sheet.getByRole("button", { name: "Done" }).click();
  await expect(sheet).toBeHidden();
  await expect.poll(() => focused(page)).toBe("title:kids");
  expect(await asOwner(async (c) => (await c.subscribe<{ profiles: { profile: string; keys: string[] }[] }>("kidsKeys")).now().profiles.find((p) => p.profile === ada)?.keys)).toEqual([`movie:${BBB_ID}`]);
});
