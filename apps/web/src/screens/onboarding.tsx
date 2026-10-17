/** Welcome: one decision per step. 1: the two doors. 2 (Start fresh): add a
 * source. 3: the setup check, run on its own with a fix per row. The step
 * lives in the URL, so Back walks back through the steps. */

import { A, useSearchParams } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ChevronRight from "lucide-solid/icons/chevron-right";
import DoorOpen from "lucide-solid/icons/door-open";
import Plus from "lucide-solid/icons/plus";
import QrCode from "lucide-solid/icons/qr-code";
import Sparkle from "lucide-solid/icons/sparkle";
import { createEffect, createSignal, For, Match, on, onCleanup, Show, Switch } from "solid-js";
import { displayCaps, link, liveView, op, ping, reasonOf } from "../core/client";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import { autoFocus } from "../focus/dom";
import type { Tone } from "../lib/words";
import { InstallField } from "./addons";
import "./browse.css";
import "./settings.css";
import "./onboarding.css";

// Official Stremio addons for metadata and subtitles. Marquee ships no
// stream sources; the user adds those.
const SUGGESTED = [
  { url: "https://v3-cinemeta.strem.io/manifest.json", name: "Cinemeta", what: "Catalogs and details" },
  { url: "https://opensubtitles-v3.strem.io/manifest.json", name: "OpenSubtitles v3", what: "Subtitles" },
];

interface Row {
  label: string;
  tone: Tone;
  text: string;
  fix?: { label: string; run: () => void };
}

export default function Onboarding() {
  const [query, setQuery] = useSearchParams();
  const step = () => (query.step === "2" ? 2 : query.step === "3" ? 3 : 1);
  const go = (n: 1 | 2 | 3) => setQuery({ step: n === 1 ? undefined : String(n) });
  // A new step puts focus on its first action (on a remote or after keys).
  createEffect(on(step, () => queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>(".welcome [data-autofocus]")))));

  const addons = liveView<AddonsSnapshot>("addons");
  const list = () => addons.state.data?.addons ?? [];
  const [busy, setBusy] = createSignal<string | null>(null);
  const [note, setNote] = createSignal<{ tone: Tone; text: string } | null>(null);
  const add = async (url: string, name: string) => {
    setBusy(url);
    setNote(null);
    try {
      await op("installAddon", { url });
      setNote({ tone: "ok", text: `Installed ${name}` });
    } catch (e) {
      setNote({ tone: "bad", text: `Could not install ${name}: ${reasonOf(e)}` });
    } finally {
      setBusy(null);
    }
  };
  const installed = (url: string) => list().some((a) => a.url === url);

  return (
    <main class="welcome page" data-vscroll>
      <header class="welcome-head">
        <div class="brand">Marquee</div>
        <A class="skip" href="/" data-f="welcome:skip">
          Browse first
          <ChevronRight aria-hidden />
        </A>
      </header>
      <Switch>
        <Match when={step() === 1}>
          <section class="step-body" aria-label="Choose a start">
            <h1>How would you like to begin?</h1>
            <p class="lead">No account needed. Everything stays on this device until you turn on sync.</p>
            <div class="doors" data-fgroup="doors">
              <A class="door" href="/import" data-f="welcome:stremio" data-autofocus>
                <DoorOpen aria-hidden />
                <span>
                  <b>I use Stremio</b>
                  <small>Bring your addons, library and Continue Watching over with a code.</small>
                </span>
                <ChevronRight aria-hidden class="chev" />
              </A>
              <button class="door" data-f="welcome:fresh" onClick={() => go(2)}>
                <Sparkle aria-hidden />
                <span>
                  <b>Start fresh</b>
                  <small>Add a source, then browse and play.</small>
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
            <p class="aside">A source is an addon that finds a playable stream for a title. Marquee does not include any.</p>
          </section>
        </Match>
        <Match when={step() === 2}>
          <section class="step-body" aria-label="Add a source">
            <h1>Add a source</h1>
            <p class="lead">Paste the link an addon's website gives you. Sources find streams; Marquee does not include any.</p>
            <InstallField fkey="welcome" />
            <h2 class="kicker">Also useful</h2>
            <div class="suggest" data-fgroup="suggest">
              <For each={SUGGESTED}>
                {(s) => (
                  <button
                    class="sugg"
                    classList={{ done: installed(s.url) }}
                    data-f={`welcome:add:${s.name}`}
                    aria-disabled={installed(s.url) || busy() === s.url}
                    onClick={() => !installed(s.url) && busy() !== s.url && void add(s.url, s.name)}
                  >
                    <Show when={installed(s.url)} fallback={<Plus aria-hidden />}>
                      <Check aria-hidden />
                    </Show>
                    <span>
                      <b>{s.name}</b>
                      <small>{installed(s.url) ? `Installed. ${s.what}` : busy() === s.url ? "Installing" : s.what}</small>
                    </span>
                  </button>
                )}
              </For>
            </div>
            <Show when={note()}>
              {(n) => (
                <p class={`st ${n().tone}`} role="status">
                  <i class="dot" />
                  {n().text}
                </p>
              )}
            </Show>
            <div class="acts">
              <button class="btn primary" data-f="welcome:next" data-autofocus onClick={() => go(3)}>
                Continue
                <ChevronRight aria-hidden />
              </button>
            </div>
          </section>
        </Match>
        <Match when={step() === 3}>
          <SetupCheck list={list()} onAdd={add} onSource={() => go(2)} />
        </Match>
      </Switch>
    </main>
  );
}

/** Runs on its own; each row that is not ready offers its fix. */
function SetupCheck(props: { list: AddonsSnapshot["addons"]; onAdd: (url: string, name: string) => void; onSource: () => void }) {
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
      {
        label: "Catalogs",
        tone: catalogs.length ? "ok" : "idle",
        text: catalogs.length ? catalogs.join(", ") : "None yet",
        fix: catalogs.length ? undefined : { label: "Add Cinemeta", run: () => props.onAdd(SUGGESTED[0]?.url ?? "", "Cinemeta") },
      },
      {
        label: "Sources",
        tone: sources.length ? "ok" : "idle",
        text: sources.length ? sources.join(", ") : "None yet: titles cannot play",
        fix: sources.length ? undefined : { label: "Add a source", run: props.onSource },
      },
      {
        label: "Subtitles",
        tone: subs.length ? "ok" : "idle",
        text: subs.length ? subs.join(", ") : "None yet",
        fix: subs.length ? undefined : { label: "Add OpenSubtitles", run: () => props.onAdd(SUGGESTED[1]?.url ?? "", "OpenSubtitles v3") },
      },
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
      <div class="acts">
        <A class="btn primary" href="/" data-f="welcome:home" data-autofocus>
          Go to Home
          <ChevronRight aria-hidden />
        </A>
      </div>
    </section>
  );
}
