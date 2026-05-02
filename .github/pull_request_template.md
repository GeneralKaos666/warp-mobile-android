## Summary

<!-- 1-3 bullet points. What does this PR do? -->



## Why

<!-- Linked issue + 1-2 sentences on the rationale. -->

Closes #

## Test plan

<!-- Tick what you've done -->

- [ ] Unit tests added / updated (`cargo test -p <crate>`)
- [ ] Device test driver re-run on Galaxy S24 Ultra (or note why deferred)
- [ ] `cargo audit` + `cargo deny check` pass (CI will gate this anyway)
- [ ] Manually exercised the affected path on-device

## Compatibility notes

<!-- Tick all that apply (or strike through with ~~~ if N/A) -->

- [ ] Doesn't change `warp_terminal/warpui` Cargo edges (D1.5-hybrid invariant)
- [ ] No new GPL-only / proprietary-SDK deps (AGPL-3.0 compliance)
- [ ] No new permissions in AndroidManifest beyond what's already there
- [ ] APK size impact < 1 MB (or note the tradeoff)

## Carry-over impact

<!-- If this closes a known carry-over from a milestone close-out doc, list which:
     - .omc/m{1..6}-artifacts/M*-go-no-go.md §5
     - .omc/v1-release-kickoff.md §3 / §4
   If this introduces a new carry-over, add it to the next milestone's plan.
-->

---

🤖 If this PR was authored with [Claude Code](https://claude.com/claude-code), the commit message footer should still mention it. The model line is for transparency, not attribution credit.
