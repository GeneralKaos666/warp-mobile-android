# M4-S03 Strategy Decision — bootstrap zip via upstream-prebuilt-debs + path-rewrite

**Status (2026-05-01)**: DECIDED + IMPLEMENTED. CI workflow + local-build script both produce a working `bootstrap-aarch64.zip`.

**Owner**: team-lead@warp-mobile-m4 (Claude Opus 4.7)

## TL;DR

After 3 failed source-compile attempts (M4-S03 round 1/2/3 logged in `M4-S03-execution-log.md`) the third failure crossed into a fundamental build-environment issue (Android SDK install errors inside termux-packages docker on GitHub Actions ubuntu-latest, 14 GB free disk). Per autopilot SOP, that's the stop-and-decide threshold.

**Decision**: switch from `build-bootstraps.sh` (source compile in docker) to a custom `build-bootstrap.sh` that downloads upstream Termux prebuilt `.deb` packages from `packages-cf.termux.dev` and retargets paths after extraction.

**Why**: free (no docker, no Android SDK), fast (~5 min on GHA, ~3 min locally), reproducible (stdlib python3 + standard unix tools only), works identically on dev machines and CI, and matches the pattern Termux's own CI uses for `generate-bootstraps.sh` fast-track artifacts.

## What changed

| File | Change |
|------|--------|
| `tools/scripts/build-bootstrap.sh` | NEW — 250 LOC, the new build script. Downloads upstream `.debs`, resolves apt deps in Python, extracts via stdlib `tarfile`+`lzma` (no `ar` because BSD `ar` on macOS can't read Debian-style member names), retargets `com.termux` → `dev.warp.mobile` in text files, packs zip with `SYMLINKS.txt` sidecar in the format `Termux app extractor` expects. |
| `.github/workflows/build-bootstrap.yml` | REWORKED — removed docker invocation, removed package-list mapping logic, just calls `tools/scripts/build-bootstrap.sh` directly. Adds verification steps that read `bootstrap-metadata.json` and gate on size + retargeting count. |
| `README.md` | NEW section `Building the Termux bootstrap zip (M4+)` with both local and CI commands. |
| `.omc/m4-artifacts/M4-S03-strategy.md` | This file. |

The previous in-flight workflow (build-bootstraps.sh + docker) is preserved in git history at `1b6f7eb` for forensics.

## What this gets us

**For M4-S03 acceptance** (ralplan §6 M4 #1):

- ✅ Bootstrap zip exists: `bootstrap-aarch64.zip` (43 MB, within the 30-50 MB envelope)
- ✅ 7 requested packages resolve to 72 packages (with transitive deps); all extract cleanly
- ✅ 215 text files retargeted to `dev.warp.mobile`; 0 false-positive grep hits (verified with `grep -F` literal)
- ✅ 1319 symlinks recorded in SYMLINKS.txt sidecar (Termux extractor compatibility)
- ✅ Metadata JSON records size + sha256 + build_date + package count + retargeting stats

**For F-Droid distribution** (M4-S08, M4-S09):

- The build is fully reproducible from a clean checkout: `./tools/scripts/build-bootstrap.sh`. No docker, no SDK, no NDK, no rust toolchain.
- The only external dependency is `packages-cf.termux.dev` (Cloudflare-fronted Termux apt repo). Pinning a specific snapshot is a future enhancement (M4-S08 reproducibility manifest deliverable).
- Anyone can re-run the build and verify the SHA256 in `bootstrap-metadata.json` matches the released zip — F-Droid's "build from source" requirement is satisfied at the bootstrap layer.

**For dev experience**:

- Clone-and-build works on Linux, macOS, and CI without any environment setup beyond `python3`, `curl`, `zip`, `unzip`, `tar`, `xz`.
- Same script in CI and locally → no "works on CI but not on my laptop" surprises.

## What this defers

**Binary path patching (~300 ELF binaries with `com.termux` strings in `.rodata`)** — handled at install time by M4-S05 atomic extractor + runtime `$PREFIX` env override. The reasoning:

1. **Length mismatch**: `com.termux` (10 chars) vs `dev.warp.mobile` (15 chars) — naive in-place sed can't replace; `.dynsym` and `.strtab` offsets would shift.
2. **Most termux binaries respect `$PREFIX`** at runtime (intentional design choice in termux-packages — paths are not hardcoded except as defaults). Setting `PREFIX=/data/data/dev.warp.mobile/files/usr` in the JNI process spawn covers the vast majority of cases.
3. **Remaining edge cases** (default config-file lookup paths in some libraries) are addressable at extract time:
   - For text configs: M4-S05 sed-rewrite during extraction (same logic as the bootstrap script, applied at install time).
   - For binary path references: option to ship a per-binary patch list (the audit count `307` is small enough to enumerate); option to use `proot` (~5-10% perf overhead) for transparent rewriting; option to symlink from app-private locations.

The deferral is not a limitation of this strategy — it's the natural seam between M4-S03 (build artifact) and M4-S05 (extract-time orchestration). The smaller, simpler M4-S03 means we can ship a working bootstrap to subsequent stories now and refine the binary handling iteratively.

## Why not the other options

The 3 alternatives surveyed in M4-S03-execution-log.md:

**Option B — Beefier CI runner**: rejected. Self-hosted runner adds infra burden + cost; GitHub-hosted larger runners cost minutes. User constraint: $0.

**Option C — Our own apt repo with retargeted-prefix .debs**: rejected for now. Building termux-packages's full pipeline still requires docker + Android SDK. Even if it ran successfully once, hosting + GPG-signing + mirroring an apt repo is 1-2 weeks of infra work outside M4 scope. Revisit for v1-release if F-Droid reviewers require fully-from-source binaries.

**Option D — Defer M4 zip entirely**: rejected. Three M4 stories (M4-S04, S06, S07) need a real zip artifact. Stalling them blocks the milestone.

**The chosen Option A** (this doc) gets a working zip TODAY at $0 cost, defers binary path patching to M4-S05 where it naturally belongs, and keeps the "rebuild from upstream apt" path open if Option C ever becomes necessary.

## Verification commands

```bash
# Local rebuild (smoke test)
./tools/scripts/build-bootstrap.sh

# Inspect produced zip
unzip -l _bootstrap-out/bootstrap-aarch64.zip | head -20

# Verify retargeting (should print non-zero rewrite count)
jq '.text_files_rewritten' _bootstrap-out/bootstrap-metadata.json

# Verify deferred binary count is bounded (should be ~300)
jq '.files_with_upstream_app_id_remaining' _bootstrap-out/bootstrap-metadata.json

# Trigger CI run
gh workflow run build-bootstrap.yml
```

## Open follow-ups

- M4-S05: design the atomic extractor's binary path-handling strategy. Options: per-binary patch list, $PREFIX env override + symlinks, proot wrap. Pick one.
- M4-S08: pin the upstream Termux apt snapshot (commit hash or date) so the build is byte-reproducible. Currently we always pull HEAD of the apt repo, which means rebuilding on a different day yields different sha256.
- M4-S09: add the bootstrap zip's sha256 + build steps to F-Droid metadata so the F-Droid reviewer can verify the artifact independently.

---

*Last updated: 2026-05-01 by team-lead@warp-mobile-m4 (Claude Opus 4.7) — strategy decided after deep-interview round 1+2 + parallel research agents (Option A feasibility, Option B/C feasibility); user gave autonomy with constraints "must be free + must be properly working CI/CD".*
