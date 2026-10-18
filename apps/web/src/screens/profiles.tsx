/** Profiles (plans/m11-profiles.md, P1 apps and v2): the picker in its three
 * modes, the PIN keypad, Manage profiles and the edit page. */

import { A, useNavigate, useParams } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import Delete from "lucide-solid/icons/delete";
import Lock from "lucide-solid/icons/lock";
import Plus from "lucide-solid/icons/plus";
import { createEffect, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { AvatarArt } from "../components/avatar";
import { BridgeError } from "../core/bridge";
import { op } from "../core/client";
import type { ProfileInfo } from "../core/generated/ProfileInfo";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
import { markChosen, profiles, switchProfile, unlockProfile } from "../core/profiles";
import { autoFocus, goBack } from "../focus/dom";
import { type AvatarColor, COLORS, formatAvatar, GLYPHS, type Glyph, nextAvatar, parseAvatar } from "../lib/avatar";
import { profileProblem, waitText } from "../lib/profiles";
import "./browse.css";
import "./settings.css";
import "./profiles.css";

const BACK_CODES = new Set([10009, 461]);
const isBack = (e: KeyboardEvent) => e.key === "Escape" || e.key === "GoBack" || e.key === "BrowserBack" || BACK_CODES.has(e.keyCode);

/** While a modal is open, everything outside it is inert (no focus, no clicks, hidden from
 * screen readers); focus goes back where it was when it closes. Without `inside`, the modal is
 * the page's main and only the navigation goes inert. */
function holdModal(inside?: () => HTMLElement) {
  const before = document.activeElement as HTMLElement | null;
  const held: HTMLElement[] = [];
  onMount(() => {
    const keep = inside?.();
    const outside = keep ? siblingsUpTo(keep) : [...document.querySelectorAll<HTMLElement>(".app > nav")];
    for (const el of outside) {
      if (el.inert) continue;
      el.inert = true;
      held.push(el);
    }
  });
  onCleanup(() => {
    for (const el of held) el.inert = false;
    if (before?.isConnected) queueMicrotask(() => before.focus());
  });
}

/** Every element beside `el` and beside each of its ancestors, up to the body. */
function siblingsUpTo(el: HTMLElement): HTMLElement[] {
  const out: HTMLElement[] = [];
  for (let node: HTMLElement | null = el; node && node !== document.body; node = node.parentElement) {
    for (const sib of node.parentElement?.children ?? []) if (sib !== node && sib instanceof HTMLElement) out.push(sib);
  }
  return out;
}

/** What a PIN try came to. */
type Tried = { kind: "ok" } | { kind: "wrong" } | { kind: "wait"; ms: number } | { kind: "problem"; text: string };

function tried(e: unknown): Tried {
  if (e instanceof BridgeError && e.code === "wrongPin") return { kind: "wrong" };
  if (e instanceof BridgeError && e.code === "locked" && e.retryAfterMs !== undefined) return { kind: "wait", ms: e.retryAfterMs };
  const code = e instanceof BridgeError ? e.code : undefined;
  return { kind: "problem", text: profileProblem(code, e instanceof Error ? e.message : String(e)) };
}

/** Four digits, one try at a time. The digits live only here and go when it closes or the app hides. */
export function Keypad(props: { title: string; note?: string; onSubmit: (pin: string) => Promise<Tried>; onCancel: () => void; cancelLabel?: string }) {
  const [digits, setDigits] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [shake, setShake] = createSignal(false);
  const [said, setSaid] = createSignal("");
  const [until, setUntil] = createSignal(0);
  const [now, setNow] = createSignal(Date.now());
  let root!: HTMLDivElement;
  let attempt = 0;
  let alive = true;
  const waiting = () => until() > now();
  const timer = setInterval(() => setNow(Date.now()), 1000);
  const hide = () => document.hidden && setDigits("");
  document.addEventListener("visibilitychange", hide);
  onCleanup(() => {
    alive = false;
    clearInterval(timer);
    document.removeEventListener("visibilitychange", hide);
    setDigits("");
  });
  onMount(() => queueMicrotask(() => autoFocus(root.querySelector<HTMLElement>('[data-f="pin:5"]'))));
  holdModal();
  // When a wait ends, the remote goes back to the digits.
  createEffect(
    on(waiting, (w, was) => {
      if (was && !w) {
        setSaid("You can try again.");
        queueMicrotask(() => autoFocus(root.querySelector<HTMLElement>('[data-f="pin:5"]')));
      }
    }),
  );

  const submit = async (pin: string) => {
    setBusy(true);
    const mine = ++attempt;
    const result = await props.onSubmit(pin);
    if (!alive || mine !== attempt) return;
    setBusy(false);
    setDigits("");
    if (result.kind === "wrong") {
      setSaid("Wrong PIN");
      setShake(true);
      setTimeout(() => setShake(false), 220);
    } else if (result.kind === "wait") {
      setUntil(Date.now() + result.ms);
      setNow(Date.now());
      setSaid(`Too many tries. Try again in ${Math.ceil(result.ms / 60_000)} ${result.ms > 60_000 ? "minutes" : "minute"}.`);
      queueMicrotask(() => autoFocus(root.querySelector<HTMLElement>('[data-f="pin:cancel"]')));
    } else if (result.kind === "problem") setSaid(result.text);
  };
  const press = (d: string) => {
    if (busy() || waiting() || digits().length >= 4) return;
    const next = digits() + d;
    setDigits(next);
    if (next.length === 4) void submit(next);
  };
  const back = () => {
    if (busy()) return;
    if (digits()) setDigits(digits().slice(0, -1));
    else props.onCancel();
  };
  // The keypad owns digits, Delete and Back while it is open, wherever focus
  // is (a digit typed before focus lands is never lost), ahead of the app's
  // own key handling; arrows still move focus across its keys.
  const onKey = (e: KeyboardEvent) => {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    if (e.repeat && /^\d$/.test(e.key)) {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    if (/^\d$/.test(e.key)) press(e.key);
    else if (e.key === "Backspace" || isBack(e)) back();
    else return;
    e.preventDefault();
    e.stopPropagation();
  };
  window.addEventListener("keydown", onKey, { capture: true });
  onCleanup(() => window.removeEventListener("keydown", onKey, { capture: true }));
  const keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "", "0", "del"];
  return (
    <div class="keypad" ref={root} role="dialog" aria-modal="true" aria-label={props.title}>
      <h2>{props.title}</h2>
      <div class="pin-dots" classList={{ shake: shake() }} role="img" aria-label={`${digits().length} of 4 digits entered`}>
        <For each={[0, 1, 2, 3]}>{(i) => <i classList={{ on: digits().length > i }} />}</For>
      </div>
      <p class="pin-said" role="status" aria-live="polite">
        {said()}
      </p>
      <Show when={waiting()}>
        <p class="pin-wait" aria-hidden="true">
          Try again in <span class="num">{waitText(until() - now())}</span>
        </p>
      </Show>
      <div class="pin-grid" data-fgroup="keypad">
        <For each={keys}>
          {(k) =>
            k === "" ? (
              <span />
            ) : (
              <button
                class="pin-key"
                data-f={`pin:${k}`}
                aria-label={k === "del" ? "Delete" : k}
                aria-disabled={busy() || waiting()}
                onClick={() => (k === "del" ? back() : press(k))}
              >
                {k === "del" ? <Delete aria-hidden /> : k}
              </button>
            )
          }
        </For>
      </div>
      <Show when={props.note}>
        <p class="note small">{props.note}</p>
      </Show>
      {/* A try with the core cannot be called off half way: leaving waits for its answer. */}
      <button class="btn" data-f="pin:cancel" aria-disabled={busy()} onClick={() => !busy() && props.onCancel()}>
        {props.cancelLabel ?? "Choose another profile"}
      </button>
    </div>
  );
}

/** One profile tile: its avatar, name, and lock or syncing state, spoken as one. */
function Tile(props: { p: ProfileInfo; active: boolean; onPick: () => void }) {
  const spoken = () =>
    [props.p.name, props.active ? "current profile" : "", props.p.syncing ? "still syncing" : props.p.hasPin ? "locked with a PIN" : ""].filter(Boolean).join(", ");
  return (
    <button
      class="ptile"
      classList={{ syncing: props.p.syncing }}
      data-f={`profile:${props.p.id}`}
      data-autofocus={props.active ? "" : undefined}
      aria-label={spoken()}
      aria-disabled={props.p.syncing}
      onClick={() => !props.p.syncing && props.onPick()}
    >
      <AvatarArt avatar={props.p.avatar} name={props.p.name} size="l" />
      <span class="pname">
        {props.p.name}
        <Show when={props.p.hasPin && !props.p.syncing}>
          <Lock aria-hidden class="plock" />
        </Show>
      </span>
      <Show when={props.p.syncing}>
        <small>Syncing</small>
      </Show>
    </button>
  );
}

/** The picker. `start` and `locked` own the screen; `switch` was opened over it and Back returns. */
export function Picker(props: { mode: "start" | "locked" | "switch" }) {
  const navigate = useNavigate();
  const s = () => profiles();
  const [target, setTarget] = createSignal<ProfileInfo | null>(null);
  const [said, setSaid] = createSignal("");
  const owner = () => s()?.profiles.find((p) => p.id === s()?.active)?.isOwner === true;
  const locked = () => s()?.locked === true;

  // A profile removed on another device while it was chosen.
  createEffect(() => {
    const t = target();
    if (t && !s()?.profiles.some((p) => p.id === t.id)) {
      setTarget(null);
      setSaid("That profile was removed on another device.");
    }
  });

  const enter = async (p: ProfileInfo, pin?: string): Promise<Tried> => {
    const same = p.id === s()?.active;
    try {
      if (same && locked()) await unlockProfile(pin);
      else if (same) markChosen();
      else await switchProfile(p.id, pin);
    } catch (e) {
      return tried(e);
    }
    setTarget(null);
    // The same profile, unlocked: the app opens where it was asked to (a
    // deep link, a reload). Another profile: the shell opens Home, as a cold start would.
    if (same && props.mode === "switch") goBack();
    return { kind: "ok" };
  };
  const pick = async (p: ProfileInfo) => {
    setSaid("");
    // The current profile, unlocked, needs nothing: the start picker opens it, the switch picker closes.
    if (p.id === s()?.active && !locked()) {
      if (props.mode === "switch") goBack();
      else markChosen();
      return;
    }
    if (p.hasPin) {
      setTarget(p);
      return;
    }
    const r = await enter(p);
    if (r.kind !== "ok") setSaid(r.kind === "problem" ? r.text : "That did not work. Try again.");
  };
  // Managing profiles is for the open app: the start and locked pickers only choose.
  const manages = () => props.mode === "switch" && owner() && !locked();
  const canAdd = () => manages() && (s()?.profiles.length ?? 0) < (s()?.max ?? 0);
  const forgot = () =>
    target()?.isOwner ? "Forgot the PIN? Reset it from Marquee on your phone or TV, in Settings, Profiles. This device gets the change at its next sync." : undefined;

  return (
    <main class="profiles page" data-vscroll>
      <Show
        when={target()}
        fallback={
          <section class="picker" aria-label="Profiles">
            <h1>Who's watching?</h1>
            <Show when={locked()}>
              <p class="lede">This profile is locked. Choose a profile, then enter its PIN.</p>
            </Show>
            <div class="pgrid" data-fgroup="profiles">
              <For each={s()?.profiles ?? []}>{(p) => <Tile p={p} active={p.id === s()?.active} onPick={() => void pick(p)} />}</For>
              <Show when={canAdd()}>
                <A class="ptile add" href="/profiles/new" data-f="profile:add" aria-label="Add profile">
                  <span class="avatar avatar-l add-art">
                    <Plus aria-hidden />
                  </span>
                  <span class="pname">Add profile</span>
                </A>
              </Show>
            </div>
            <p class="st bad" role="status" aria-live="polite">
              {said()}
            </p>
            <Show when={manages()}>
              <A class="btn" href="/profiles/manage" data-f="profiles:manage">
                Manage profiles
              </A>
            </Show>
          </section>
        }
      >
        {(t) => (
          <Keypad
            title={`Enter the PIN for ${t().name}`}
            note={forgot()}
            onSubmit={(pin) => enter(t(), pin)}
            onCancel={() => {
              const id = t().id;
              setTarget(null);
              queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>(`[data-f="profile:${id}"]`)));
            }}
          />
        )}
      </Show>
    </main>
  );
}

/** The voluntary picker, at /profiles. */
export default function ProfilesScreen() {
  return <Picker mode="switch" />;
}

/** Manage profiles: every profile, each opening its edit page (the owner only). */
export function ManageProfiles() {
  const s = () => profiles();
  const owner = () => s()?.profiles.find((p) => p.id === s()?.active)?.isOwner === true;
  return (
    <main class="browse page settings-page profiles-manage" data-vscroll>
      <h1>Manage profiles</h1>
      <Show when={owner()} fallback={<p class="note">Only the main profile manages profiles. You can edit your own from Settings.</p>}>
        <div class="prows" data-fgroup="manage">
          <For each={s()?.profiles ?? []}>
            {(p, i) => (
              <A class="link-row prow" href={`/profiles/edit/${p.id}`} data-f={`manage:${p.id}`} data-autofocus={i() === 0 ? "" : undefined}>
                <AvatarArt avatar={p.avatar} name={p.name} size="s" />
                <span>
                  <b>{p.name}</b>
                  <small>{[p.isOwner ? "Main profile" : "", p.syncing ? "Syncing" : p.hasPin ? "PIN set" : ""].filter(Boolean).join(" · ") || "No PIN"}</small>
                </span>
              </A>
            )}
          </For>
          <Show when={(s()?.profiles.length ?? 0) < (s()?.max ?? 0)} fallback={<p class="note small">This account has as many profiles as it can hold ({s()?.max}).</p>}>
            <A class="link-row prow" href="/profiles/new" data-f="manage:add">
              <span class="avatar avatar-s add-art">
                <Plus aria-hidden />
              </span>
              <span>
                <b>Add profile</b>
                <small>A new profile starts with your addons and no PIN.</small>
              </span>
            </A>
          </Show>
        </div>
      </Show>
    </main>
  );
}

/** Which PIN steps a page runs: set (new twice), change (current, new twice), remove (current). */
type PinFlow = "set" | "change" | "remove";

/** The edit page, and Add at /profiles/new. The stored avatar is kept unless the user picks one. */
export function EditProfile() {
  const params = useParams();
  const navigate = useNavigate();
  const s = () => profiles();
  const isNew = () => !params.id;
  const p = () => (params.id ? s()?.profiles.find((x) => x.id === params.id) : undefined);
  const mine = () => !!p() && p()?.id === s()?.active;
  const owner = () => s()?.profiles.find((x) => x.id === s()?.active)?.isOwner === true;
  const [name, setName] = createSignal("");
  const [glyph, setGlyph] = createSignal<Glyph>("letter");
  const [color, setColor] = createSignal<AvatarColor>("ember");
  const [touched, setTouched] = createSignal(false);
  const [said, setSaid] = createSignal<{ text: string; bad: boolean } | null>(null);
  const [flow, setFlow] = createSignal<PinFlow | null>(null);
  // The stored PIN came from a newer version: it can only be removed (the core lets its owner do that without it).
  const [unreadable, setUnreadable] = createSignal(false);
  const [confirmDelete, setConfirmDelete] = createSignal(false);
  // Filled once from the stored profile (or a fresh avatar for a new one), never again on later pushes.
  let filled = false;
  createEffect(() => {
    if (filled) return;
    if (isNew()) {
      const a = parseAvatar(nextAvatar(s()?.profiles.map((x) => x.avatar) ?? []));
      setGlyph(a.glyph);
      setColor(a.color ?? "ember");
      filled = true;
    } else if (p()) {
      const a = parseAvatar(p()!.avatar);
      setName(p()!.name);
      setGlyph(a.glyph);
      setColor(a.color ?? "ember");
      filled = true;
    }
  });
  // Removed on another device while open.
  createEffect(() => {
    if (!isNew() && filled && !p()) navigate("/profiles/manage", { replace: true });
  });

  const save = async (e: Event) => {
    e.preventDefault();
    const n = name().trim();
    if (!n || n.length > 30) {
      setSaid({ text: profileProblem("badName", ""), bad: true });
      return;
    }
    try {
      if (isNew()) {
        await op<ProfilesSnapshot>("createProfile", { name: n, avatar: formatAvatar({ glyph: glyph(), color: color() }) });
        navigate("/profiles/manage", { replace: true });
        return;
      }
      const fields: Record<string, unknown> = { profile: p()!.id, name: n };
      if (touched()) fields.avatar = formatAvatar({ glyph: glyph(), color: color() });
      await op<ProfilesSnapshot>("updateProfile", fields);
      setSaid({ text: "Saved", bad: false });
    } catch (err) {
      setSaid({ text: tried(err).kind === "problem" ? (tried(err) as { text: string }).text : "That did not work.", bad: true });
    }
  };
  const removeOther = async () => {
    try {
      await op("setPin", { profile: p()!.id, pin: null });
      setSaid({ text: `${p()!.name}'s PIN was removed.`, bad: false });
    } catch (err) {
      const t = tried(err);
      setSaid({ text: t.kind === "problem" ? t.text : "That did not work.", bad: true });
    }
  };
  const removeUnreadable = async () => {
    try {
      await op("setPin", { profile: p()!.id, pin: null });
      setUnreadable(false);
      setSaid({ text: "PIN removed", bad: false });
    } catch (err) {
      const t = tried(err);
      setSaid({ text: t.kind === "problem" ? t.text : "That did not work.", bad: true });
    }
  };
  const remove = async () => {
    try {
      await op("deleteProfile", { profile: p()!.id });
      navigate("/profiles/manage", { replace: true });
    } catch (err) {
      const t = tried(err);
      setSaid({ text: t.kind === "problem" ? t.text : "That did not work.", bad: true });
      setConfirmDelete(false);
    }
  };

  return (
    <main class="browse page settings-page profiles-edit" data-vscroll>
      <Show when={isNew() ? owner() : mine() || owner()} fallback={<p class="note">Only the main profile can change other profiles.</p>}>
      <Show
        when={!flow()}
        fallback={
          <PinSteps
            flow={flow()!}
            profile={p()!}
            onUnreadable={() => {
              setFlow(null);
              setUnreadable(true);
              queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>('[data-f="pedit:pin:unreadable"]')));
            }}
            onDone={(text) => {
              setFlow(null);
              if (text) setSaid({ text, bad: false });
            }}
          />
        }
      >
        <h1>{isNew() ? "Add profile" : mine() ? "Your profile" : `Edit ${p()?.name ?? ""}`}</h1>
        <form class="pedit" onSubmit={save}>
          <div class="pedit-preview">
            <AvatarArt avatar={formatAvatar({ glyph: glyph(), color: color() })} name={name() || "?"} size="l" />
          </div>
          <label class="field-line">
            <span class="kicker">Name</span>
            <input type="text" maxLength={30} value={name()} onInput={(e) => setName(e.currentTarget.value)} data-f="pedit:name" data-autofocus autocomplete="off" />
          </label>
          <div class="chips-row wrap-row" role="radiogroup" aria-label="Picture" data-fgroup="pedit-glyph">
            <span class="kicker">Picture</span>
            <div class="glyphs">
              <For each={GLYPHS}>
                {(g) => (
                  <button
                    type="button"
                    class="glyph-pick"
                    classList={{ on: glyph() === g }}
                    role="radio"
                    aria-checked={glyph() === g}
                    aria-label={g === "letter" ? "First letter" : g === "gamepad" ? "Game controller" : g.charAt(0).toUpperCase() + g.slice(1)}
                    data-f={`pedit:glyph:${g}`}
                    onClick={() => {
                      setGlyph(g);
                      setTouched(true);
                    }}
                  >
                    <AvatarArt avatar={formatAvatar({ glyph: g, color: color() })} name={name() || "?"} size="s" />
                  </button>
                )}
              </For>
            </div>
          </div>
          <div class="chips-row wrap-row" role="radiogroup" aria-label="Color" data-fgroup="pedit-color">
            <span class="kicker">Color</span>
            <div class="colors">
              <For each={COLORS}>
                {(c) => (
                  <button
                    type="button"
                    class="color-pick"
                    classList={{ on: color() === c }}
                    role="radio"
                    aria-checked={color() === c}
                    aria-label={c.charAt(0).toUpperCase() + c.slice(1)}
                    data-f={`pedit:color:${c}`}
                    style={{ background: `var(--avatar-${c})` }}
                    onClick={() => {
                      setColor(c);
                      setTouched(true);
                    }}
                  >
                    <Show when={color() === c}>
                      <Check aria-hidden />
                    </Show>
                  </button>
                )}
              </For>
            </div>
          </div>
          <div class="acts">
            <button class="btn primary" type="submit" data-f="pedit:save">
              {isNew() ? "Add profile" : "Save"}
            </button>
          </div>
          <Show when={said()}>
            {(m) => (
              <p class={`st ${m().bad ? "bad" : "ok"}`} role="status">
                <i class="dot" />
                {m().text}
              </p>
            )}
          </Show>
        </form>

        <Show when={!isNew() && p()}>
          {(cur) => (
            <section class="pedit-pin" aria-label="PIN">
              <h2 class="kicker">PIN</h2>
              <Show when={cur().syncing}>
                <p class="note small">Waiting for this profile's lock to arrive from your other devices.</p>
              </Show>
              <Show when={!cur().syncing && mine() && unreadable()}>
                <p class="note small">{profileProblem("pinUnreadable", "")}</p>
                <div class="acts">
                  <button class="btn" data-f="pedit:pin:unreadable" onClick={() => void removeUnreadable()}>
                    Remove PIN
                  </button>
                </div>
              </Show>
              <Show when={!cur().syncing && mine() && !unreadable()}>
                <div class="acts">
                  <Show
                    when={cur().hasPin}
                    fallback={
                      <button class="btn" data-f="pedit:pin:set" onClick={() => setFlow("set")}>
                        Set a PIN
                      </button>
                    }
                  >
                    <button class="btn" data-f="pedit:pin:change" onClick={() => setFlow("change")}>
                      Change PIN
                    </button>
                    <button class="btn" data-f="pedit:pin:remove" onClick={() => setFlow("remove")}>
                      Remove PIN
                    </button>
                  </Show>
                </div>
              </Show>
              <Show when={!cur().syncing && !mine() && owner()}>
                <Show when={cur().hasPin} fallback={<p class="note small">{cur().name} has no PIN. They can set one from their own profile.</p>}>
                  <button class="btn" data-f="pedit:pin:clear" onClick={() => void removeOther()}>
                    Remove {cur().name}'s PIN
                  </button>
                </Show>
              </Show>
            </section>
          )}
        </Show>

        <Show when={!isNew() && owner() && p() && !p()!.isOwner}>
          <section class="pedit-danger">
            <Show
              when={confirmDelete()}
              fallback={
                <button class="btn" data-f="pedit:delete" onClick={() => setConfirmDelete(true)}>
                  Delete profile
                </button>
              }
            >
              <DeleteConfirm name={p()!.name} onDelete={() => void remove()} onKeep={() => setConfirmDelete(false)} />
            </Show>
          </section>
        </Show>
      </Show>
      </Show>
    </main>
  );
}

/** Deleting a profile, asked as a modal: the rest of the app waits, Escape or Back keeps it. */
function DeleteConfirm(props: { name: string; onDelete: () => void; onKeep: () => void }) {
  let root!: HTMLDivElement;
  holdModal(() => root);
  const onKey = (e: KeyboardEvent) => {
    if (!isBack(e)) return;
    e.preventDefault();
    e.stopPropagation();
    props.onKeep();
  };
  window.addEventListener("keydown", onKey, { capture: true });
  onCleanup(() => window.removeEventListener("keydown", onKey, { capture: true }));
  return (
              <div class="confirm" ref={root} role="alertdialog" aria-modal="true" aria-label={`Delete ${props.name}?`}>
                <p>
                  Delete {props.name}? Their library, progress and settings are removed from this device now and from your other devices when they next sync.
                </p>
                <div class="acts">
                  <button class="btn danger" data-f="pedit:delete:yes" data-autofocus onClick={() => props.onDelete()}>
                    Delete {props.name}
                  </button>
                  <button class="btn" data-f="pedit:delete:no" onClick={() => props.onKeep()}>
                    Keep
                  </button>
                </div>
              </div>
  );
}

/** The keypad steps for your own PIN, by the core's rules: change and remove ask the current one first. */
function PinSteps(props: { flow: PinFlow; profile: ProfileInfo; onUnreadable: () => void; onDone: (said?: string) => void }) {
  const steps = props.flow === "set" ? ["new", "again"] : props.flow === "change" ? ["current", "new", "again"] : ["current"];
  const [step, setStep] = createSignal(0);
  let current: string | undefined;
  let fresh = "";
  onCleanup(() => {
    current = undefined;
    fresh = "";
  });
  // Hiding the app half way through gives the PIN change up: no PIN stays in memory behind it.
  const hide = () => document.hidden && props.onDone();
  document.addEventListener("visibilitychange", hide);
  onCleanup(() => document.removeEventListener("visibilitychange", hide));
  const title = () =>
    ({ current: "Enter your current PIN", new: props.flow === "set" ? "Choose a PIN" : "Enter the new PIN", again: "Enter it again" })[steps[step()] as "current" | "new" | "again"];
  const submit = async (pin: string): Promise<Tried> => {
    const kind = steps[step()];
    if (kind === "current" && props.flow === "change") {
      current = pin;
      setStep(step() + 1);
      return { kind: "ok" };
    }
    if (kind === "new") {
      fresh = pin;
      setStep(step() + 1);
      return { kind: "ok" };
    }
    if (kind === "again" && pin !== fresh) {
      fresh = "";
      setStep(steps.indexOf("new"));
      return { kind: "problem", text: "The PINs did not match. Choose one again." };
    }
    try {
      if (props.flow === "remove") await op("setPin", { profile: props.profile.id, pin: null, currentPin: pin });
      else await op("setPin", { profile: props.profile.id, pin: fresh, currentPin: current });
    } catch (e) {
      if (e instanceof BridgeError && e.code === "pinUnreadable") {
        props.onUnreadable();
        return { kind: "ok" };
      }
      const t = tried(e);
      // A wrong current PIN starts the change again from the current one.
      if (t.kind === "wrong" && props.flow === "change") setStep(0);
      if (e instanceof BridgeError && e.code === "weakPin") setStep(steps.indexOf("new"));
      return t;
    }
    props.onDone(props.flow === "remove" ? "PIN removed" : "PIN saved");
    return { kind: "ok" };
  };
  return <Keypad title={title()} onSubmit={submit} onCancel={() => props.onDone()} cancelLabel="Cancel" />;
}
