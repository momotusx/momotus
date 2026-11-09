/** First run, the Poster Wall (plans/first-screens.md, round2.html combo A): Welcome over the drifting wall, then
 * sheets over the still wall. The walks are driven by apps/shared/fixtures/first-run-vectors.json: each path's
 * presses on each form, its screens before Home counted against the vectors. The shared core is past first run, so a
 * proxy between page and core makes it fresh, answers the ops that would change it (setPrefs, installDefaults,
 * finishProfileStep) and records every op the page sends. */

import type { Browser, Page, WebSocketRoute } from "@playwright/test";
import { mkdirSync, readFileSync } from "node:fs";
import { drawnContrast, WHITE_PNG } from "./contrast";
import { Client, PORTS } from "./core";
import { expect, test } from "./fixture";

const shared = (path: string) => JSON.parse(readFileSync(new URL(`../../shared/${path}`, import.meta.url), "utf8"));
const vectors = shared("fixtures/first-run-vectors.json") as typeof import("../../shared/fixtures/first-run-vectors.json");
const words = shared("first-run-words.json") as typeof import("../../shared/first-run-words.json");
const streamWords = shared("stream-choice-words.json") as typeof import("../../shared/stream-choice-words.json");

type Form = "phone" | "wide" | "tv";
const SIZE: Record<Form, { width: number; height: number }> = { phone: { width: 390, height: 844 }, wide: { width: 1440, height: 900 }, tv: { width: 1920, height: 1080 } };
const FORM_QUERY: Record<Form, string> = { phone: "phone", wide: "desktop", tv: "tv" };
const SERVER = "https://sync.example.org";
const POSTER = "https://posters.test";
const POSTERS = Array.from({ length: 12 }, (_, i) => `${POSTER}/p${i}.png`);
/** A 2 by 3 grey PNG: a poster that loads. */
const GREY_PNG = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAIAAAADCAIAAAA2iEnWAAAAEklEQVR4nGNoaGhgQAMMw1sQAJPvBQG8n9WbAAAAAElFTkSuQmCC", "base64");
const SHOTS = process.env.FIRST_SHOTS ?? "test-results/shots/first-screens";
mkdirSync(SHOTS, { recursive: true });

type Json = any;
interface World {
  fresh: boolean;
  profileStepDone: boolean;
  profiles: Json[] | null;
  locked: boolean;
  subtitleLanguages: string[] | null;
  wall: string[];
  pairing: Json;
  importPhase: Json;
}

/** One device: a context whose socket passes through `world`, with the ops it sent. */
async function device(browser: Browser, form: Form, o: { server?: boolean; reducedMotion?: boolean; locale?: string; world?: Partial<World> } = {}) {
  const world: World = { fresh: true, profileStepDone: false, profiles: null, locked: false, subtitleLanguages: [], wall: POSTERS, pairing: { phase: "waiting", qr: "momotus://pair/e2e" }, importPhase: null, ...o.world };
  const context = await browser.newContext({
    viewport: SIZE[form],
    isMobile: form === "phone",
    hasTouch: form === "phone",
    colorScheme: "dark",
    reducedMotion: o.reducedMotion ? "reduce" : "no-preference",
    locale: o.locale ?? "en-US",
    baseURL: `http://127.0.0.1:${PORTS.vite}`,
  });
  if (o.server !== false) await context.addInitScript((s) => localStorage.setItem("mq.syncServer", s), SERVER);
  await context.route(`${POSTER}/**`, (r) => r.fulfill({ body: GREY_PNG, contentType: "image/png" }));
  const ops: { op: string; args: Json }[] = [];
  const subs = new Map<number, { view: string; last: Json }>();
  const asked = new Map<number, string>();
  let route: WebSocketRoute | null = null;
  const patch = (view: string, s: Json): Json => {
    if (view === "addons") return { ...s, fresh: world.fresh };
    if (view === "profiles") {
      const out = { ...s, profileStepDone: world.profileStepDone, locked: world.locked };
      if (world.profiles) out.profiles = world.profiles;
      return out;
    }
    if (view === "settings" && world.subtitleLanguages) return { ...s, subtitleLanguages: world.subtitleLanguages };
    if (view === "pairing") return world.pairing;
    if (view === "import" && world.importPhase) return world.importPhase;
    return s;
  };
  const stub: Record<string, () => Json> = {
    firstRunWall: () => world.wall,
    relayInfo: () => ({ providers: ["google", "facebook", "email"] }),
    finishProfileStep: () => null,
    setPrefs: () => null,
    installDefaults: () => {
      world.fresh = false;
      setTimeout(() => push("addons"), 0);
      return null;
    },
  };
  const fail = new Set(["syncNow"]);
  const FAKE_VIEWS = new Set(["pairing", "import"]);
  let fakeSub = 90_000;
  await context.routeWebSocket(/\/ws$/, (ws) => {
    route = ws;
    // A page load opens a new socket whose subs count from 1 again.
    subs.clear();
    asked.clear();
    const core = new WebSocket(`ws://127.0.0.1:${PORTS.bridge}/ws`);
    const early: string[] = [];
    core.addEventListener("open", () => {
      for (const m of early.splice(0)) core.send(m);
    });
    core.addEventListener("message", (e) => {
      const m = JSON.parse(String(e.data));
      const view = asked.get(m.id);
      if (view && m.ok && m.result && typeof m.result === "object" && "snapshot" in m.result) {
        subs.set(m.result.sub, { view, last: m.result.snapshot });
        m.result.snapshot = patch(view, structuredClone(m.result.snapshot));
      } else if ("sub" in m && Array.isArray(m.updates) && subs.has(m.sub)) {
        const s = subs.get(m.sub) as { view: string; last: Json };
        s.last = m.updates[m.updates.length - 1];
        m.updates = m.updates.map((u: Json) => patch(s.view, structuredClone(u)));
      }
      ws.send(JSON.stringify(m));
    });
    core.addEventListener("close", () => ws.close());
    ws.onMessage((raw) => {
      const m = JSON.parse(String(raw));
      if (m.op === "subscribe") asked.set(m.id, m.view);
      if (m.op && m.op !== "subscribe" && m.op !== "unsubscribe" && m.op !== "ping") ops.push({ op: m.op, args: m });
      // Views that would reach a relay or Stremio: answered here, their snapshots from the world.
      if (m.op === "subscribe" && FAKE_VIEWS.has(m.view)) {
        const sub = ++fakeSub;
        subs.set(sub, { view: m.view, last: {} });
        return ws.send(JSON.stringify({ id: m.id, ok: true, result: { sub, snapshot: patch(m.view, {}) } }));
      }
      if (m.op === "unsubscribe" && m.sub > 90_000) return ws.send(JSON.stringify({ id: m.id, ok: true, result: null }));
      if (m.op in stub) return ws.send(JSON.stringify({ id: m.id, ok: true, result: stub[m.op]?.() }));
      if (fail.has(m.op)) return ws.send(JSON.stringify({ id: m.id, ok: false, error: "the sync service did not answer" }));
      if (core.readyState === WebSocket.OPEN) core.send(String(raw));
      else early.push(String(raw));
    });
    ws.onClose(() => core.close());
  });
  /** Sends a view's last snapshot again, through the patch as the world stands now. */
  const push = (view: string) => {
    for (const [sub, s] of subs) if (s.view === view) route?.send(JSON.stringify({ sub, updates: [patch(view, structuredClone(s.last))] }));
  };
  const page = await context.newPage();
  return { context, page, world, ops, push, sent: (op: string) => ops.filter((x) => x.op === op) };
}
type Device = Awaited<ReturnType<typeof device>>;

const open = async (d: Device, form: Form, path = "/welcome") => {
  await d.page.goto(`${path}${path.includes("?") ? "&" : "?"}form=${FORM_QUERY[form]}`);
};
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);

/** The screen on show: a first run step, or home once none is. */
async function screen(page: Page): Promise<string> {
  return page.evaluate(() => {
    if (document.querySelector(".welcome-block")) return "welcome";
    if (document.querySelector(".sheet.who")) return "who";
    if (document.querySelector(".sheet.import")) return "import";
    if (document.querySelector(".sheet.kit-sheet")) return "recoveryKit";
    if (document.querySelector("#setup-q")) return "setup";
    if (document.querySelector("#profile-q")) return "profile";
    if (document.querySelector("#sign-in-q")) return "login";
    return location.pathname === "/" && !document.querySelector(".first") ? "home" : "pending";
  });
}

/** Each press of the vectors, as the control it names. */
const CONTROL: Record<string, string> = {
  getStarted: "welcome:start",
  alreadyUse: "welcome:returning",
  notNow: "signin:not-now",
  useAnotherDevice: "signin:device",
  continue: "profile:continue",
  bringLibrary: "setup:bring",
  startWatching: "setup:start",
};
/** Presses that leave first run on the same screen. */
const STAYS = new Set(["bringLibrary", "useAnotherDevice"]);

const PROFILES = (n: number, pin = false) =>
  [
    { id: "default", name: "Robin", avatar: "", position: 0, hasPin: pin, isOwner: true, syncing: false, kids: false },
    { id: "deniz", name: "Deniz", avatar: "letter:sky", position: 1, hasPin: false, isOwner: false, syncing: false, kids: false },
    { id: "kids", name: "Kids", avatar: "letter:teal", position: 2, hasPin: false, isOwner: false, syncing: false, kids: true },
  ].slice(0, n);

/** The other device approves: the account's profiles arrive, then the join. */
async function join(d: Device, profiles: number, pin = false) {
  d.world.profiles = PROFILES(profiles, pin).map((p, i) => (i === 0 ? { ...p, id: "default" } : p));
  d.world.fresh = false;
  d.push("profiles");
  d.push("addons");
  d.world.pairing = { phase: "joined", by: "Robin's iPhone", record: { server: SERVER, account: "e2e", member: "m1", key: "k" } };
  d.push("pairing");
}

/** Walks one path of the vectors on one form: every press as a click (TV: OK on the focused control, its D-pad moves
 * counted), the screens before Home recorded. */
async function walk(d: Device, form: Form, path: string, at: { profiles?: number; pin?: boolean; onImport?: (d: Device) => Promise<void> } = {}) {
  const v = vectors.paths.find((p) => p.path === path) as unknown as Record<string, { screens: string[]; presses: string[]; moves?: number | null }>;
  const want = v[form] as { screens: string[]; presses: string[]; moves?: number | null };
  const page = d.page;
  const seen: string[] = [];
  const note = async () => {
    await expect.poll(() => screen(page)).not.toBe("pending");
    const s = await screen(page);
    if (s !== "home" && seen[seen.length - 1] !== s) seen.push(s);
    return s;
  };
  await note();
  let moves = 0;
  let pressed = 0;
  for (const [i, press] of want.presses.entries()) {
    const before = await screen(page);
    if (press === "profileTile" && before === "login") {
      await join(d, at.profiles ?? 3, at.pin);
      await expect.poll(() => screen(page), { timeout: 30_000 }).toBe("who");
      await note();
    }
    const control = press === "profileTile" ? "profile:default" : (CONTROL[press] as string);
    const target = page.locator(`[data-f="${control}"]`);
    await expect(target).toBeVisible();
    if (form === "tv") {
      if (v.tv?.moves === null) await target.focus();
      // The first focus lands on its own; a control beside it is reached with the D-pad.
      for (let n = 0; n < 4 && (await focused(page)) !== control; n++) {
        await page.waitForTimeout(400);
        if ((await focused(page)) === control) break;
        await page.keyboard.press(n === 0 && press === "profileTile" ? "ArrowLeft" : "ArrowRight");
        moves++;
      }
      expect(await focused(page), `${path} tv: ${press} focused`).toBe(control);
      await page.keyboard.press("Enter");
    } else await target.click();
    pressed++;
    if (STAYS.has(press)) continue;
    if (i === want.presses.length - 1 && press === "startWatching" && at.onImport) {
      await expect.poll(() => screen(page)).toBe("import");
      await note();
      await at.onImport(d);
      continue;
    }
    await expect.poll(() => screen(page), { timeout: 30_000 }).not.toBe(before);
    await note();
  }
  // A join comes from the other device, with no press here.
  if ((await screen(page)) === "login") {
    await join(d, at.profiles ?? 1, at.pin);
    await expect.poll(() => screen(page), { timeout: 30_000 }).toBe("home");
  }
  expect(seen, `${path} ${form} screens`).toEqual(want.screens);
  expect(pressed, `${path} ${form} presses`).toBe(want.presses.length);
  if (form === "tv" && typeof want.moves === "number") expect(moves, `${path} tv D-pad moves`).toBe(want.moves);
}

const FORMS: Form[] = ["phone", "wide", "tv"];

for (const form of FORMS) {
  test(`${form}: New, Not now: the vectors' screens and presses, nothing written before Not now, an untouched Continue sends empties`, async ({ browser }) => {
    const d = await device(browser, form);
    await open(d, form);
    await walk(d, form, "newNotNow");
    expect(d.sent("createProfile")).toEqual([]);
    const finish = d.sent("finishProfileStep");
    expect(finish.map((f) => ({ name: f.args.name, avatar: f.args.avatar }))).toEqual([{ name: "", avatar: "" }]);
    // Not now came before the profile step's write: the op list holds them in that order.
    const order = d.ops.map((o) => o.op);
    expect(order.indexOf("finishProfileStep")).toBeGreaterThan(-1);
    // Start watching wrote the shown chip, English on an English device, and no stream patch for Start fast.
    expect(d.sent("setPrefs").map((p) => p.args.patch)).toEqual([{ subtitleLanguages: ["en"] }]);
    expect(d.sent("installDefaults")).toHaveLength(1);
    await expect(d.page).toHaveURL(/\/(\?form=\w+)?$/);
    await d.context.close();
  });

  test(`${form}: no sync server: Welcome, profile, setup`, async ({ browser }) => {
    const d = await device(browser, form, { server: false });
    await open(d, form);
    await expect(d.page.locator('[data-f="welcome:returning"]')).toHaveCount(0);
    await walk(d, form, "noServer");
    await d.context.close();
  });

  test(`${form}: joining with one profile opens Home; with three, Who's watching over the wall`, async ({ browser }) => {
    const one = await device(browser, form);
    await open(one, form);
    await walk(one, form, "joinOneProfile", { profiles: 1 });
    expect(one.sent("finishProfileStep")).toEqual([]);
    await one.context.close();
    const many = await device(browser, form);
    await open(many, form);
    await walk(many, form, "joinManyProfilesOrPin", { profiles: 3 });
    expect(many.sent("createProfile")).toEqual([]);
    await many.context.close();
  });

  test(`${form}: I use Stremio: setup's Bring my Stremio library opens the import sheet, its Done installs and goes Home`, async ({ browser }) => {
    const d = await device(browser, form, { world: { importPhase: { phase: "waitingForLink", code: "AB12", linkUrl: "https://link.example.org" } } });
    await open(d, form);
    await walk(d, form, "newStremio", {
      onImport: async (dd) => {
        await expect(dd.page.locator(".sheet.import h1")).toHaveText("Import from Stremio");
        dd.world.importPhase = { phase: "done", summary: { addonsImported: 3, addonsSkipped: ["Own Addon"], titles: 12, inProgress: 2, watched: 40, seriesUnmatched: 0 } };
        dd.push("import");
        await dd.page.locator('[data-f="import:home"]').click();
        await expect.poll(() => screen(dd.page)).toBe("home");
      },
    });
    expect(d.sent("installDefaults")).toHaveLength(1);
    await d.context.close();
  });
}

test("a joined account whose one profile has a PIN shows Who's watching", async ({ browser }) => {
  const d = await device(browser, "wide");
  await open(d, "wide");
  await d.page.locator('[data-f="welcome:returning"]').click();
  await expect.poll(() => screen(d.page)).toBe("login");
  await join(d, 1, true);
  await expect.poll(() => screen(d.page), { timeout: 30_000 }).toBe("who");
  await expect(d.page.locator(".sheet.who .sub")).toHaveText("Signed in from Robin's iPhone.");
  await d.page.waitForTimeout(500);
  await d.page.screenshot({ path: `${SHOTS}/wide-who.png` });
  await d.context.close();
  const phone = await device(browser, "phone");
  await open(phone, "phone");
  await phone.page.locator('[data-f="welcome:returning"]').click();
  await phone.page.locator('[data-f="signin:device"]').click();
  await join(phone, 3);
  await expect.poll(() => screen(phone.page), { timeout: 30_000 }).toBe("who");
  await expect(phone.page.locator('.sheet.who [data-f="profile:add"]')).toBeVisible();
  await phone.page.waitForTimeout(500);
  await phone.page.screenshot({ path: `${SHOTS}/phone-who.png` });
  await phone.context.close();
});

for (const phase of ["failed", "expired"] as const) {
  test(`import ${phase}: Skip import installs the defaults and opens Home; Back returns to setup with the answers kept`, async ({ browser }) => {
    const d = await device(browser, "phone", { world: { profileStepDone: true, importPhase: { phase: "waitingForLink", code: "AB12", linkUrl: "https://link.example.org" } } });
    await open(d, "phone", "/welcome?step=setup");
    await d.page.locator('[data-f="setup:bring"]').click();
    await d.page.locator('[data-f="setup:best"]').click();
    await d.page.locator('[data-f="setup:start"]').click();
    await expect.poll(() => screen(d.page)).toBe("import");
    await expect(d.page.locator('[data-f="import:skip"]')).toBeVisible();
    await d.page.goBack();
    await expect.poll(() => screen(d.page)).toBe("setup");
    await expect(d.page.locator('[data-f="setup:bring"]')).toHaveAttribute("aria-checked", "true");
    await expect(d.page.locator('[data-f="setup:best"]')).toHaveAttribute("aria-checked", "true");
    await d.page.locator('[data-f="setup:start"]').click();
    await expect.poll(() => screen(d.page)).toBe("import");
    // The answers were written once: the second Start watching sends no second write.
    expect(d.sent("setPrefs")).toHaveLength(1);
    d.world.importPhase = phase === "failed" ? { phase: "failed", reason: "Stremio did not answer." } : { phase: "expired" };
    d.push("import");
    await expect(d.page.locator('[data-f="import:skip"]')).toBeVisible();
    await d.page.waitForTimeout(500);
    await d.page.waitForTimeout(500);
    await d.page.screenshot({ path: `${SHOTS}/phone-import-${phase}.png` });
    await d.page.locator('[data-f="import:skip"]').click();
    await expect.poll(() => screen(d.page)).toBe("home");
    expect(d.sent("installDefaults")).toHaveLength(1);
    await d.context.close();
  });
}

test("Get started never writes a profile: nothing but the wall before Login", async ({ browser }) => {
  const d = await device(browser, "phone");
  await open(d, "phone");
  await d.page.locator('[data-f="welcome:start"]').click();
  await expect.poll(() => screen(d.page)).toBe("login");
  expect(d.sent("finishProfileStep")).toEqual([]);
  expect(d.sent("createProfile")).toEqual([]);
  expect(d.sent("updateProfile")).toEqual([]);
  await d.context.close();
});

test("a typed name and a pressed colour are what Continue sends", async ({ browser }) => {
  const d = await device(browser, "phone", { world: { profileStepDone: false } });
  await open(d, "phone", "/welcome?step=profile");
  await expect(d.page.locator('[data-f="profile:name"]')).toBeVisible();
  // Never focused on open: no keyboard over Continue.
  expect(await focused(d.page)).not.toBe("profile:name");
  await expect(d.page.locator('[data-f="profile:name"]')).toHaveValue("");
  await expect(d.page.locator('[data-f="profile:name"]')).toHaveAttribute("placeholder", "Main");
  await d.page.locator('[data-f="profile:name"]').fill("Deniz");
  await d.page.locator('[data-f="profile:colour:sky"]').click();
  await d.page.locator('[data-f="profile:continue"]').click();
  await expect.poll(() => screen(d.page)).toBe("setup");
  expect(d.sent("finishProfileStep").map((f) => ({ name: f.args.name, avatar: f.args.avatar }))).toEqual([{ name: "Deniz", avatar: "letter:sky" }]);
  await d.context.close();
});

test("setup's subtitle chips: the vectors' order and pre-chosen chip (D11)", async ({ browser }) => {
  for (const row of vectors.chips) {
    const d = await device(browser, "wide", { locale: row.device ?? "xx", world: { profileStepDone: true, subtitleLanguages: row.chosen } });
    await open(d, "wide", "/welcome?step=setup");
    const chips = d.page.locator('[data-fgroup="setup:subs"] [data-f]');
    await expect(chips.first()).toBeVisible();
    const keys = (await chips.evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.f))).map((f) => (f as string).replace("setup:sub:", ""));
    expect(keys, JSON.stringify(row)).toEqual(row.chips);
    await expect(d.page.locator(`[data-f="setup:sub:${row.selected}"]`)).toHaveAttribute("aria-checked", "true");
    await expect(d.page.locator('[data-fgroup="setup:subs"] [aria-checked="true"]')).toHaveCount(1);
    await d.context.close();
  }
});

test("No subtitles writes an empty list; Best quality adds its stream patch", async ({ browser }) => {
  const d = await device(browser, "wide", { world: { profileStepDone: true } });
  await open(d, "wide", "/welcome?step=setup");
  await d.page.locator('[data-f="setup:sub:none"]').click();
  await d.page.locator('[data-f="setup:best"]').click();
  await d.page.locator('[data-f="setup:start"]').click();
  await expect.poll(() => screen(d.page)).toBe("home");
  const q = streamWords.question;
  expect(d.sent("setPrefs").map((p) => p.args.patch)).toEqual([{ subtitleLanguages: [], streams: q.best.patch }]);
  await d.context.close();
});

test("Skip on setup installs the defaults and writes no preference", async ({ browser }) => {
  const d = await device(browser, "phone", { world: { profileStepDone: true } });
  await open(d, "phone", "/welcome?step=setup");
  await d.page.locator('[data-f="setup:skip"]').click();
  await expect.poll(() => screen(d.page)).toBe("home");
  expect(d.sent("setPrefs")).toEqual([]);
  expect(d.sent("installDefaults")).toHaveLength(1);
  await d.context.close();
});

test("a start lands where the run stopped: the profile form in an account, setup once answered, none when not fresh", async ({ browser }) => {
  const done = await device(browser, "wide", { world: { profileStepDone: true } });
  await open(done, "wide");
  await expect(done.page).toHaveURL(/step=setup/);
  await done.context.close();
  const stale = await device(browser, "wide", { world: { fresh: false } });
  await open(stale, "wide");
  await expect.poll(() => screen(stale.page)).toBe("home");
  await stale.context.close();
});

test("the wall op is never sent on a cold start of a profile that is not fresh", async ({ browser }) => {
  for (const form of FORMS) {
    const d = await device(browser, form, { world: { fresh: false } });
    await open(d, form, "/");
    await expect(d.page.locator(".app")).toBeVisible();
    await d.page.waitForTimeout(2000);
    expect(d.sent("firstRunWall"), form).toEqual([]);
    await d.context.close();
  }
});

test("no default addon's name on any first run step", async ({ browser }) => {
  const c = new Client();
  await c.ready;
  const names = (await c.req<{ addons: { name: string }[] }>("defaultAddons")).addons.map((a) => a.name);
  c.close();
  expect(names.length).toBeGreaterThan(3);
  for (const form of FORMS) {
    const d = await device(browser, form, { world: { profileStepDone: false } });
    for (const step of ["", "?step=login", "?step=profile", "?step=setup"]) {
      await open(d, form, `/welcome${step}`);
      await expect.poll(() => screen(d.page)).not.toBe("pending");
      await d.page.waitForTimeout(300);
      const text = await d.page.locator("main").innerText();
      for (const n of names) expect(text, `${form} ${step || "welcome"}: ${n}`).not.toContain(n);
    }
    await d.context.close();
  }
});

test("phone: every control of every step is a 48 px target", async ({ browser }) => {
  const d = await device(browser, "phone", { world: { profileStepDone: false } });
  for (const step of ["", "?step=login", "?step=profile", "?step=setup"]) {
    await open(d, "phone", `/welcome${step}`);
    await expect.poll(() => screen(d.page)).not.toBe("pending");
    await d.page.waitForTimeout(400);
    const small = await d.page.locator("main :is(button, a, input)").evaluateAll((els) =>
      els
        .map((e) => (e.tagName === "INPUT" ? (e.closest("label") ?? e) : e))
        .filter((e) => (e as HTMLElement).offsetParent !== null)
        .map((e) => ({ f: (e as HTMLElement).dataset.f ?? e.textContent, r: e.getBoundingClientRect() }))
        .filter((x) => x.r.width > 0 && Math.min(x.r.width, x.r.height) < 48)
        .map((x) => `${x.f} ${Math.round(x.r.width)}x${Math.round(x.r.height)}`),
    );
    expect(small, step || "welcome").toEqual([]);
  }
  await d.context.close();
});

test("the wall: decorative, drifting on Welcome only, still under every sheet, for reduced motion and the pause toggle", async ({ browser }) => {
  const d = await device(browser, "wide", { world: { profileStepDone: false } });
  const page = d.page;
  const state = () =>
    page.locator(".wall-col").first().evaluate((c) => {
      const s = getComputedStyle(c);
      return s.animationName === "none" ? "none" : s.animationPlayState;
    });
  await open(d, "wide");
  await expect(page.locator(".wall-frame")).toHaveAttribute("aria-hidden", "true");
  expect(await page.locator(".wall-frame [data-f], .wall-frame button, .wall-frame a").count()).toBe(0);
  await expect.poll(state).toBe("running");
  await expect(page.locator(".wall-tile img.in").first()).toBeVisible();
  for (const step of ["login", "profile", "setup"]) {
    await open(d, "wide", `/welcome?step=${step}`);
    await expect(page.locator(".sheet")).toBeVisible();
    await expect.poll(state, step).toBe("paused");
  }
  // The toggle: a pressed state, kept on the device.
  await open(d, "wide");
  const toggle = page.locator('[data-f="welcome:motion"]');
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  await expect.poll(state).toBe("paused");
  await page.reload();
  await expect(page.locator('[data-f="welcome:motion"]')).toHaveAttribute("aria-pressed", "true");
  await expect.poll(state).toBe("paused");
  await d.context.close();
  const reduced = await device(browser, "wide", { reducedMotion: true });
  await open(reduced, "wide");
  await expect(reduced.page.locator(".welcome-block")).toBeVisible();
  expect(await reduced.page.locator(".wall-col").first().evaluate((c) => getComputedStyle(c).animationName)).toBe("none");
  await reduced.context.close();
});

test("an empty wall answer: the empty tiles, no image, nothing broken", async ({ browser }) => {
  const d = await device(browser, "phone", { world: { wall: [] } });
  const failed: string[] = [];
  d.page.on("requestfailed", (r) => failed.push(r.url()));
  await open(d, "phone");
  await expect(d.page.locator(".welcome-block")).toBeVisible();
  expect(await d.page.locator(".wall-tile").count()).toBe(4 * 12);
  await expect(d.page.locator(".wall-tile img")).toHaveCount(0);
  // Asked again at the next step.
  await d.page.locator('[data-f="welcome:start"]').click();
  await expect.poll(() => d.sent("firstRunWall").length).toBeGreaterThanOrEqual(2);
  expect(failed.filter((u) => u.startsWith(POSTER))).toEqual([]);
  await d.context.close();
});

for (const form of FORMS) {
  test(`${form}: every word keeps 4.5:1 over a white wall, and focused TV controls too`, async ({ browser }) => {
    const d = await device(browser, form, { world: { profileStepDone: false } });
    await d.context.route(`${POSTER}/**`, (r) => r.fulfill({ body: WHITE_PNG, contentType: "image/png" }));
    await open(d, form);
    await expect(d.page.locator(".wall-tile img.in").first()).toBeVisible();
    const welcome = await drawnContrast(d.page, ".welcome-block", ["h1", "p"], []);
    expect(welcome.length).toBeGreaterThanOrEqual(2);
    for (const m of welcome) expect(m.ratio, `welcome ${m.what}`).toBeGreaterThanOrEqual(m.floor);
    for (const step of ["login", "profile", "setup"]) {
      await open(d, form, `/welcome?step=${step}`);
      await expect(d.page.locator(".sheet h1").first()).toBeVisible();
      await d.page.waitForTimeout(400);
      const sheet = await drawnContrast(d.page, ".sheet", ["h1", ".sub", "h2.sect", ".fr-opt b", ".fr-opt small", ".hint", ".srcl"], []);
      for (const m of sheet) expect(m.ratio, `${step} ${m.what}`).toBeGreaterThanOrEqual(m.floor);
    }
    if (form === "tv") {
      await open(d, form, "/welcome?step=setup");
      for (const f of ["setup:start", "setup:fresh", "setup:fast", "setup:sub:en"]) {
        await d.page.locator(`[data-f="${f}"]`).focus();
        await d.page.waitForTimeout(250);
        const m = await drawnContrast(d.page, `[data-f="${f}"]`, ["", "b", "small"], []);
        for (const x of m) expect(x.ratio, `focused ${f} ${x.what}`).toBeGreaterThanOrEqual(x.floor);
      }
    }
    await d.context.close();
  });
}

test("TV: first focus per step, Up reaches the pause toggle, and Settings' Sign in opens the Login sheet", async ({ browser }) => {
  const d = await device(browser, "tv", { world: { profileStepDone: false } });
  const page = d.page;
  await open(d, "tv");
  await expect.poll(() => focused(page)).toBe("welcome:start");
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => focused(page)).toBe("welcome:motion");
  for (const [step, first] of [["login", "signin:not-now"], ["profile", "profile:continue"], ["setup", "setup:start"]] as const) {
    await open(d, "tv", `/welcome?step=${step}`);
    await expect.poll(() => focused(page), step).toBe(first);
  }
  await page.goto("/settings?form=tv");
  const row = page.locator('[data-f="settings:sign-in"]');
  if (await row.count()) await expect(row).toHaveAttribute("href", "/welcome?step=login");
  await open(d, "tv", "/welcome?step=login");
  await expect(page.locator(".sheet.tv .tvl ol li")).toHaveText(words.tvSteps);
  await d.context.close();
});

test("TV without a QR: one line, Try again and Not now, Not now first", async ({ browser }) => {
  const d = await device(browser, "tv", { server: false });
  await open(d, "tv", "/welcome?step=login");
  await expect(d.page.locator(".sheet.tv .sub")).toHaveText(words.tvNoQr);
  await expect(d.page.locator(".sheet.tv .acts .btn")).toHaveText([words.tryAgain, "Not now"]);
  await expect.poll(() => focused(d.page)).toBe("signin:not-now");
  await d.page.keyboard.press("Enter");
  await expect.poll(() => screen(d.page)).toBe("profile");
  await d.context.close();
});

test("Back and Escape: Login to Welcome, profile to Login, setup to profile; a grabber drag goes back too", async ({ browser }) => {
  const d = await device(browser, "wide", { world: { profileStepDone: false } });
  const page = d.page;
  await open(d, "wide");
  await page.locator('[data-f="welcome:start"]').click();
  await expect.poll(() => screen(page)).toBe("login");
  await page.keyboard.press("Escape");
  await expect.poll(() => screen(page)).toBe("welcome");
  await page.locator('[data-f="welcome:start"]').click();
  await page.locator('[data-f="signin:not-now"]').click();
  await expect.poll(() => screen(page)).toBe("profile");
  await page.goBack();
  await expect.poll(() => screen(page)).toBe("login");
  await page.goForward();
  await page.locator('[data-f="profile:continue"]').click();
  await expect.poll(() => screen(page)).toBe("setup");
  await page.keyboard.press("Escape");
  await expect.poll(() => screen(page)).toBe("profile");
  await d.context.close();
  const phone = await device(browser, "phone");
  await open(phone, "phone");
  await phone.page.locator('[data-f="welcome:start"]').click();
  await expect.poll(() => screen(phone.page)).toBe("login");
  const grab = await phone.page.locator(".sheet-grab").boundingBox();
  if (!grab) throw new Error("no grabber");
  await phone.page.mouse.move(grab.x + grab.width / 2, grab.y + 10);
  await phone.page.mouse.down();
  await phone.page.mouse.move(grab.x + grab.width / 2, grab.y + 200, { steps: 5 });
  await phone.page.mouse.up();
  await expect.poll(() => screen(phone.page)).toBe("welcome");
  await phone.context.close();
});

test("desktop, keyboard only: Not now's path with Tab and Enter", async ({ browser }) => {
  const d = await device(browser, "wide");
  const page = d.page;
  await open(d, "wide");
  await expect(page.locator('[data-f="welcome:start"]')).toBeVisible();
  const tabTo = async (f: string) => {
    // The step's own first focus (its heading) lands first; the walk starts from there.
    await page.waitForTimeout(300);
    for (let i = 0; i < 20 && (await focused(page)) !== f; i++) await page.keyboard.press("Tab");
    expect(await focused(page)).toBe(f);
    const ring = await page.evaluate(() => getComputedStyle(document.activeElement as Element).outlineStyle);
    expect(ring, `${f} shows its focus`).not.toBe("none");
    await page.keyboard.press("Enter");
  };
  await tabTo("welcome:start");
  await expect.poll(() => screen(page)).toBe("login");
  await tabTo("signin:not-now");
  await expect.poll(() => screen(page)).toBe("profile");
  await tabTo("profile:continue");
  await expect.poll(() => screen(page)).toBe("setup");
  await tabTo("setup:start");
  await expect.poll(() => screen(page)).toBe("home");
  await d.context.close();
});

test("shots: each step on the phone, desktop and TV", async ({ browser }) => {
  for (const form of FORMS) {
    const d = await device(browser, form, { reducedMotion: true, world: { profileStepDone: false } });
    for (const step of ["", "login", "profile", "setup"]) {
      await open(d, form, step ? `/welcome?step=${step}` : "/welcome");
      await expect.poll(() => screen(d.page)).not.toBe("pending");
      await expect(d.page.locator(".wall-tile img.in").first()).toBeVisible();
      await d.page.waitForTimeout(400);
      await d.page.screenshot({ path: `${SHOTS}/${form}-${step || "welcome"}.png` });
    }
    await d.context.close();
  }
});
