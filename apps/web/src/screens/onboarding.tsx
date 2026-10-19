/** Welcome: one decision per step. 1: the doors. 2: the recommended addons,
 * a switch each (Start fresh, Browse first and a finished import all lead
 * here; nothing is installed before Continue). "choice": how Marquee picks
 * what to play, asked once (plans/m13-stream-choice.md 4.5). 3: the setup
 * check, run on its own with a fix per row. The step lives in the URL, so Back walks back
 * through the steps. The Settings page of the same switches is at the end. */

import { A, useNavigate, useSearchParams } from "@solidjs/router";
import ChevronRight from "lucide-solid/icons/chevron-right";
import DoorOpen from "lucide-solid/icons/door-open";
import Gem from "lucide-solid/icons/gem";
import Link2 from "lucide-solid/icons/link-2";
import PackageSearch from "lucide-solid/icons/package-search";
import QrCode from "lucide-solid/icons/qr-code";
import Sparkle from "lucide-solid/icons/sparkle";
import Zap from "lucide-solid/icons/zap";
import { createEffect, createSignal, For, Index, Match, on, onCleanup, Show, Switch } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { displayCaps, link, liveView, op, ping, reasonOf } from "../core/client";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import type { DefaultAddonInfo } from "../core/generated/DefaultAddonInfo";
import type { DefaultAddons } from "../core/generated/DefaultAddons";
import { autoFocus } from "../focus/dom";
import { form } from "../form";
import { afterContinue, catalogMissing, choicesFor, defaultsWords, grouped, switchable } from "../lib/defaults";
import { choiceWords, findAddons, type Tone } from "../lib/words";
import { InstallField } from "./addons";
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
  return { addons, defaults, error };
}

export default function Onboarding() {
  const [query, setQuery] = useSearchParams();
  const step = () => (query.step === "2" ? 2 : query.step === "3" ? 3 : query.step === "choice" ? "choice" : 1);
  // A new step puts focus on its first action (on a remote or after keys).
  createEffect(on(step, () => queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>(".welcome [data-autofocus]")))));
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
        <div class="brand">Marquee</div>
        {/* Browse first still passes the switches: nothing is installed unasked. */}
        <Show when={step() === 1}>
          <A class="skip" href={fresh() ? "/welcome?step=2&then=home" : "/"} data-f="welcome:skip">
            Browse first
            <ChevronRight aria-hidden />
          </A>
        </Show>
        {/* Skip writes nothing: the defaults are Start fast's. */}
        <Show when={step() === "choice"}>
          <A class="skip" href={afterContinue(query.then as string | undefined)} data-f="welcome:skip">
            {choiceWords.question.skip.label}
            <ChevronRight aria-hidden />
          </A>
        </Show>
      </header>
      <Switch>
        <Match when={step() === 1}>
          <section class="step-body" aria-label="Choose a start">
            <h1>How would you like to begin?</h1>
            <p class="lead">No account needed. Everything stays on this device until you turn on sync.</p>
            <div class="doors" data-fgroup="doors">
              <A class="door" href="/import?from=welcome" data-f="welcome:stremio" data-autofocus>
                <DoorOpen aria-hidden />
                <span>
                  <b>I use Stremio</b>
                  <small>Bring your addons, library and Continue Watching over with a code.</small>
                </span>
                <ChevronRight aria-hidden class="chev" />
              </A>
              <button class="door" data-f="welcome:fresh" onClick={() => setQuery({ step: "2" })}>
                <Sparkle aria-hidden />
                <span>
                  <b>Start fresh</b>
                  <small>Pick from the recommended addons, then browse and play.</small>
                </span>
                <ChevronRight aria-hidden class="chev" />
              </button>
              <A class="door" href="/pair" data-f="welcome:pair">
                <QrCode aria-hidden />
                <span>
                  <b>I already use Marquee</b>
                  <small>Join your other device's sync account with a code. Library and addons follow.</small>
                </span>
                <ChevronRight aria-hidden class="chev" />
              </A>
            </div>
          </section>
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
        <Match when={step() === 3}>
          <SetupCheck list={addons.state.data?.addons ?? []} defaults={defaults()?.addons ?? []} note={note()} onFix={fix} />
        </Match>
      </Switch>
    </main>
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
      // Review (the Home card) goes home; a first run asks the one question.
      navigate(props.review ? afterContinue("home") : `/welcome?step=choice${props.then === "home" ? "&then=home" : ""}`);
    } catch (e) {
      setProblem(`Could not add the addons: ${reasonOf(e)}`);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section class="step-body" aria-label={defaultsWords.title}>
      <h1>{defaultsWords.title}</h1>
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
      navigate(afterContinue(props.then));
    } catch (e) {
      setProblem(`Could not save the choice: ${reasonOf(e)}`);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section class="step-body" aria-label={q.title}>
      <h1>{q.title}</h1>
      <p class="lead">{q.lead}</p>
      <div class="doors two" data-fgroup="choice">
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
      <h1>Setup check</h1>
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
        <A class="btn primary" href="/" data-f="welcome:home" data-autofocus>
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
      <h1>{defaultsWords.title}</h1>
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
