/** The desktop profile panel (plans/profile-panel.md, the owner's pick 7): a full-height glass panel from the right,
 * under the header, opened by the header's avatar. The current profile on the backdrop of what it last watched, the
 * other profiles to switch to with theirs, Incognito, then Manage profiles and Settings. Phone and TV keep the menu. */

import HatGlasses from "lucide-solid/icons/hat-glasses";
import Lock from "lucide-solid/icons/lock";
import LogOut from "lucide-solid/icons/log-out";
import SettingsIcon from "lucide-solid/icons/settings";
import Users from "lucide-solid/icons/users";
import X from "lucide-solid/icons/x";
import { createEffect, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { op } from "../core/client";
import type { LastWatched } from "../core/generated/LastWatched";
import type { ProfileInfo } from "../core/generated/ProfileInfo";
import { profiles } from "../core/profiles";
import { pushBack } from "../focus/back";
import { focusEl } from "../focus/dom";
import { holdModal } from "../focus/modal";
import { INCOGNITO, panelRowLabel, panelWords } from "../lib/profiles";
import { AvatarArt } from "./avatar";
import { incognitoWords } from "./incognito";
import "./profile-panel.css";

/** Art that shows once decoded and leaves no trace when it fails. */
function Art(props: { src: string; class: string }) {
  const [failed, setFailed] = createSignal(false);
  return (
    <Show when={!failed()}>
      <img class={`${props.class} fade`} src={props.src} alt="" decoding="async" onLoad={(e) => e.currentTarget.classList.add("in")} onError={() => setFailed(true)} />
    </Show>
  );
}

export function ProfilePanel(props: {
  me: ProfileInfo;
  incognito: boolean;
  offersIncognito: boolean;
  leave: () => void;
  leaveProblem: string | null;
  onClose: () => void;
  onChoose: (p: Pick<ProfileInfo, "id" | "syncing">) => void;
  onManage: () => void;
  onSettings: () => void;
}) {
  let root!: HTMLDivElement;
  // The page under it is inert; the header stays live (its avatar closes the panel), and so does Incognito's warning.
  holdModal(() => root, ".topnav, .incog-notice");
  const kids = () => profiles()?.kids === true;
  const others = () => (profiles()?.profiles ?? []).filter((q) => q.id !== props.me.id);
  const manages = () => props.me.isOwner && !kids() && !props.incognito && !profiles()?.locked;

  // Each profile's last title, asked once on open and again when a profile finishes syncing. Never for a kid or in
  // Incognito (the core refuses both): those see names and avatars only. Rows keep their height while it is out, and
  // a failure leaves them so.
  const [last, setLast] = createSignal<Record<string, LastWatched>>({});
  const ask = () => {
    if (kids() || props.incognito) return;
    const asked = profiles()?.active;
    op<LastWatched[]>("profilesLastWatched")
      .then((list) => profiles()?.active === asked && setLast(Object.fromEntries(list.map((w) => [w.profile, w]))))
      .catch(() => undefined);
  };
  createEffect(on(() => others().filter((q) => q.syncing).map((q) => q.id).join(), ask));
  const mine = () => last()[props.me.id];

  onMount(() => {
    onCleanup(pushBack(props.onClose));
    // The first thing to act on: the first profile to switch to, else Manage profiles, else Settings.
    queueMicrotask(() => {
      const first = root.querySelector<HTMLElement>(".pp-row, [data-f='pmenu:manage'], [data-f='pmenu:settings']");
      if (first) focusEl(first);
    });
  });

  // Tab and Shift+Tab stay inside; Up and Down move through the panel's controls.
  const keys = (e: KeyboardEvent) => {
    const items = [...root.querySelectorAll<HTMLElement>(".pp [data-f]")].filter((el) => el.offsetWidth > 0);
    const at = items.indexOf(document.activeElement as HTMLElement);
    let to: HTMLElement | undefined;
    if (e.key === "Tab") to = e.shiftKey ? (at <= 0 ? items.at(-1) : undefined) : at === items.length - 1 || at < 0 ? items[0] : undefined;
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      to = items[Math.min(items.length - 1, Math.max(0, at + (e.key === "ArrowDown" ? 1 : -1)))];
    }
    if (!to) return;
    e.preventDefault();
    focusEl(to);
  };

  return (
    <div class="pp-layer" ref={root} id="profile-panel" role="dialog" aria-modal="true" aria-labelledby="pp-title" onKeyDown={keys}>
      <div class="pp-scrim" aria-hidden="true" onClick={props.onClose} />
      <aside class="pp glass strong" data-fgroup="profile-panel">
        <header class="pp-head">
          <h2 id="pp-title">{panelWords.title}</h2>
          <button class="pp-x" data-f="pmenu:close" aria-label={panelWords.close} onClick={props.onClose}>
            <X aria-hidden />
          </button>
        </header>
        <div class="pp-body">
          <section class="pp-card" classList={{ art: !!mine()?.art }} role="group" aria-label={`${props.me.name}, current profile`}>
            <Show when={mine()?.art} keyed>
              {(src) => <Art src={src} class="pp-card-art" />}
            </Show>
            <div class="pp-me">
              <AvatarArt avatar={props.me.avatar} name={props.me.name} size="s" />
              <div>
                <b>{props.me.name}</b>
                <span class="pp-sub">{props.incognito ? incognitoWords.kept : props.me.syncing ? panelWords.syncing : panelWords.watchingNow}</span>
              </div>
            </div>
            <Show when={mine()}>
              {(w) => (
                <p class="pp-last">
                  <span aria-hidden="true">
                    {panelWords.lastWatched} <b>{w().label}</b>
                  </span>
                  <span class="sr">{`${panelWords.lastWatched} ${w().spoken}`}</span>
                </p>
              )}
            </Show>
            {/* Leave stays here until it is done: a failure says so in place. */}
            <Show when={props.incognito}>
              <button class="pp-pill pp-leave" data-f="pmenu:leave-incognito" onClick={() => props.leave()}>
                <LogOut aria-hidden />
                {incognitoWords.leave}
              </button>
              <Show when={props.leaveProblem}>
                {(e) => (
                  <p class="pp-said st bad" role="status">
                    {e()}
                  </p>
                )}
              </Show>
            </Show>
          </section>
          <Show when={others().length > 0 || (props.offersIncognito && !props.incognito)}>
            <p class="pp-label" id="pp-switch">
              {panelWords.switchTo}
            </p>
          </Show>
          <div class="pp-rows" role="group" aria-labelledby="pp-switch">
            <For each={others()}>
              {(q) => {
                const w = () => (q.syncing ? undefined : last()[q.id]);
                return (
                  <button class="pp-row" data-f={`pmenu:${q.id}`} aria-label={panelRowLabel(q, w()?.spoken, kids())} aria-disabled={q.syncing || undefined} onClick={() => props.onChoose(q)}>
                    <AvatarArt avatar={q.avatar} name={q.name} size="s" />
                    <span class="pp-two">
                      <b>
                        <span class="pp-name">{q.name}</span>
                        <Show when={q.hasPin && !q.syncing}>
                          <Lock aria-hidden class="pp-lock" />
                        </Show>
                        <Show when={q.kids}>
                          <span class="pp-kb">{panelWords.kids}</span>
                        </Show>
                      </b>
                      <Show when={q.syncing} fallback={<Show when={w()}>{(x) => <small>{x().label}</small>}</Show>}>
                        <small>{panelWords.syncing}</small>
                      </Show>
                    </span>
                    <Show when={w()?.art} keyed>
                      {(src) => <Art src={src} class="pp-thumb" />}
                    </Show>
                  </button>
                );
              }}
            </For>
            {/* Incognito last; while it runs, the card above is Incognito's and Leave is its action. */}
            <Show when={props.offersIncognito && !props.incognito}>
              <button class="pp-row" data-f="pmenu:incognito" aria-label={`${incognitoWords.name}, ${panelWords.incognitoLine}`} onClick={() => props.onChoose({ id: INCOGNITO, syncing: false })}>
                <span class="avatar avatar-s pp-incog" aria-hidden="true">
                  <HatGlasses aria-hidden />
                </span>
                <span class="pp-two">
                  <b>
                    <span class="pp-name">{incognitoWords.name}</span>
                  </b>
                  <small>{panelWords.incognitoLine}</small>
                </span>
              </button>
            </Show>
          </div>
        </div>
        <footer class="pp-foot">
          <Show when={manages()}>
            <button class="pp-pill" data-f="pmenu:manage" onClick={() => props.onManage()}>
              <Users aria-hidden />
              {panelWords.manage}
            </button>
          </Show>
          <button class="pp-pill" data-f="pmenu:settings" onClick={() => props.onSettings()}>
            <SettingsIcon aria-hidden />
            {panelWords.settings}
          </button>
        </footer>
      </aside>
    </div>
  );
}
