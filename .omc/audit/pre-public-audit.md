# Pre-Public Audit Report (2026-05-01)

## TL;DR

- Total tracked files: 169
- Files to UNTRACK: 3 (1 broken symlink .so, 1 hardcoded-path script, 1 .gitattributes)
- .gitignore additions needed: 4 patterns
- Personal info concerns: 3 categories (hardcoded paths in 1 active script; /Users/ in historical artifact JSON files; device serials in M1-M3 result artifacts)

---

## Findings

### CRITICAL (must fix before public)

**C1. Broken symlink tracked in git — absolute path to iml1s machine**
- File: `spikes/vulkan-surface-recreate/android/app/src/main/jniLibs/arm64-v8a/libvulkan_surface_recreate.so`
- This is a broken symlink pointing to `/Users/iml1s/Documents/mine/warp_termux/spikes/vulkan-surface-recreate/target/aarch64-linux-android/release/libvulkan_surface_recreate.so`
- The symlink target path encodes another developer's macOS home directory and a dead repo path. It resolves to nothing on any other machine. Worse, `git ls-files` tracks this object and the path is visible to anyone who clones.
- Severity: CRITICAL — private dev machine path permanently in git history, causes confusing clone failures.
- Fix: `git rm --cached spikes/vulkan-surface-recreate/android/app/src/main/jniLibs/arm64-v8a/libvulkan_surface_recreate.so` then add `spikes/vulkan-surface-recreate/android/app/src/main/jniLibs/` to `.gitignore`. The spike README already documents how to rebuild the .so locally.

**C2. Hardcoded absolute path to iml1s machine in active script**
- File: `scripts/run-symlink-test.sh:20`
- Line: `ADB_PATH="/Users/iml1s/Library/Android/sdk/platform-tools/adb"`
- This is an ACTIVE test driver script (not a historical artifact), and it hardcodes a machine-specific path that will silently break on every other dev machine. Unlike the M3-era scripts that were already fixed to use `${ADB:-$(which adb)}`, this M0-era script was missed.
- Severity: CRITICAL — both path disclosure and functional breakage for any external contributor.
- Fix: Replace `ADB_PATH="/Users/iml1s/..."` with `ADB_PATH="${ADB:-$(which adb)}"` matching the pattern used in all other fixed scripts.

---

### IMPORTANT (should fix before public)

**I1. Device serials in 13 tracked result/go-no-go artifact files**
- Files containing `R5CX10VFFBA`, `RFCNC0WNT9H`, or `RFCY71LAFYE`:
  - `.omc/m1-artifacts/M1-S06-result.json`
  - `.omc/m1-artifacts/M1-S07-result.json`
  - `.omc/m1-artifacts/M1-S08-result.json`
  - `.omc/m1-artifacts/M1-S09-result.json`
  - `.omc/m1-artifacts/M1-go-no-go.md`
  - `.omc/m2-artifacts/M2-S04-result.json`
  - `.omc/m2-artifacts/M2-S05-result.json`
  - `.omc/m2-artifacts/M2-go-no-go.md`
  - `.omc/m3-artifacts/M3-S05-result.json`
  - `.omc/m3-artifacts/M3-S07-result.json`
  - `.omc/m3-artifacts/M3-S08-result.json`
  - `.omc/m3-artifacts/M3-S09-result.json`
  - `.omc/m3-artifacts/M3-go-no-go.md`
- Note: commit `82a5f7a` anonymized serials in FORWARD-LOOKING docs per project memory. These are HISTORICAL milestone evidence files.
- Assessment: These are verifier artifacts with intentional test evidence. The project memory explicitly notes the decision to "keep historical evidence intact." However serials are identifiable device info (Samsung IMEI-derived). Public exposure is a privacy concern.
- Fix options: (a) bulk `sed -i` replace all three serial strings with `<DEVICE_SERIAL>` in these 13 files and commit, OR (b) accept as historical dev evidence and add a note to README that serial numbers in artifacts refer to internal test devices.

**I2. /Users/setsuna-new/ absolute paths in 8 tracked M3/M4 artifact files**
- Files: `.omc/m3-artifacts/M3-S08-result.json`, `.omc/m3-artifacts/M3-S09-result.json`, `.omc/m3-artifacts/M3-S10-apk-listing.txt`, `.omc/m3-artifacts/M3-S11-result.json`, `.omc/m3-artifacts/M3-go-no-go.md`, `.omc/m3-artifacts/M3-kickoff-confirmed.md`, `.omc/m4-artifacts/M4-kickoff-confirmed.md`, `.omc/m2-artifacts/M2-kickoff-confirmed.md`
- These contain paths like `/Users/setsuna-new/Documents/warp-mobile-android/...` and `/Users/setsuna-new/development/android-sdk/...` encoding the current dev machine username and local SDK location.
- The M3-S08-result.json paths are file reference metadata (pointing to artifact files within the repo). The kickoff docs record machine environment for repeatability.
- Assessment: Lower privacy risk than serials (username is in the GitHub account name anyway), but confusing for external contributors. The M3-S10-apk-listing.txt explicitly contains `Archive: /Users/setsuna-new/Documents/.../app-release-unsigned.apk`.
- Fix: Redact `/Users/setsuna-new/` to `<dev-machine>/` in these 8 files, OR justify as internal-only dev evidence.

**I3. /Users/iml1s/ paths in 5 tracked M0/handoff files**
- Files: `.omc/handoffs/team-plan.md`, `.omc/m0-artifacts/M0-env-report.md`, `.omc/m2-artifacts/M2-go-no-go.md`, `.omc/m3-artifacts/M3-S11-result.json`, `.omc/m3-artifacts/M3-go-no-go.md`
- These reference the original project owner's machine (`/Users/iml1s/Documents/mine/warp_termux/`).
- The M0 env report and team-plan.md encode the initial dev environment. The go-no-go files mention the adb path fix history.
- Assessment: Historical. The iml1s username is already publicly associated with the upstream Warp fork at github.com/ImL1s/warp, so not novel disclosure. But the home directory path structure is private.
- Fix: Same strategy as I2 — redact or document-as-internal-evidence.

**I4. Spike binary .so file (475KB) tracked in git**
- File: `spikes/symlink-jnilibs/app/src/main/jniLibs/arm64-v8a/libhello_exec.so` (474,968 bytes)
- This is a compiled ARM64 binary from the M0 symlink-jniLibs spike.
- Unlike C1 (which is a broken symlink), this is an actual ELF binary committed to git. It bloats clone size, is a compiled artifact of unknown reproducibility, and sets a bad precedent.
- The spike README describes it as a "hello world" test binary to validate the symlink-jniLibs pattern.
- Fix: `git rm --cached spikes/symlink-jnilibs/app/src/main/jniLibs/arm64-v8a/libhello_exec.so` and add `spikes/symlink-jnilibs/app/src/main/jniLibs/` to `.gitignore`. Add a build step to the spike README if not already present.

**I5. Cargo.lock tracked for workspace that may be library-only**
- File: `Cargo.lock` (root workspace)
- Per Cargo convention, `Cargo.lock` should be tracked for applications/binaries and omitted for pure libraries. The root `Cargo.toml` produces `warp-mobile-android-host` crate which compiles to a `.so` (cdylib) consumed by Android — this is effectively an application artifact, so tracking Cargo.lock is correct.
- Assessment: NOT a problem — KEEP `Cargo.lock` tracked. The spike `Cargo.lock` files (`spikes/symlink-jnilibs/hello-exec/Cargo.lock`, `spikes/vulkan-surface-recreate/Cargo.lock`) are likewise correct for their binary/cdylib crates.
- No action required.

**I6. Missing CONTRIBUTING.md for public repo**
- No `CONTRIBUTING.md` is tracked. `LICENSE-AGPL` and `NOTICE.md` are present.
- For a public AGPL project, contributors need to know: how to build, how to submit patches, CLA requirements (if any), code style expectations.
- Fix: Create `CONTRIBUTING.md` with at minimum: build instructions (already in README), patch submission policy, and AGPL attribution requirements for contributions.

**I7. No SECURITY.md for vulnerability reporting**
- No `SECURITY.md` tracked. GitHub recommends this for all public repos.
- Without it, security reporters have no formal channel — they may file public issues instead.
- Fix: Create minimal `SECURITY.md` with a responsible disclosure email (or GitHub private vulnerability reporting toggle).

---

### NICE-TO-HAVE (can defer)

**N1. Two gradle-wrapper.jar binaries tracked**
- Files: `android/gradle/wrapper/gradle-wrapper.jar`, `spikes/symlink-jnilibs/gradle/wrapper/gradle-wrapper.jar`
- These are standard Gradle convention — the wrapper jar is intentionally committed so contributors can run `./gradlew` without pre-installing Gradle. Total size is ~65KB each. This is normal and widely accepted in Android projects.
- No action required.

**N2. Large M3 logcat evidence files (~1MB total)**
- Files: `.omc/m3-artifacts/M3-S08-logcat-evidence.txt` (963KB), `.omc/m3-artifacts/M3-S09-logcat-evidence.txt` (364KB), `.omc/m3-artifacts/M3-S09-logcat-gesture.txt` (285KB)
- No device serials found in these files. They are raw logcat dumps used as milestone verifier evidence.
- These bloat clone size but contain no PII beyond the dev package name `dev.warp.mobile`. The M3 go-no-go decision depends on them as evidence artifacts.
- Option: Move to a GitHub Release attachment and replace with summary references. Not blocking for public launch.

**N3. `node_modules/` pattern absent from .gitignore**
- The `.gitignore` does not include `node_modules/`. No `node_modules/` directory is currently tracked. Given this is a Rust+Android project with no JS tooling, the risk is low.
- Defensive addition recommended anyway.

**N4. `.omc/m2-artifacts/M2-S08-screenshot.png` binary tracked**
- File size: ~105KB (within the 100KB grep threshold). Screenshot of M2-S08 render. Intentional artifact evidence.
- No action required.

**N5. API key scan: all hits are false positives**
- The grep for `sk-`, `ghp_`, `gho_`, `AIza`, `AKIA`, `xox[bps]-` across tracked files produced hits only for:
  - `sk-` → "task-graph", "task-restart-under-stress" (build.gradle, rotation test script, progress.txt)
  - `AIza` → zero matches
  - `gho_` → zero matches
  - All other patterns → zero matches
- The M3-S08-logcat hit for `sk-` was `TaskLaunchParamsModifier` log lines — not an API key.
- NO actual API keys or tokens found in tracked files.

**N6. GitHub Actions workflow references ImL1s/termux-packages fork**
- File: `.github/workflows/build-bootstrap.yml`
- The workflow checks out `ImL1s/termux-packages` by hardcoded repository name. When the project goes public, this fork reference is visible. It's functional but couples the CI to a specific fork.
- Not a security concern. Defer until the fork ownership is clarified or transferred.

---

## .gitignore patches recommended

```diff
# Add to the "Spike build outputs" section:
+ spikes/symlink-jnilibs/app/src/main/jniLibs/
+ spikes/vulkan-surface-recreate/android/app/src/main/jniLibs/

# Add to the defensive section (after .DS_Store block):
+ node_modules/

# Optional: add to protect against accidental future .claude/ re-tracking
# (already in .gitignore as ".claude/" — confirmed present)
```

---

## Files to git rm --cached (untrack but keep on disk)

```bash
# C1: Broken symlink with private path
git rm --cached "spikes/vulkan-surface-recreate/android/app/src/main/jniLibs/arm64-v8a/libvulkan_surface_recreate.so"

# I4: Compiled spike binary
git rm --cached "spikes/symlink-jnilibs/app/src/main/jniLibs/arm64-v8a/libhello_exec.so"
```

## Active script fix required (C2)

```bash
# In scripts/run-symlink-test.sh, line 20:
# BEFORE:
ADB_PATH="/Users/iml1s/Library/Android/sdk/platform-tools/adb"
# AFTER:
ADB_PATH="${ADB:-$(which adb)}"
```

---

## Summary table

| Finding | Severity | Category | Action |
|---------|----------|----------|--------|
| C1: Broken symlink → /Users/iml1s/ machine path | CRITICAL | Path disclosure + broken artifact | `git rm --cached` + gitignore |
| C2: `scripts/run-symlink-test.sh:20` hardcoded path | CRITICAL | Path disclosure + functional break | Edit script |
| I1: Device serials in 13 M1-M3 artifact files | IMPORTANT | Privacy | Redact or document-as-evidence |
| I2: /Users/setsuna-new/ in 8 M3/M4 artifact files | IMPORTANT | Path disclosure | Redact or document-as-evidence |
| I3: /Users/iml1s/ in 5 M0/handoff files | IMPORTANT | Path disclosure | Redact or document-as-evidence |
| I4: Compiled ARM64 .so in spike (475KB) | IMPORTANT | Binary artifact | `git rm --cached` + gitignore |
| I5: Cargo.lock tracking | — | N/A | Correct — keep |
| I6: No CONTRIBUTING.md | IMPORTANT | Docs | Create file |
| I7: No SECURITY.md | IMPORTANT | Docs | Create file |
| N1: gradle-wrapper.jar | N/A | Normal Android practice | Keep |
| N2: Large logcat files | NICE-TO-HAVE | Clone bloat | Defer |
| N3: No node_modules/ in gitignore | NICE-TO-HAVE | Defensive | Add pattern |
| N4: M2-S08-screenshot.png | N/A | Intentional evidence | Keep |
| N5: API key scan | CLEAR | No secrets found | No action |
| N6: Workflow → ImL1s/termux-packages | NICE-TO-HAVE | Fork coupling | Defer |

---

## Final verdict

**NEEDS-CLEANUP-FIRST**

Two CRITICAL items must be resolved before going public:
1. Remove the broken `/Users/iml1s/` symlink (C1) — it leaks a private absolute path and causes confusing clone behavior.
2. Fix `scripts/run-symlink-test.sh:20` hardcoded ADB path (C2) — active script that breaks for any external contributor.

Four IMPORTANT items are strongly recommended (I1 device serials, I2-I3 /Users/ paths in artifacts, I4 compiled binary, I6-I7 missing community docs). These do not block a technical launch but matter for a clean AGPL public release.

No API keys or tokens found. LICENSE-AGPL and NOTICE.md are present. .gitignore is comprehensive and correctly covers `.cargo/config.toml`, `warp-src/`, `termux-packages/`, `.omc/state/`, and `.claude/`.
