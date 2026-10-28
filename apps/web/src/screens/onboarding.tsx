/** Welcome: one decision per step. 1: the doors. 2: the recommended addons,
 * a switch each (Start fresh, Browse first and a finished import all lead
 * here; nothing is installed before Continue). "choice": how Marquee picks
 * what to play, asked once (plans/m13-stream-choice.md 4.5). "subs": the
 * language subtitles start in (plans/m13-subtitle-languages.md 7). 3: the
 * setup check, run on its own with a fix per row. The step lives in the URL, so Back walks back
 * through the steps. The Settings page of the same switches is at the end. */

import { A, useNavigate, useSearchParams } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ChevronLeft from "lucide-solid/icons/chevron-left";
import ChevronRight from "lucide-solid/icons/chevron-right";
import DoorOpen from "lucide-solid/icons/door-open";
import Gem from "lucide-solid/icons/gem";
import Link2 from "lucide-solid/icons/link-2";
import PackageSearch from "lucide-solid/icons/package-search";
import QrCode from "lucide-solid/icons/qr-code";
import Sparkle from "lucide-solid/icons/sparkle";
import Zap from "lucide-solid/icons/zap";
import { createEffect, createSignal, For, Index, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { displayCaps, link, liveView, op, ping, prefsView, reasonOf } from "../core/client";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import type { DefaultAddonInfo } from "../core/generated/DefaultAddonInfo";
import type { DefaultAddons } from "../core/generated/DefaultAddons";
import type { LanguageChoice } from "../core/generated/LanguageChoice";
import { autoFocusSoon, goBack } from "../focus/dom";
import type { FirstRunStill } from "../core/generated/FirstRunStill";
import { form } from "../form";
import { resetHistoryBase } from "../history";
import { afterContinue, catalogMissing, choicesFor, defaultsWords, grouped, switchable } from "../lib/defaults";
import { choiceWords, findAddons, languageName, type Tone } from "../lib/words";
import * as sync from "../core/sync";
import { InstallField } from "./addons";
import { allLanguages, QUICK } from "./settings";
import { SignInStart, TvSignIn } from "./sign-in";
import "./browse.css";
import Brand from "../components/brand";
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
  return { addons, defaults, error };
}

/** Leaving the welcome steps: the step never stays under the next one, and
 * Home opens as the root, so Back never walks back into Setup or the stream
 * question (whose doors would rewrite the stream preferences). */
function leave(navigate: (to: string, o?: { replace?: boolean }) => void, to: string) {
  if (to === "/") resetHistoryBase();
  navigate(to, { replace: true });
}

/** The subtitles step, after the stream question: Home or the setup check next, as the question's `then` says. */
const subsStep = (then: string | undefined) => `/welcome?step=subs${then ? `&then=${then}` : ""}`;
/** The subtitles step's Continue is saving: its Skip waits, so the step never leaves twice. */
const [subsSaving, setSubsSaving] = createSignal(false);

export default function Onboarding() {
  const [query, setQuery] = useSearchParams();
  const step = () =>
    query.step === "2" ? 2 : query.step === "3" ? 3 : query.step === "choice" ? "choice" : query.step === "subs" ? "subs" : query.step === "doors" ? "doors" : 1;
  // A new step puts focus on its first action (on a remote or after keys). Off the TV a step change puts it on
  // the step's heading, so a screen reader starts at the question.
  createEffect(on(step, () => autoFocusSoon(() => document.querySelector<HTMLElement>(".welcome [data-autofocus]"))));
  createEffect(
    on(step, () => form() !== "tv" && requestAnimationFrame(() => document.querySelector<HTMLElement>(".welcome h1")?.focus({ preventScroll: true })), {
      defer: true,
    }),
  );
  // The steps a first run walks: the doors, the recommended addons, the stream question, subtitles.
  const count = () => ({ 1: 1, doors: 1, 2: 2, choice: 3, subs: 4 })[String(step())] ?? null;
  const { addons, defaults, error } = useDefaults();
  const fresh = () => addons.state.data?.fresh ?? true;
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
        <Show when={count()}>
          {(n) => (
            <div class="welcome-step">
              <Show when={n() > 1}>
                <button class="skip step-back" data-f="welcome:back" onClick={() => goBack()}>
                  <ChevronLeft aria-hidden />
                  Back
                </button>
              </Show>
              <span class="step-count">
                <span class="dots" aria-hidden="true">
                  <For each={[1, 2, 3, 4]}>{(i) => <i classList={{ on: i === n() }} />}</For>
                </span>
                Step {n()} of 4
              </span>
            </div>
          )}
        </Show>
        {/* Browse first still passes the switches: nothing is installed unasked. */}
        <Show when={step() === 1 || step() === "doors"}>
          <A class="skip" href={fresh() ? "/welcome?step=2&then=home" : "/"} replace={!fresh()} onClick={() => !fresh() && resetHistoryBase()} data-f="welcome:skip">
            Browse first
            <ChevronRight aria-hidden />
          </A>
        </Show>
        {/* Skip writes nothing: the defaults are Start fast's. */}
        <Show when={step() === "choice"}>
          <A class="skip" href={subsStep(query.then as string | undefined)} replace data-f="welcome:skip">
            {choiceWords.question.skip.label}
            <ChevronRight aria-hidden />
          </A>
        </Show>
        {/* Skip writes nothing: no subtitle turns on by itself. */}
        <Show when={step() === "subs"}>
          <A
            class="skip"
            href={afterContinue(query.then as string | undefined)}
            replace
            aria-disabled={subsSaving()}
            onClick={(e) => {
              if (subsSaving()) e.preventDefault();
              else if (query.then === "home") resetHistoryBase();
            }}
            data-f="welcome:skip"
          >
            Skip
            <ChevronRight aria-hidden />
          </A>
        </Show>
      </header>
      <Switch>
        {/* Sign-in first (plans/accounts.md 4.1, 4.2), on a device with no account yet; with no
            sync service that offers it, the doors. */}
        <Match when={step() === 1}>
          <Show when={!sync.joined() && !sync.locked()} fallback={<Doors onFresh={() => setQuery({ step: "2" })} />}>
            <Show when={form() === "tv"} fallback={<SignInStart from="welcome" doors={() => <Doors onFresh={() => setQuery({ step: "2" })} />} />}>
              <TvSignIn doors={() => <Doors onFresh={() => setQuery({ step: "2" })} />} />
            </Show>
          </Show>
        </Match>
        <Match when={step() === "doors"}>
          <Doors onFresh={() => setQuery({ step: "2" })} />
        </Match>
        <Match when={step() === 2}>
          <Recommended
            defaults={defaults()}
            error={error()}
            hasCatalog={hasCatalog()}
            review={query.review === "1"}
            fromImport={query.from === "import"}
            then={query.then as string | undefined}
          />
        </Match>
        <Match when={step() === "choice"}>
          <Question then={query.then as string | undefined} />
        </Match>
        <Match when={step() === "subs"}>
          <Subtitles then={query.then as string | undefined} />
        </Match>
        <Match when={step() === 3}>
          <SetupCheck list={addons.state.data?.addons ?? []} defaults={defaults()?.addons ?? []} note={note()} onFix={fix} />
        </Match>
      </Switch>
    </main>
  );
}

// The still every framed step shows (`firstRunStill`): asked once per app run, at the first framed step.
let still: Promise<FirstRunStill | null> | undefined;
const firstStill = () => (still ??= op<FirstRunStill | null>("firstRunStill").catch(() => null));

/** B4, the letterbox (the owner's pick of 2026-10-28): a 2.39:1 frame holding a film's still with the step's
 * question set as its subtitle; no still (offline, slow), the frame stays black with the question. The lead
 * line rides under the question on TV and desktop, under the frame on the phone. Both are decorative art and
 * a real heading. */
function Film(props: { title: string; lead?: string }) {
  const [art, setArt] = createSignal<string | null>(null);
  onMount(() => void firstStill().then((s) => setArt(s?.background ?? null)));
  return (
    <>
      <div class="film">
        <Show when={art()} keyed>
          {(src) => <img src={src} alt="" decoding="async" class="fade" onLoad={(e) => e.currentTarget.classList.add("in")} />}
        </Show>
        <div class="film-cue">
          <h1 id="welcome-q" tabindex="-1">
            <span>{props.title}</span>
          </h1>
          <Show when={props.lead && form() !== "phone"}>
            <p class="lead">
              <span>{props.lead}</span>
            </p>
          </Show>
        </div>
      </div>
      <Show when={props.lead && form() === "phone"}>
        <p class="lead">{props.lead}</p>
      </Show>
    </>
  );
}

/** The doors: I use Stremio, Start fresh, and (no account here yet) I already use Marquee. */
function Doors(props: { onFresh: () => void }) {
  return (
    <section class="step-body framed" aria-labelledby="welcome-q">
      {/* After Create the account exists: the line would say the opposite. */}
      <Film title="How would you like to begin?" lead={sync.joined() ? undefined : "No account needed. Everything stays on this device until you turn on sync."} />
      <div class="doors" role="group" aria-labelledby="welcome-q" data-fgroup="doors">
        <A class="door" href="/import?from=welcome" data-f="welcome:stremio" data-autofocus>
          <DoorOpen aria-hidden />
          <span>
            <b>I use Stremio</b>
            <small>Bring your addons, library and Continue Watching over with a code.</small>
          </span>
          <ChevronRight aria-hidden class="chev" />
        </A>
        <button class="door" data-f="welcome:fresh" onClick={() => props.onFresh()}>
          <Sparkle aria-hidden />
          <span>
            <b>Start fresh</b>
            <small>Pick from the recommended addons, then browse and play.</small>
          </span>
          <ChevronRight aria-hidden class="chev" />
        </button>
        <Show when={!sync.joined()}>
          <A class="door" href="/pair" data-f="welcome:pair">
            <QrCode aria-hidden />
            <span>
              <b>I already use Momotus</b>
              <small>Join your other device's sync account with a code. Library and addons follow.</small>
            </span>
            <ChevronRight aria-hidden class="chev" />
          </A>
        </Show>
      </div>
    </section>
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

/** Step 2: the nine, grouped, each on until turned off. After an import the
 * ones Stremio brought show without a switch; Review (the Home card) shows
 * only the missing ones. Continue names exactly the rows with a switch. */
function Recommended(props: { defaults: DefaultAddons | null; error: string | null; hasCatalog: boolean; review: boolean; fromImport: boolean; then?: string }) {
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
  const present = (a: DefaultAddonInfo) => (props.fromImport ? "From Stremio" : a.installed === "on" ? "Installed" : "Installed, off");
  const go = async () => {
    if (busy() || !props.defaults) return;
    setBusy(true);
    setProblem(null);
    try {
      await op("installDefaults", { choices: choicesFor(all(), off()) });
      // Back from the next step must not call Marquee's own installs "From
      // Stremio": this entry stops saying it came from an import. (A second
      // router navigation here would be dropped, so the entry is rewritten.)
      if (props.fromImport) {
        const here = new URL(window.location.href);
        here.searchParams.delete("from");
        history.replaceState(history.state, "", `${here.pathname}${here.search}`);
      }
      // Review (the Home card) and Browse first go home; a first run asks the one question.
      if (props.review || props.then === "home") leave(navigate, afterContinue("home"));
      else navigate("/welcome?step=choice");
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

/** The one question: Start fast (focused) or Best quality, each a patch of
 * the stream preferences from the shared words. */
function Question(props: { then?: string }) {
  const navigate = useNavigate();
  const q = choiceWords.question;
  const [busy, setBusy] = createSignal(false);
  const [problem, setProblem] = createSignal<string | null>(null);
  const pick = async (patch: Record<string, unknown>) => {
    if (busy()) return;
    setBusy(true);
    setProblem(null);
    try {
      await op("setPrefs", { patch: { streams: patch } });
      leave(navigate, subsStep(props.then));
    } catch (e) {
      setProblem(`Could not save the choice: ${reasonOf(e)}`);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section class="step-body framed question" aria-labelledby="welcome-q">
      <Film title={q.title} lead={q.lead} />
      <div class="doors two" role="group" aria-labelledby="welcome-q" data-fgroup="choice">
        <button class="door" data-f="welcome:fast" data-autofocus aria-disabled={busy()} onClick={() => void pick(q.fast.patch)}>
          <Zap aria-hidden />
          <span>
            <b>{q.fast.label}</b>
            <small>{q.fast.text}</small>
          </span>
          <ChevronRight aria-hidden class="chev" />
        </button>
        <button class="door" data-f="welcome:best" aria-disabled={busy()} onClick={() => void pick(q.best.patch)}>
          <Gem aria-hidden />
          <span>
            <b>{q.best.label}</b>
            <small>{q.best.text}</small>
          </span>
          <ChevronRight aria-hidden class="chev" />
        </button>
      </div>
      <Show when={problem()}>
        {(p) => (
          <p class="st bad" role="status">
            <i class="dot" />
            {p()}
          </p>
        )}
      </Show>
    </section>
  );
}

/** The language subtitles start in, one choice: this device's language
 * chosen (No subtitles on an English device: most titles are in English, and
 * English audio the file does not tag would get English subtitles over it),
 * then the common ones and No subtitles, every other language behind More
 * languages. Continue writes it as the profile's subtitle languages; Skip
 * writes nothing. A profile that has languages already passes by. */
function Subtitles(props: { then?: string }) {
  const navigate = useNavigate();
  const prefs = prefsView();
  const next = () => leave(navigate, afterContinue(props.then));
  // Once, when the preferences are in: a Continue here must not pass twice.
  createEffect(on(() => prefs.state.data !== null, (ready) => ready && (prefs.state.data?.subtitleLanguages.length ?? 0) > 0 && next()));
  const [all, setAll] = createSignal<LanguageChoice[]>([]);
  const [chosen, setChosen] = createSignal<string | null>(null);
  // The device's language as the core lists it ("no" for a Norwegian "nb"):
  // a code it does not list would be a preference no track ever matches.
  const device = () => all().find((l) => l.device)?.code ?? null;
  let touched = false;
  const pick = (code: string | null) => {
    touched = true;
    setChosen(code);
  };
  // Continue waits for the list: before it, the device's language is unknown.
  const [loaded, setLoaded] = createSignal(false);
  const [more, setMore] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [problem, setProblem] = createSignal<string | null>(null);
  const load = (list: LanguageChoice[]) => {
    setAll(list);
    const mine = device();
    if (!touched && mine !== "en") setChosen(mine);
    setLoaded(true);
  };
  onMount(
    () =>
      void allLanguages().then(load, (e: unknown) => {
        setProblem(`Could not load the languages: ${reasonOf(e)}`);
        load([]);
      }),
  );
  const nameOf = (code: string) => all().find((l) => l.code === code)?.name ?? languageName(code);
  const common = () => [...new Set([device(), ...QUICK].filter((c): c is string => c !== null))];
  // The core's list comes sorted by name.
  const rest = () => all().filter((l) => !common().includes(l.code));
  const go = async () => {
    if (busy() || !loaded()) return;
    setBusy(true);
    setSubsSaving(true);
    setProblem(null);
    try {
      const code = chosen();
      await op("setPrefs", { patch: { subtitleLanguages: code ? [code] : [] } });
      next();
    } catch (e) {
      setProblem(`Could not save the language: ${reasonOf(e)}`);
    } finally {
      setBusy(false);
      setSubsSaving(false);
    }
  };
  // The device's line follows the core's list, which can come after the row.
  const row = (code: string | null, name: string, line?: string) => (
    <button class="drow subrow" role="radio" aria-checked={chosen() === code} data-f={`welcome:sub:${code ?? "none"}`} onClick={() => pick(code)}>
      <span class="dtext">
        <b>{name}</b>
        <Show when={line ?? (code !== null && code === device() ? "This device's language" : undefined)}>
          {(text) => <small>{text()}</small>}
        </Show>
      </span>
      <Check aria-hidden class="lcheck" />
    </button>
  );
  return (
    <section class="step-body framed" aria-labelledby="welcome-q">
      <Film title="Subtitles" lead="Pick the language subtitles start in. They turn on when a film's audio is in another language. Change it later in Settings, under Playback." />
      <div class="langs" role="radiogroup" aria-labelledby="welcome-q" data-fgroup="subs">
        <For each={common()}>{(code) => row(code, nameOf(code))}</For>
        {row(null, "No subtitles", "Turn them on while watching")}
      </div>
      <Show when={problem()}>
        {(p) => (
          <p class="st bad" role="status">
            <i class="dot" />
            {p()}
          </p>
        )}
      </Show>
      <div class="acts">
        <button class="btn primary" data-f="welcome:next" data-autofocus aria-disabled={busy() || !loaded()} onClick={() => void go()}>
          Continue
          <ChevronRight aria-hidden />
        </button>
        <button class="btn" data-f="welcome:sub-more" aria-expanded={more()} onClick={() => setMore(!more())}>
          {more() ? "Fewer languages" : "More languages"}
        </button>
      </div>
      <Show when={more()}>
        <div class="langs" role="radiogroup" aria-label="More languages" data-fgroup="subs-all">
          <For each={rest()}>{(l) => row(l.code, l.name)}</For>
        </div>
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
  const { defaults, error } = useDefaults();
  const [problem, setProblem] = createSignal<string | null>(null);
  const flip = (a: DefaultAddonInfo) => {
    setProblem(null);
    op("installDefaults", { choices: [{ key: a.key, on: a.installed !== "on" }] }).catch((e) => setProblem(`Could not change ${a.name}: ${reasonOf(e)}`));
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
