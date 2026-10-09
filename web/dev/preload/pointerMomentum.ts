export type PointerAxis = { position: number; velocity: number };

/** An underdamped spring retains velocity when the pointer stops or reverses.
 * The analytic update gives the same response at different frame rates.
 */
export function advancePointerAxis(
  axis: PointerAxis,
  target: number,
  seconds: number,
  frequency = 5.2,
) {
  if (seconds <= 0 || !Number.isFinite(seconds)) return;
  const decay = frequency * 0.64;
  const oscillation = frequency * Math.sqrt(1 - 0.64 ** 2);
  const offset = axis.position - target;
  const envelope = Math.exp(-decay * seconds);
  const cosine = Math.cos(oscillation * seconds);
  const sine = Math.sin(oscillation * seconds);
  const nextOffset =
    envelope * (offset * cosine + ((axis.velocity + decay * offset) / oscillation) * sine);
  axis.velocity =
    envelope *
    (axis.velocity * cosine -
      ((decay * axis.velocity + frequency * frequency * offset) / oscillation) * sine);
  axis.position = target + nextOffset;
}
