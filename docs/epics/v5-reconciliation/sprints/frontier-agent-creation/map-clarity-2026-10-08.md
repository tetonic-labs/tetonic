# Work-map clarity and activity — October 8, 2026

## Experience

Preserve the existing map, inspector, portraits, composer and brand. Make the distinction between an effort, its contributions and its coordination legible without opening every record.

- Portfolio cards emphasize effort names, running counts, recorded completion counts and review needs. A small segmented strip shows the contribution states; it is not time-based progress. The worst unresolved state remains visible even when other contributions are running.
- Related efforts use the actual team name. Wider groups use up to six columns, replacing the preceding pass's four-column maximum. At smaller zoom levels, duplicate kind labels and secondary detail yield to names and state.
- Within an effort, dependency layers run left to right. Contributions with no recorded ordering share a band. Subsequent bands follow the real dependency links. Terminal coordination has its own labeled band, and earlier participants without a visible assignment have an “Also involved” area.
- Dependency routes use clear direct corridors or exterior channels. Missing/cyclic links are surfaced for review. The layout does not manufacture dependencies or imply that parallel contributions execute sequentially.
- A three-bar activity cue accompanies recorded running counts. It stops when the engine is disconnected, the document is hidden, the operator pauses motion, or reduced motion is enabled. Static status remains readable. The cue indicates activity, not model-token throughput, estimated progress or a successful outcome.
- Fix camera restoration between maps with different world dimensions: a scope switch must not be treated as growth of the previous graph. Narrow views initially focus a readable card; Fit still shows the full map.

## Implementation boundary

`teamWorkspace` projects optional presentation roles/kinds from existing plan links. `projectLayout` owns grouping and routing. `ProjectMapPortfolio` and `MapActivity` are focused presentation components. The map memoizes layouts across camera animation frames. The shared `workSignals` logic continues to own status interpretation.

No engine execution, persistence, permissions, model calls or new orchestration path was added. The map still reads the authorized connected-workspace snapshot. It does not manufacture MCP destinations when the API does not report them.

## Verification

- Full web suite: 190 tests across 29 files passed; an additional participant-placement test subsequently passed in the 11-test focused layout suite.
- Regression coverage includes scope-switch framing, running work alongside blocked work, pause/disconnection/visibility/reduced motion, plan roles, missing dependencies, and an 18-contribution / 36-edge graph without routes crossing work cards.
- TypeScript, production build, architecture and quality gates passed. The existing Vite bundle-size warning remains.
- Browser checked with the isolated 12-effort fixture and existing engine records. Verified computed CSS animation stops on Pause. Reviewed overview and contribution views; inspected the live narrow view. No work was submitted to an inference provider or inserted into the engine.

## Remaining scale limits

This improves the current overview; it does not provide unlimited information density. Hundreds of efforts still need aggregate area summaries, history pagination and virtualization. Very long work titles are clamped to two lines with the full title available on hover and through the inspector. Layout avoids node/edge collisions but does not promise zero edge/edge crossings for dense arbitrary graphs.
