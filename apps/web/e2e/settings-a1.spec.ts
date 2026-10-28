import { type Browser, expect, type Page, test } from "@playwright/test";
import { drawnContrast, WHITE_PNG } from "./contrast";
import { PORTS } from "./core";
import { type Json, patchedContext } from "./patch";

/** Settings A1 (design/DECISION.md, the owner's pick of 2026-10-28): the profile on a film's backdrop, a section
 * menu beside grouped lists in one centered column. */

type Form = "tv" | "desktop" | "phone";
const SIZE: Record<Form, [number, number]> = { tv: [1920, 1080], desktop: [1440, 900], phone: [390, 844] };
const CONTINUE = "continue-watching";

/** Every `background` under `node` set to null. */
const bare = (node: Json): Json => {
  if (Array.isArray(node)) return node.map(bare);
  if (node && typeof node === "object") return Object.fromEntries(Object.entries(node).map(([k, v]) => [k, k === "background" ? null : bare(v)]));
  return node;
};
/** Continue Watching's tiles without a backdrop, the rest as the core sent it. */
const bareContinue = (node: Json): Json => {
  if (Array.isArray(node)) return node.map(bareContinue);
  if (node && typeof node === "object") return node.rowId === CONTINUE ? bare(node) : Object.fromEntries(Object.entries(node).map(([k, v]) => [k, bareContinue(v)]));
  return node;
};
const kidsProfiles = (s: Json) => ({ ...s, profiles: s.profiles.map((p: Json) => (p.id === s.active ? { ...p, kids: true, isOwner: false } : p)) });

async function open(browser: Browser, form: Form, opts: { size?: [number, number]; home?: (s: Json) => Json; profiles?: (s: Json) => Json } = {}) {
  const [width, height] = opts.size ?? SIZE[form];
  const { context } = await patchedContext(
    browser,
    PORTS.bridge,
    { viewport: { width, height }, isMobile: form === "phone", hasTouch: form === "phone", reducedMotion: "reduce" },
    (view, s) => (view === "home" && opts.home ? opts.home(s) : view === "profiles" && opts.profiles ? opts.profiles(s) : s),
  );
  const page = await context.newPage();
  await page.goto(`/settings?form=${form}`);
  await expect(page.locator('[data-f="settings:profiles"]')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-f="settings:addons"] small')).toContainText(/installed/);
  await expect(page.locator('.set-head[data-home="ready"]')).toHaveCount(1, { timeout: 30_000 });
  return { context, page };
}
const headHeight = (page: Page) => page.locator(".set-head").evaluate((h) => h.getBoundingClientRect().height);
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);

for (const form of ["tv", "desktop"] as const) {
  test(`${form}: settings is one centered column at every width, the header on its line`, async ({ browser }) => {
    const sizes: [number, number][] = form === "tv" ? [[1920, 1080], [1280, 720]] : [[1024, 768], [1280, 720], [1440, 900], [1920, 1080], [2560, 1440]];
    for (const size of sizes) {
      const { context, page } = await open(browser, form, { size });
      const at = await page.evaluate(() => {
        const menu = (document.querySelector(".set-menu .snav") as HTMLElement).getBoundingClientRect();
        const list = (document.querySelector(".set-list .sgroup") as HTMLElement).getBoundingClientRect();
        const who = (document.querySelector(".set-who") as HTMLElement).getBoundingClientRect();
        return { left: menu.left, right: innerWidth - list.right, who: who.left, list: list.left, width: innerWidth };
      });
      expect(Math.abs(at.left - at.right), `${size} centered`).toBeLessThanOrEqual(2);
      expect(Math.abs(at.who - at.left), `${size} header on the column's line`).toBeLessThanOrEqual(2);
      expect(at.list, `${size} list right of the menu`).toBeGreaterThan(at.left + 100);
      // Desktop's column is the gallery's 1120 CSS pixels wherever the window has room for it.
      if (form === "desktop" && size[0] >= 1440) expect(at.width - at.left - at.right, `${size} column`).toBeLessThanOrEqual(1121);
      await context.close();
    }
  });
}

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: the header holds one height with art, before the art lands, and with none`, async ({ browser }) => {
    const loaded = await open(browser, form);
    const art = loaded.page.locator(".set-art img");
    await expect(art).toHaveClass(/\bin\b/, { timeout: 30_000 });
    const src = (await art.getAttribute("src")) as string;
    const full = await headHeight(loaded.page);
    // The header's own height (the gallery's 380 on desktop, 340 on the phone, 600 TV points), not its words'.
    expect(full).toBeCloseTo({ tv: 600, desktop: 380, phone: 340 }[form], 0);
    await loaded.context.close();

    // The art held back: the header is already at its height, its words in place.
    const waiting = await open(browser, form);
    await waiting.page.route(src, () => undefined);
    await waiting.page.reload();
    await expect(waiting.page.locator('[data-f="settings:profiles"]')).toBeVisible({ timeout: 30_000 });
    await expect(waiting.page.locator(".set-art img")).toHaveCount(1);
    await expect(waiting.page.locator(".set-art img")).not.toHaveClass(/\bin\b/);
    expect(await headHeight(waiting.page)).toBe(full);
    await waiting.context.close();

    // No title with a backdrop: the same header on the page color, no picture and no gradient in its place.
    const none = await open(browser, form, { home: bare });
    await expect(none.page.locator(".set-who h2")).toBeVisible();
    await expect(none.page.locator(".set-art")).toHaveCount(0);
    expect(await none.page.locator(".set-head").evaluate((h) => getComputedStyle(h).backgroundImage)).toBe("none");
    expect(await headHeight(none.page)).toBe(full);
    await none.context.close();
  });
}

test("the backdrop is Continue Watching's, else the hero's; a kids profile takes its own Continue Watching only", async ({ browser }) => {
  const srcOf = async (opened: Promise<{ context: import("@playwright/test").BrowserContext; page: Page }>) => {
    const { context, page } = await opened;
    await expect(page.locator(".set-art img")).toHaveCount(1);
    const src = await page.locator(".set-art img").getAttribute("src");
    await context.close();
    return src;
  };
  // Continue Watching's art first; without it, the hero's, another picture.
  const cw = await srcOf(open(browser, "desktop"));
  const hero = await srcOf(open(browser, "desktop", { home: bareContinue }));
  expect(hero).not.toBe(cw);
  // A kids profile: its Continue Watching's art; none when it has none, whatever the hero has.
  expect(await srcOf(open(browser, "desktop", { profiles: kidsProfiles }))).toBe(cw);
  const kidsBare = await open(browser, "desktop", { profiles: kidsProfiles, home: bareContinue });
  await expect(kidsBare.page.locator(".set-who h2")).toBeVisible();
  await expect(kidsBare.page.locator(".set-art")).toHaveCount(0);
  await kidsBare.context.close();
});

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: a kids profile shows no Edit profile and no Manage profiles`, async ({ browser }) => {
    const owner = await open(browser, form);
    await expect(owner.page.locator('[data-f="settings:profile-edit"]')).toHaveCount(1);
    await expect(owner.page.locator('[data-f="settings:profiles-manage"]')).toHaveCount(1);
    await owner.context.close();
    const kids = await open(browser, form, { profiles: kidsProfiles });
    await expect(kids.page.locator('[data-f="settings:profile-edit"]')).toHaveCount(0);
    await expect(kids.page.locator('[data-f="settings:profiles-manage"]')).toHaveCount(0);
    await kids.context.close();
  });
}

test("TV: the profile takes the first focus; the remote moves from the menu to the list and back", async ({ browser }) => {
  const { context, page } = await open(browser, "tv");
  await expect.poll(() => focused(page)).toBe("settings:profiles");
  await page.locator('[data-f="settings-nav:sources"]').focus();
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => focused(page)).toMatch(/^settings:/);
  await page.keyboard.press("ArrowLeft");
  await expect.poll(() => focused(page)).toMatch(/^settings-nav:/);
  // A menu entry takes the remote into its section.
  await page.locator('[data-f="settings-nav:home"]').focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => focused(page)).toBe("settings:home-rows");
  await expect(page.locator('[data-f="settings-nav:home"]')).toHaveAttribute("aria-current", "true");
  await context.close();
});

test("desktop: a menu entry brings its section to the top and fills; the keyboard lands on its heading", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop");
  await expect(page.locator('[data-f="settings-nav:profiles"]')).toHaveAttribute("aria-current", "true");
  await page.locator('[data-f="settings-nav:playback"]').click();
  await expect(page.locator('[data-f="settings-nav:playback"]')).toHaveAttribute("aria-current", "true");
  await expect(page.locator('[aria-current="true"]')).toHaveCount(1);
  expect(await page.evaluate(() => document.activeElement?.id)).toBe("set-h-playback");
  const top = await page.evaluate(() => (document.getElementById("set-playback") as HTMLElement).getBoundingClientRect().top / innerHeight);
  expect(top).toBeLessThan(0.2);
  // Scrolling back up fills the section that reaches the top third again.
  await page.evaluate(() => (document.querySelector(".settings-page") as HTMLElement).scrollTo(0, 0));
  await expect(page.locator('[data-f="settings-nav:profiles"]')).toHaveAttribute("aria-current", "true");
  await context.close();
});

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: every word on the header keeps 4.5:1 and every glyph 3:1 over a white backdrop`, async ({ browser }) => {
    const { context, page } = await open(browser, form);
    const art = page.locator(".set-art img");
    await expect(art).toHaveClass(/\bin\b/, { timeout: 30_000 });
    await page.route((await art.getAttribute("src")) as string, (r) => r.fulfill({ body: WHITE_PNG, contentType: "image/png" }));
    await page.reload();
    await expect(page.locator(".set-art img")).toHaveClass(/\bin\b/, { timeout: 30_000 });
    const texts = [".set-name h2", ".set-name small", ...(form === "phone" ? [".set-title"] : [".set-acts .btn"])];
    const glyphs = form === "phone" ? [".set-who .chev"] : [".set-acts .btn svg"];
    const measured = await drawnContrast(page, ".set-head", texts, glyphs);
    expect(measured.length).toBeGreaterThanOrEqual(texts.length);
    for (const m of measured) expect(m.ratio, `${m.what}: ${m.ratio}`).toBeGreaterThanOrEqual(m.floor);
    await context.close();
  });
}

/** Every avatar on the page that shows, and whether each is a circle (and the editor's picks with their rings). */
const avatarShapes = (page: Page) =>
  page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>(".avatar, .glyph-pick")]
      .filter((el) => el.offsetWidth > 0)
      .map((el) => {
        const r = getComputedStyle(el).borderTopLeftRadius;
        const round = r.endsWith("%") ? parseFloat(r) >= 50 : parseFloat(r) >= Math.min(el.offsetWidth, el.offsetHeight) / 2 - 0.5;
        return { what: `${el.className} ${r}`, round };
      }),
  );

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: every profile avatar is a circle (the owner's rule)`, async ({ browser }) => {
    const { context, page } = await open(browser, form);
    const edit = (await page.locator('[data-f="settings:profile-edit"]').getAttribute("href")) as string;
    const seen: string[] = [];
    for (const [path, ready] of [
      ["/settings", ".set-who .avatar"],
      ["/", form === "phone" ? ".ph-top .avatar" : ".pavatar .avatar"],
      ["/profiles", ".ptile .avatar"],
      ["/profiles/manage", ".prow .avatar"],
      [edit, ".glyph-pick .avatar"],
    ] as const) {
      await page.goto(`${path}?form=${form}`);
      await expect(page.locator(ready).first()).toBeVisible({ timeout: 30_000 });
      for (const a of await avatarShapes(page)) {
        seen.push(path);
        expect(a.round, `${path}: ${a.what}`).toBe(true);
      }
    }
    expect(new Set(seen).size).toBe(5);
    await context.close();
  });
}

test("desktop: every menu entry fills itself; Profiles takes the page back to its top and the keyboard to the profile", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop");
  for (const id of ["sources", "account", "home", "playback", "device", "about", "profiles"]) {
    await page.locator(`[data-f="settings-nav:${id}"]`).click();
    await expect(page.locator('.set-menu [aria-current="true"]'), id).toHaveAttribute("data-f", `settings-nav:${id}`);
  }
  await expect.poll(() => page.evaluate(() => (document.querySelector(".settings-page") as HTMLElement).scrollTop)).toBe(0);
  expect(await focused(page)).toBe("settings:profiles");
  await context.close();
});

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: groups in one section keep their gap`, async ({ browser }) => {
    const { context, page } = await open(browser, form);
    await expect(page.locator("#set-playback .sgroup")).toHaveCount(2, { timeout: 30_000 });
    const gap = await page.locator("#set-playback .sgroup").evaluateAll((g) => g[1]!.getBoundingClientRect().top - g[0]!.getBoundingClientRect().bottom);
    expect(gap).toBeGreaterThanOrEqual(8);
    await context.close();
  });
}

test("desktop 844x390: the section menu stays inside a short window; its last entry scrolls into reach", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", { size: [844, 390] });
  // Down the page, where the menu holds its place under the bar.
  await page.evaluate(() => (document.querySelector(".settings-page") as HTMLElement).scrollTo(0, 1200));
  const box = await page.locator(".set-menu").evaluate((m) => {
    m.scrollTop = m.scrollHeight;
    const last = (m.querySelector('[data-f="settings-nav:about"]') as HTMLElement).getBoundingClientRect();
    return { bottom: m.getBoundingClientRect().bottom, last: last.bottom, vh: innerHeight };
  });
  expect(box.bottom).toBeLessThanOrEqual(box.vh);
  expect(box.last).toBeLessThanOrEqual(box.vh);
  await context.close();
});
