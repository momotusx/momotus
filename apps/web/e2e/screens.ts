/** The screens every form is shot on (shots.spec.ts) and walked by keyboard
 * on (focus.spec.ts): each one's path and how to tell it is ready. A plain
 * module, because Playwright refuses a spec that imports a spec. */

import { expect, type Page } from "@playwright/test";
import { GOT, reseedBbb, startIfPaused } from "./core";
import { BBB_ID } from "./source";
import { fakeYouTube } from "./yt-fake";

export type Form = "tv" | "desktop" | "phone";
export const FORMS: Record<Form, { width: number; height: number; scale: number }> = {
  tv: { width: 1920, height: 1080, scale: 1 },
  desktop: { width: 1440, height: 900, scale: 1 },
  phone: { width: 390, height: 844, scale: 2 },
};

export interface Screen {
  name: string;
  path: string;
  ready: (page: Page, form: Form) => Promise<void>;
  /** Puts shared core state back after the shot. */
  after?: (page: Page) => Promise<void>;
  /** Live codes and links covered in the image (account link codes). */
  mask?: string;
}

export const visible = (sel: string, timeout = 30_000) => async (page: Page) => {
  await expect(page.locator(sel).first()).toBeVisible({ timeout });
};

/** Episode stills load lazily inside their list; a shot waits a moment for
 * the first ones (a still that never comes keeps its stand-in, also valid). */
const stills = (page: Page) =>
  page.waitForFunction(() => document.querySelectorAll(".ep-still img.in").length >= 4, null, { timeout: 10_000 }).catch(() => undefined);

export const SCREENS: Screen[] = [
  {
    name: "home",
    path: "/",
    ready: async (page) => {
      await visible(".hero-page.on .reason")(page);
      for (const row of [0, 1]) {
        await expect(page.locator(".row").nth(row).locator(".tile img.in").first()).toBeVisible({ timeout: 30_000 });
      }
      // Every mounted poster in Continue Watching has decoded.
      await expect.poll(() => page.locator(".row").first().locator(".tile img.fade:not(.in)").count()).toBe(0);
    },
  },
  {
    name: "title",
    path: `/title/series/${GOT.id}`,
    ready: async (page) => {
      await visible(".ep-t")(page);
      await stills(page);
    },
  },
  {
    name: "title-trailer",
    path: `/title/movie/${BBB_ID}`,
    ready: async (page) => {
      // A stand-in for YouTube's player: the shot is of our overlay, and a run
      // needs no third-party page (its scripts fail without their network).
      await fakeYouTube(page);
      await visible('[data-f="title:trailer"]')(page);
      await page.locator('[data-f="title:trailer"]').focus();
      await page.keyboard.press("Enter");
      await visible(".trailer-player iframe")(page);
      // Playing, with its length on the bar.
      await page.locator('[data-f="trailer:scrub"][aria-valuemax="120"]').waitFor();
    },
  },
  {
    name: "title-watched",
    path: `/title/movie/${BBB_ID}`,
    ready: async (page) => {
      const watched = page.locator('[data-f="title:watched"]');
      await expect(watched).toBeVisible({ timeout: 30_000 });
      // By keyboard, as a remote would, so the focused toggle shows its word.
      await watched.focus();
      if ((await watched.getAttribute("aria-pressed")) !== "true") await page.keyboard.press("Enter");
      await expect(watched).toHaveAttribute("aria-pressed", "true");
    },
    // Watched movies leave Continue Watching; later shots need it back.
    after: async (page) => {
      const watched = page.locator('[data-f="title:watched"]');
      await watched.click();
      await expect(watched).toHaveAttribute("aria-pressed", "false");
    },
  },
  {
    // An episode row with focus shows its secondary actions (phone: always).
    name: "title-episode",
    path: `/title/series/${GOT.id}?season=1`,
    ready: async (page, form) => {
      const row = page.locator('[data-f="ep:tt0944947:1:5"]');
      await expect(row).toBeVisible({ timeout: 30_000 });
      if (form !== "phone") await row.focus();
      else await row.evaluate((el) => el.scrollIntoView({ block: "center" }));
      await stills(page);
    },
  },
  // A movie whose metadata lists no trailer: no Trailer action at all.
  { name: "title-no-trailer", path: "/title/movie/tt43718175", ready: visible('[data-f="title:watched"]') },
  {
    name: "versions",
    path: `/title/series/${GOT.id}/versions/${GOT.resume}`,
    ready: async (page, form) => {
      await visible(".vrow")(page);
      // On TV and desktop show a focused row with its full release name.
      if (form !== "phone") await page.locator(".vrow").first().focus();
    },
  },
  {
    name: "player",
    path: `/play/movie/${BBB_ID}/${BBB_ID}`,
    ready: async (page, form) => {
      await visible('[data-f="player:toggle"]')(page);
      await startIfPaused(page);
      await expect.poll(() => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0), { timeout: 30_000 }).toBeGreaterThan(152);
      // Touch has no focus ring; TV and desktop show where the remote is.
      if (form !== "phone") await page.locator('[data-f="player:toggle"]').focus();
    },
  },
  {
    // Inside the fixture's credits (TheIntroDB's shape), controls hidden, Skip focused.
    name: "player-skip",
    path: `/play/movie/${BBB_ID}/${BBB_ID}`,
    ready: async (page) => {
      await visible('[data-f="player:toggle"]')(page);
      await startIfPaused(page);
      await expect.poll(() => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0), { timeout: 30_000 }).toBeGreaterThan(150);
      await page.evaluate(() => {
        const v = document.querySelector("video");
        if (v) v.currentTime = 493;
      });
      await visible('[data-f="player:skip"]')(page);
      await expect(page.locator(".player-scr")).toHaveClass(/idle/, { timeout: 10_000 });
    },
    after: async (page) => {
      await page.evaluate(() => {
        const v = document.querySelector("video");
        if (v) {
          v.currentTime = 150;
          v.pause();
        }
      });
      await page.waitForTimeout(500);
    },
  },
  {
    // The movie's end: the card with Back and Play again.
    name: "player-end",
    path: `/play/movie/${BBB_ID}/${BBB_ID}`,
    ready: async (page) => {
      await visible('[data-f="player:toggle"]')(page);
      await startIfPaused(page);
      await expect.poll(() => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0), { timeout: 30_000 }).toBeGreaterThan(150);
      await page.evaluate(() => {
        const v = document.querySelector("video");
        if (v) v.currentTime = v.duration - 2;
      });
      await visible(".endcard", 15_000)(page);
    },
    after: async (page) => {
      await reseedBbb(page);
    },
  },
  { name: "discover", path: "/discover", ready: visible(".grid .tile img.in") },
  {
    name: "search",
    path: "/search",
    ready: async (page) => {
      await page.locator('[data-f="search:input"]').fill("thrones");
      await visible(".hit img.in")(page);
    },
  },
  {
    // The palette over Home, opened from the navigation as a person would;
    // TV walks to the first result.
    name: "search-palette",
    path: "/",
    ready: async (page, form) => {
      await visible(".hero-page.on .reason")(page);
      if (form === "phone") await page.locator('.tabs a[href="/search"]').tap();
      else await page.locator('[data-f="nav:/search"]').click();
      await page.locator('[data-f="search:input"]').fill("thrones");
      await visible(".hit img.in")(page);
      if (form === "tv") {
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
      }
    },
  },
  { name: "library", path: "/library", ready: visible(".grid .tile img.in") },
  { name: "addons", path: "/addons", ready: visible(".addon") },
  {
    // TV and desktop: the first row picked up for moving.
    name: "home-rows",
    path: "/home-rows",
    ready: async (page, form) => {
      await visible(".hrow-main")(page);
      if (form === "phone") return;
      await page.locator(".hrow-main").first().focus();
      await page.keyboard.press("Enter");
      await visible(".hrow.moving")(page);
    },
    after: async (page) => {
      if (await page.locator(".hrow.moving").count()) await page.keyboard.press("Enter");
    },
  },
  {
    name: "calendar",
    path: "/calendar",
    ready: async (page) => {
      await visible(".cal-card")(page);
      await expect(page.locator(".cal-head .st.warn")).toHaveCount(0, { timeout: 60_000 });
      // Stills on screen have decoded (a missing one keeps its plain frame).
      await page
        .waitForFunction(
          () =>
            [...document.querySelectorAll<HTMLImageElement>(".cal-card img.fade")].every((img) => {
              const r = img.getBoundingClientRect();
              return img.classList.contains("in") || r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth;
            }),
          null,
          { timeout: 15_000 },
        )
        .catch(() => undefined);
    },
  },
  {
    // Playback (prefs) and the addons line come from live views; "This
    // device" renders at once, so it cannot stand for the whole screen.
    name: "settings",
    path: "/settings",
    ready: async (page) => {
      await visible('[data-f="pref-subs:open"]')(page);
      await expect(page.locator('[data-f="settings:stream-choice"] small')).toContainText(/shared by at least/);
      await expect(page.locator('[data-f="settings:addons"] small')).toContainText(/\d+ installed/);
    },
  },
  { name: "stream-choice", path: "/stream-choice", ready: visible('[data-fgroup="choice-size"]') },
  { name: "welcome", path: "/welcome", ready: visible(".door") },
  { name: "welcome-choice", path: "/welcome?step=choice", ready: visible(".doors.two .door") },
  { name: "welcome-source", path: "/welcome?step=2", ready: visible(".drow") },
  { name: "welcome-check", path: "/welcome?step=3", ready: visible(".check li") },
  { name: "import", path: "/import", ready: visible(".code, .problem"), mask: ".code, .qrbox, b.mono, [data-f='import:open']" },
  { name: "pairing", path: "/pair", ready: visible(".install") },
  { name: "find-addons", path: "/find-addons", ready: visible(".fa-card:not(.skel)") },
  { name: "recommended-addons", path: "/recommended-addons", ready: visible("button.drow") },
  { name: "debrid", path: "/debrid", ready: visible(".dcard") },
  { name: "profiles", path: "/profiles", ready: visible(".ptile") },
  { name: "profiles-manage", path: "/profiles/manage", ready: visible(".prow") },
  { name: "profile-edit", path: "/profiles/new", ready: visible(".pedit") },
];

/** One stop of a keyboard walk (plans/design-v3-rollout.md, phase 1a). Desktop and phone:
 * the focused element, its art, ring, avatar or icon, or the field that holds it carries a key
 * ring at least 4 px wide, and that ring alone: the element that just lost focus looks as it did
 * focused in transform and fill (tv.apple.com draws no fill and no lift). A deliberate deviation from the plan's check (plans/design-v3-rollout.md,
 * section 5): a phone's text field keeps its 2 px inset key edge, because a tap makes an input
 * `:focus-visible` too, and a 4 px ring there would show on every tap. TV: the white lift, so the element that just lost focus differs from
 * its focused self in transform or fill, never in color alone (the last stop is judged at the
 * next one). Animations are finished first, so a transition never reads as no change. */
export const focusStop = (page: Page, form: Form) =>
  page.evaluate((form) => {
    type Stop = { el: HTMLElement; key: string; on: string[] };
    const w = window as unknown as { __stop?: Stop };
    for (const a of document.getAnimations()) {
      try {
        a.finish();
      } catch {
        // An endless animation (a skeleton's pulse) cannot finish; it never carries focus.
      }
    }
    // An input shows focus on the field that holds it (the language picker's .lp-field, the palette field).
    const holders = (el: HTMLElement) => (el.tagName === "INPUT" ? [el.parentElement, el.parentElement?.parentElement].filter((n): n is HTMLElement => !!n) : []);
    // The download control draws its ring on its mark (components/download.css), the circle it sits in.
    const parts = (el: HTMLElement) => [el, ...el.querySelectorAll<HTMLElement>(".pic, .ring, .avatar, .cal-art, .dl-mark, svg"), ...holders(el)];
    const look = (el: HTMLElement) => parts(el).map((n) => `${getComputedStyle(n).transform} ${getComputedStyle(n).backgroundColor}`);
    const out: { key: string; ok: boolean; why: string }[] = [];
    const prev = w.__stop;
    const el = document.activeElement as HTMLElement | null;
    if (prev && prev.el !== el && prev.el.isConnected && !prev.el.contains(el)) {
      const off = look(prev.el);
      const changed = off.some((v, i) => v !== prev.on[i]);
      out.push({ key: prev.key, ok: form === "tv" ? changed : !changed, why: `${form === "tv" ? "no lift" : "a fill or a lift beside the ring"}: focused ${prev.on.join(", ")}; unfocused ${off.join(", ")}` });
    }
    // TV draws no ring at all: no outline on the focused element, its art or its field.
    if (form === "tv" && el && el !== document.body) {
      const ringed = parts(el).filter((n) => { const s = getComputedStyle(n); return s.outlineStyle !== "none" && parseFloat(s.outlineWidth) > 0; });
      if (ringed.length) out.push({ key: el.dataset.f ?? el.tagName.toLowerCase(), ok: false, why: `a ring on ${ringed.map((n) => n.className || n.tagName).join(", ")}` });
    }
    if (!el || el === document.body) {
      w.__stop = undefined;
      return { key: null, results: out };
    }
    const key = el.dataset.f ?? el.tagName.toLowerCase();
    w.__stop = { el, key, on: look(el) };
    if (form !== "tv") {
      // The field that holds an input draws the ring (the language picker's .lp-field, the palette field): two levels up at most.
      const ringed = [...new Set([...parts(el), el.parentElement, el.parentElement?.parentElement])].filter((n): n is HTMLElement => !!n);
      const blue = "rgb(10, 132, 255)";
      const field = form === "phone" && el.tagName === "INPUT";
      const ring = ringed
        .map((n) => getComputedStyle(n))
        .find((s) => (s.outlineStyle !== "none" && parseFloat(s.outlineWidth) >= 4 && s.outlineColor === blue) || (field && s.boxShadow.includes(blue) && s.boxShadow.includes("inset")));
      out.push({ key, ok: !!ring, why: ringed.map((n) => `${n.className || n.tagName}: ${getComputedStyle(n).outlineWidth} ${getComputedStyle(n).outlineColor}`).join("; ") });
    }
    return { key, results: out };
  }, form);
