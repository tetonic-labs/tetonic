/** Activity is a count, not progress or an estimate of time remaining. */
export function MapActivity({ count }: { count: number }) {
  if (!count) return null;
  return (
    <span className="pm-activity">
      <span className="pm-activity-glyph" aria-hidden="true">
        <i />
        <i />
        <i />
      </span>
      <span>{count} running</span>
    </span>
  );
}
