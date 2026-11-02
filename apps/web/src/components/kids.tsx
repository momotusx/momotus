/** The owner's ways to choose what a kids profile watches (plans/m11-kids.md 6.1): every tile's menu (a right click,
 * a long press, the menu key, or OK held on TV), the title page's "Allow for {name}" or, with two kids profiles or
 * more, its "Kids" sheet "Who can watch this?". Allowed or not comes from the core's `kidsKeys`, never from the app. */

import Check from "lucide-solid/icons/check";
import CirclePlus from "lucide-solid/icons/circle-plus";
import Users from "lucide-solid/icons/users";
import X from "lucide-solid/icons/x";
import { createEffect, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { BridgeError } from "../core/bridge";
import { reasonOf } from "../core/client";
import { allowTitle, kidsKeys, removeTitle } from "../core/kids";
import { pushBack } from "../focus/back";
import { focusEl } from "../focus/dom";
import { holdModal } from "../focus/modal";
import { form } from "../form";
import { allowProblem, type KidsChoice, kidsChoices } from "../lib/kids";
import { kidsLine } from "../lib/words";
import "./download.css";
import "../screens/onboarding.css";
import "./kids.css";

/** A title as a tile or a page names it. */
export type KidsTitle = { kind: string; id: string; name: string };
type Ask = KidsTitle & { sheet: boolean };

const [asked, setAsked] = createSignal<Ask | null>(null);

/** The owner has a kids profile to choose for: the menus and the title page's control show. */
export const offersKids = () => (kidsKeys()?.profiles.length ?? 0) > 0;

/** Opens the tile menu (`sheet` false) or the title page's sheet over the screen. */
export const openKidsMenu = (t: KidsTitle, sheet = false) => offersKids() && setAsked({ ...t, sheet });

/** How long OK is held on a TV tile before its menu opens in place of the title. */
const HOLD_MS = 500;

/** A tile's ways to its menu, for the owner while a kids profile exists; nothing changes for anyone else. On TV, OK
 * opens the title when it is let go, and held it opens the menu. */
export function tileMenu(t: () => KidsTitle) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    onContextMenu: (e: MouseEvent) => {
      if (!offersKids()) return;
      e.preventDefault();
      openKidsMenu(t());
    },
    onKeyDown: (e: KeyboardEvent) => {
      if (!offersKids()) return;
      if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
        e.preventDefault();
        openKidsMenu(t());
        return;
      }
      if (e.key !== "Enter" || form() !== "tv") return;
      e.preventDefault();
      if (e.repeat || timer) return;
      timer = setTimeout(() => {
        timer = undefined;
        openKidsMenu(t());
      }, HOLD_MS);
    },
    onKeyUp: (e: KeyboardEvent & { currentTarget: HTMLElement }) => {
      if (e.key !== "Enter" || !timer) return;
      clearTimeout(timer);
      timer = undefined;
      e.currentTarget.click();
    },
  };
}

/** One Allow or Remove at a time per press: what it says while the core works, and its refusal in words. */
function useToggle(t: () => KidsTitle) {
  const [busy, setBusy] = createSignal<{ profile: string; allowing: boolean } | null>(null);
  const [said, setSaid] = createSignal<string | null>(null);
  const toggle = async (c: KidsChoice): Promise<boolean> => {
    if (busy()) return false;
    setBusy({ profile: c.profile, allowing: !c.allowed });
    setSaid(null);
    try {
      if (c.allowed) await removeTitle(c.profile, t().id);
      else await allowTitle(c.profile, t().kind, t().id);
      return true;
    } catch (e) {
      setSaid(allowProblem(e instanceof BridgeError ? e.code : undefined, c.name) ?? reasonOf(e));
      return false;
    } finally {
      setBusy(null);
    }
  };
  return { busy, said, toggle };
}

/** The menu or sheet the screen asked for, over everything. Mounted with the app, it keeps the owner's `kidsKeys` open,
 * so a tile's first right click or held OK already knows whether to offer the menu. */
export function KidsMenuHost() {
  return <Show when={offersKids() && asked()}>{(a) => <KidsMenu ask={a()} onClose={() => setAsked(null)} />}</Show>;
}

function KidsMenu(props: { ask: Ask; onClose: () => void }) {
  let root!: HTMLDivElement;
  let list!: HTMLDivElement;
  holdModal(() => root);
  const choices = () => kidsChoices(kidsKeys(), props.ask.kind, props.ask.id);
  const { busy, said, toggle } = useToggle(() => props.ask);
  // The last kids profile went (here or by sync), or the owner's session ended: nothing is left to choose.
  createEffect(() => !offersKids() && props.onClose());
  onMount(() => {
    onCleanup(pushBack(props.onClose));
    queueMicrotask(() => {
      const first = list.querySelector<HTMLElement>("[data-f]");
      if (first) focusEl(first);
    });
  });
  const press = async (c: KidsChoice) => {
    if ((await toggle(c)) && !props.ask.sheet) props.onClose();
  };
  // Up and Down move through the items; the page under it does not move.
  const keys = (e: KeyboardEvent) => {
    const items = [...list.querySelectorAll<HTMLElement>("[data-f]")];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = items[Math.min(items.length - 1, Math.max(0, at + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) focusEl(next);
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "Tab") e.preventDefault();
  };
  const title = () => (props.ask.sheet ? kidsLine("sheetTitle") : props.ask.name);
  return (
    <div class="dl-layer kids-layer" ref={root} role="dialog" aria-modal="true" aria-labelledby="kids-menu-title">
      <div class="dl-scrim" aria-hidden="true" onClick={props.onClose} />
      <section class="dl-card" data-fgroup="kids-menu">
        <h2 id="kids-menu-title">{title()}</h2>
        <Show when={props.ask.sheet}>
          <p class="dl-detail">{props.ask.name}</p>
        </Show>
        <div class="dl-items" role={props.ask.sheet ? "group" : "menu"} aria-labelledby="kids-menu-title" ref={list} onKeyDown={keys}>
          <For each={choices()}>
            {(c) => (
              <Show
                when={props.ask.sheet}
                fallback={
                  <button class="dl-item" role="menuitem" data-f={`kidsmenu:${c.profile}`} aria-disabled={!!busy()} onClick={() => void press(c)}>
                    <Show when={c.allowed} fallback={<CirclePlus aria-hidden />}>
                      <X aria-hidden />
                    </Show>
                    {busy()?.profile === c.profile && busy()?.allowing ? kidsLine("allowing") : kidsLine(c.allowed ? "remove" : "allow", { name: c.name })}
                  </button>
                }
              >
                <button class="drow kids-switch" role="switch" aria-checked={c.allowed} data-f={`kidsmenu:${c.profile}`} aria-disabled={!!busy()} onClick={() => void press(c)}>
                  <span class="dtext">
                    <b>{c.name}</b>
                    <Show when={busy()?.profile === c.profile && busy()?.allowing}>
                      <small>{kidsLine("allowing")}</small>
                    </Show>
                  </span>
                  <span class="switch" aria-hidden="true">
                    <i />
                  </span>
                </button>
              </Show>
            )}
          </For>
          <Show when={props.ask.sheet}>
            <button class="dl-item kids-done" data-f="kidsmenu:done" onClick={props.onClose}>
              <Check aria-hidden />
              {kidsLine("done")}
            </button>
          </Show>
        </div>
        <Show when={said()}>
          {(t) => (
            <p class="st bad kids-said" role="status">
              <i class="dot" />
              {t()}
            </p>
          )}
        </Show>
      </section>
    </div>
  );
}

/** Search's add mode (plans/m11-kids.md 6.1, "Add titles for {name}"): one result's Allow switch for one kids profile.
 * A refusal goes to the page's line (`onSaid`). */
export function AllowSwitch(props: { title: KidsTitle; profile: string; fkey: string; onSaid: (text: string) => void }) {
  const choice = () => kidsChoices(kidsKeys(), props.title.kind, props.title.id).find((c) => c.profile === props.profile);
  const { busy, said, toggle } = useToggle(() => props.title);
  createEffect(on(said, (t) => t && props.onSaid(t), { defer: true }));
  return (
    <Show when={choice()}>
      {(c) => (
        <button
          class="drow hit-allow"
          role="switch"
          aria-checked={c().allowed}
          aria-label={`${kidsLine("addSwitch")} ${props.title.name}`}
          aria-disabled={!!busy()}
          data-f={props.fkey}
          onClick={() => void toggle(c())}
        >
          <span class="switch" aria-hidden="true">
            <i />
          </span>
          <span class="dword" aria-hidden="true">
            {busy()?.allowing ? kidsLine("allowing") : kidsLine("addSwitch")}
          </span>
        </button>
      )}
    </Show>
  );
}

/** The title page's control: "Allow for {name}" with one kids profile (pressed again it removes), "Kids" opening the
 * sheet with two or more. A refusal says why under the page's actions (`onSaid`). */
export function TitleKids(props: { title: KidsTitle; onSaid: (text: string | null) => void }) {
  const choices = () => kidsChoices(kidsKeys(), props.title.kind, props.title.id);
  const { busy, said, toggle } = useToggle(() => props.title);
  createEffect(() => props.onSaid(said()));
  return (
    <Show when={choices().length > 0}>
      <Show
        when={choices().length === 1 ? choices()[0] : null}
        fallback={
          <button class="btn" data-f="title:kids" aria-haspopup="dialog" onClick={() => openKidsMenu(props.title, true)}>
            <Users aria-hidden />
            {kidsLine("kidsButton")}
          </button>
        }
      >
        {(c) => (
          <button
            class="btn"
            data-f="title:kids"
            aria-pressed={c().allowed}
            aria-label={c().allowed ? kidsLine("allowedSpoken", { name: c().name }) : undefined}
            aria-disabled={!!busy()}
            onClick={() => void toggle(c())}
          >
            <Show when={c().allowed} fallback={<CirclePlus aria-hidden />}>
              <Check aria-hidden />
            </Show>
            {busy()?.allowing ? kidsLine("allowing") : kidsLine(c().allowed ? "allowed" : "allow", { name: c().name })}
          </button>
        )}
      </Show>
    </Show>
  );
}
