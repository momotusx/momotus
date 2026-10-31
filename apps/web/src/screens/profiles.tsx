/** Profiles (plans/m11-profiles.md, P1 apps and v2): the picker in its three
 * modes, the PIN keypad, Manage profiles and the edit page. */

import { A, useBeforeLeave, useNavigate, useParams } from "@solidjs/router";
import Delete from "lucide-solid/icons/delete";
import Lock from "lucide-solid/icons/lock";
import Plus from "lucide-solid/icons/plus";
import { createEffect, createSignal, For, type JSX, on, onCleanup, onMount, Show } from "solid-js";
import { AvatarArt } from "../components/avatar";
import { BridgeError } from "../core/bridge";
import { op, reasonOf } from "../core/client";
import type { ProfileInfo } from "../core/generated/ProfileInfo";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
import { markChosen, profiles, switchProfile, takePick, unlockProfile } from "../core/profiles";
import { autoFocusSoon, goBack } from "../focus/dom";
import { holdModal } from "../focus/modal";
import { markBack, setLeaveGuarded, takeAfterBack, takeBack } from "../history";
import { type AvatarColor, formatAvatar, GLYPHS, type Glyph, nextAvatar, parseAvatar } from "../lib/avatar";
import { profileProblem, waitText } from "../lib/profiles";
import "./browse.css";
import "./settings.css";
import "./profiles.css";

const BACK_CODES = new Set([10009, 461]);
const isBack = (e: KeyboardEvent) => e.key === "Escape" || e.key === "GoBack" || e.key === "BrowserBack" || BACK_CODES.has(e.keyCode);

/** How long a PIN try waits for the core before the keypad offers a reload. */
const PIN_WAIT_MS = 20_000;

/** What a PIN try came to. */
type Tried = { kind: "ok" } | { kind: "wrong" } | { kind: "wait"; ms: number } | { kind: "problem"; text: string };

function tried(e: unknown): Tried {
  if (e instanceof BridgeError && e.code === "wrongPin") return { kind: "wrong" };
  if (e instanceof BridgeError && e.code === "locked" && e.retryAfterMs !== undefined) return { kind: "wait", ms: e.retryAfterMs };
  const code = e instanceof BridgeError ? e.code : undefined;
  return { kind: "problem", text: profileProblem(code, reasonOf(e)) };
}

/** Four digits, one try at a time. The digits live only here and go when it closes or the app hides. */
export function Keypad(props: { title: string; note?: string; onSubmit: (pin: string) => Promise<Tried>; onCancel: () => void; cancelLabel?: string }) {
  const [digits, setDigits] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [shake, setShake] = createSignal(false);
  const [said, setSaid] = createSignal("");
  const [stuck, setStuck] = createSignal(false);
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
  onMount(() => autoFocusSoon(() => root.querySelector<HTMLElement>('[data-f="pin:5"]')));
  holdModal();
  // When a wait ends, the remote goes back to the digits.
  createEffect(
    on(waiting, (w, was) => {
      if (was && !w) {
        setSaid("You can try again.");
        autoFocusSoon(() => root.querySelector<HTMLElement>('[data-f="pin:5"]'));
      }
    }),
  );

  const submit = async (pin: string) => {
    setBusy(true);
    const mine = ++attempt;
    // A core that does not answer: the try may still go through, so nothing is
    // called off. The keypad says so and offers a reload, which starts a fresh
    // session with the core.
    const slow = setTimeout(() => mine === attempt && setStuck(true), PIN_WAIT_MS);
    const result = await props.onSubmit(pin);
    clearTimeout(slow);
    setStuck(false);
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
      autoFocusSoon(() => root.querySelector<HTMLElement>('[data-f="pin:cancel"]'));
    } else if (result.kind === "problem") setSaid(result.text);
  };
  const press = (d: string) => {
    if (busy() || waiting() || digits().length >= 4) return;
    const next = digits() + d;
    setDigits(next);
    if (next.length === 4) void submit(next);
  };
  // The Delete key on screen only deletes; Backspace and Back, which a remote and a keyboard have no other key for, leave
  // when there is nothing left to delete.
  const del = () => !busy() && setDigits(digits().slice(0, -1));
  const back = () => {
    if (busy()) return;
    if (digits()) del();
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
                onClick={() => (k === "del" ? del() : press(k))}
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
      <Show when={stuck()}>
        <p class="pin-said" role="status">
          The core is not answering.
        </p>
        <button class="btn" data-f="pin:reload" onClick={() => location.reload()}>
          Reload
        </button>
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
  // Opened by the bar's menu for one profile: picked here, as its tile would be. A keypad it opened cancels back to
  // where the menu was.
  const asked = props.mode === "switch" ? takePick() : null;
  onMount(() => {
    const p = asked ? s()?.profiles.find((q) => q.id === asked) : undefined;
    if (p) void pick(p);
  });
  // Managing profiles is for the open app: the start and locked pickers only choose.
  const manages = () => props.mode === "switch" && owner() && !locked();
  const canAdd = () => manages() && (s()?.profiles.length ?? 0) < (s()?.max ?? 0);
  const forgot = () =>
    target()?.isOwner ? "Forgot the PIN? Reset it from Momotus on your phone or TV, in Settings, Profiles. This device gets the change at its next sync." : undefined;

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
            cancelLabel={t().id === asked ? "Cancel" : undefined}
            onCancel={() => {
              const id = t().id;
              if (id === asked) return goBack();
              setTarget(null);
              autoFocusSoon(() => document.querySelector<HTMLElement>(`[data-f="profile:${id}"]`));
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
    <main class="browse page settings-page column profiles-manage" data-vscroll>
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
  // The profile an Add made on this page, once the core has it: later saves on the page update it, never add another.
  const [created, setCreated] = createSignal<string | null>(null);
  const id = () => params.id ?? created();
  const isNew = () => !id();
  const p = () => (id() ? s()?.profiles.find((x) => x.id === id()) : undefined);
  const mine = () => !!p() && p()?.id === s()?.active;
  const owner = () => s()?.profiles.find((x) => x.id === s()?.active)?.isOwner === true;
  const [name, setName] = createSignal("");
  const [glyph, setGlyph] = createSignal<Glyph>("letter");
  const [color, setColor] = createSignal<AvatarColor>("ember");
  const [touched, setTouched] = createSignal(false);
  const [said, setSaid] = createSignal<{ text: string; bad: boolean } | null>(null);
  // Under the Name field, where it is fixed.
  const [nameError, setNameError] = createSignal<string | null>(null);
  const [flow, setFlow] = createSignal<PinFlow | null>(null);
  // The stored PIN came from a newer version: it can only be removed (the core lets its owner do that without it).
  const [unreadable, setUnreadable] = createSignal(false);
  const [confirmDelete, setConfirmDelete] = createSignal(false);
  // A PIN taken off with one tap asks first: another profile's, or one this version cannot read.
  const [confirmPin, setConfirmPin] = createSignal<"other" | "unreadable" | null>(null);
  // The name the page was opened with (empty for a new profile), what unsaved edits are measured against.
  const [base, setBase] = createSignal("");
  let nameInput: HTMLInputElement | undefined;
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
      setBase(p()!.name);
      setGlyph(a.glyph);
      setColor(a.color ?? "ember");
      filled = true;
    }
  });
  // Removed on another device while open.
  // A leave this page makes itself (after Save, Delete, or a profile removed elsewhere) is not asked about.
  let free = false;
  const leave = (to: string) => {
    free = true;
    navigate(to, { replace: true });
  };
  let seen = false;
  createEffect(() => {
    if (p()) seen = true;
    else if (!isNew() && filled && seen) leave("/profiles/manage");
  });

  // Unsaved edits ask before any way out (a tab, Back, Escape, the browser's Back): Save, Discard or Keep editing.
  const dirty = () => (isNew() ? owner() : mine() || owner()) && (touched() || name().trim() !== base());
  const [asking, setAsking] = createSignal<{ go: (force: boolean) => void; back: boolean } | null>(null);
  useBeforeLeave((e) => {
    if (free || !(dirty() || asking())) return;
    e.preventDefault();
    // The shell marked this leave as a Back before the router stopped it: that mark must not outlive it.
    const back = takeBack();
    // A PIN keypad open over the form gives way to the question (it renders with the form).
    if (flow()) setFlow(null);
    if (!asking()) setAsking({ go: (force) => e.retry(force), back });
  });
  // A reload, a deep link or a closed tab leaves the document, which the router cannot ask about: the browser does.
  const warn = (e: BeforeUnloadEvent) => {
    if (free || !dirty()) return;
    e.preventDefault();
    e.returnValue = "";
  };
  window.addEventListener("beforeunload", warn);
  onCleanup(() => window.removeEventListener("beforeunload", warn));
  // The TV rail does not open a tab on focus alone while edits are unsaved: only Select on a tab asks.
  createEffect(() => setLeaveGuarded(dirty()));
  onCleanup(() => setLeaveGuarded(false));

  // One save at a time: a second press while the first is with the core would create a second profile.
  const [saving, setSaving] = createSignal(false);
  /** Saves the name and the picture, true once the core has them. A new profile stays on its page: the caller goes on. */
  const commit = async (): Promise<boolean> => {
    if (saving()) return false;
    const n = name().trim();
    if (!n || n.length > 30) {
      setNameError(profileProblem("badName", ""));
      nameInput?.focus();
      return false;
    }
    setNameError(null);
    const picture = glyph();
    setSaving(true);
    try {
      if (isNew()) {
        const before = new Set(s()?.profiles.map((x) => x.id));
        const after = await op<ProfilesSnapshot>("createProfile", { name: n, avatar: formatAvatar({ glyph: glyph(), color: color() }) });
        // The page is now an edit of what it made, with that as its saved state.
        setCreated(after.profiles.find((x) => !before.has(x.id) && x.name === n)?.id ?? after.profiles.find((x) => !before.has(x.id))?.id ?? null);
        setBase(n);
        if (glyph() === picture) setTouched(false);
      } else {
        const fields: Record<string, unknown> = { profile: id()!, name: n };
        if (touched()) fields.avatar = formatAvatar({ glyph: glyph(), color: color() });
        await op<ProfilesSnapshot>("updateProfile", fields);
        setBase(n);
        // A picture chosen while this was with the core is still unsaved: only the one that was sent is clean.
        if (glyph() === picture) setTouched(false);
        setSaid({ text: "Saved", bad: false });
      }
      return true;
    } catch (err) {
      setSaid({ text: tried(err).kind === "problem" ? (tried(err) as { text: string }).text : "That did not work.", bad: true });
      return false;
    } finally {
      setSaving(false);
    }
  };
  const save = async (e: Event) => {
    e.preventDefault();
    const adding = isNew();
    if ((await commit()) && adding) leave("/profiles/manage");
  };
  // The router put the page back with a second popstate (which marked a Back again), and a Back from the TV rail left
  // its "then Home" behind: staying clears both, or a later Save would open its page as a Back.
  const keepEditing = () => {
    setAsking(null);
    takeBack();
    takeAfterBack();
    // What had the focus (a keypad key, the tab) may be gone: the Name field takes it, once the question has returned it.
    setTimeout(() => (!document.activeElement || document.activeElement === document.body) && nameInput?.focus());
  };
  const goOn = () => {
    const a = asking();
    if (!a) return;
    free = true;
    if (a.back) markBack();
    setAsking(null);
    a.go(true);
  };
  const saveAndLeave = async () => {
    // A second press while the first is with the core waits for it: the question stays and the leave goes on after it.
    if (saving()) return;
    const question = asking();
    if (!question) return;
    // What goes to the core now: the leave goes on only if it is still this question and nothing was typed since.
    const sent = { name: name().trim(), picture: glyph() };
    const saved = await commit();
    if (asking() !== question) return;
    if (!saved) keepEditing();
    else if (name().trim() === sent.name && glyph() === sent.picture) goOn();
    else setAsking(null);
  };
  const removePin = async (own: boolean) => {
    setConfirmPin(null);
    try {
      await op("setPin", { profile: p()!.id, pin: null });
      if (own) setUnreadable(false);
      setSaid({ text: own ? "PIN removed" : `${p()!.name}'s PIN was removed.`, bad: false });
    } catch (err) {
      const t = tried(err);
      setSaid({ text: t.kind === "problem" ? t.text : "That did not work.", bad: true });
    }
  };
  // One delete at a time: while it is with the core, neither Delete nor Keep acts.
  const [deleting, setDeleting] = createSignal(false);
  const remove = async () => {
    if (deleting()) return;
    setDeleting(true);
    try {
      await op("deleteProfile", { profile: p()!.id });
      leave("/profiles/manage");
    } catch (err) {
      const t = tried(err);
      setSaid({ text: t.kind === "problem" ? t.text : "That did not work.", bad: true });
      setConfirmDelete(false);
    } finally {
      setDeleting(false);
    }
  };

  return (
    <main class="browse page settings-page column profiles-edit" data-vscroll>
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
              autoFocusSoon(() => document.querySelector<HTMLElement>('[data-f="pedit:pin:unreadable"]'));
            }}
            onDone={(text) => {
              setFlow(null);
              if (text) setSaid({ text, bad: false });
            }}
            onLate={(text, bad) => setSaid({ text, bad })}
          />
        }
      >
        <h1>{isNew() ? "Add profile" : mine() ? "Your profile" : `Edit ${p()?.name ?? ""}`}</h1>
        <form class="pedit" onSubmit={save}>
          <div class="pedit-preview">
            <AvatarArt avatar={formatAvatar({ glyph: glyph(), color: color() })} name={name() || "?"} size="l" />
          </div>
          <div class="pedit-name">
            <label class="field-line">
              <span class="kicker">Name</span>
              <input
                ref={nameInput}
                type="text"
                maxLength={30}
                value={name()}
                onInput={(e) => {
                  setName(e.currentTarget.value);
                  setNameError(null);
                }}
                data-f="pedit:name"
                data-autofocus
                autocomplete="off"
                aria-invalid={nameError() ? "true" : undefined}
                aria-describedby={nameError() ? "pedit-name-error" : undefined}
              />
            </label>
            <Show when={nameError()}>
              {(t) => (
                <p class="st bad" id="pedit-name-error" role="status">
                  <i class="dot" />
                  {t()}
                </p>
              )}
            </Show>
          </div>
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
          <div class="acts">
            <button class="btn primary" type="submit" data-f="pedit:save" aria-disabled={saving()}>
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

        {/* Under the Save row, where the press was made: the page is long, and the question must be in view. */}
        <Show when={asking()}>
          <Confirm label="Leave without saving?" onKeep={keepEditing}>
            <p>Your changes to this profile are not saved yet.</p>
            <div class="acts">
              <button class="btn primary" data-f="pedit:leave:save" aria-disabled={saving()} onClick={() => void saveAndLeave()}>
                Save
              </button>
              <button class="btn" data-f="pedit:leave:discard" onClick={goOn}>
                Discard
              </button>
              <button class="btn" data-f="pedit:leave:keep" data-autofocus onClick={keepEditing}>
                Keep editing
              </button>
            </div>
          </Confirm>
        </Show>

        <Show when={!isNew() && p()}>
          {(cur) => (
            <section class="pedit-pin" aria-label="PIN">
              <h2 class="kicker">PIN</h2>
              <Show when={cur().syncing}>
                <p class="note small">Waiting for this profile's lock to arrive from your other devices.</p>
              </Show>
              <Show when={!cur().syncing && mine() && unreadable()}>
                <p class="note small">{profileProblem("pinUnreadable", "")}</p>
                <Show
                  when={confirmPin() === "unreadable"}
                  fallback={
                    <div class="acts">
                      <button class="btn" data-f="pedit:pin:unreadable" onClick={() => setConfirmPin("unreadable")}>
                        Remove PIN
                      </button>
                    </div>
                  }
                >
                  <PinConfirm label="Remove your PIN?" onYes={() => void removePin(true)} onNo={() => setConfirmPin(null)} />
                </Show>
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
                  <Show
                    when={confirmPin() === "other"}
                    fallback={
                      <button class="btn" data-f="pedit:pin:clear" onClick={() => setConfirmPin("other")}>
                        Remove {cur().name}'s PIN
                      </button>
                    }
                  >
                    <PinConfirm label={`Remove ${cur().name}'s PIN?`} onYes={() => void removePin(false)} onNo={() => setConfirmPin(null)} />
                  </Show>
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
              <Confirm label={`Delete ${p()!.name}?`} busy={deleting()} onKeep={() => setConfirmDelete(false)}>
                <p>Delete {p()!.name}? Their library, progress and settings are removed from this device now and from your other devices when they next sync.</p>
                <div class="acts">
                  <button class="btn danger" data-f="pedit:delete:yes" aria-disabled={deleting()} onClick={() => !deleting() && void remove()}>
                    Delete {p()!.name}
                  </button>
                  <button class="btn" data-f="pedit:delete:no" data-autofocus aria-disabled={deleting()} onClick={() => !deleting() && setConfirmDelete(false)}>
                    Keep
                  </button>
                </div>
              </Confirm>
            </Show>
          </section>
        </Show>
      </Show>
      </Show>
    </main>
  );
}

/** A question asked as a modal: the rest of the app waits, Escape or Back keeps what is there (`onKeep`). */
function Confirm(props: { label: string; busy?: boolean; onKeep: () => void; children: JSX.Element }) {
  let root!: HTMLDivElement;
  holdModal(() => root);
  // A dialog takes the focus when it opens (WEB-05), whoever opened it: a pointer or a touch brings none to it (the
  // app's autofocus waits for keys), and the control that had it is now inert. It comes into view by itself too.
  onMount(() => {
    root.scrollIntoView({ block: "nearest" });
    root.querySelector<HTMLElement>("[data-autofocus]")?.focus({ preventScroll: true });
  });
  // A held OK repeats: only the first press counts while it is open. Backspace,
  // the app's Back, keeps what is there like Escape.
  const onKey = (e: KeyboardEvent) => {
    if (e.repeat && (e.key === "Enter" || e.key === " ")) {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    if (!isBack(e) && e.key !== "Backspace") return;
    e.preventDefault();
    e.stopPropagation();
    if (!props.busy) props.onKeep();
  };
  window.addEventListener("keydown", onKey, { capture: true });
  onCleanup(() => window.removeEventListener("keydown", onKey, { capture: true }));
  return (
    <div class="confirm" ref={root} role="alertdialog" aria-modal="true" aria-label={props.label}>
      {props.children}
    </div>
  );
}

/** Taking a PIN off with one tap: asked first, as Delete profile is. */
function PinConfirm(props: { label: string; onYes: () => void; onNo: () => void }) {
  return (
    <Confirm label={props.label} onKeep={props.onNo}>
      <p>{props.label} Anyone who uses this device can then open this profile.</p>
      <div class="acts">
        <button class="btn danger" data-f="pedit:pin:yes" onClick={props.onYes}>
          Remove PIN
        </button>
        <button class="btn" data-f="pedit:pin:no" data-autofocus onClick={props.onNo}>
          Keep
        </button>
      </div>
    </Confirm>
  );
}

/** The keypad steps for your own PIN, by the core's rules: change and remove ask the current one first. */
function PinSteps(props: { flow: PinFlow; profile: ProfileInfo; onUnreadable: () => void; onDone: (said?: string) => void; onLate: (said: string, bad: boolean) => void }) {
  const steps = props.flow === "set" ? ["new", "again"] : props.flow === "change" ? ["current", "new", "again"] : ["current"];
  const [step, setStep] = createSignal(0);
  let current: string | undefined;
  let fresh = "";
  onCleanup(() => {
    current = undefined;
    fresh = "";
  });
  // A flow that left (the app was hidden) never closes a newer one: a save it
  // sent still says how it ended, through the page.
  let alive = true;
  onCleanup(() => (alive = false));
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
      if (!alive) {
        // The flow left first: the page says the save did not go through.
        const t = tried(e);
        props.onLate(t.kind === "problem" ? t.text : t.kind === "wrong" ? "Wrong PIN: the PIN was not changed." : "The PIN was not changed.", true);
        return { kind: "ok" };
      }
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
    const text = props.flow === "remove" ? "PIN removed" : "PIN saved";
    if (alive) props.onDone(text);
    else props.onLate(text, false);
    return { kind: "ok" };
  };
  return <Keypad title={title()} onSubmit={submit} onCancel={() => props.onDone()} cancelLabel="Cancel" />;
}
