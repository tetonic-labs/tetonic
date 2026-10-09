const REGIONS = [
  { name: 'swell', x: 0, y: 0, spread: 0, fullness: 0 },
  { name: 'ripple', x: -0.72, y: -0.64, spread: 0.85, fullness: 0.24 },
  { name: 'bloom', x: 0.68, y: -0.6, spread: 1, fullness: 1 },
  { name: 'braid', x: -0.64, y: 0.65, spread: 0.4, fullness: 0.76 },
  { name: 'shoal', x: 0.72, y: 0.62, spread: 0.08, fullness: 0.08 },
] as const;

type RegionName = (typeof REGIONS)[number]['name'];
export type FlockStateMap = {
  weights: Record<RegionName, number>;
  spread: number;
  fullness: number;
};

/** Local radial fields blend whole states in two dimensions. Neither axis is
 * assigned an effect: the same horizontal move has a different result at each y.
 */
export function sampleStateMap(horizontal: number, vertical: number): FlockStateMap {
  const x = Math.max(-1, Math.min(1, horizontal));
  const y = Math.max(-1, Math.min(1, vertical));
  const weights = { swell: 0, ripple: 0, bloom: 0, braid: 0, shoal: 0 };
  let total = 0;
  for (const region of REGIONS) {
    const weight = Math.exp(-((x - region.x) ** 2 + (y - region.y) ** 2) / 0.24);
    weights[region.name] = weight;
    total += weight;
  }
  let spread = 0;
  let fullness = 0;
  for (const region of REGIONS) {
    weights[region.name] /= total;
    spread += weights[region.name] * region.spread;
    fullness += weights[region.name] * region.fullness;
  }
  return { weights, spread, fullness };
}
