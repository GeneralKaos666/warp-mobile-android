# v1.0 Release Checklist

Tick-list for the day you decide to cut a v1.0 (or any future signed release). Walk top-to-bottom; most steps are 30 seconds. The few that aren't are flagged ⏱.

For background, see [`.omc/v1-release-kickoff.md`](v1-release-kickoff.md).

---

## §1. Pre-flight (10 min)

- [ ] On `main`, working tree clean: `git status` → "nothing to commit"
- [ ] CI green on `main` HEAD: `gh run list -L 1 --workflow=test.yml` → `✓ completed:success`
- [ ] No critical issues in past week: `gh issue list --state open --label "release-blocker"` → empty
- [ ] Repo is in sync with origin: `git pull --ff-only origin main`
- [ ] Pull the latest crate advisory DB: `cargo audit` → exit 0 (current 276 deps)

## §2. Generate / verify keystore (one-time setup, then verify)

If this is the first signed release:
- [ ] ⏱ Generate the keystore (irreversible identity decision):
  ```bash
  keytool -genkey -v \
    -keystore android/keystore.jks \
    -alias warp-mobile-release \
    -keyalg RSA -keysize 4096 -validity 25000 \
    -dname "CN=ImL1s, OU=Warp Mobile, O=Personal, L=, S=, C="
  ```
- [ ] ⏱ Populate `android/keystore.properties` (gitignored — never commits):
  ```
  storeFile=keystore.jks
  storePassword=<chosen-password>
  keyAlias=warp-mobile-release
  keyPassword=<chosen-password>
  ```
- [ ] ⏱ Backup the keystore offline: 1Password / encrypted USB / secure-cloud-vault. **Losing this kills all future signed releases for this app**.
- [ ] (Optional, for CI signed builds) Add to GitHub repo secrets:
  - `KEYSTORE_BASE64` = `base64 -i android/keystore.jks` output
  - `KEYSTORE_PASSWORD`, `KEY_ALIAS`, `KEY_PASSWORD`

For subsequent signed releases:
- [ ] `ls -la android/keystore.jks android/keystore.properties` → both present
- [ ] (If using CI signing) `gh secret list` → KEYSTORE_BASE64 + 3 others present

## §3. Refresh upstream Termux pin (if needed)

The M4-S08 reproducibility gate checks the upstream Termux apt snapshot hash on every build. If upstream drifted since the last release, you must accept the new snapshot:

- [ ] Try a dry-run build to detect drift: `./tools/scripts/build-bootstrap.sh aarch64 tools/scripts/m4-bootstrap-packages.txt /tmp/_release-test`
  - If drift detected → message says `[!] Upstream Packages snapshot drift detected`. Continue below.
  - If clean → skip to §4.
- [ ] ⏱ Refresh: `UPDATE_SNAPSHOT=1 ./tools/scripts/build-bootstrap.sh aarch64 tools/scripts/m4-bootstrap-packages.txt /tmp/_release-test`
- [ ] Commit: `git commit tools/scripts/m4-bootstrap-snapshot.sha256 -m "release: refresh bootstrap pin"`
- [ ] Push: `git push origin main`

## §4. Bump version

- [ ] Decide the version: `<major>.<minor>.<patch>` for v1+ (semver), or `<major>.<milestone>.<patch>-m<N>` pre-v1.
- [ ] Update `android/app/build.gradle`:
  ```
  versionCode <bump-by-1>
  versionName "<new-version>"
  ```
- [ ] Update `metadata/dev.warp.mobile.yml`:
  ```
  Builds:
    - versionName: <new-version>
      versionCode: <bump-by-1>
  CurrentVersion: <new-version>
  CurrentVersionCode: <bump-by-1>
  ```
- [ ] Add a changelog file `fastlane/metadata/android/en-US/changelogs/<versionCode>.txt` (~100 words user-facing).
- [ ] Update `CHANGELOG.md` `[Unreleased]` → `[<new-version>] — <date>` heading.
- [ ] Commit: `git commit -am "release: bump to v<new-version>"`
- [ ] Push: `git push origin main` — wait for CI green before continuing.

## §5. Local dry-run (catches anything CI missed)

- [ ] `./tools/scripts/release.sh <new-version> --dry-run`
  - Builds APK + bootstrap zip
  - Generates SHA256SUMS + RELEASE_NOTES.md at `dist/<version>/`
  - Does NOT publish
- [ ] Inspect `dist/<version>/RELEASE_NOTES.md` — looks right?
- [ ] Inspect APK size: `ls -lh dist/<version>/warp-mobile-<version>.apk` — within ±5% of last release?
- [ ] (Optional) Install on a test device: `adb install -r dist/<version>/warp-mobile-<version>.apk`. Smoke-test the AI flow + a handful of shell commands.

## §6. Tag + push

- [ ] Create the tag: `git tag -a v<new-version> -m "<one-line summary>"`
- [ ] Push the tag: `git push origin v<new-version>`
  - This fires `.github/workflows/release.yml` automatically.
  - Watch: `gh run watch` (or `gh run list -L 1 --workflow=release.yml`).

## §7. Verify GitHub Release

- [ ] CI release run completed green: ~5-7 min from tag push.
- [ ] Visit `https://github.com/ImL1s/warp-mobile-android/releases/tag/v<new-version>` — release exists with:
  - `warp-mobile-<version>.apk` (signed if keystore secrets are set; else unsigned)
  - `bootstrap-aarch64-<version>.zip`
  - `SHA256SUMS`
  - Body: auto-generated from changelogs/<versionCode>.txt + git log
- [ ] Verify the SHA256SUMS file:
  ```bash
  cd /tmp && curl -OL <release-url>/SHA256SUMS \
                   <release-url>/warp-mobile-<version>.apk \
                   <release-url>/bootstrap-aarch64-<version>.zip
  shasum -a 256 -c SHA256SUMS    # → both OK
  ```
- [ ] If signed: verify with `apksigner` from Android SDK build-tools:
  ```bash
  $ANDROID_HOME/build-tools/<version>/apksigner verify \
    --print-certs warp-mobile-<version>.apk
  ```

## §8. F-Droid (only on first ship; subsequent releases auto-pick-up via the recipe)

First ship to F-Droid:
- [ ] Open the F-Droid Data PR template at <https://gitlab.com/fdroid/fdroiddata>
- [ ] Submit `metadata/dev.warp.mobile.yml` (already prepared in this repo).
- [ ] ⏱ F-Droid maintainer review (typically 1-3 weeks for new apps).
- [ ] Once accepted, F-Droid CI rebuilds at the pinned snapshot. Verify reproducibility — the published APK's SHA256 should match the byte-identical local rebuild result.

Subsequent releases:
- [ ] F-Droid auto-picks up new tags matching `metadata/dev.warp.mobile.yml`'s `AutoUpdateMode`. Currently `AutoUpdateMode: None` (manual versioning); switch to `Tags` after v1.0 to automate.

## §9. Post-release

- [ ] README badge updated: bump status line if a milestone changed
- [ ] Open a follow-up issue if any post-release item carries:
  - bug found during the §5 smoke
  - docs out-of-sync
  - F-Droid review feedback
- [ ] If this was a release-candidate (e.g. `v1.0.0-rc1`), schedule the soak window:
  ```
  ScheduleWakeup +7 days "Tag v1.0.0 if no critical issues filed"
  ```
- [ ] Announce (channels TBD per user preference): GitHub Release link, F-Droid link once accepted.

---

## Rollback (if something's wrong post-tag)

If you tagged too early:
- [ ] Delete the tag locally + remote: `git tag -d v<version>; git push origin :refs/tags/v<version>`
- [ ] Delete the GitHub Release: `gh release delete v<version> --yes`
- [ ] Fix the issue
- [ ] Re-tag (same version) + push

If users have already pulled the broken release:
- [ ] DO NOT re-tag the same version. Cut a `v<version>+1` (e.g. `v1.0.1`).
- [ ] Add a deprecation note to the broken release's body.
- [ ] Keep the broken artifact downloadable (don't delete) — some users may have build pipelines pinning to it.

---

*Last updated: 2026-05-02. If you change the release pipeline, update this checklist + `tools/scripts/release.sh` together.*
