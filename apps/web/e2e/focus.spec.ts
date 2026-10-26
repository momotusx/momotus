import { type Browser, type BrowserContext, expect, type Page, test } from "@playwright/test";
import { GOT } from "./core";
import { type Form, FORMS, focusStop, SCREENS, type Screen, visible } from "./screens";

/** Where focus lands stays readable: a focused card and its ring sit clear
 * of a list's fade, and a chip row fades only toward chips left to scroll
 * to (mirrored right to left). */

async function open(browser: Browser, path: string, viewport: { width: number; height: number }, rtl = false) {
  // Reduced motion: scrolling and the focus lift land at once, so a check
  // reads the final layout.
  const context = await browser.newContext({ viewport, reducedMotion: "reduce" });
  const page = await context.newPage();
  if (rtl) await page.addInitScript(() => document.addEventListener("DOMContentLoaded", () => (document.documentElement.dir = "rtl")));
  await page.goto(path);
  return { context, page };
}

/** The focused element as drawn, its rise and scale included (TV draws no ring), inside its
 * list, below its group's pinned heading if it has one, and above the list's fade (canvas
 * pixels, scaled like the page). */
const clearOfFade = (page: Page, fade: number) =>
  page.evaluate((fade) => {
    const el = document.activeElement as HTMLElement;
    const list = el.closest<HTMLElement>("[data-vscroll]");
    if (!list) return { key: el.dataset.f, ok: false, why: "outside any list", left: true };
    const l = list.getBoundingClientRect();
    const k = l.height / list.offsetHeight || 1;
    const e = el.getBoundingClientRect();
    const ring = 0;
    const head = el.closest(".vgroup")?.querySelector("h3")?.getBoundingClientRect();
    const top = Math.max(l.top, head ? head.bottom : l.top);
    const why = `ring ${Math.round(e.top - ring)} to ${Math.round(e.bottom + ring)}, clear from ${Math.round(top)} to ${Math.round(l.bottom - fade * k)}`;
    return { key: el.dataset.f, ok: e.top - ring >= top - 1 && e.bottom + ring <= l.bottom - fade * k + 1, why, left: false };
  }, fade);

/** Presses a key until focus stops moving or leaves the list, checking every stop. */
async function walk(page: Page, key: "ArrowDown" | "ArrowUp", fade: number, max: number) {
  const seen: string[] = [];
  for (let i = 0; i < max; i++) {
    await page.keyboard.press(key);
    const at = await clearOfFade(page, fade);
    if (at.left || (at.key && seen.at(-1) === at.key)) break;
    seen.push(at.key ?? "");
    expect(at.ok, `focused ${at.key} clear of the fade: ${at.why}`).toBe(true);
  }
  return seen;
}

test("TV: the last episode of a season, focused, sits clear of the list's fade", async ({ browser }) => {
  const { context, page } = await open(browser, `/title/series/${GOT.id}?season=1&form=tv`, { width: 1920, height: 1080 });
  const first = page.locator('[data-f="ep:tt0944947:1:1"]');
  await expect(first).toBeVisible({ timeout: 30_000 });
  await first.focus();
  const seen = await walk(page, "ArrowDown", 56, 14);
  // Down from the last card goes on to its actions; it got there clear too.
  expect(seen).toContain("ep:tt0944947:1:10");
  await context.close();
});

test("TV: rows of a group taller than the list clear its pinned heading and the fade, down and up", async ({ browser }) => {
  const { context, page } = await open(browser, `/title/series/${GOT.id}/versions/${GOT.resume}?form=tv`, { width: 1920, height: 1080 });
  const first = page.locator(".vrow").first();
  await expect(first).toBeVisible({ timeout: 30_000 });
  // Every addon has answered, so the list no longer changes under the copies.
  await expect(page.locator(".vstatus")).toHaveText("", { timeout: 60_000 });
  // Live groups hold a row or two: copies of the first row make its group
  // taller than the list. Focus and scrolling read the DOM, not the data.
  await first.evaluate((row) => {
    for (let i = 0; i < 14; i++) {
      const copy = row.cloneNode(true) as HTMLElement;
      copy.dataset.f = `v:copy:${i}`;
      row.after(copy);
    }
  });
  await first.focus();
  const down = await walk(page, "ArrowDown", 48, 20);
  expect(down.length).toBeGreaterThanOrEqual(14);
  const up = await walk(page, "ArrowUp", 48, 20);
  expect(up.length).toBeGreaterThanOrEqual(14);
  await context.close();
});

for (const rtl of [false, true]) {
test(`TV: a chip row fades only toward chips left to scroll to (${rtl ? "right to left" : "left to right"})`, async ({ browser }) => {
  const { context, page } = await open(browser, "/discover?form=tv", { width: 1920, height: 1080 }, rtl);
  // Four choices at most are a segmented control (more are a pull-down): one squeezed narrower
  // than its chips scrolls, as a long one does on a phone.
  const kinds = page.locator('[data-fgroup="kinds"] .chips-track');
  await expect(kinds.locator(".chip").nth(1)).toBeAttached({ timeout: 30_000 });
  const mask = (row: string) =>
    page.locator(`[data-fgroup="${row}"] .chips-track`).evaluate((t) => getComputedStyle(t).maskImage || getComputedStyle(t).webkitMaskImage);
  const fadesAtEnd = (m: string) => m.endsWith("rgba(0, 0, 0, 0))");
  const fadesAtStart = (m: string) => /^linear-gradient\(\d+deg, rgba\(0, 0, 0, 0\)/.test(m);
  // The fade runs toward the row's end: rightward, or leftward right to left.
  const way = rtl ? "linear-gradient(270deg" : "linear-gradient(90deg";

  // Nothing to scroll to: no fade at all.
  await expect.poll(() => mask("kinds")).toBe("none");
  await kinds.evaluate((t) => {
    t.style.flex = "none";
    t.style.width = `${(t.firstElementChild as HTMLElement).offsetWidth + 80}px`;
  });
  // At the start of a long row: the end fades, the start does not.
  await expect.poll(async () => fadesAtEnd(await mask("kinds"))).toBe(true);
  expect(fadesAtStart(await mask("kinds"))).toBe(false);
  expect((await mask("kinds")).startsWith(way)).toBe(true);
  // At its end: the last chip is not faded; the start now fades.
  await kinds.evaluate((t, rtl) => t.scrollTo({ left: rtl ? -t.scrollWidth : t.scrollWidth }), rtl);
  await expect.poll(async () => fadesAtEnd(await mask("kinds"))).toBe(false);
  expect(fadesAtStart(await mask("kinds"))).toBe(true);
  await context.close();
});
}

test("TV: an episode's actions lay over its card: no width change, Right in, Left and Back out", async ({ browser }) => {
  const { context, page } = await open(browser, `/title/series/${GOT.id}?season=1&form=tv`, { width: 1920, height: 1080 });
  const card = page.locator('[data-f="ep:tt0944947:1:2"]');
  await expect(card).toBeVisible({ timeout: 30_000 });
  // Layout widths (offsetWidth ignores the focus lift's scale).
  const widths = () => page.locator(".ep-main").evaluateAll((els) => [...new Set(els.map((e) => (e as HTMLElement).offsetWidth))]);
  const before = await widths();
  expect(before).toHaveLength(1);
  await card.focus();
  await expect(page.locator('[data-f="epv:tt0944947:1:2"]')).toBeVisible();
  // The strip shows over the card: every card keeps its width, the focused one too.
  expect(await widths()).toEqual(before);
  const focused = () => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");
  const address = page.url();
  for (const [key, want] of [
    ["ArrowRight", "epv:tt0944947:1:2"],
    ["ArrowDown", "mark:tt0944947:1:2"],
    ["ArrowLeft", "ep:tt0944947:1:2"],
    ["ArrowRight", "epv:tt0944947:1:2"],
    ["Escape", "ep:tt0944947:1:2"],
  ] as const) {
    await page.keyboard.press(key);
    await expect.poll(focused, { message: `${key} lands on ${want}` }).toBe(want);
  }
  // Back from the strip went to the card, not off the page.
  expect(page.url()).toBe(address);
  await context.close();
});

// The focus loop (plans/design-v3-rollout.md, phase 1a): Tab through every screen the shots
// cover, on each form, until the walk comes back to a stop it judged, and judge every stop
// (focusStop). The player screens wait for phase 5. Home rows is walked at rest (its shot picks a
// row up), and Settings once more with More languages open, for its search field.
const LOOP: Screen[] = [
  ...SCREENS.filter((s) => !s.name.startsWith("player") && s.name !== "title-trailer" && s.name !== "home-rows"),
  { name: "home-rows at rest", path: "/home-rows", ready: visible(".hrow-main") },
  {
    name: "settings with more languages",
    path: "/settings",
    ready: async (page) => {
      await visible('[data-fgroup="pref-subs"]')(page);
      await page.locator('[data-f="pref-subs:more"]').click();
      await page.locator(".lang-search input").focus();
    },
  },
];
for (const form of Object.keys(FORMS) as Form[]) {
  for (const screen of LOOP) {
    test(`focus loop: ${screen.name} ${form}`, async ({ browser }) => {
      const f = FORMS[form];
      const context = await browser.newContext({ viewport: { width: f.width, height: f.height }, reducedMotion: "reduce", isMobile: form === "phone", hasTouch: form === "phone" });
      const page = await context.newPage();
      await page.goto(`${screen.path}${screen.path.includes("?") ? "&" : "?"}form=${form}`);
      await screen.ready(page, form);
      const seen: string[] = [];
      const failed: string[] = [];
      // The screen's own first focus is the first stop.
      for (let i = 0; i < 40; i++) {
        if (i > 0 || (await page.evaluate(() => document.activeElement === document.body))) {
          await page.keyboard.press("Tab");
          // Past the last control the page itself takes focus; the next Tab starts over.
          if (await page.evaluate(() => document.activeElement === document.body)) await page.keyboard.press("Tab");
        }
        const stop = await focusStop(page, form);
        for (const r of stop.results) if (!r.ok) failed.push(`${r.key}: ${r.why}`);
        if (!stop.key || seen.includes(stop.key)) break;
        seen.push(stop.key);
      }
      // The last stop is judged when focus leaves it.
      await page.keyboard.press("Tab");
      for (const r of (await focusStop(page, form)).results) if (!r.ok) failed.push(`${r.key}: ${r.why}`);
      test.info().annotations.push({ type: "stops", description: String(seen.length) });
      expect(seen.length, "Tab reached focusable elements").toBeGreaterThan(0);
      expect(failed, `stops: ${seen.join(", ")}`).toEqual([]);
      await screen.after?.(page);
      await context.close();
    });
  }
}

// The phase 1a review (plans/reviews/design-v3-phase1a-a.md, -b.md): the art's lift fits its
// lane, the caption stays over the shadow, one white per action row, the white profile, dark words.
const settle = (page: Page) =>
  page.evaluate(() => {
    for (const a of document.getAnimations()) {
      try {
        a.finish();
      } catch {
        // An endless pulse cannot finish.
      }
    }
  });

/** The focused art's box, its transform and its shadow, against the lane that scrolls it. */
const artInLane = (page: Page, art: string) =>
  page.evaluate((art) => {
    const pic = art === "self" ? (document.activeElement as HTMLElement | null) : document.activeElement?.querySelector<HTMLElement>(art);
    const lane = pic?.closest<HTMLElement>("[data-hscroll]");
    if (!pic || !lane) return null;
    const s = getComputedStyle(pic);
    // The lift is the shadow with the widest blur (the first may be the transparent no-halo).
    const shadows = [...s.boxShadow.matchAll(/(-?[\d.]+)px (-?[\d.]+)px ([\d.]+)px/g)].map((m) => ({ y: Number(m[2]), blur: Number(m[3]) }));
    const { y, blur } = shadows.sort((p, q) => q.blur - p.blur)[0] ?? { y: 0, blur: 0 };
    const p = pic.getBoundingClientRect();
    const l = lane.getBoundingClientRect();
    const m = new DOMMatrix(s.transform);
    return {
      room: { left: p.left - blur - l.left, top: p.top + y - blur - l.top, bottom: l.bottom - (p.bottom + y + blur) },
      scale: m.a,
      rise: art === "self" ? parseFloat(s.translate.split(" ")[1] ?? "0") : m.f,
      halo: s.boxShadow.includes("0px 0px 30px"),
      caption: getComputedStyle(pic.nextElementSibling ?? pic).position,
    };
  }, art);

test("TV: a focused tile and a calendar still rise as art and keep their whole shadow inside the lane", async ({ browser }) => {
  const { context, page } = await open(browser, "/?form=tv", { width: 1920, height: 1080 });
  // A catalog row long enough to scroll (Continue Watching holds two or three).
  const tile = page.locator(".row").nth(1).locator("[data-f]").first();
  await expect(tile).toBeVisible({ timeout: 30_000 });
  for (const step of [0, 3, 8]) {
    await tile.focus();
    for (let i = 0; i < step; i++) await page.keyboard.press("ArrowRight");
    await settle(page);
    const at = await artInLane(page, ".pic");
    expect(at, `the focused tile after ${step} steps right`).not.toBeNull();
    expect(at!.scale).toBeCloseTo(1.1, 2);
    expect(at!.rise).toBeCloseTo(-10, 1);
    expect(at!.halo).toBe(false);
    expect(at!.caption).not.toBe("static");
    expect(at!.room.left).toBeGreaterThanOrEqual(-1);
    expect(at!.room.top).toBeGreaterThanOrEqual(-1);
    expect(at!.room.bottom).toBeGreaterThanOrEqual(-1);
    // Revealed inside the row, not in the room past its edge.
    const box = await page.evaluate(() => (document.activeElement as HTMLElement).getBoundingClientRect().toJSON());
    expect(box.right, `the focused tile after ${step} steps right is on screen`).toBeLessThanOrEqual(1920 - 80);
  }
  await page.goto("/calendar?form=tv");
  const card = page.locator(".cal-card").first();
  await expect(card).toBeVisible({ timeout: 30_000 });
  await card.focus();
  await settle(page);
  const still = await artInLane(page, ".cal-art");
  expect(still).not.toBeNull();
  expect(still!.scale).toBeCloseTo(1.1, 2);
  expect(still!.room.left).toBeGreaterThanOrEqual(-1);
  expect(still!.room.top).toBeGreaterThanOrEqual(-1);
  expect(still!.room.bottom).toBeGreaterThanOrEqual(-1);
  await context.close();
});

test("TV: one white per action row, a white profile with a dark glyph, dark words on a white row that rises", async ({ browser }) => {
  const { context, page } = await open(browser, `/title/series/${GOT.id}?form=tv`, { width: 1920, height: 1080 });
  const fill = (key: string) => page.locator(`[data-f="${key}"]`).evaluate((el) => getComputedStyle(el).backgroundColor);
  const resume = page.locator('[data-f="title:play"]');
  await expect(resume).toBeVisible({ timeout: 30_000 });
  // Start over sits in Resume's row: while it holds focus, Resume steps back to the secondary fill.
  await page.locator('[data-f="title:startover"]').focus();
  await settle(page);
  expect(await fill("title:startover")).toBe("rgba(255, 255, 255, 0.92)");
  expect(await fill("title:play")).toBe("rgba(255, 255, 255, 0.16)");
  // With focus elsewhere on the page (Trailer, another row) it stays white.
  await page.locator('[data-f="title:trailer"]').focus();
  await settle(page);
  expect(await fill("title:play")).toBe("rgba(255, 255, 255, 0.92)");

  await page.goto("/profiles?form=tv");
  const tile = page.locator(".ptile").first();
  await expect(tile).toBeVisible({ timeout: 30_000 });
  await tile.focus();
  await settle(page);
  const avatar = await tile.locator(".avatar").evaluate((el) => {
    const s = getComputedStyle(el);
    return { fill: s.backgroundColor, ink: s.color, sheen: s.backgroundImage.startsWith("linear-gradient") };
  });
  expect(avatar).toEqual({ fill: "rgba(255, 255, 255, 0.92)", ink: "rgba(0, 0, 0, 0.88)", sheen: true });

  await page.goto("/settings?form=tv");
  const row = page.locator('[data-f="settings:stream-choice"]');
  await expect(row).toBeVisible({ timeout: 30_000 });
  await row.focus();
  await settle(page);
  expect(await row.locator("small").first().evaluate((el) => getComputedStyle(el).color)).toBe("rgba(0, 0, 0, 0.64)");
  // A row rises by focus.rise over the 0 30px 60px lift, never as high as art.
  const lift = await row.evaluate((el) => ({ rise: getComputedStyle(el).translate, shadow: getComputedStyle(el).boxShadow }));
  expect(lift.rise).toBe("0px -8px");
  expect(lift.shadow).toContain("0px 30px 60px");
  await context.close();
});

test("TV: a focused chip rises by focus.rise and keeps its whole lift shadow inside its lane", async ({ browser }) => {
  const { context, page } = await open(browser, "/discover?form=tv", { width: 1920, height: 1080 });
  const chip = page.locator(".chips-track .chip").first();
  await expect(chip).toBeVisible({ timeout: 30_000 });
  await chip.focus();
  await settle(page);
  const at = await artInLane(page, "self");
  expect(at).not.toBeNull();
  expect(at!.scale).toBeCloseTo(1.06, 2);
  expect(at!.rise).toBeCloseTo(-8, 1);
  expect(at!.room.left).toBeGreaterThanOrEqual(-1);
  expect(at!.room.top).toBeGreaterThanOrEqual(-1);
  expect(at!.room.bottom).toBeGreaterThanOrEqual(-1);
  await context.close();
});

test("desktop: the mouse wheel scrolls Home over a row's heading, the gap between rows and a tile", async ({ browser }) => {
  const { context, page } = await open(browser, "/?form=desktop", { width: 1440, height: 900 });
  const row = page.locator(".row").nth(1);
  await expect(row.locator("[data-f]").first()).toBeVisible({ timeout: 30_000 });
  const scroller = () => page.evaluate(() => (document.querySelector(".row")?.closest("[data-vscroll]") as HTMLElement | null)?.scrollTop ?? -1);
  // The row in the middle of the window (the full-bleed hero holds the first screen), from where each wheel starts.
  await row.evaluate((r) => r.scrollIntoView({ block: "center" }));
  const base = await scroller();
  // The points: the row's heading, 8 px above it (the gap after the row before), and its first tile.
  const points = await row.evaluate((r) => {
    const head = r.querySelector(".row-head")?.getBoundingClientRect();
    const tile = r.querySelector("[data-f]")?.getBoundingClientRect();
    if (!head || !tile) return [];
    return [
      { where: "heading", x: head.left + 200, y: head.top + head.height / 2 },
      { where: "gap", x: head.left + 200, y: head.top - 8 },
      { where: "tile", x: tile.left + tile.width / 2, y: tile.top + tile.height / 2 },
    ];
  });
  expect(points.length).toBe(3);
  for (const p of points) {
    await page.evaluate((base) => {
      const s = document.querySelector(".row")?.closest("[data-vscroll]") as HTMLElement | null;
      if (s) s.scrollTop = base;
    }, base);
    await page.mouse.move(p.x, p.y);
    await page.mouse.wheel(0, 400);
    await expect.poll(scroller, { message: `the wheel over the ${p.where} scrolls the page` }).toBeGreaterThan(base);
  }
  await context.close();
});

test("TV: a focused field is white with its lift and no edge, and its row's primary steps back", async ({ browser }) => {
  const { context, page } = await open(browser, "/pair?form=tv", { width: 1920, height: 1080 });
  // The fields sit under Advanced: open while no server is known, else opened by remote.
  const advanced = page.locator('[data-f="pair:advanced"]');
  await expect(advanced).toBeVisible({ timeout: 30_000 });
  if ((await advanced.getAttribute("aria-expanded")) !== "true") {
    await advanced.focus();
    await page.keyboard.press("Enter");
  }
  const name = page.locator('[data-f="pair:name"]');
  await expect(name).toBeVisible({ timeout: 30_000 });
  await name.focus();
  await settle(page);
  const field = await name.evaluate((el) => ({ fill: getComputedStyle(el).backgroundColor, shadow: getComputedStyle(el).boxShadow }));
  expect(field.fill).toBe("rgba(255, 255, 255, 0.92)");
  expect(field.shadow).not.toContain("inset");
  expect(field.shadow).toContain("0px 30px 60px");
  expect(await page.locator('[data-f="pair:start"]').evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgba(255, 255, 255, 0.16)");
  await context.close();
});

// A focus the app did not place (a screen reader's, Tab's, a script's) is the viewer's: a screen's first focus that is
// still waiting for its list never takes it back. Here the Addons list is held until the focus has moved to the bar
// with no key pressed, then let go: the focus stays on the bar.
test("TV: a focus moved without a key stays where it went when the screen's list lands after", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const hold = await holdAddons(context);
  const page = await context.newPage();
  const focused = () => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
  await page.goto("/settings?form=tv");
  const addons = page.locator('[data-f="settings:addons"]');
  await expect(addons).toBeVisible({ timeout: 30_000 });
  await addons.focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/addons/);
  await expect.poll(() => hold.release !== null).toBe(true);
  // No key: the focus moves as a screen reader or a script would move it.
  await page.locator('[data-f="nav:/settings"]').focus();
  hold.release?.();
  await expect(page.locator('[data-f="addon:0:enabled"]')).toBeVisible();
  await page.waitForTimeout(800);
  expect(await focused()).toBe("nav:/settings");
  await context.close();
});

/** Holds the Addons list (its second subscription: the Settings page asks first) until `release`. */
async function holdAddons(context: BrowserContext) {
  const hold = { release: null as (() => void) | null };
  let asked = 0;
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((message) => {
      const text = String(message);
      let held = false;
      try {
        const m = JSON.parse(text) as { op?: string; view?: string };
        held = m.op === "subscribe" && m.view === "addons" && ++asked > 1;
      } catch {
        // Not JSON: passed on as it is.
      }
      if (held) hold.release = () => server.send(text);
      else server.send(text);
    });
  });
  return hold;
}

// The window or tab regaining focus re-announces the focused element (as an overlay, a voice assistant or a resume
// hands the TV back): no one moved, so a first focus still waiting for its list lands when the list does.
test("TV: the window regaining focus is no move: the waiting first focus still lands", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const hold = await holdAddons(context);
  const page = await context.newPage();
  const focused = () => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
  await page.goto("/settings?form=tv");
  const addons = page.locator('[data-f="settings:addons"]');
  await expect(addons).toBeVisible({ timeout: 30_000 });
  await addons.focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/addons/);
  await expect.poll(() => hold.release !== null).toBe(true);
  // The app has placed a focus on the page while its list is on the way.
  await expect.poll(focused).toMatch(/^addons:/);
  await page.evaluate(() => {
    window.dispatchEvent(new FocusEvent("blur"));
    window.dispatchEvent(new FocusEvent("focus"));
    document.activeElement?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
  });
  hold.release?.();
  await expect.poll(focused).toBe("addon:0:enabled");
  await context.close();
});

// A screen's own `.focus()` (Calendar's first card of the first day from today) ends the first-focus wait: nothing
// takes it away later, past the wait's 4 s fallback.
test("TV: Calendar's own first focus stays where it put it", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto("/calendar?form=tv");
  await expect(page.locator(".cal-card").first()).toBeVisible({ timeout: 60_000 });
  const today = await page.evaluate(() => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  });
  const days = await page.locator(".cal-day").evaluateAll((els) => els.map((e) => e.getAttribute("data-day") ?? ""));
  const first = days.find((d) => d >= today);
  const at = () => page.evaluate(() => document.activeElement?.closest(".cal-day")?.getAttribute("data-day") ?? null);
  await expect.poll(at).toBe(first);
  const key = await page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
  await page.waitForTimeout(4500);
  expect(await at()).toBe(first);
  expect(await page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null)).toBe(key);
  await context.close();
});
