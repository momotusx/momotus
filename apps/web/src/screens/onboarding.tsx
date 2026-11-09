/** First run, the Poster Wall (plans/first-screens.md 2 and 3, round2.html combo A): Welcome over a drifting wall of
 * real posters, then sheets over the same wall held still: Login (`step=login`), Make your first profile
 * (`step=profile`), the one-page setup (`step=setup`), then Home. The order is lib/first-run.ts's, held equal to the
 * Android and Apple apps by apps/shared/fixtures/first-run-vectors.json. The step lives in the URL, so Back walks back
 * through the sheets. Outside first run: the recommended addons' Review (`step=2&review=1`), the setup check
 * (`step=3`) and the Settings page of the same switches. */

import { A, useNavigate, useSearchParams } from "@solidjs/router";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Gem from "lucide-solid/icons/gem";
import Layers from "lucide-solid/icons/layers";
import Link2 from "lucide-solid/icons/link-2";
import PackageSearch from "lucide-solid/icons/package-search";
import SearchIcon from "lucide-solid/icons/search";
import Zap from "lucide-solid/icons/zap";
import { createEffect, createMemo, createSignal, For, Index, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { displayCaps, link, liveView, op, ping, prefsView, reasonOf } from "../core/client";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import type { DefaultAddonInfo } from "../core/generated/DefaultAddonInfo";
import type { DefaultAddons } from "../core/generated/DefaultAddons";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
import { gateOpen, profiles } from "../core/profiles";
import { signInServer } from "../core/sign-in";
import * as sync from "../core/sync";
import { AvatarArt } from "../components/avatar";
import Brand from "../components/brand";
import { askWall, intoHome, MotionToggle, Sheet, SheetHead, WallPage } from "../components/wall";
import { autoFocusSoon, goBack } from "../focus/dom";
import { screenChanged } from "../focus/ghost-tap";
import { form } from "../form";
import { resetHistoryBase } from "../history";
import { type AvatarColor, formatAvatar, formColors, parseAvatar } from "../lib/avatar";
import { catalogMissing, choicesFor, defaultsWords, grouped, switchable } from "../lib/defaults";
import { deviceSubtitleLanguage, firstRunWords as fw, MORE, nextStep, NONE, setupChips, setupPatch, type Step } from "../lib/first-run";
import { kidsSession, ownerOf } from "../lib/kids";
import { profileProblem } from "../lib/profiles";
import { choiceWords, findAddons, languageName, type Tone } from "../lib/words";
import { Flag } from "../player/flags";
import { InstallField } from "./addons";
import { LanguageOverlay, LanguageSearch, NoneFlag } from "./language-picker";
import { SignInStart, TvSignIn } from "./sign-in";
import "./browse.css";
import "./settings.css";
import "./onboarding.css";

interface Row {
  label: string;
  tone: Tone;
  text: string;
  fix?: { label: string; run: () => void };
}

/** The core's defaults, read again whenever the addon list changes. */
function useDefaults() {
  const addons = liveView<AddonsSnapshot>("addons");
  // Each answer is reconciled by key, so an addon keeps its object across
  // reloads and its row (and the focus on it) stays when a switch changes.
  const [store, setStore] = createStore<{ defaults: DefaultAddons | null }>({ defaults: null });
  const defaults = () => store.defaults;
  const [error, setError] = createSignal<string | null>(null);
  const load = () =>
    op<DefaultAddons>("defaultAddons").then(
      (d) => {
        setStore("defaults", reconcile(d, { key: "key" }));
        setError(null);
      },
      (e) => setError(reasonOf(e)),
    );
  // The store keeps its objects across pushes, so the list is watched by value.
  createEffect(on(() => addons.state.data?.addons.map((a) => `${a.url} ${a.enabled}`).join("|"), () => void load()));
  return { addons, defaults, error, reload: load };
}

/** Leaving first run: the step never stays under the next one, and Home opens as the root, so Back never walks back
 * into the sheets. */
function leave(navigate: (to: string, o?: { replace?: boolean }) => void, to: string) {
  if (to === "/") intoHome(navigate);
  else navigate(to, { replace: true });
}

/** Every recommended default the profile does not have, on: what Start watching, Skip and an import's Done send.
 * `installDefaults` ends `fresh`, so Home stops sending the profile here. */
export async function installAllDefaults() {
  const d = await op<DefaultAddons>("defaultAddons");
  await op("installDefaults", { choices: choicesFor(d.addons, new Set()) });
}

type FirstStep = "welcome" | "login" | "profile" | "setup";
const firstSteps: readonly string[] = ["login", "profile", "setup"];

export default function Onboarding() {
  const [query] = useSearchParams();
  const step = () => (query.step === "2" ? 2 : query.step === "3" ? 3 : firstSteps.includes(query.step as string) ? (query.step as FirstStep) : "welcome");
  // A new step puts focus on its first action (on a remote or after keys). Off the TV a step change puts it on
  // the step's heading, so a screen reader starts at the question.
  createEffect(on(step, () => autoFocusSoon(() => document.querySelector<HTMLElement>("main [data-autofocus]"))));
  // A new step under the finger: a double tap answers one question, never the next (QA r1, Q1 row 30).
  createEffect(on(step, () => screenChanged(), { defer: true }));
  createEffect(on(step, () => form() !== "tv" && requestAnimationFrame(focusHeading), { defer: true }));
  return (
    <Switch>
      <Match when={step() === 2 || step() === 3}>
        <Settled step={step() as 2 | 3} review={query.review === "1"} first={query.first === "1"} />
      </Match>
      <Match when={true}>
        <FirstRun step={step() as FirstStep} returning={query.returning === "1"} />
      </Match>
    </Switch>
  );
}

/** The pages outside first run: Review and the setup check. */
function Settled(props: { step: 2 | 3; review: boolean; first: boolean }) {
  const { addons, defaults, error } = useDefaults();
  const [note, setNote] = createSignal<{ tone: Tone; text: string } | null>(null);
  // A setup check fix: the one default it names, on.
  const fix = async (key: string, name: string) => {
    setNote(null);
    try {
      await op("installDefaults", { choices: [{ key, on: true }] });
      setNote({ tone: "ok", text: `Added ${name}` });
    } catch (e) {
      setNote({ tone: "bad", text: `Could not add ${name}: ${reasonOf(e)}` });
    }
  };
  const hasCatalog = () => (addons.state.data?.addons ?? []).some((a) => a.enabled && a.provides.includes("catalog"));
  return (
    <main class="welcome page" data-vscroll>
      <header class="welcome-head">
        <div class="brand">
          <Brand />
        </div>
      </header>
      <Show
        when={props.step === 2}
        fallback={<SetupCheck list={addons.state.data?.addons ?? []} defaults={defaults()?.addons ?? []} note={note()} onFix={fix} />}
      >
        <Recommended defaults={defaults()} error={error()} hasCatalog={hasCatalog()} review={props.review} first={props.first} />
      </Show>
    </main>
  );
}

/** What the rule needs from the core and this browser: the profile's state, the account, the sync server. */
function useEntry() {
  const addons = liveView<AddonsSnapshot>("addons");
  return (after: Parameters<typeof nextStep>[0]["after"], s: ProfilesSnapshot | null | undefined = profiles()): Step | null => {
    const fresh = addons.state.data?.fresh;
    if (!s || fresh === undefined) return null;
    const only = s.profiles.length === 1 ? s.profiles[0] : undefined;
    // Once the gate is open its picker has been answered: a start is past `who` (else a bare wall, nothing to press).
    const answered = after === "start" && gateOpen();
    return nextStep({
      fresh,
      kidsSession: kidsSession(s),
      server: !!signInServer(),
      inAccount: sync.joined(),
      // An older core never sends it: read as answered, so it never shows the form.
      profileStepDone: s.profileStepDone !== false,
      profiles: answered ? 1 : s.profiles.length,
      pin: only?.hasPin === true,
      locked: answered ? false : s.locked,
      after,
    });
  };
}

/** First run's steps over one wall, kept mounted from step to step so the drift's phase and the posters stay. */
function FirstRun(props: { step: FirstStep; returning: boolean }) {
  const navigate = useNavigate();
  const entry = useEntry();
  // Where a start lands: Welcome, or the profile form or setup when an earlier run got that far; no first run when the
  // profile is not fresh.
  const start = createMemo(() => (props.step === "welcome" ? entry("start") : null));
  createEffect(() => {
    const s = start();
    if (s === "profile" || s === "setup") navigate(`/welcome?step=${s}`, { replace: true });
    else if (s === "none" || s === "home") leave(navigate, "/");
  });
  const to = (step: Step | null, o?: { replace?: boolean; returning?: boolean }) => {
    if (step === "login" || step === "profile" || step === "setup") navigate(`/welcome?step=${step}${o?.returning ? "&returning=1" : ""}`, { replace: o?.replace });
  };
  // Past first run (the TV's Settings > Sign in opens this sheet too), Not now goes back where it came from.
  const notNow = () => {
    const next = entry("notNow") ?? "profile";
    if (next === "profile") to(next);
    else goBack();
  };
  // An account made here (table 5): Login is behind it, so a Back that lands there goes on to the profile form.
  createEffect(() => props.step === "login" && sync.joined() && entry("start") === "profile" && to("profile", { replace: true }));
  // An empty or missed wall answer is asked again at the next step (the wall stays mounted from step to step).
  createEffect(on(() => props.step, () => askWall(), { defer: true }));
  return (
    // No wall (and no wall request) before a start is known to be first run's.
    <Show when={props.step !== "welcome" || start() === "welcome"}>
    <WallPage moving={props.step === "welcome"}>
      <Switch>
        <Match when={props.step === "welcome"}>
          <Show when={start() === "welcome"}>
            <Welcome onStart={() => to(entry("getStarted"))} onReturning={() => to(entry("alreadyUse"), { returning: true })} />
          </Show>
        </Match>
        <Match when={props.step === "login"}>
          <Show when={form() === "tv"} fallback={<SignInStart from="welcome" returning={props.returning} />}>
            <TvSignIn onNotNow={notNow} />
          </Show>
        </Match>
        <Match when={props.step === "profile"}>
          {/* A profile past first run (its addons came meanwhile) has no setup to show: Home. */}
          <ProfileStep onDone={() => ((entry("profileDone") ?? "setup") === "setup" ? to("setup") : leave(navigate, "/"))} />
        </Match>
        <Match when={props.step === "setup"}>
          <SetupStep />
        </Match>
      </Switch>
    </WallPage>
    </Show>
  );
}

/** Welcome: the lockup, the line, Get started and (with a sync server) I already use Momotus; Pause motion at the top
 * right. The only step that moves. */
function Welcome(props: { onStart: () => void; onReturning: () => void }) {
  return (
    <>
      <section class="welcome-block" aria-labelledby="welcome-q">
        <div class="brand">
          <Brand />
        </div>
        <h1 id="welcome-q" tabindex="-1">
          {fw.welcomeTitle}
        </h1>
        <p>{fw.welcomeLead}</p>
        <div class="acts" data-fgroup="welcome">
          <button class="btn primary" data-f="welcome:start" data-autofocus onClick={() => props.onStart()}>
            {fw.getStarted}
          </button>
          <Show when={signInServer()}>
            <button class="btn" data-f="welcome:returning" onClick={() => props.onReturning()}>
              {fw.alreadyUse}
            </button>
          </Show>
        </div>
      </section>
      {/* After the buttons by Tab (3.1); drawn at the top right. */}
      <MotionToggle />
    </>
  );
}

/** Focus on the step's heading, whichever step it is (a sign-in step's too). */
function focusHeading() {
  const h = document.querySelector<HTMLElement>("main h1");
  if (h && !h.hasAttribute("tabindex")) h.tabIndex = -1;
  h?.focus({ preventScroll: true });
}

const colourName = (c: string) => c.charAt(0).toUpperCase() + c.slice(1);

/** Make your first profile: names the owner profile every device already has (never a new one). Nothing is filled
 * in: an empty Name keeps the current one, and only a colour pressed here is sent (an untouched Continue sends
 * `{name: "", avatar: ""}`, which writes nothing). */
function ProfileStep(props: { onDone: () => void }) {
  const owner = () => ownerOf(profiles());
  const stored = () => parseAvatar(owner()?.avatar ?? "");
  const [name, setName] = createSignal("");
  const [pressed, setPressed] = createSignal<AvatarColor | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [nameError, setNameError] = createSignal<string | null>(null);
  const [problem, setProblem] = createSignal<string | null>(null);
  const shown = () => pressed() ?? stored().color ?? "ember";
  // Made an account here: this form is where the app starts now, and Back leaves it (table 5).
  onMount(() => sync.joined() && resetHistoryBase());
  const go = async () => {
    if (busy()) return;
    setBusy(true);
    setNameError(null);
    setProblem(null);
    try {
      const c = pressed();
      await op<ProfilesSnapshot>("finishProfileStep", { name: name().trim(), avatar: c ? formatAvatar({ glyph: stored().glyph, color: c }) : "" });
      // Busy until the next step shows: a second press sends nothing.
      props.onDone();
    } catch (e) {
      const code = e && typeof e === "object" && "code" in e ? String((e as { code?: unknown }).code) : "";
      if (code === "badName") setNameError(profileProblem("badName", ""));
      else setProblem(fw.setupPrefsFailed);
      setBusy(false);
    }
  };
  return (
    <Sheet size="narrow" labelledby="profile-q">
      <SheetHead id="profile-q" title={fw.profileTitle} />
      <p class="sub">{sync.joined() ? fw.profileLineAccount : fw.profileLineLocal}</p>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void go();
        }}
      >
        <div class="idr">
          <AvatarArt avatar={formatAvatar({ glyph: stored().glyph, color: shown() })} name={name().trim() || owner()?.name || fw.namePlaceholder} size="l" />
          {/* Never focused on open: no keyboard over Continue on a phone (MOB-24). */}
          <label class="fr-field">
            <small>{fw.nameLabel}</small>
            <input
              type="text"
              name="profile-name"
              maxLength={30}
              placeholder={fw.namePlaceholder}
              value={name()}
              autocomplete="off"
              enterkeyhint="done"
              data-f="profile:name"
              aria-invalid={nameError() ? "true" : undefined}
              aria-describedby={nameError() ? "profile-name-error" : undefined}
              onInput={(e) => {
                setName(e.currentTarget.value);
                setNameError(null);
              }}
              onFocus={(e) => {
                // Continue stays above the keyboard (MOB-24), once the keyboard is up.
                const f = e.currentTarget.form;
                setTimeout(() => f?.querySelector(".fr-foot")?.scrollIntoView({ block: "nearest" }), 300);
              }}
            />
          </label>
        </div>
        <Show when={nameError()}>
          <p class="st bad" id="profile-name-error" role="status">
            <i class="dot" />
            {nameError()}
          </p>
        </Show>
        <div class="swx" role="radiogroup" aria-label={fw.colourLabel} data-fgroup="profile:colours">
          <For each={formColors(stored().color ?? "ember")}>
            {(c) => (
              <button
                type="button"
                role="radio"
                aria-checked={shown() === c}
                aria-label={colourName(c)}
                data-f={`profile:colour:${c}`}
                style={{ background: `var(--avatar-${c})` }}
                onClick={() => setPressed(c)}
              />
            )}
          </For>
        </div>
        <Show when={problem()}>
          <p class="st bad" role="alert">
            <i class="dot" />
            {problem()}
          </p>
        </Show>
        <div class="fr-foot">
          <button class="btn primary" type="submit" data-f="profile:continue" data-autofocus={form() === "tv" ? "" : undefined} aria-busy={busy() || undefined}>
            {fw.continue}
          </button>
        </div>
      </form>
    </Sheet>
  );
}

/** A language's own name ("Türkçe"), else its English one. */
function ownName(code: string) {
  try {
    const own = new Intl.DisplayNames([code], { type: "language" }).of(code);
    if (own && own.toLowerCase() !== code) return own.charAt(0).toLocaleUpperCase(code) + own.slice(1);
  } catch {
    // Not a code this browser knows.
  }
  return languageName(code);
}
/** A device language this app can name counts; one it cannot counts as absent. */
const named = (code: string) => languageName(code) !== code.toUpperCase();

// The setup sheet's answers, kept while first run lasts: Back from Import or Review finds them as they were.
const [setup, setSetup] = createStore<{ profile: string | null; bring: boolean; best: boolean; chip: string | null; picked: string | null }>({
  profile: null,
  bring: false,
  best: false,
  chip: null,
  picked: null,
});
/** The profile and patch already written, so Try again or a second press never writes the same answers twice. */
let written = "";

/** Set up {profile}: one page, every answer pre-chosen, so Start watching alone finishes first run. */
function SetupStep() {
  const navigate = useNavigate();
  // The answers are this profile's: another profile's first run starts from the defaults.
  const active = profiles()?.active ?? null;
  if (setup.profile !== active) setSetup({ profile: active, bring: false, best: false, chip: null, picked: null });
  const prefs = prefsView();
  const q = choiceWords.question;
  const [busy, setBusy] = createSignal(false);
  const [failed, setFailed] = createSignal<"prefs" | "sources" | null>(null);
  const [open, setOpen] = createSignal(false);
  const name = () => profiles()?.profiles.find((p) => p.id === profiles()?.active)?.name ?? "";
  const chips = createMemo(() => {
    const p = prefs.state.data;
    if (!p) return null;
    const base = setupChips(deviceSubtitleLanguage(navigator.language || "en"), p.subtitleLanguages, named);
    // A language picked from More takes its place before No subtitles.
    const extra = setup.picked && !base.chips.includes(setup.picked) ? [setup.picked] : [];
    return { chips: [...base.chips.filter((c) => c !== NONE && c !== MORE), ...extra, NONE, MORE], selected: base.selected };
  });
  const chip = () => setup.chip ?? chips()?.selected ?? null;
  const finish = () => leave(navigate, "/");
  /** The sources, then Home; false when they failed. */
  const sources = async () => {
    try {
      await installAllDefaults();
      finish();
      return true;
    } catch {
      setFailed("sources");
      return false;
    }
  };
  const go = async () => {
    const c = chip();
    if (busy() || !c) return;
    setBusy(true);
    setFailed(null);
    try {
      const patch = setupPatch(c, setup.best ? q.best.patch : null);
      const key = `${active}|${JSON.stringify(patch)}`;
      if (written !== key) {
        try {
          await op("setPrefs", { patch });
          written = key;
        } catch {
          setFailed("prefs");
          setBusy(false);
          return;
        }
      }
      // On success the sheet stays busy while it leaves: a key press during the fade does nothing.
      if (setup.bring) navigate("/import?from=welcome");
      else if (!(await sources())) setBusy(false);
    } catch {
      setBusy(false);
    }
  };
  // Skip writes no preference: the recommended sources go on, so first run ends.
  const skip = async () => {
    if (busy()) return;
    setBusy(true);
    setFailed(null);
    if (!(await sources())) setBusy(false);
  };
  const radio = (on: boolean) => ({ role: "radio" as const, "aria-checked": on });
  return (
    <Sheet size="setup" labelledby="setup-q" busy={busy()}>
      <SheetHead id="setup-q" title={fw.setupTitle.replace("{profile}", name())}>
        <button class="flink" data-f="setup:skip" aria-disabled={busy() || undefined} onClick={() => void skip()}>
          {fw.skip}
        </button>
      </SheetHead>
      <Show when={form() !== "phone"}>
        <p class="sub">{fw.setupLine}</p>
      </Show>
      <div class="g3">
        <section aria-labelledby="setup-bring">
          <h2 class="sect" id="setup-bring">
            {fw.bringTitle}
          </h2>
          <div role="radiogroup" aria-labelledby="setup-bring" data-fgroup="setup:bring">
            <For each={[true, false]}>
              {(bring) => (
                <button class="fr-opt" {...radio(setup.bring === bring)} data-f={`setup:${bring ? "bring" : "fresh"}`} onClick={() => setSetup("bring", bring)}>
                  <span class="tx">
                    <b>{bring ? fw.bringLibrary : fw.startFresh}</b>
                    <Show when={form() !== "phone"}>
                      <small>{bring ? fw.bringLibraryLine : fw.startFreshLine}</small>
                    </Show>
                  </span>
                  <i class="rad" aria-hidden="true" />
                </button>
              )}
            </For>
          </div>
        </section>
        <section aria-labelledby="setup-pick">
          <h2 class="sect" id="setup-pick">
            {q.title}
          </h2>
          <div class="seg" role="radiogroup" aria-labelledby="setup-pick" data-fgroup="setup:pick">
            <button {...radio(!setup.best)} data-f="setup:fast" onClick={() => setSetup("best", false)}>
              <Zap aria-hidden />
              {q.fast.label}
            </button>
            <button {...radio(setup.best)} data-f="setup:best" onClick={() => setSetup("best", true)}>
              <Gem aria-hidden />
              {q.best.label}
            </button>
          </div>
          <p class="hint">{setup.best ? q.best.text : q.fast.text}</p>
        </section>
        <section aria-labelledby="setup-subs">
          <h2 class="sect" id="setup-subs">
            {fw.subtitlesTitle}
          </h2>
          <div class="fr-chips" role="radiogroup" aria-labelledby="setup-subs" data-fgroup="setup:subs">
            <For each={chips()?.chips ?? []}>
              {(c) => (
                <Show
                  when={c !== MORE}
                  fallback={
                    <button class="chip" data-f="setup:sub:more" aria-haspopup="dialog" onClick={() => setOpen(true)}>
                      <SearchIcon aria-hidden />
                      {fw.moreLanguages}
                    </button>
                  }
                >
                  <button class="chip" {...radio(chip() === c)} data-f={`setup:sub:${c}`} onClick={() => setSetup("chip", c)}>
                    <Show when={c !== NONE} fallback={<NoneFlag />}>
                      <Flag language={c} />
                    </Show>
                    <span lang={c === NONE ? undefined : c}>{c === NONE ? fw.noSubtitles : ownName(c)}</span>
                  </button>
                </Show>
              )}
            </For>
          </div>
        </section>
      </div>
      <Show when={failed()}>
        {(f) => (
          <div class="sign-in-failure">
            <p class="st bad" role="alert">
              <i class="dot" />
              {f() === "prefs" ? fw.setupPrefsFailed : fw.setupSourcesFailed}
            </p>
          </div>
        )}
      </Show>
      <div class="sfoot">
        <span class="srcl">
          <Layers aria-hidden />
          {fw.sourcesOn}
          <A class="flink" href="/welcome?step=2&review=1&first=1" data-f="setup:review">
            {fw.review}
          </A>
        </span>
        <button class="btn primary" data-f="setup:start" data-autofocus aria-busy={busy() || undefined} aria-disabled={!chip() || undefined} onClick={() => void go()}>
          {failed() ? fw.tryAgain : fw.startWatching}
        </button>
      </div>
      <Show when={open()}>
        <LanguageOverlay title={fw.subtitlesTitle} done="Cancel" onClose={() => setOpen(false)}>
          <LanguageSearch
            group="welcome:lang"
            chosen={[]}
            isOn={(code) => (chip() === NONE ? null : chip()) === code}
            onPick={(code) => {
              setSetup({ picked: code, chip: code ?? NONE });
              setOpen(false);
            }}
            focusField={form() !== "tv"}
            focusFirst={form() === "tv"}
          />
        </LanguageOverlay>
      </Show>
    </Sheet>
  );
}

/** A default's mark: its logo when the core sends one (`logo` is optional
 * until it does), else its initial on a neutral tile, also when the logo
 * does not load. */
function AddonMark(props: { addon: DefaultAddonInfo & { logo?: string | null } }) {
  const [broken, setBroken] = createSignal(false);
  return (
    <span class="dlogo" aria-hidden="true">
      <Show when={props.addon.logo && !broken()} fallback={<b>{props.addon.name.charAt(0)}</b>}>
        <img src={props.addon.logo as string} alt="" decoding="async" onError={() => setBroken(true)} />
      </Show>
    </span>
  );
}

/** One default: a switch when it can be chosen, its state in words when the
 * profile already has it. The whole row is the button, for a remote. */
export function DefaultRow(props: {
  addon: DefaultAddonInfo;
  on: boolean | null;
  status?: string;
  fkey: string;
  autofocus?: boolean;
  onToggle?: () => void;
}) {
  const text = () => (
    <>
      <AddonMark addon={props.addon} />
      <span class="dtext">
        <b>{props.addon.name}</b>
        <small>{props.addon.line}</small>
      </span>
    </>
  );
  return (
    <Show
      when={props.on !== null}
      fallback={
        <div class="drow fixed">
          {text()}
          <span class="dstate">{props.status}</span>
        </div>
      }
    >
      <button class="drow" role="switch" aria-checked={props.on ?? false} data-f={props.fkey} data-autofocus={props.autofocus ? "" : undefined} onClick={() => props.onToggle?.()}>
        {text()}
        <span class="switch" aria-hidden="true">
          <i />
        </span>
        {/* The role speaks the state; the word is for eyes (TV and desktop). */}
        <span class="dword" aria-hidden="true">{props.on ? "On" : "Off"}</span>
      </button>
    </Show>
  );
}

/** Review: the recommended addons the profile does not have yet, each on until turned off. Continue names exactly
 * the rows with a switch, then goes Home (Home's card) or back to the setup sheet (first run's Review). */
function Recommended(props: { defaults: DefaultAddons | null; error: string | null; hasCatalog: boolean; review: boolean; first: boolean }) {
  const navigate = useNavigate();
  const [off, setOff] = createSignal<ReadonlySet<string>>(new Set());
  const [busy, setBusy] = createSignal(false);
  const [problem, setProblem] = createSignal<string | null>(null);
  const [more, setMore] = createSignal(false);
  // One primary: Continue, unless a link waits in the field (then Install).
  const [draft, setDraft] = createSignal("");
  const all = () => props.defaults?.addons ?? [];
  const shown = () => (props.review ? switchable(all()) : all());
  const toggle = (key: string) => {
    const next = new Set(off());
    if (!next.delete(key)) next.add(key);
    setOff(next);
  };
  const present = (a: DefaultAddonInfo) => (a.installed === "on" ? "Installed" : "Installed, off");
  const go = async () => {
    if (busy() || !props.defaults) return;
    setBusy(true);
    setProblem(null);
    try {
      await op("installDefaults", { choices: choicesFor(all(), off()) });
      if (props.first) goBack();
      else leave(navigate, "/");
    } catch (e) {
      setProblem(`Could not add the addons: ${reasonOf(e)}`);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section class="step-body" aria-label={defaultsWords.title}>
      <h1 tabindex="-1">{defaultsWords.title}</h1>
      <p class="lead">{props.review ? defaultsWords.review : defaultsWords.lead}</p>
      <Show when={props.error}>
        {(e) => (
          <p class="st bad" role="status">
            <i class="dot" />
            {e()}
          </p>
        )}
      </Show>
      <div class="dgroups" data-fgroup="defaults">
        <Index each={grouped(shown())}>
          {(g) => (
            <section class="dgroup" aria-label={g().title}>
              <h2 class="kicker">{g().title}</h2>
              <For each={g().addons}>
                {(a) => (
                  <DefaultRow
                    addon={a}
                    on={a.installed === "absent" ? !off().has(a.key) : null}
                    status={present(a)}
                    fkey={`welcome:default:${a.key}`}
                    onToggle={() => toggle(a.key)}
                  />
                )}
              </For>
            </section>
          )}
        </Index>
      </div>
      <Show when={props.defaults && !props.hasCatalog && catalogMissing(all(), off())}>
        <p class="st warn" role="status">
          <i class="dot" />
          {defaultsWords.noCatalog}
        </p>
      </Show>
      <Show when={problem()}>
        {(p) => (
          <p class="st bad" role="status">
            <i class="dot" />
            {p()}
          </p>
        )}
      </Show>
      <div class="acts">
        <button
          class="btn"
          classList={{ primary: !draft().trim() }}
          data-f="welcome:next"
          data-autofocus
          aria-disabled={!props.defaults || busy()}
          onClick={() => void go()}
        >
          Continue
          <ChevronRight aria-hidden />
        </button>
        <Show when={!props.review && form() !== "tv"}>
          <button class="btn" data-f="welcome:more" aria-expanded={more()} onClick={() => setMore(!more())}>
            <Link2 aria-hidden />
            Add another addon
          </button>
        </Show>
      </div>
      <Show when={!props.review && form() === "tv"}>
        <p class="aside">{defaultsWords.tvMore}</p>
      </Show>
      <Show when={more()}>
        <InstallField fkey="welcome" onDraft={setDraft} />
        <A class="btn find-link" href="/find-addons" data-f="welcome:find">
          <PackageSearch aria-hidden />
          {findAddons.title}
        </A>
      </Show>
    </section>
  );
}

/** Runs on its own; each row that is not ready offers its fix. */
function SetupCheck(props: {
  list: AddonsSnapshot["addons"];
  defaults: DefaultAddonInfo[];
  note: { tone: Tone; text: string } | null;
  onFix: (key: string, name: string) => void;
}) {
  const [online, setOnline] = createSignal(navigator.onLine);
  const onNet = () => setOnline(navigator.onLine);
  window.addEventListener("online", onNet);
  window.addEventListener("offline", onNet);
  // Liveness from the core itself, every few seconds while this step is open.
  const [pingMs, setPingMs] = createSignal<number | null>(null);
  const check = () => ping().then(setPingMs, () => setPingMs(null));
  void check();
  const pinger = setInterval(check, 5000);
  onCleanup(() => {
    window.removeEventListener("online", onNet);
    window.removeEventListener("offline", onNet);
    clearInterval(pinger);
  });
  const names = (kind: string) => props.list.filter((a) => a.enabled && a.provides.includes(kind)).map((a) => a.name);
  // A missing kind is fixed by its recommended addon, named as the core names it.
  const fixWith = (key: string) => {
    const d = props.defaults.find((x) => x.key === key);
    return d ? { label: `Add ${d.name}`, run: () => props.onFix(d.key, d.name) } : undefined;
  };
  const rows = (): Row[] => {
    const caps = displayCaps();
    const catalogs = names("catalog");
    const sources = names("stream");
    const subs = names("subtitles");
    const out: Row[] = [];
    // The app's engine is only worth a row when it is not answering.
    if (pingMs() === null) out.push({ label: "App engine", tone: link() === "closed" ? "bad" : "warn", text: link() === "closed" ? "Not reachable" : "Connecting" });
    out.push(
      { label: "Network", tone: online() ? "ok" : "bad", text: online() ? "Online" : "Offline" },
      { label: "Display", tone: "ok", text: `${caps.displayHeight >= 2160 ? "4K" : `${caps.displayHeight}p`}${caps.hdr.length ? ", HDR" : ""}` },
      { label: "Catalogs", tone: catalogs.length ? "ok" : "idle", text: catalogs.length ? catalogs.join(", ") : "None yet", fix: catalogs.length ? undefined : fixWith("cinemeta") },
      {
        label: "Sources",
        tone: sources.length ? "ok" : "idle",
        text: sources.length ? sources.join(", ") : "None yet: titles cannot play",
        fix: sources.length ? undefined : fixWith("torrentio"),
      },
      { label: "Subtitles", tone: subs.length ? "ok" : "idle", text: subs.length ? subs.join(", ") : "None yet", fix: subs.length ? undefined : fixWith("opensubtitles") },
    );
    return out;
  };
  return (
    <section class="step-body check" aria-label="Setup check">
      <h1 tabindex="-1">Setup check</h1>
      <ul>
        <For each={rows()}>
          {(c) => (
            <li>
              <span class="cl">{c.label}</span>
              <span class={`st ${c.tone}`}>
                <i class="dot" />
                {c.text}
              </span>
              <Show when={c.fix}>
                {(f) => (
                  <button class="fix" data-f={`check:${c.label}`} onClick={f().run}>
                    {f().label}
                  </button>
                )}
              </Show>
            </li>
          )}
        </For>
      </ul>
      <Show when={props.note}>
        {(n) => (
          <p class={`st ${n().tone}`} role="status">
            <i class="dot" />
            {n().text}
          </p>
        )}
      </Show>
      <div class="acts">
        <A class="btn primary" href="/" replace onClick={() => resetHistoryBase()} data-f="welcome:home" data-autofocus>
          Go to Home
          <ChevronRight aria-hidden />
        </A>
      </div>
    </section>
  );
}

/** Settings: the same switches, applied at once. A switch turned on adds a
 * missing default; one the profile has turns on or off where it is. */
export function RecommendedAddons() {
  const { defaults, error, reload } = useDefaults();
  const [problem, setProblem] = createSignal<string | null>(null);
  // Read again once applied: a plugin repository changes no addon, so the addon list never says so (QA r2, Q6 row 18).
  const flip = (a: DefaultAddonInfo) => {
    setProblem(null);
    op("installDefaults", { choices: [{ key: a.key, on: a.installed !== "on" }] }).then(reload, (e) => setProblem(`Could not change ${a.name}: ${reasonOf(e)}`));
  };
  return (
    <main class="browse page settings-page column" data-vscroll>
      <h1 tabindex="-1">{defaultsWords.title}</h1>
      <p class="lede">{defaultsWords.settingsLede}</p>
      <For each={[error(), problem()].filter(Boolean)}>
        {(e) => (
          <p class="st bad" role="status">
            <i class="dot" />
            {e}
          </p>
        )}
      </For>
      <div class="dgroups" data-fgroup="defaults">
        {/* Groups by position, rows by addon: a reload keeps every row. */}
        <Index each={grouped(defaults()?.addons ?? [])}>
          {(g, gi) => (
            <section class="dgroup" aria-label={g().title}>
              <h2 class="kicker">{g().title}</h2>
              <For each={g().addons}>
                {(a, i) => (
                  <DefaultRow addon={a} on={a.installed === "on"} fkey={`defaults:${a.key}`} autofocus={gi === 0 && i() === 0} onToggle={() => flip(a)} />
                )}
              </For>
            </section>
          )}
        </Index>
      </div>
    </main>
  );
}

/** The import's last step on first run: the recommended sources the import did not bring, then Home. */
export function useFinishImport() {
  const navigate = useNavigate();
  const [busy, setBusy] = createSignal(false);
  const [failed, setFailed] = createSignal(false);
  const finish = async () => {
    if (busy()) return;
    setBusy(true);
    setFailed(false);
    // Busy until the page leaves: a key press during the fade into Home does nothing.
    try {
      await installAllDefaults();
      leave(navigate, "/");
    } catch {
      setFailed(true);
      setBusy(false);
    }
  };
  return { finish, busy, failed };
}
