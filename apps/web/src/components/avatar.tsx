/** A profile's avatar (plans/m11-profiles.md, P1 apps D): a glyph or the
 * name's letter, white on one of the avatar color tokens, in a rounded square.
 * It is decoration: the name next to it is what a screen reader reads. */

import Cat from "lucide-solid/icons/cat";
import Clapperboard from "lucide-solid/icons/clapperboard";
import Dog from "lucide-solid/icons/dog";
import Flame from "lucide-solid/icons/flame";
import Gamepad2 from "lucide-solid/icons/gamepad-2";
import Ghost from "lucide-solid/icons/ghost";
import Leaf from "lucide-solid/icons/leaf";
import Moon from "lucide-solid/icons/moon";
import Headphones from "lucide-solid/icons/headphones";
import Popcorn from "lucide-solid/icons/popcorn";
import Rocket from "lucide-solid/icons/rocket";
import Tv from "lucide-solid/icons/tv";
import Check from "lucide-solid/icons/check";
import Lock from "lucide-solid/icons/lock";
import SettingsIcon from "lucide-solid/icons/settings";
import { useNavigate } from "@solidjs/router";
import { type Component, createEffect, createSignal, For, on, onCleanup, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import type { ProfileInfo } from "../core/generated/ProfileInfo";
import { pickOnOpen, profiles } from "../core/profiles";
import { pushBack } from "../focus/back";
import { focusByViewer, focusEl } from "../focus/dom";
import { type Glyph, initial, parseAvatar } from "../lib/avatar";
import "./avatar.css";

const ICONS: Record<Exclude<Glyph, "letter">, Component<{ "aria-hidden"?: boolean }>> = {
  popcorn: Popcorn,
  clapperboard: Clapperboard,
  tv: Tv,
  rocket: Rocket,
  ghost: Ghost,
  cat: Cat,
  dog: Dog,
  leaf: Leaf,
  moon: Moon,
  flame: Flame,
  headphones: Headphones,
  gamepad: Gamepad2,
};

export function AvatarArt(props: { avatar: string; name: string; size?: "s" | "m" | "l" }) {
  const a = () => parseAvatar(props.avatar);
  return (
    <span
      class={`avatar avatar-${props.size ?? "m"}`}
      style={{ "--avatar-fill": a().color ? `var(--avatar-${a().color})` : "var(--raised)" }}
      aria-hidden="true"
    >
      {a().glyph === "letter" ? (
        <span class="avatar-letter">{initial(props.name)}</span>
      ) : (
        <Dynamic component={ICONS[a().glyph as Exclude<Glyph, "letter">]} aria-hidden />
      )}
    </span>
  );
}

/** The bar's far-right item (plans/nav-profile.md): the active profile's avatar, opening a menu of the profiles,
 * then Settings. At the end of the TV tab bar and the desktop header; on the phone, the capsule's last tab with the
 * word "Profile". `lit` while Settings shows: the bar's current-place look, and the TV rail enters on it (`.on`). */
export function ProfileMenu(props: { lit: boolean; tab?: boolean; onSettings: () => void }) {
  const navigate = useNavigate();
  const [open, setOpen] = createSignal(false);
  let wrap!: HTMLDivElement;
  let button!: HTMLButtonElement;
  let list: HTMLDivElement | undefined;
  const me = () => profiles()?.profiles.find((p) => p.id === profiles()?.active);
  const close = (refocus = true) => {
    setOpen(false);
    // The viewer closed it (a choice, Back): the focus they get back is theirs, so a place shown from the menu leaves it
    // on the bar, as one shown from a tab does.
    if (refocus) focusByViewer(button);
  };
  // Opened: focus on the current profile's row. Back closes it, ahead of the app's own Back (the rail's Home).
  createEffect(
    on(open, (o) => {
      if (!o || !list) return;
      const on = list.querySelector<HTMLElement>('[aria-checked="true"]') ?? list.querySelector<HTMLElement>("[role^=menuitem]");
      if (on) focusEl(on);
      onCleanup(pushBack(() => close()));
    }),
  );
  const outside = (e: PointerEvent) => {
    if (!wrap.contains(e.target as Node)) close(false);
  };
  createEffect(() => {
    if (!open()) return;
    document.addEventListener("pointerdown", outside, true);
    onCleanup(() => document.removeEventListener("pointerdown", outside, true));
  });
  // Tab out of the menu closes it. Only a focus that went to a real element outside: a click does not focus a button
  // in Safari (the blur then names no element), and a press outside is the pointer listener's.
  const tabbedOut = (e: FocusEvent) => {
    const to = e.relatedTarget as Node | null;
    if (open() && to && !wrap.contains(to)) close(false);
  };
  // The list keeps the arrows, as a pull-down does: Up and Down stop at its ends, the page under it does not move.
  const keys = (e: KeyboardEvent) => {
    const rows = [...(list?.querySelectorAll<HTMLElement>("[role^=menuitem]") ?? [])];
    const at = rows.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = rows[Math.min(rows.length - 1, Math.max(0, at + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) focusEl(next);
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") e.preventDefault();
  };
  // A row: the picker's tile rule. The current profile closes the menu, a syncing one does nothing, any other opens
  // the picker, which picks it there (a PIN's keypad, or the switch once this profile's screens have gone).
  const choose = (p: ProfileInfo) => {
    close();
    if (p.id === me()?.id || p.syncing) return;
    pickOnOpen(p.id);
    navigate("/profiles");
  };
  return (
    <Show when={me()}>
      {(p) => (
        <div class="pmenu" ref={wrap} on:focusout={tabbedOut}>
          <button
            class={props.tab ? "ptab" : "nav pavatar"}
            classList={{ on: props.lit }}
            data-f="nav:/profiles"
            aria-label={`Profile: ${p().name}`}
            aria-haspopup="menu"
            aria-expanded={open()}
            ref={button}
            onClick={() => setOpen((o) => !o)}
          >
            <AvatarArt avatar={p().avatar} name={p().name} size="s" />
            <Show when={props.tab}>
              <span>Profile</span>
            </Show>
          </button>
          <Show when={open()}>
            <div class="pmenu-list" role="menu" aria-label="Profiles" ref={list} onKeyDown={keys}>
              <For each={profiles()?.profiles ?? []}>
                {(q) => (
                  <button
                    class="pmenu-row"
                    role="menuitemradio"
                    aria-checked={q.id === p().id}
                    aria-label={[q.name, q.id === p().id ? "current profile" : "", q.syncing ? "still syncing" : q.hasPin ? "locked with a PIN" : ""].filter(Boolean).join(", ")}
                    aria-disabled={q.syncing || undefined}
                    data-f={`pmenu:${q.id}`}
                    onClick={() => choose(q)}
                  >
                    <Show when={q.id === p().id}>
                      <Check aria-hidden class="pmenu-check" />
                    </Show>
                    <b>{q.name}</b>
                    <Show when={q.hasPin && !q.syncing && q.id !== p().id}>
                      <Lock aria-hidden class="pmenu-lock" />
                    </Show>
                    <Show when={q.syncing}>
                      <span>Syncing</span>
                    </Show>
                  </button>
                )}
              </For>
              <div class="pmenu-line" role="separator" />
              <button
                class="pmenu-row"
                role="menuitem"
                data-f="pmenu:settings"
                onClick={() => {
                  close();
                  props.onSettings();
                }}
              >
                <SettingsIcon aria-hidden class="pmenu-check" />
                <b>Settings</b>
              </button>
            </div>
          </Show>
        </div>
      )}
    </Show>
  );
}
