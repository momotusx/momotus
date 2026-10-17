import { A } from "@solidjs/router";
import ChevronRight from "lucide-solid/icons/chevron-right";
import DoorOpen from "lucide-solid/icons/door-open";
import Plus from "lucide-solid/icons/plus";
import Sparkle from "lucide-solid/icons/sparkle";
import { createSignal, For, onCleanup, Show } from "solid-js";
import { link, liveView, op, reasonOf } from "../core/client";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import { focusEl } from "../focus/dom";
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

interface Check {
  label: string;
  tone: Tone;
  text: string;
  fix?: { label: string; run: () => void };
}

export default function Onboarding() {
  const addons = liveView<AddonsSnapshot>("addons");
  const list = () => addons.state.data?.addons ?? [];
  const [online, setOnline] = createSignal(navigator.onLine);
  const onNet = () => setOnline(navigator.onLine);
  window.addEventListener("online", onNet);
  window.addEventListener("offline", onNet);
  onCleanup(() => {
    window.removeEventListener("online", onNet);
    window.removeEventListener("offline", onNet);
  });
  const [busy, setBusy] = createSignal<string | null>(null);
  const [note, setNote] = createSignal<{ tone: Tone; text: string } | null>(null);

  const focusInstall = () => {
    const el = document.querySelector<HTMLElement>('[data-f="welcome:url"]');
    if (el) focusEl(el);
  };
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
  const names = (kind: string) => list().filter((a) => a.enabled && a.provides.includes(kind)).map((a) => a.name);

  const checks = (): Check[] => {
    const lines = Math.min(screen.width, screen.height) * (window.devicePixelRatio || 1);
    const hdr = matchMedia("(dynamic-range: high)").matches;
    const catalogs = names("catalog");
    const sources = names("stream");
    const subs = names("subtitles");
    return [
      {
        label: "Core",
        tone: link() === "open" ? "ok" : link() === "closed" ? "bad" : "warn",
        text: link() === "open" ? "Connected" : link() === "closed" ? "Not reachable" : "Connecting",
      },
      { label: "Network", tone: online() ? "ok" : "bad", text: online() ? "Online" : "Offline" },
      { label: "Display", tone: "ok", text: `${lines >= 2000 ? "4K" : lines >= 1000 ? "1080p" : "720p"}${hdr ? ", HDR" : ""}` },
      {
        label: "Catalogs",
        tone: catalogs.length ? "ok" : "idle",
        text: catalogs.length ? catalogs.join(", ") : "None yet",
        fix: catalogs.length ? undefined : { label: "Add Cinemeta", run: () => void add(SUGGESTED[0]?.url ?? "", "Cinemeta") },
      },
      {
        label: "Sources",
        tone: sources.length ? "ok" : "idle",
        text: sources.length ? sources.join(", ") : "None yet: titles cannot play",
        fix: sources.length ? undefined : { label: "Add a source", run: focusInstall },
      },
      {
        label: "Subtitles",
        tone: subs.length ? "ok" : "idle",
        text: subs.length ? subs.join(", ") : "None yet",
        fix: subs.length ? undefined : { label: "Add OpenSubtitles", run: () => void add(SUGGESTED[1]?.url ?? "", "OpenSubtitles v3") },
      },
    ];
  };

  return (
    <main class="welcome page" data-vscroll>
      <header class="welcome-head">
        <div class="brand">Marquee</div>
        <h1>How would you like to begin?</h1>
        <p>No account needed. Everything stays on this device until you turn on sync.</p>
      </header>
      <div class="welcome-grid">
        <section class="doors" aria-label="Choose a start" data-fgroup="doors">
          <A class="door" href="/import" data-f="welcome:stremio" data-autofocus>
            <DoorOpen aria-hidden />
            <span>
              <b>I use Stremio</b>
              <small>Bring your addons, library and Continue Watching over with a code.</small>
            </span>
            <ChevronRight aria-hidden class="chev" />
          </A>
          <button class="door" data-f="welcome:fresh" onClick={focusInstall}>
            <Sparkle aria-hidden />
            <span>
              <b>Start fresh</b>
              <small>Browse catalogs right away. Add a source when you want to play.</small>
            </span>
            <ChevronRight aria-hidden class="chev" />
          </button>
          <p class="aside">A source is an addon that finds a playable stream for a title. Marquee does not include any.</p>
        </section>

        <section class="panel-card" aria-label="Add addons">
          <h2 class="kicker">Add addons</h2>
          <div class="suggest" data-fgroup="suggest">
            <For each={SUGGESTED}>
              {(s) => (
                <button class="sugg" data-f={`welcome:add:${s.name}`} disabled={installed(s.url) || busy() === s.url} onClick={() => void add(s.url, s.name)}>
                  <Plus aria-hidden />
                  <span>
                    <b>{s.name}</b>
                    <small>{installed(s.url) ? "Installed" : busy() === s.url ? "Installing" : s.what}</small>
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
          <InstallField fkey="welcome" />
        </section>

        <section class="panel-card check" aria-label="Setup check">
          <h2 class="kicker">Setup check</h2>
          <ul>
            <For each={checks()}>
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
        </section>
      </div>
      <A class="skip" href="/" data-f="welcome:skip">
        Go to Home
        <ChevronRight aria-hidden />
      </A>
    </main>
  );
}
