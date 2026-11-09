import { expect, test } from "bun:test";
import vectors from "../../../shared/fixtures/first-run-vectors.json";
import { type After, type Entry, MORE, NONE, nextStep, setupChips, setupPatch, type Step } from "./first-run";

test("part entry: every row's next step", () => {
  for (const row of vectors.entry) {
    const { step, ...e } = row;
    expect(nextStep(e as Entry), JSON.stringify(row)).toBe(step as Step);
  }
});

/** A walk through the rule, press by press: the screens it shows before Home. Presses that move first run on are
 * fed to `nextStep`; the others stay on their screen (a choice on Setup, Use another device's code in the sheet). */
function walk(presses: string[], o: { server: boolean; profiles: number; web: boolean }): string[] {
  const e: Entry = { fresh: true, kidsSession: false, server: o.server, inAccount: false, profileStepDone: false, profiles: 1, pin: false, locked: false, after: "start" };
  let screen: string = nextStep(e);
  const shown = [screen];
  let bring = false;
  const go = (after: After, change: Partial<Entry> = {}) => {
    Object.assign(e, change, { after });
    screen = nextStep(e);
    if (screen !== "home") shown.push(screen);
  };
  const joined = () => go("joined", { fresh: false, inAccount: true, profiles: o.profiles });
  for (const p of presses) {
    if (p === "getStarted" || p === "alreadyUse" || p === "notNow") go(p);
    else if (p === "continueWithGoogle") {
      // An account is made at once; the web shows its recovery kit first.
      if (o.web) shown.push((screen = "recoveryKit"));
      else go("created", { inAccount: true });
    } else if (p === "kitDone") go("created", { inAccount: true });
    else if (p === "continue") go("profileDone", { profileStepDone: true });
    else if (p === "bringLibrary") bring = true;
    else if (p === "startWatching") {
      if (bring) shown.push((screen = "import"));
      else screen = "home";
    } else if (p === "profileTile") {
      if (screen === "login") joined();
      screen = "home";
    }
    else if (p !== "useAnotherDevice") throw new Error(`unknown press ${p}`);
  }
  // A join comes from the other device: no press here.
  if (screen === "login") joined();
  return shown;
}

test("part paths: each path's screens follow from its presses, on every form", () => {
  for (const path of vectors.paths) {
    const forms = Object.entries(path).filter(([k]) => ["phone", "wide", "tv", "web"].includes(k)) as [string, { screens: string[]; presses: string[] }][];
    expect(forms.length).toBeGreaterThan(0);
    for (const [form, want] of forms) {
      const o = { server: path.path !== "noServer", profiles: path.path === "joinManyProfilesOrPin" ? 3 : 1, web: form === "web" };
      expect(walk(want.presses, o), `${path.path} ${form}`).toEqual(want.screens);
    }
  }
});

test("part paths: Not now and a new account are 4 screens and 4 presses, no server 3 (acceptance bar)", () => {
  const p = (name: string) => vectors.paths.find((x) => x.path === name) as unknown as Record<string, { screens: string[]; presses: string[] }>;
  for (const f of ["phone", "wide", "tv"]) expect([p("newNotNow")[f]?.screens.length, p("newNotNow")[f]?.presses.length]).toEqual([4, 4]);
  expect(p("newProvider").web?.presses.length).toBe(5);
  for (const f of ["phone", "wide", "tv"]) expect(p("noServer")[f]?.presses.length).toBe(3);
});

test("part chips: order and the chosen chip, device language first (D11)", () => {
  // The vectors' unknown device: null. Any code the test names counts as having a subtitle name.
  const named = (c: string) => /^[a-z]{2,3}$/.test(c);
  for (const row of vectors.chips) {
    expect(setupChips(row.device, row.chosen, named), JSON.stringify(row)).toEqual({ chips: row.chips, selected: row.selected });
  }
  // A device language with no subtitle name counts as absent.
  expect(setupChips("xx", [], (c) => c !== "xx")).toEqual({ chips: ["en", NONE, MORE], selected: "en" });
});

test("Start watching writes the shown chip always, streams only for Best quality", () => {
  expect(setupPatch("en", null)).toEqual({ subtitleLanguages: ["en"] });
  expect(setupPatch(NONE, null)).toEqual({ subtitleLanguages: [] });
  expect(setupPatch("tr", { minSeeders: 20 })).toEqual({ subtitleLanguages: ["tr"], streams: { minSeeders: 20 } });
});
