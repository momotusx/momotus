/** First run's order (plans/first-screens.md 2, 4.3): which step shows next, and the setup sheet's subtitle chips.
 * Pure, so lib/first-run.test.ts holds them equal to apps/shared/fixtures/first-run-vectors.json, the file the
 * Android and Apple apps test against too. Every word is apps/shared/first-run-words.json's. */

import shared from "../../../shared/first-run-words.json";

export const firstRunWords = shared;

/** What just happened: a cold start, a press on Welcome or Login, an account made or joined, the profile step done. */
export type After = "start" | "getStarted" | "alreadyUse" | "notNow" | "created" | "joined" | "profileDone";
/** welcome, login, profile, who (the start picker over the wall), setup, home (first run ends), none (no first run). */
export type Step = "welcome" | "login" | "profile" | "who" | "setup" | "home" | "none";

export interface Entry {
  /** The core's `addons.fresh`. */
  fresh: boolean;
  kidsSession: boolean;
  /** A sync server is known, so Login can show. */
  server: boolean;
  /** This device is in an account (made here or joined). */
  inAccount: boolean;
  profileStepDone: boolean;
  /** How many profiles the account has. */
  profiles: number;
  /** The joined account's only profile has a PIN. */
  pin: boolean;
  locked: boolean;
  after: After;
}

/** The next step. A kids session never sees first run; after a join, the picker only when there is a choice or a PIN
 * (D2); a locked session meets the picker first. */
export function nextStep(e: Entry): Step {
  if (e.kidsSession) return "none";
  if (e.after === "joined") return e.profiles > 1 || e.pin ? "who" : "home";
  if (!e.fresh) return "none";
  switch (e.after) {
    case "getStarted":
      return e.server ? "login" : "profile";
    case "alreadyUse":
      return "login";
    case "notNow":
    case "created":
      return "profile";
    case "profileDone":
      return "setup";
    default:
      if (e.locked || e.profiles > 1) return "who";
      if (e.profileStepDone) return "setup";
      return e.inAccount ? "profile" : "welcome";
  }
}

/** One chip of "Subtitles start in": a language code, No subtitles, or More (the language picker, always last). */
export type Chip = string;
export const NONE = "none";
export const MORE = "more";

/** The chips in order and the one chosen as the sheet opens (D11): the profile's own first language, else the
 * device's, then the device language and English when not shown yet, then No subtitles, then More. A device language
 * this app has no subtitle name for counts as absent, and English is chosen. */
export function setupChips(device: string | null, chosen: readonly string[], named: (code: string) => boolean): { chips: Chip[]; selected: Chip } {
  const own = device && named(device) ? device : null;
  const langs: string[] = [];
  for (const c of [chosen[0], own, "en"]) if (c && !langs.includes(c)) langs.push(c);
  return { chips: [...langs, NONE, MORE], selected: langs[0] as string };
}

/** Device locales the core lists under another tag (crates/mq-runtime/src/languages.rs `ALIASES`, its device
 * locales): Norway's Bokmål and Nynorsk are Norwegian, the Philippines' Filipino is Tagalog, and the regional
 * Portuguese, Chinese and Spanish have rows of their own. */
const DEVICE_ALIASES: Record<string, string> = {
  nb: "no",
  nn: "no",
  fil: "tl",
  "pt-pt": "pt",
  "pt-br": "pt-BR",
  "zh-cn": "zh-Hans",
  "zh-sg": "zh-Hans",
  "zh-tw": "zh-Hant",
  "zh-hk": "zh-Hant",
  "es-mx": "es-419",
  "es-la": "es-419",
};

/** The device's language as the core lists it: the whole locale first ("pt-BR"), then its language ("nb-NO" is
 * Norwegian, "no"); a language with no alias as it is. */
export function deviceSubtitleLanguage(locale: string): string {
  const tag = locale.toLowerCase();
  const lang = tag.split("-")[0] ?? tag;
  return DEVICE_ALIASES[tag] ?? DEVICE_ALIASES[lang] ?? lang;
}

/** What Start watching writes: the shown chip as the profile's subtitle languages (`[]` for No subtitles, always
 * written) and the stream patch only for Best quality. */
export function setupPatch(chip: Chip, best: Record<string, unknown> | null): Record<string, unknown> {
  return { subtitleLanguages: chip === NONE ? [] : [chip], ...(best ? { streams: best } : {}) };
}
