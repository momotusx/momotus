/** Sign-in as the Login sheet over the poster wall (plans/first-screens.md 3.3, D10), from Settings' Sign in and from
 * first run alike: "Sign in" with Not now at its right, the providers in a column (Google and Facebook in their own
 * marks and colors on every form), email, Other ways; desktop puts the live QR to the right, the phone a Use another
 * device button. From Settings, Escape is Not now. A relay of its own lists Google, Facebook, email and the mock
 * OpenID provider; the mock signs in as alice for the `new` step. */

import { type Browser, expect, type Page, test } from "./fixture";
import type { ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Client, PORTS, SHOTS, SIGN_IN_PORTS as P, startBridge, startMockProvider, startRelay } from "./core";

const SERVER = `http://127.0.0.1:${P.relay}`;
const procs: { proc: ChildProcess; data?: string }[] = [];
let scratch = "";
/** The next `op` the page asks is answered with no code, as when the sync service does not answer. */
let failNext = "";
/** The addons view says `fresh`, as on a profile that never had a list: first run's steps go on to setup. */
let freshAddons = false;

const FORMS = {
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
};

async function device(browser: Browser, form: keyof typeof FORMS): Promise<Page> {
  const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.addInitScript((server) => localStorage.setItem("mq.syncServer", server), SERVER);
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const core = new WebSocket(`ws://127.0.0.1:${P.core}/ws`);
    const early: string[] = [];
    core.addEventListener("open", () => {
      for (const m of early.splice(0)) core.send(m);
    });
    // The addons view's requests and its sub, for `freshAddons`.
    const addonsAsks = new Set<number>();
    let addonsSub = -1;
    core.addEventListener("message", (e) => {
      if (!freshAddons) return ws.send(String(e.data));
      const m = JSON.parse(String(e.data));
      if (addonsAsks.has(m.id) && m.result?.snapshot) {
        addonsSub = m.result.sub;
        m.result.snapshot.fresh = true;
      } else if (m.sub === addonsSub && Array.isArray(m.updates)) for (const u of m.updates) u.fresh = true;
      ws.send(JSON.stringify(m));
    });
    core.addEventListener("close", () => ws.close());
    ws.onMessage((m) => {
      const request = JSON.parse(String(m)) as { id?: number; op?: string; view?: string };
      if (request.op === "subscribe" && request.view === "addons" && request.id !== undefined) addonsAsks.add(request.id);
      if (failNext && request.op === failNext) {
        failNext = "";
        return ws.send(JSON.stringify({ id: request.id, ok: false, error: "the sync service did not answer" }));
      }
      if (core.readyState === WebSocket.OPEN) core.send(String(m));
      else early.push(String(m));
    });
    ws.onClose(() => core.close());
  });
  const page = await context.newPage();
  await page.goto(`/settings?form=${form}`);
  return page;
}

const go = (page: Page, path: string) =>
  page.evaluate((p) => {
    history.pushState(null, "", p);
    dispatchEvent(new PopStateEvent("popstate"));
  }, path);

const box = async (page: Page, selector: string) => (await page.locator(selector).first().boundingBox())!;

test.describe.serial("desktop sign-in stage", () => {
  test.beforeAll(async () => {
    // A first run builds the relay and the mock provider.
    test.setTimeout(600_000);
    scratch = mkdtempSync(join(tmpdir(), "mq-sign-in-e2e-"));
    const mock = await startMockProvider(P.mock);
    procs.push(mock);
    const identity = join(scratch, "identity.json");
    writeFileSync(
      identity,
      JSON.stringify({
        key: randomBytes(32).toString("hex"),
        publicUrl: SERVER,
        google: { webClientId: "web", webClientSecret: "ws", issuer: mock.url },
        facebook: { appId: "app", issuer: mock.url },
        oidc: [{ id: "mock", issuer: mock.url, clientId: "marquee", clientSecret: "ms" }],
        email: { accessKeyId: "AKIDMOCK", secretAccessKey: "mock/secret/key", region: "us-east-1", from: "noreply@example.org", endpoint: mock.url },
      }),
    );
    procs.push(await startRelay(P.relay, undefined, ["--identity", identity, "--public-host", "127.0.0.1"]));
    procs.push(await startBridge(P.core));
    // Something on Home, so the device is past the welcome steps.
    const c = new Client(`ws://127.0.0.1:${P.core}/ws`);
    await c.req("installAddon", { url: `http://127.0.0.1:${PORTS.source}/manifest.json` });
    c.close();
  });

  test.afterAll(async () => {
    for (const { proc, data } of procs) {
      proc.kill();
      if (data) rmSync(data, { recursive: true, force: true });
    }
    if (scratch) rmSync(scratch, { recursive: true, force: true });
  });

  test("desktop: the Login sheet over the wall, the brands' buttons, the live QR at its right; Tab, Escape", async ({ browser }) => {
    const page = await device(browser, "desktop");
    await go(page, "/sign-in");
    const sheet = page.locator(".sheet.login");
    await expect(sheet.locator("h1")).toHaveText("Sign in");
    await expect(sheet.locator(".sub")).toHaveText("This device's library will be added to your account.");
    await expect(page.locator(".wall-frame")).toHaveCount(1);
    await expect(sheet.locator(".sheet-head .flink")).toHaveText("Not now");
    await expect(page.locator(".prov .btn")).toHaveText(["Continue with Google", "Continue with Facebook", "Continue with mock", "Continue with email"]);
    // Google's white button and Facebook's blue one, each with its own mark; the others keep the app's own.
    const look = (f: string) =>
      page.locator(`[data-f="${f}"]`).evaluate((el) => {
        const s = getComputedStyle(el);
        const mark = el.querySelector("img.provider-mark") as HTMLImageElement | null;
        return { bg: s.backgroundColor, color: s.color, mark: mark?.naturalWidth ?? 0 };
      });
    const google = await look("signin:google");
    expect(google).toMatchObject({ bg: "rgb(255, 255, 255)", color: "rgba(0, 0, 0, 0.88)" });
    expect(google.mark).toBeGreaterThan(0);
    const facebook = await look("signin:facebook");
    expect(facebook).toMatchObject({ bg: "rgb(24, 119, 242)", color: "rgb(255, 255, 255)" });
    expect(facebook.mark).toBeGreaterThan(0);
    expect((await look("signin:mock")).mark).toBe(0);

    // The sheet: 860 wide, centered; the providers in a column.
    const s = await box(page, ".sheet.login");
    expect(Math.abs(s.width - 860)).toBeLessThanOrEqual(2);
    expect(Math.abs(s.x + s.width / 2 - 720)).toBeLessThanOrEqual(2);
    const [g, f] = [await box(page, '[data-f="signin:google"]'), await box(page, '[data-f="signin:facebook"]')];
    expect(f.y).toBeGreaterThan(g.y + g.height);

    // The QR column: "Sign in with your phone", how, the live code and that it waits, right of the ways in.
    const corner = page.locator("#sign-in-qr");
    await expect(corner.locator("h2")).toHaveText("Sign in with your phone");
    await expect(corner.locator(".qrbox svg")).toBeVisible({ timeout: 30_000 });
    await expect(corner.locator(".pair-steps .step")).toHaveText("Point your phone's camera at the code, then sign in on the page that opens.");
    await expect(corner.locator(".wait")).toHaveText("Waiting for your phone");
    const c = await box(page, "#sign-in-qr");
    for (const f of ["signin:google", "signin:email", "signin:other"]) {
      const b = await box(page, `[data-f="${f}"]`);
      expect(b.x + b.width, `${f} runs into the QR`).toBeLessThan(c.x);
    }
    await expect(page.locator('[data-f="signin:already"]')).toHaveCount(0);
    await page.screenshot({ path: `${SHOTS}/sign-in-sheet-desktop.png` });

    // Tab walks the ways in order, then Other ways, then the QR's Copy code, each with a visible focus.
    await page.locator('[data-f="signin:google"]').focus();
    const walked: string[] = [];
    for (let i = 0; i < 5; i++) {
      await page.keyboard.press("Tab");
      walked.push(
        await page.evaluate(() => {
          const el = document.activeElement as HTMLElement;
          return `${el.dataset.f}:${getComputedStyle(el).outlineStyle !== "none"}`;
        }),
      );
    }
    expect(walked).toEqual(["signin:facebook:true", "signin:mock:true", "signin:email:true", "signin:other:true", "pair:copy:true"]);

    // A window closed without a return: the line and its buttons in the sheet, the QR stays.
    const [closed] = await Promise.all([page.waitForEvent("popup"), page.locator('[data-f="signin:mock"]').click()]);
    await closed.getByText("Sign in as alice").waitFor();
    await closed.close();
    await page.evaluate(() => dispatchEvent(new Event("focus")));
    await expect(sheet.locator(".sign-in-failure .st")).toHaveText("Didn't finish signing in?");
    await expect(corner.locator(".qrbox svg")).toBeVisible();

    // Escape is Not now: Account's sync page.
    await page.keyboard.press("Escape");
    await expect(page).toHaveURL(/\/sync$/);
    await page.context().close();
  });

  test("desktop: no sync service in reach says so in the step with Try again and Not now", async ({ browser }) => {
    const page = await device(browser, "desktop");
    failNext = "relayInfo";
    await go(page, "/sign-in");
    await expect(page.locator(".sheet .sign-in-failure .st")).toHaveText("Momotus can't reach its sync service right now.");
    await expect(page.locator(".sheet .sign-in-failure .btn")).toHaveText(["Try again", "Not now"]);
    await page.locator('[data-f="signin:relay:retry"]').click();
    await expect(page.locator('[data-f="signin:google"]')).toBeVisible();
    await page.context().close();
  });

  test("desktop: a failed create after a provider leaves a sheet with Create and the QR's way in, Escape back to Login", async ({ browser }) => {
    const page = await device(browser, "desktop");
    await go(page, "/sign-in");
    // The account is created at once after a provider sign-in; only a failed create shows this stage.
    failNext = "createSyncAccount";
    const [popup] = await Promise.all([page.waitForEvent("popup"), page.locator('[data-f="signin:mock"]').click()]);
    await popup.getByText("Sign in as alice").click();
    await expect(page).toHaveURL(/\/sign-in\/new$/);
    const stage = page.locator(".sheet");
    await expect(stage.locator("h1")).toHaveText("Create your Momotus account");
    await expect(stage.locator(".sub")).toHaveText("This sign-in has no Momotus account yet. This device's library will be added to your account.");
    await expect(page.locator(".sheet > .acts .btn")).toHaveText(["Create your Momotus account", "I already use Momotus on another device"]);
    await expect(page.locator(".sign-in-failure")).toBeVisible();
    await expect(page.locator("#sign-in-qr .qrbox svg")).toBeVisible({ timeout: 30_000 });
    await page.screenshot({ path: `${SHOTS}/sign-in-sheet-new-desktop.png` });
    await page.locator('[data-f="signin:already"]').click();
    await expect(page.locator("#sign-in-qr")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(page).toHaveURL(/\/sign-in$/);
    await page.context().close();
  });

  test("phone: the bottom sheet, the brands' buttons, Use another device in place of the QR, 48 px targets", async ({ browser }) => {
    const page = await device(browser, "phone");
    await go(page, "/sign-in");
    await expect(page.locator(".sheet h1")).toHaveText("Sign in");
    await expect(page.locator('[data-f="signin:google"]')).toHaveAttribute("data-brand", "google");
    await expect(page.locator('[data-f="signin:facebook"]')).toHaveAttribute("data-brand", "facebook");
    await expect(page.locator("#sign-in-qr")).toHaveCount(0);
    await expect(page.locator(".sheet-grab")).toHaveCount(1);
    const device_ = page.locator('[data-f="signin:device"]');
    await expect(device_).toBeVisible();
    await device_.click();
    await expect(page.locator(".sheet .qrbox svg")).toBeVisible({ timeout: 30_000 });
    // Every control of the sheet is a 48 px target (QA r3, Q2: the provider buttons and Not now were 46).
    const heights = await page.locator(".sheet :is(.btn, .flink)").evaluateAll((els) => els.map((e) => e.getBoundingClientRect().height));
    expect(heights.length).toBeGreaterThan(4);
    expect(heights.filter((h) => h < 48)).toEqual([]);
    await page.context().close();
  });

  // Last: it makes an account on this spec's core. The vectors' newProvider path on the web, from Login on (the core
  // here is past Welcome): a provider makes the account at once, the recovery kit sheet, then the profile form.
  test("first run, a new Google account: Login, the recovery kit, Make your first profile, setup", async ({ browser }) => {
    const vectors = JSON.parse(readFileSync(new URL("../../shared/fixtures/first-run-vectors.json", import.meta.url), "utf8"));
    const want = vectors.paths.find((p: { path: string }) => p.path === "newProvider").web as { screens: string[]; presses: string[] };
    freshAddons = true;
    const page = await device(browser, "desktop");
    await go(page, "/welcome?step=login");
    await expect(page.locator(".sheet.login h1")).toHaveText("Sign in");
    const seen = ["login"];
    const [popup] = await Promise.all([page.waitForEvent("popup"), page.locator('[data-f="signin:google"]').click()]);
    await popup.getByText("Sign in as alice").click();
    await expect(page).toHaveURL(/\/sync\/kit\?first=1&then=profile$/, { timeout: 30_000 });
    await expect(page.locator(".sheet.kit-sheet h1")).toBeVisible();
    seen.push("recoveryKit");
    await page.locator('[data-f="sync-kit:cancel"]').click();
    await expect(page).toHaveURL(/\/welcome\?step=profile$/);
    await expect(page.locator("#profile-q")).toHaveText("Make your first profile");
    await expect(page.locator(".sheet .sub")).toHaveText("Your account has no profiles yet. A profile keeps its own library, subtitles and Continue Watching.");
    seen.push("profile");
    // An account was made: Back and Escape never return to Login (table 5, "Account made: leaves the app").
    await page.goBack();
    await page.waitForTimeout(500);
    await expect(page.locator(".sheet.login")).toHaveCount(0);
    await expect(page.locator("#profile-q")).toBeVisible();
    await expect(page).toHaveURL(/step=profile/);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);
    await expect(page.locator(".sheet.login")).toHaveCount(0);
    await expect(page.locator("#profile-q")).toBeVisible();
    await page.locator('[data-f="profile:continue"]').click();
    await expect(page.locator("#setup-q")).toBeVisible();
    seen.push("setup");
    expect(seen).toEqual(want.screens.slice(1));
    // Get started, Continue with Google, the kit's Later, Continue; Start watching comes next.
    expect(want.presses.slice(1, 4)).toEqual(["continueWithGoogle", "kitDone", "continue"]);
    await page.context().close();
  });
});
