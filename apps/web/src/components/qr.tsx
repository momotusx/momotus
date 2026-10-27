import { encode } from "uqr";

/** A QR code drawn as one SVG path (dark modules on a light quiet zone). The pairing code asks for
 * error correction L and a four-module margin, to be read from across a room. */
export function Qr(props: { text: string; label: string; ecc?: "L" | "M"; border?: number }) {
  const code = () => encode(props.text, { ecc: props.ecc ?? "M", border: props.border ?? 2 });
  const path = () => {
    let d = "";
    code().data.forEach((row, y) =>
      row.forEach((on, x) => {
        if (on) d += `M${x} ${y}h1v1h-1z`;
      }),
    );
    return d;
  };
  return (
    <svg class="qr" viewBox={`0 0 ${code().size} ${code().size}`} role="img" aria-label={props.label} shape-rendering="crispEdges">
      <rect width="100%" height="100%" fill="#fff" />
      <path d={path()} fill="#0F0D0B" />
    </svg>
  );
}
