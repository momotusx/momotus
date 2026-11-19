/** A decorative lucide icon drawn once and cloned. lucide-solid builds every instance through prop merging, two memos
 * and a `Dynamic` per path, which a list of hundreds pays per row (Search's hits spent about half their build there on
 * Chromium 79). A glyph is the same `<svg>`, `aria-hidden`, with a fixed class: for static icons in long lists. */

import type { LucideProps } from "lucide-solid";
import { type Component, createRoot } from "solid-js";

export function glyph(icon: Component<LucideProps>): Component<{ class?: string }> {
  let drawn: SVGSVGElement | undefined;
  return (props) => {
    drawn ??= createRoot((dispose) => {
      const svg = (icon({}) as unknown as SVGSVGElement).cloneNode(true) as SVGSVGElement;
      dispose();
      return svg;
    });
    const svg = drawn.cloneNode(true) as SVGSVGElement;
    if (props.class) svg.classList.add(...props.class.split(" ").filter(Boolean));
    return svg;
  };
}
