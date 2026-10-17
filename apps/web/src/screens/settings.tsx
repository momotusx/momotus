import { A } from "@solidjs/router";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Download from "lucide-solid/icons/download";
import Puzzle from "lucide-solid/icons/puzzle";
import QrCode from "lucide-solid/icons/qr-code";
import Wrench from "lucide-solid/icons/wrench";
import { createSignal, For, type JSX, Show } from "solid-js";
import { liveView, op, prefsView, reasonOf } from "../core/client";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import type { Prefs } from "../core/generated/Prefs";
import type { QualityPref } from "../core/generated/QualityPref";
import { chooseForm, chooseTheme, type Form, form, type Theme, theme } from "../form";
import { languageName } from "../lib/words";
import "./browse.css";
import "./settings.css";

// ISO 639-1 for audio (stream prefs) and 639-2 for subtitles, as the core expects.
const LANGS: [iso1: string, iso2: string][] = [
  ["en", "eng"],
  ["tr", "tur"],
  ["es", "spa"],
  ["fr", "fre"],
  ["de", "ger"],
  ["it", "ita"],
  ["pt", "por"],
  ["ja", "jpn"],
];

function Choice<T>(props: {
  label: string;
  group: string;
  items: { value: T; label: string }[];
  isOn: (v: T) => boolean;
  onPick: (v: T) => void;
  autofocus?: boolean;
  multi?: boolean;
}) {
  return (
    <div class="chips-row wrap-row" role={props.multi ? "group" : "radiogroup"} aria-label={props.label} data-fgroup={props.group}>
      <span class="kicker">{props.label}</span>
      <div class="chips-track wrap">
        <For each={props.items}>
          {(item, i) => (
            <button
              class="chip"
              classList={{ on: props.isOn(item.value) }}
              role={props.multi ? "checkbox" : "radio"}
              aria-checked={props.isOn(item.value)}
              data-f={`${props.group}:${String(item.value)}`}
              data-autofocus={props.autofocus && i() === 0 ? "" : undefined}
              onClick={() => props.onPick(item.value)}
            >
              {item.label}
            </button>
          )}
        </For>
      </div>
    </div>
  );
}

function LinkRow(props: { href: string; icon: JSX.Element; title: string; text: string; fkey: string }) {
  return (
    <A class="link-row" href={props.href} data-f={props.fkey} data-autofocus={props.fkey === "settings:addons" ? "" : undefined}>
      {props.icon}
      <span>
        <b>{props.title}</b>
        <small>{props.text}</small>
      </span>
      <ChevronRight aria-hidden class="chev" />
    </A>
  );
}

export default function SettingsScreen() {
  const prefs = prefsView();
  const addons = liveView<AddonsSnapshot>("addons");
  const p = () => prefs.state.data;
  const [error, setError] = createSignal<string | null>(null);
  const patch = (x: Record<string, unknown>) => {
    setError(null);
    op<Prefs>("setPrefs", { patch: x }).catch((e) => setError(reasonOf(e)));
  };
  const toggle = (list: string[], v: string) => (list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);
  const count = () => addons.state.data?.addons.length ?? 0;
  const sources = () => addons.state.data?.addons.filter((a) => a.enabled && a.provides.includes("stream")).map((a) => a.name) ?? [];

  return (
    <main class="browse page settings-page" data-vscroll>
      <h1>Settings</h1>
      <Show when={error()}>
        <p class="st bad" role="status">
          <i class="dot" />
          {error()}
        </p>
      </Show>

      <section class="set">
        <h2>Sources</h2>
        <div class="links">
          <LinkRow
            href="/addons"
            fkey="settings:addons"
            icon={<Puzzle aria-hidden />}
            title="Addons"
            text={count() ? `${count()} installed. ${sources().length ? `Streams from ${sources().join(", ")}.` : "None of them finds streams yet."}` : "None installed yet."}
          />
          <LinkRow href="/import" fkey="settings:import" icon={<Download aria-hidden />} title="Import from Stremio" text="Addons, library and progress, with a code. No password typed here." />
          <LinkRow href="/welcome" fkey="settings:setup" icon={<Wrench aria-hidden />} title="Setup check" text="Network, display, catalogs, sources and subtitles at a glance." />
          <LinkRow href="/pair" fkey="settings:pair" icon={<QrCode aria-hidden />} title="Pair this device" text="Join your sync account by scanning a code on a device that already syncs." />
        </div>
      </section>

      <Show when={p()}>
        {(x) => (
          <section class="set">
            <h2>Playback</h2>
            <div class="filters">
              <Choice<QualityPref>
                label="Pick"
                group="pref-quality"
                items={[
                  { value: "best", label: "Best quality" },
                  { value: "balanced", label: "Balanced" },
                  { value: "dataSaver", label: "Data saver" },
                ]}
                isOn={(v) => x().streams.quality === v}
                onPick={(v) => patch({ streams: { quality: v } })}
              />
              <Choice<number | null>
                label="Up to"
                group="pref-height"
                items={[
                  { value: null, label: "Any resolution" },
                  { value: 2160, label: "4K" },
                  { value: 1080, label: "1080p" },
                  { value: 720, label: "720p" },
                ]}
                isOn={(v) => x().streams.maxHeight === v}
                onPick={(v) => patch({ streams: { maxHeight: v } })}
              />
              <Choice<string>
                multi
                label="Audio"
                group="pref-audio"
                items={LANGS.map(([iso1]) => ({ value: iso1, label: languageName(iso1) }))}
                isOn={(v) => x().streams.languages.includes(v)}
                onPick={(v) => patch({ streams: { languages: toggle(x().streams.languages, v) } })}
              />
              <Choice<string>
                multi
                label="Subtitles"
                group="pref-subs"
                items={LANGS.map(([, iso2]) => ({ value: iso2, label: languageName(iso2) }))}
                isOn={(v) => x().subtitleLanguages.includes(v)}
                onPick={(v) => patch({ subtitleLanguages: toggle(x().subtitleLanguages, v) })}
              />
              <Choice<boolean>
                label="Up next"
                group="pref-next"
                items={[
                  { value: true, label: "Starts on its own" },
                  { value: false, label: "Wait for me" },
                ]}
                isOn={(v) => x().autoplayNext === v}
                onPick={(v) => patch({ autoplayNext: v })}
              />
            </div>
            <p class="note small">Languages are listed in the order you turn them on; the first one wins.</p>
          </section>
        )}
      </Show>

      <section class="set">
        <h2>This device</h2>
        <div class="filters">
          <Choice<Form | null>
            label="Layout"
            group="dev-form"
            items={[
              { value: "tv", label: "TV" },
              { value: "desktop", label: "Desktop" },
              { value: "phone", label: "Phone" },
            ]}
            isOn={(v) => form() === v}
            onPick={(v) => chooseForm(v)}
          />
          <Choice<Theme | null>
            label="Theme"
            group="dev-theme"
            items={[
              { value: null, label: "Follow the system" },
              { value: "dark", label: "Dark" },
              { value: "light", label: "Light" },
            ]}
            isOn={(v) => theme() === v}
            onPick={(v) => chooseTheme(v)}
          />
        </div>
      </section>
    </main>
  );
}
