import { A } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Download from "lucide-solid/icons/download";
import KeyRound from "lucide-solid/icons/key-round";
import PackageSearch from "lucide-solid/icons/package-search";
import Puzzle from "lucide-solid/icons/puzzle";
import MonitorSmartphone from "lucide-solid/icons/monitor-smartphone";
import Radio from "lucide-solid/icons/radio";
import Rows3 from "lucide-solid/icons/rows-3";
import Wrench from "lucide-solid/icons/wrench";
import { createSignal, For, type JSX, Show } from "solid-js";
import { liveView, op, prefsView, reasonOf } from "../core/client";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import type { Prefs } from "../core/generated/Prefs";
import type { QualityPref } from "../core/generated/QualityPref";
import { chooseForm, type Form, form } from "../form";
import { findAddons, languageName, syncSummary, syncWords } from "../lib/words";
import * as sync from "../core/sync";
import { traktLink, traktLockedHere, traktStatus, traktStorage } from "../core/trakt";
import { rowLine, traktWords } from "../lib/trakt";
import "./browse.css";
import "./settings.css";
import UserPen from "lucide-solid/icons/user-pen";
import Users from "lucide-solid/icons/users";
import { AvatarArt } from "../components/avatar";
import { profiles } from "../core/profiles";
import { debridAccounts } from "../core/debrid";
import { inUse } from "../lib/debrid";

const clockTime = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

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
              <Show when={props.isOn(item.value)}>
                <Check aria-hidden class="chip-check" />
              </Show>
              {item.label}
            </button>
          )}
        </For>
      </div>
    </div>
  );
}

export function LinkRow(props: { href: string; icon: JSX.Element; title: string; text: string; fkey: string }) {
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
  const me = () => profiles()?.profiles.find((x) => x.id === profiles()?.active);
  // Warnings show here too, not only on the sync page: what failed or waits, then the time.
  const syncLine = () =>
    syncSummary({
      locked: sync.locked(),
      problem: sync.problem()?.words ?? sync.memoryProblem(),
      joined: sync.joined(),
      refused: sync.failed().refused,
      unreadable: sync.failed().unreadable,
      newer: sync.failed().newer,
      report: sync.report(),
      time: sync.syncedAt() ? new Date(sync.syncedAt() as number).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : null,
    });

  return (
    <main class="browse page settings-page" data-vscroll>
      <h1>Settings</h1>
      <Show when={error()}>
        <p class="st bad" role="status">
          <i class="dot" />
          {error()}
        </p>
      </Show>

      <Show when={me()}>
        {(m) => (
          <section class="set">
            <h2>Profiles</h2>
            <div class="links">
              <LinkRow
                href="/profiles"
                fkey="settings:profiles"
                icon={<AvatarArt avatar={m().avatar} name={m().name} size="s" />}
                title={m().name}
                text={(profiles()?.profiles.length ?? 1) > 1 ? "Switch profile" : "Add a profile for someone else, or switch"}
              />
              <LinkRow href={`/profiles/edit/${m().id}`} fkey="settings:profile-edit" icon={<UserPen aria-hidden />} title="Edit profile" text="Name, picture and PIN." />
              <Show when={m().isOwner}>
                <LinkRow href="/profiles/manage" fkey="settings:profiles-manage" icon={<Users aria-hidden />} title="Manage profiles" text="Add, edit and remove profiles on this account." />
              </Show>
            </div>
          </section>
        )}
      </Show>

      <section class="set">
        <h2>Sources</h2>
        <div class="links">
          <LinkRow href="/find-addons" fkey="settings:find-addons" icon={<PackageSearch aria-hidden />} title={findAddons.title} text={findAddons.lede} />
          <LinkRow
            href="/addons"
            fkey="settings:addons"
            icon={<Puzzle aria-hidden />}
            title="Addons"
            text={count() ? `${count()} installed. ${sources().length ? `Streams from ${sources().join(", ")}.` : "None of them finds streams yet."}` : "None installed yet."}
          />
          <LinkRow
            href="/debrid"
            fkey="settings:debrid"
            icon={<KeyRound aria-hidden />}
            title="Debrid services"
            text={debridAccounts()?.accounts.length ? inUse(debridAccounts()!.accounts.length) : "Play torrents as fast direct links with a debrid account."}
          />
          <LinkRow
            href="/trakt"
            fkey="settings:trakt"
            icon={<Radio aria-hidden />}
            title={traktWords.title}
            text={rowLine(traktStatus(), traktLink, clockTime, traktStorage()?.words ?? null, traktLockedHere())}
          />
          <LinkRow href="/import" fkey="settings:import" icon={<Download aria-hidden />} title="Import from Stremio" text="Addons, library and progress, with a code. No password typed here." />
          <LinkRow href="/welcome?step=3" fkey="settings:setup" icon={<Wrench aria-hidden />} title="Setup check" text="Network, display, catalogs, sources and subtitles at a glance." />
        </div>
      </section>

      <section class="set">
        <h2>Devices and sync</h2>
        <div class="links">
          <LinkRow href="/sync" fkey="settings:sync" icon={<MonitorSmartphone aria-hidden />} title={syncWords.title} text={syncLine()} />
        </div>
      </section>

      <section class="set">
        <h2>Home</h2>
        <div class="links">
          <LinkRow href="/home-rows" fkey="settings:home-rows" icon={<Rows3 aria-hidden />} title="Home rows" text="Which catalogs Home shows after Continue Watching, and in what order." />
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
        </div>
      </section>
    </main>
  );
}
