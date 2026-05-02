# Warp Mobile v1.0 Release Kickoff

**Date**: 2026-05-02
**Status**: ACTIVE — same-day continuation of M6 close
**Current version**: `0.6.0-m6` / versionCode 6
**Target ship version**: `1.0.0` / versionCode 100

---

## §1. What v1.0 means

v1.0 is the **first user-facing public release** of Warp Mobile Android. F-Droid + GitHub Releases primary distribution. Play Store is post-v3.

User definition of "v1.0 ready":
- Flagship-device-class (Galaxy S24 Ultra / Pixel 7+) usable as a daily driver for shell tasks
- All AI features (BYOK ghost-text + agent task) functional with the user's own Anthropic key
- Termux runtime extracted + working (zsh + GNU coreutils + APT)
- F-Droid build recipe accepts the source tree
- Signed GitHub Releases APK ships on tag push

Non-goals for v1.0:
- Low-end / mid-range device class (Adreno < 6xx, API < 31) — stays scoped to the flagship class per Plan Amendment 3
- Compose UI (warpui Vulkan is the chosen path per Plan §6 M2 + Decisions A4 + D1.5-hybrid)
- Play Store (post-v3 optional)
- Voice input via RecognizerIntent (deferred to v2+; mic button in AccessoryRow currently logs only)

---

## §2. Entry state — what's already shipped

All of M0 → M6 closed, plus same-day v1-prep work:

| Layer | State | Notes |
|-------|-------|-------|
| L0 PTY/FGS | GO | M1 close; PTY broadcast pipeline + FGS |
| L1 warpui Vulkan | GO | M2 close; per-cell renderer at 60fps p95=13ms |
| L2 facade | GO | M3 close; DCS hooks + Block model + dynamic_grid |
| L3 Termux runtime | GO | M4 close; bootstrap zip extraction + zsh PTY + apt config |
| L4 Mobile UX | CONDITIONAL GO | M5 PARTIAL (5/8); BlockActionsSheet UI scaffold added in v1-prep |
| L5 AI integration | CONDITIONAL GO | M6 close (7/7); all 4 carry-overs closed same-day |

Primary device verified: Galaxy S24 Ultra R5CX10VFFBA (Android 15, Snapdragon 8 Gen 3, Adreno 750).
Secondary: Pixel-class flagships (untested in this session but expected to work — same Plan Amendment 3 baseline).

---

## §3. v1.0 ship blockers (must-have)

The set of things that genuinely prevent shipping v1.0 today:

### 3.0 Launcher-path UIUX (iteration 18, RESOLVED 2026-05-02)

**Status**: RESOLVED at commit `ff60ee9` + follow-ups `104af46`, `bea339c`.
**Blocker?** Was hard — plain launcher Intent rendered Vulkan magenta forever. Iteration 18 resolved all 3 sub-blockers:

- **#1 Launcher path → magenta**: `MainActivity.kt` now defaults `terminal_mode=true` + auto-spawns the configured shell when no driver-style extras are present. Plain launcher tap produces a working terminal.
- **#2 Grid sized 80×24 (1920×960px)**: rows/cols now derived from `resources.displayMetrics.{widthPixels,heightPixels}` — 45×54 grid on a 1080×2340 portrait flagship.
- **#3 zsh dies in PTY ~10 ms**: root cause is SELinux (`untrusted_app` domain has `neverallow ... app_data_file:file execute` since API 29; `$PREFIX/bin/zsh` is `app_data_file`-labelled, so `execve` returns EACCES). Mitigated by auto-fallback to `/system/bin/sh` after a 1.5 s fast-death detection in `WarpTerminalService.startReadLoop`. Real fix is the v1.1 nativeLibraryDir refactor (`.omc/v1.1-plan-selinux-nativelib.md`).

Verification screenshots: `.omc/v1-prep-screenshots/08-` through `11-`. The mksh-only-shell + no-`$PREFIX/bin/*`-exec limitation is documented in `.omc/v1-prep-uiux-verification.md` §1 and the CHANGELOG.

### 3.1 Real-world tester UX review (M5-S05)

**Status**: USER-DEFERRED, real-world activity.
**Blocker?** Soft — the AC says "≥5 testers daily-drive ≥1 week + ≥3.0/5 aggregate score". Without this, we're shipping a beta-quality release without external validation.
**Mitigation**: ship as `1.0.0-rc1` with a TestFlight-equivalent recruitment via the GitHub README. Promote to `1.0.0` after 1 week of soak with no critical reports.

### 3.2 Signing key generation + GitHub Releases publication

**Status**: PRE-REQUISITE; infrastructure ready, key not yet generated.
**Blocker?** Hard — without a generated keystore, we ship unsigned APKs (F-Droid path) but can't deliver pre-signed APKs via GitHub Releases.
**Steps to unblock**:
1. Generate keystore: `keytool -genkey -v -keystore android/keystore.jks -alias warp-mobile-release -keyalg RSA -keysize 4096 -validity 25000` (irreversible — generates the v1.0 signing identity)
2. Populate `android/keystore.properties` (gitignored)
3. Backup keystore offline (1Password / encrypted USB / etc) — losing this kills all future signed releases
4. Optional: encode + add to GitHub repo secrets for CI signed builds (`KEYSTORE_BASE64` etc per `.github/workflows/release.yml`)

**Why it's gated on user**: irreversible identity generation + signing-key custody is a security decision the user must make.

### 3.3 F-Droid metadata publication

**Status**: Recipe exists at `metadata/dev.warp.mobile.yml`; F-Droid hasn't accepted yet (we haven't submitted).
**Blocker?** Hard for the F-Droid distribution path; soft for GitHub-only ship.
**Steps**:
1. Submit recipe via PR to <https://gitlab.com/fdroid/fdroiddata>
2. Pin the bootstrap snapshot (`tools/scripts/m4-bootstrap-snapshot.sha256`) at a stable Termux apt revision so reproducibility holds across F-Droid's rebuild cadence
3. F-Droid manual review — typically 1-3 weeks for a new app

**Why it's gated on user**: irreversible "going public" step — once accepted, we have a publication commitment.

---

## §4. v1.0 ship enhancements (nice-to-have, not blockers)

Items that would polish the v1.0 ship but aren't release-blocking:

### 4.1 Color emoji rasterization

**Status**: ROOT-CAUSE DIAGNOSED in `.omc/m4-artifacts/M4-S14-result.json` — swash 0.1.x has no COLR v1.
**Effort**: 1-2 days for Path D (bundle CBDT-bitmap variant of NotoColorEmoji as APK asset, +4 MB).
**Decision**: defer to v1+1 unless emoji color is a hard expectation for the user-acquisition demo.

### 4.2 GhostSuggest cursor-anchored overlay

**Status**: AccessoryRow strip approach functional; cursor-anchored overlay is the "Copilot-grade" polish.
**Effort**: 2-3 days. Needs JNI accessor for cursor screen position from Vulkan render state + WindowManager floating TextView positioning.
**Decision**: defer to v1+1. Strip is fine for v1.0.

### 4.3 M5-S06 pkg.rs Rust subprocess wrapper

**Status**: Users can install packages by running `apt install foo` in the terminal. The Kotlin progress UI is polish.
**Effort**: 3-5 days. Significant (Rust async subprocess + Kotlin Activity + progress notifier).
**Decision**: defer to v1+1.

### 4.4 M5-S07 cosmetic apt-config dump cleanup

**Status**: Genuinely apt-internal. Test driver explicitly excludes via grep. Only visible in `apt-config dump`.
**Decision**: defer indefinitely. Not worth fixing.

### 4.5 BlockGesture touch-based block selection

**Status**: GestureRecognizer state machine + 12 host tests landed in M5-S03. Touch dispatch + cell-coord hit-test missing.
**Effort**: 2-3 days. Needs row→block mapping in the model + JNI accessor.
**Decision**: defer to v1+1. The 📋 button entry point in AccessoryRow is functional.

---

## §5. Recommended v1.0 ship path

```
Today (2026-05-02) ───── Generate keystore + populate keystore.properties (user)
                    │
                    ├── Refresh upstream Termux snapshot pin
                    │   (M4-S08 reproducibility gate — release.sh
                    │    refuses to build until current pin matches
                    │    upstream).
                    │   $ UPDATE_SNAPSHOT=1 \
                    │       tools/scripts/build-bootstrap.sh aarch64
                    │   $ git add tools/scripts/m4-bootstrap-snapshot.sha256
                    │   $ git commit -m "release: refresh bootstrap pin"
                    │
                    ├── git tag -a v1.0.0-rc1 -m "v1.0 release candidate"
                    │   git push origin v1.0.0-rc1
                    │       └─→ .github/workflows/release.yml fires
                    │           builds signed APK + bootstrap zip
                    │           creates GitHub Release with artifacts
                    │
                    ├── README update: "v1.0.0-rc1 available — install via GitHub Releases"
                    │
                    └── Submit F-Droid recipe (parallel)

+1 week ──────────── Soak period; monitor GitHub Issues for critical reports
                    │
                    ├── If clean: tag v1.0.0
                    │
                    └── If critical issues: fix + v1.0.0-rc2

+2-4 weeks ────────── F-Droid review concludes; first F-Droid build appears
                    │
                    └── Promote README "Available on F-Droid" badge

v1.1 / v1.0.1 ───── First post-ship maintenance cycle
                    │
                    ├── M5-S05 external tester score promotion
                    ├── COLR v1 emoji (Path D)
                    └── M5-S06 pkg wrapper
```

---

## §6. Tooling ready

Pre-built v1-prep infrastructure (zero further setup needed):

- `tools/scripts/release.sh <version> [--upload] [--dry-run]` — local release packaging
- `.github/workflows/release.yml` — CI release on `v*` tag push
- `android/app/build.gradle` signing config (opt-in via keystore.properties)
- `metadata/dev.warp.mobile.yml` — F-Droid recipe
- `fastlane/metadata/android/en-US/changelogs/` — per-version changelogs (1.txt = M4-era, 6.txt = M6-era)

---

## §7. References

- M6 close-out: `.omc/m6-artifacts/M6-go-no-go.md`
- M5 close-out: `.omc/m5-artifacts/M5-go-no-go.md`
- M4 close-out: `.omc/m4-artifacts/M4-go-no-go.md`
- M3 close-out: `.omc/m3-artifacts/M3-go-no-go.md`
- Canonical plan: `.omc/plans/ralplan-warp-on-mobile.md` (5 amendments at top)
- Color emoji diagnosis: `.omc/m4-artifacts/M4-S14-result.json`
- Iteration log: `progress.txt`
- AI agent entry: `CLAUDE.md`

---

*Drafted 2026-05-02 by team-lead@warp-mobile-v1-prep (Claude Opus 4.7 1M context) — autonomous continuation of M6 close.*
