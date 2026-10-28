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

  // Add Kid with a cat, from the picker's Add tile (the colour is the next grey step, never picked).
  await page.goto("/profiles?form=tv");
  await expect.poll(() => focused(page)).toBe("profile:default");
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => focused(page)).toBe("profile:add");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/profiles\/new/);
  await page.locator('[data-f="pedit:name"]').fill("Kid");
  await page.locator('[data-f="pedit:glyph:cat"]').click();
  await expect(page.locator('[data-f^="pedit:color"]')).toHaveCount(0);
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

/** A second profile for the tests below, made through the protocol and removed in `finally`. */
async function withKid<T>(name: string, run: (id: string) => Promise<T>): Promise<T> {
  const c = new Client();
  let id = "";
  try {
    const s = await c.req<Snapshot>("createProfile", { name, avatar: "letter:sky" });
    id = s.profiles.find((p) => p.name === name)?.id as string;
    return await run(id);
  } finally {
    // Whatever its name became while the test ran.
    const s = await c.subscribe<Snapshot>("profiles");
    for (const p of s.now().profiles) if (p.id === id || p.name.startsWith(name)) await c.req("deleteProfile", { profile: p.id }).catch(() => undefined);
    c.close();
  }
}

const dialog = (page: Page) => page.getByRole("alertdialog", { name: "Leave without saving?" });
const names = async () => (await picker()).profiles.map((p) => p.name);

test("profiles: Add and Save answer one press once, and an empty name is said under its field", async ({ page }) => {
  await page.goto("/profiles/new?form=desktop");
  // An empty name is said under the field, which says it is invalid and is described by it.
  await page.locator('[data-f="pedit:save"]').click();
  const name = page.locator('[data-f="pedit:name"]');
  const said = page.locator("#pedit-name-error");
  await expect(said).toBeVisible();
  await expect(name).toHaveAttribute("aria-invalid", "true");
  await expect(name).toHaveAttribute("aria-describedby", "pedit-name-error");
  await expect(name).toBeFocused();
  const [field, line] = await Promise.all([name.boundingBox(), said.boundingBox()]);
  expect((line?.y ?? 0) - ((field?.y ?? 0) + (field?.height ?? 0))).toBeLessThan(40);
  await name.fill("T");
  await expect(said).toHaveCount(0);
  // Two quick presses are one profile.
  await name.fill("Twin");
  try {
    await page.locator('[data-f="pedit:save"]').dblclick();
    await expect(page).toHaveURL(/\/profiles\/manage/);
    expect((await names()).filter((n) => n === "Twin")).toHaveLength(1);
  } finally {
    const c = new Client();
    try {
      for (const p of (await picker()).profiles) if (p.name === "Twin") await c.req("deleteProfile", { profile: p.id }).catch(() => undefined);
    } finally {
      c.close();
    }
  }
});

test("profiles: unsaved edits ask before any way out, and Keep editing keeps them", async ({ page }) => {
  await withKid("Dirty Kid", async (kid) => {
    await page.goto(`/library?form=desktop`);
    await page.goto(`/profiles/edit/${kid}?form=desktop`);
    const name = page.locator('[data-f="pedit:name"]');
    await expect(name).toHaveValue("Dirty Kid");
    // Nothing changed: nothing to ask. Back goes straight.
    await page.goBack();
    await expect(page).toHaveURL(/\/library/);
    await page.goForward();
    await expect(name).toHaveValue("Dirty Kid");

    // Escape in the Name field asks; Keep editing comes back to the field with the text.
    await name.fill("Dirty Kid 2");
    await name.press("Escape");
    await expect(dialog(page)).toBeVisible();
    await expect.poll(() => focused(page)).toBe("pedit:leave:keep");
    await page.keyboard.press("Escape");
    await expect(dialog(page)).toHaveCount(0);
    await expect(page).toHaveURL(new RegExp(`/profiles/edit/${kid}`));
    await expect(name).toHaveValue("Dirty Kid 2");

    // A tab link asks too; Discard goes on to where it was going and the stored name is untouched.
    await page.locator('[data-f="nav:/library"]').click();
    await expect(dialog(page)).toBeVisible();
    await page.locator('[data-f="pedit:leave:discard"]').click();
    await expect(page).toHaveURL(/\/library/);
    expect(await names()).toContain("Dirty Kid");

    // The browser's Back asks; Save stores the name and then leaves.
    await page.goto(`/profiles/edit/${kid}?form=desktop`);
    await name.fill("Dirty Kid 3");
    await page.goBack();
    await expect(dialog(page)).toBeVisible();
    await page.locator('[data-f="pedit:leave:save"]').click();
    await expect(page).not.toHaveURL(/\/profiles\/edit/);
    expect(await names()).toContain("Dirty Kid 3");
  });
});

test("profiles on TV: the rail's focus alone asks nothing, Select on a tab asks, and a Save after Keep editing opens the next page on its own first focus", async ({ page }) => {
  // Reached from Manage profiles, so there is history to go back through (a deep link has none).
  await page.goto("/profiles/manage?form=tv");
  await page.locator('[data-f="manage:add"]').focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/profiles\/new/);
  await page.locator('[data-f="pedit:name"]').fill("Rail Kid");
  // Up into the rail, onto a tab: the focus rests there longer than the settle, and nothing asks or opens.
  await page.locator('[data-f="nav:/library"]').focus();
  await page.waitForTimeout(500);
  await expect(dialog(page)).toHaveCount(0);
  await expect(page).toHaveURL(/\/profiles\/new/);
  // Select on it asks; Back keeps the edits.
  await page.keyboard.press("Enter");
  await expect(dialog(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toHaveCount(0);
  try {
    // Back on the rail goes Home past this page: it asks, and Keep editing must leave no "then Home" behind.
    await page.locator('[data-f="nav:/library"]').focus();
    await page.keyboard.press("Escape");
    await expect(dialog(page)).toBeVisible();
    await page.locator('[data-f="pedit:leave:keep"]').press("Enter");
    await expect(dialog(page)).toHaveCount(0);
    // Back from the form asks too; Keep editing then Save: Manage shows its first focus, not a place restored, and
    // not Home.
    await page.locator('[data-f="pedit:name"]').focus();
    await page.keyboard.press("Escape");
    await expect(dialog(page)).toBeVisible();
    await page.locator('[data-f="pedit:leave:keep"]').press("Enter");
    await expect(dialog(page)).toHaveCount(0);
    await page.locator('[data-f="pedit:save"]').focus();
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/profiles\/manage/);
    await expect.poll(() => focused(page)).toBe("manage:default");
    await page.waitForTimeout(500);
    await expect(page).toHaveURL(/\/profiles\/manage/);
  } finally {
    const c = new Client();
    try {
      for (const p of (await picker()).profiles) if (p.name === "Rail Kid") await c.req("deleteProfile", { profile: p.id }).catch(() => undefined);
    } finally {
      c.close();
    }
  }
});

test("profiles: the Delete key on an empty keypad only deletes, and a PIN taken off with one tap is asked about", async ({ page }) => {
  await page.goto("/profiles/edit/default?form=desktop");
  await page.locator('[data-f="pedit:pin:set"]').click();
  await expect(page.locator(".keypad h2")).toHaveText("Choose a PIN");
  // Delete on nothing: the keypad stays. On a digit it takes it back.
  await page.locator('[data-f="pin:del"]').click();
  await expect(page.locator(".keypad h2")).toHaveText("Choose a PIN");
  await page.locator('[data-f="pin:1"]').click();
  await expect(page.locator(".pin-dots")).toHaveAttribute("aria-label", "1 of 4 digits entered");
  await page.locator('[data-f="pin:del"]').click();
  await expect(page.locator(".pin-dots")).toHaveAttribute("aria-label", "0 of 4 digits entered");
  // Backspace and Back, with nothing to delete, still leave (a remote has no other key for it).
  await page.keyboard.press("Escape");
  await expect(page.locator(".keypad")).toHaveCount(0);

  // Another profile's PIN, taken off by its owner: one tap asks first.
  const c = new Client();
  let kid = "";
  try {
    kid = (await c.req<Snapshot>("createProfile", { name: "Pin Kid", avatar: "letter:sky" })).profiles.find((p) => p.name === "Pin Kid")?.id as string;
    await c.req("switchProfile", { profile: kid });
    await c.req("setPin", { profile: kid, pin: PIN });
    await c.req("switchProfile", { profile: "default" });
    await page.goto(`/profiles/edit/${kid}?form=desktop`);
    await page.locator('[data-f="pedit:pin:clear"]').click();
    const ask = page.getByRole("alertdialog", { name: "Remove Pin Kid's PIN?" });
    await expect(ask).toBeVisible();
    await expect.poll(() => focused(page)).toBe("pedit:pin:no");
    await page.locator('[data-f="pedit:pin:no"]').click();
    expect((await picker()).profiles.find((p) => p.id === kid)?.hasPin).toBe(true);
    await page.locator('[data-f="pedit:pin:clear"]').click();
    await page.locator('[data-f="pedit:pin:yes"]').click();
    await expect(page.locator(".pedit .st")).toContainText("PIN was removed");
    expect((await picker()).profiles.find((p) => p.id === kid)?.hasPin).toBe(false);
  } finally {
    await c.req("switchProfile", { profile: "default" }).catch(() => undefined);
    if (kid) await c.req("deleteProfile", { profile: kid }).catch(() => undefined);
    c.close();
  }
});

test("profiles: avatars are circles, the chosen picture is the grey fill, and the Add tile and the PIN keys have no edge", async ({ page }) => {
  await page.goto("/profiles?form=desktop");
  await expect(page.locator(".ptile .avatar").first()).toBeVisible();
  const style = (sel: string) => page.locator(sel).first().evaluate((el) => ({ radius: getComputedStyle(el).borderTopLeftRadius, shadow: getComputedStyle(el).boxShadow, fill: getComputedStyle(el).backgroundColor }));
  const tile = await style(".ptile .avatar");
  expect(tile.radius).toBe("84px");
  expect(tile.shadow).toBe("none");
  await page.goto("/profiles/new?form=desktop");
  await expect(page.locator('[data-f="pedit:name"]')).toBeVisible();
  // No Color row: the colour is the next grey step.
  await expect(page.locator('[role="radiogroup"][aria-label="Color"]')).toHaveCount(0);
  const chosen = await page.locator(".glyph-pick.on").first().evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(chosen).toBe("rgb(99, 99, 102)");
  expect(await style(".glyph-pick.on")).toMatchObject({ shadow: "none" });
  await page.goto("/profiles?form=desktop");
  // Add profile is on the picker only for the owner with room: it is there on a core with one profile.
  const add = page.locator(".ptile.add .add-art");
  await expect(add).toBeVisible();
  expect((await style(".ptile.add .add-art")).shadow).toBe("none");
});

/** Holds the core's answer to one operation until `release()`, on this page's own socket only; `sent()` counts the asks. */
async function holdAnswers(page: Page, op: string) {
  const held: string[] = [];
  const waiting = new Set<number>();
  let open = false;
  let asked = 0;
  let sendHeld = () => {};
  await page.routeWebSocket(/\/ws$/, (ws) => {
    const core = ws.connectToServer();
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m)) as { id?: number; op?: string };
      if (msg.op === op && msg.id !== undefined) {
        asked++;
        waiting.add(msg.id);
      }
      core.send(String(m));
    });
    core.onMessage((m) => {
      const msg = JSON.parse(String(m)) as { id?: number };
      if (!open && msg.id !== undefined && waiting.has(msg.id)) held.push(String(m));
      else ws.send(String(m));
    });
    sendHeld = () => held.splice(0).forEach((m) => ws.send(m));
  });
  return {
    sent: () => asked,
    release: () => {
      open = true;
      sendHeld();
    },
  };
}

test("profiles: a picture chosen while Save is with the core stays unsaved and still asks", async ({ page }) => {
  await withKid("Slow Kid", async (kid) => {
    const core = await holdAnswers(page, "updateProfile");
    await page.goto(`/profiles/edit/${kid}?form=desktop`);
    const name = page.locator('[data-f="pedit:name"]');
    await expect(name).toHaveValue("Slow Kid");
    await page.locator('[data-f="pedit:glyph:cat"]').click();
    await page.locator('[data-f="pedit:save"]').click();
    await expect.poll(() => core.sent()).toBe(1);
    // Another picture while the first is with the core.
    await page.locator('[data-f="pedit:glyph:gamepad"]').click();
    core.release();
    await expect(page.locator(".pedit .st")).toContainText("Saved");
    // The second picture was never sent: leaving still asks.
    await name.press("Escape");
    await expect(dialog(page)).toBeVisible();
  });
});

test("profiles: Save in the leave question pressed twice sends once, stays open until the answer, then leaves", async ({ page }) => {
  await withKid("Twice Kid", async (kid) => {
    const core = await holdAnswers(page, "updateProfile");
    await page.goto("/library?form=desktop");
    await page.goto(`/profiles/edit/${kid}?form=desktop`);
    await page.locator('[data-f="pedit:name"]').fill("Twice Kid 2");
    await page.locator('[data-f="nav:/library"]').click();
    await expect(dialog(page)).toBeVisible();
    await page.locator('[data-f="pedit:leave:save"]').dblclick();
    await expect.poll(() => core.sent()).toBe(1);
    await expect(dialog(page)).toBeVisible();
    core.release();
    await expect(page).toHaveURL(/\/library/);
    expect(await names()).toContain("Twice Kid 2");
  });
});

test("profiles on a phone: the leave question comes into view, whatever the page's length, and is not under the tab bar", async ({ browser }) => {
  await withKid("Phone Kid", async (kid) => {
    const context = await browser.newContext({ viewport: { width: 390, height: 700 }, hasTouch: true, isMobile: true, reducedMotion: "reduce" });
    const page = await context.newPage();
    await page.goto(`/profiles/edit/${kid}?form=phone`);
    await page.locator('[data-f="pedit:name"]').fill("Phone Kid 2");
    // A tap on a tab, which brings no focus to the question (autofocus waits for keys).
    await page.locator('.tabs a[href="/library"]').tap();
    await expect(dialog(page)).toBeVisible();
    const keep = page.locator('[data-f="pedit:leave:keep"]');
    await expect(keep).toBeInViewport({ ratio: 1 });
    // Clear of the tab bar over the page's foot, which is inert and so cannot be asked what is under a point.
    const [button, bar] = await Promise.all([keep.boundingBox(), page.locator(".tabs").boundingBox()]);
    expect((button?.y ?? 0) + (button?.height ?? 0)).toBeLessThanOrEqual(bar?.y ?? 0);
    await context.close();
  });
});

test("profiles: Keep editing while a leave's Save is with the core means the Save does not leave, whatever it answers", async ({ page }) => {
  await withKid("Kid A", async (kid) => {
    const core = await holdAnswers(page, "updateProfile");
    await page.goto("/library?form=desktop");
    await page.goto(`/profiles/edit/${kid}?form=desktop`);
    const name = page.locator('[data-f="pedit:name"]');
    await name.fill("Kid B");
    await page.locator('[data-f="nav:/library"]').click();
    await expect(dialog(page)).toBeVisible();
    await page.locator('[data-f="pedit:leave:save"]').click();
    await expect.poll(() => core.sent()).toBe(1);
    // Keep editing while it is with the core, then a new edit and another way out.
    await page.locator('[data-f="pedit:leave:keep"]').click();
    await expect(dialog(page)).toHaveCount(0);
    await name.fill("Kid C");
    await page.locator('[data-f="nav:/discover"]').click();
    await expect(dialog(page)).toBeVisible();
    // The first Save is answered: B is stored, but C is not, and nothing leaves.
    core.release();
    await expect.poll(async () => (await names()).includes("Kid B")).toBe(true);
    await page.waitForTimeout(500);
    await expect(page).toHaveURL(new RegExp(`/profiles/edit/${kid}`));
    await expect(dialog(page)).toBeVisible();
    expect(await names()).not.toContain("Kid C");
  });
});

test("profiles: leaving with the PIN keypad open asks about unsaved edits too", async ({ page }) => {
  await page.goto("/library?form=desktop");
  await page.goto("/profiles/edit/default?form=desktop");
  const name = page.locator('[data-f="pedit:name"]');
  const was = await name.inputValue();
  await name.fill(`${was} edited`);
  await page.locator('[data-f="pedit:pin:set"]').click();
  await expect(page.locator(".keypad")).toBeVisible();
  // The browser's Back: the keypad gives way, and the question shows with the edit still there.
  await page.goBack();
  await expect(dialog(page)).toBeVisible();
  await expect(page.locator(".keypad")).toHaveCount(0);
  await page.locator('[data-f="pedit:leave:keep"]').click();
  await expect(name).toHaveValue(`${was} edited`);
  // The focus does not fall to the page: the keypad key that had it is gone, so the Name field takes it.
  await expect(name).toBeFocused();
  await expect(page).toHaveURL(/\/profiles\/edit\/default/);
});

test("profiles: an Add whose Save was answered after Keep editing is an edit of that profile, never a second Add", async ({ page }) => {
  const core = await holdAnswers(page, "createProfile");
  await page.goto("/library?form=desktop");
  await page.goto("/profiles/new?form=desktop");
  const name = page.locator('[data-f="pedit:name"]');
  try {
    await name.fill("Created Kid");
    await page.locator('[data-f="nav:/library"]').click();
    await expect(dialog(page)).toBeVisible();
    await page.locator('[data-f="pedit:leave:save"]').click();
    await expect.poll(() => core.sent()).toBe(1);
    await page.locator('[data-f="pedit:leave:keep"]').click();
    core.release();
    await expect.poll(async () => (await names()).filter((n) => n === "Created Kid").length).toBe(1);
    // A new name and Save: the same profile is renamed, no second one is made.
    await expect(page.locator("h1")).toHaveText("Edit Created Kid");
    await name.fill("Created Kid 2");
    await page.locator('[data-f="pedit:save"]').click();
    await expect(page.locator(".pedit .st")).toContainText("Saved");
    const all = (await names()).filter((n) => n.startsWith("Created Kid"));
    expect(all).toEqual(["Created Kid 2"]);
    expect(core.sent()).toBe(1);
  } finally {
    const c = new Client();
    try {
      for (const p of (await picker()).profiles) if (p.name.startsWith("Created Kid")) await c.req("deleteProfile", { profile: p.id }).catch(() => undefined);
    } finally {
      c.close();
    }
  }
});
