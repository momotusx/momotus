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
import { A } from "@solidjs/router";
import { type Component, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { profiles } from "../core/profiles";
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

/** The active profile's avatar, opening the picker: at the end of the TV tab bar and the desktop
 * header, and at the top right of the phone's Home. */
export function ProfileButton() {
  const me = () => profiles()?.profiles.find((p) => p.id === profiles()?.active);
  return (
    <Show when={me()}>
      {(p) => (
        <A href="/profiles" class="nav pavatar" data-f="nav:/profiles" aria-label={`Profile: ${p().name}, switch profile`}>
          <AvatarArt avatar={p().avatar} name={p().name} size="s" />
        </A>
      )}
    </Show>
  );
}
