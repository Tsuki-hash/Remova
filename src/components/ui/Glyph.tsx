/**
 * Decorative / status glyphs (REV-UX-04).
 * Characters are always `aria-hidden` unless `label` is provided (then role=img + label).
 * Prefer pairing with real text so screen readers are not the only channel.
 */
export function Deco({
  ch,
  label,
  title,
}: {
  ch: "×" | "⋯" | "ⓘ" | "✓" | "✦" | "○";
  label?: string;
  title?: string;
}) {
  if (label) {
    return (
      <span role="img" aria-label={label} title={title || label}>
        {ch}
      </span>
    );
  }
  return (
    <span aria-hidden="true" title={title}>
      {ch}
    </span>
  );
}

/** Close-mark for icon buttons that already carry aria-label on the button. */
export function CloseGlyph() {
  return <Deco ch="×" />;
}
