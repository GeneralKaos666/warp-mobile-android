#!/usr/bin/env bash
# build-bootstrap.sh — M4-S03 bootstrap zip builder for warp-mobile-android.
#
# Strategy (chosen per Plan Amendment 6 / .omc/m4-artifacts/M4-S03-strategy.md):
#   - Download upstream Termux prebuilt .deb packages from packages-cf.termux.dev
#     (the same source termux's own CI uses).
#   - Extract them into a staging rootfs (still under /data/data/com.termux/files/usr).
#   - Retarget paths to /data/data/dev.warp.mobile/files/usr across THREE surfaces:
#       1. Rename the on-disk directory tree.
#       2. Sed-rewrite text files (shebangs, configs, scripts).
#       3. patchelf --set-rpath rewrites DT_RUNPATH on every ELF binary
#          (without this, the dynamic linker can't resolve libs unless
#          LD_LIBRARY_PATH is set at every spawn — Codex M4-S03 round-4 fix).
#       4. Symlink targets pointing at /data/data/com.termux/... rewritten
#          to /data/data/dev.warp.mobile/... in SYMLINKS.txt sidecar.
#   - Pack the result into bootstrap-<arch>.zip in the format the Termux app
#     extractor expects (relative paths, SYMLINKS.txt sidecar).
#
# Why this script exists:
#   - Free + fast: runs on GitHub Actions ubuntu-latest in ~2 min and on any
#     dev laptop with bash + python3 + curl + zip + patchelf.
#   - Avoids termux-packages's docker source-compile path which hits an
#     Android SDK install bug inside the docker container on GHA's 14 GB-disk
#     runners (.omc/m4-artifacts/M4-S03-execution-log.md run 3).
#   - Clone-and-build friendly: no Android SDK, no gradle, no rust toolchain —
#     just stock unix tools + python3 + patchelf.
#
# What this is NOT (yet):
#   - Byte-reproducible: the script always pulls HEAD of the upstream Termux
#     apt repo, so two builds on different days yield different sha256.
#     M4-S08 deliverable: pin the apt snapshot for reproducible rebuilds.
#   - Complete binary retargeting: 116 residual com.termux strings remain
#     in compile-time defaults across zsh module_path, git libexec-path,
#     OpenSSL CA cert path, terminfo path, locale path, and dpkg/apt
#     internal fallbacks. These are runtime-overridable: zsh-specific
#     paths require shell-array assignment in $ZDOTDIR/.zshenv (zsh 5.9
#     ignores MODULE_PATH env var and reinitializes from compile-time
#     default); the rest take standard env vars (GIT_EXEC_PATH,
#     SSL_CERT_FILE, TERMINFO, LOCPATH, HOME). M4-S06 + M4-S07
#     deliverable.
#
# Usage:
#   ./tools/scripts/build-bootstrap.sh [arch] [package_list] [output_dir]
#
# Defaults:
#   arch          = aarch64
#   package_list  = tools/scripts/m4-bootstrap-packages.txt
#   output_dir    = $PWD
#
# Output:
#   <output_dir>/bootstrap-<arch>.zip
#   <output_dir>/bootstrap-metadata.json

set -euo pipefail

ARCH="${1:-aarch64}"
PKG_LIST_FILE="${2:-tools/scripts/m4-bootstrap-packages.txt}"
OUT_DIR="${3:-$PWD}"

# Normalize OUT_DIR to an absolute path. Step 6 cd's into the staging tree
# before invoking zip, so a relative OUT_DIR would resolve against the
# wrong cwd and `zip -@` would fail with "Could not create output file".
mkdir -p "$OUT_DIR"
OUT_DIR=$(cd "$OUT_DIR" && pwd)

REPO_BASE_URL="${REPO_BASE_URL:-https://packages-cf.termux.dev/apt/termux-main}"
WARP_APP_ID="${WARP_APP_ID:-dev.warp.mobile}"
UPSTREAM_APP_ID="com.termux"

# Accept aarch64|arm|i686|x86_64 (matches termux's arch list).
case "$ARCH" in
    aarch64|arm|i686|x86_64) ;;
    *) echo "[!] Unsupported arch: $ARCH (expected aarch64|arm|i686|x86_64)" >&2; exit 1 ;;
esac

# Tooling sanity check — fail fast with a useful message instead of a stack
# trace 30 lines into the script. Note: we deliberately do NOT depend on `ar`
# because macOS BSD ar is incompatible with Debian-style ar archives (trailing
# slashes in member names break both `ar -p` and `ar -x`). We use python3 +
# tarfile/lzma instead, which works identically on Linux and macOS.
#
# patchelf is a small standalone utility (Linux: apt install patchelf;
# macOS: brew install patchelf) used to rewrite the DT_RUNPATH entry on
# every ELF binary so they search /data/data/dev.warp.mobile/files/usr/lib
# at load time instead of the upstream com.termux path. Codex M4-S03 round-4
# blocking finding: without this, zsh/git/apt fail to launch on device
# unless LD_LIBRARY_PATH is set explicitly at every spawn.
for cmd in curl python3 zip unzip tar xz find sed grep awk file sha256sum patchelf; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "[!] Required command not found: $cmd" >&2
        if [ "$cmd" = "patchelf" ]; then
            echo "    Install via:" >&2
            echo "      Linux:  sudo apt install patchelf" >&2
            echo "      macOS:  brew install patchelf" >&2
        else
            echo "    Install via your package manager (apt/brew/pacman)." >&2
        fi
        exit 1
    fi
done

WORK=$(mktemp -d -t warp-bootstrap.XXXXXX)
trap 'rm -rf "$WORK"' EXIT

echo "[*] Build bootstrap-$ARCH.zip"
echo "    Repo:        $REPO_BASE_URL"
echo "    Upstream ID: $UPSTREAM_APP_ID"
echo "    Target ID:   $WARP_APP_ID"
echo "    Work dir:    $WORK"
echo "    Output dir:  $OUT_DIR"
echo

mkdir -p "$WORK/debs" "$WORK/rootfs" "$OUT_DIR"

# ─── 1. Download package indices ──────────────────────────────────────────────
echo "[1/6] Downloading package indices..."
curl -fsSL "$REPO_BASE_URL/dists/stable/main/binary-$ARCH/Packages" \
    -o "$WORK/Packages.$ARCH"
# binary-all may not exist for all repos; tolerate 404.
curl -fsSL "$REPO_BASE_URL/dists/stable/main/binary-all/Packages" \
    -o "$WORK/Packages.all" 2>/dev/null || : > "$WORK/Packages.all"

# M4-S08 reproducibility: pin the upstream Packages snapshot. Once the snapshot
# .sha256 file exists, every build verifies the downloaded Packages file matches
# (transitively pinning all .deb shas because the .debs' shas are listed inside
# Packages and verified per-download in step 3). To bump the pin, run with
# UPDATE_SNAPSHOT=1 — the pin file is rewritten with the current upstream sha.
SNAPSHOT_PIN_FILE="${SNAPSHOT_PIN_FILE:-tools/scripts/m4-bootstrap-snapshot.sha256}"
# Compute by sha256-ing the file CONTENTS, not by chaining sha256sum which
# would include the mktemp path in the inner hash and yield non-deterministic
# results across runs.
ACTUAL_SNAPSHOT_SHA=$(cat "$WORK/Packages.$ARCH" "$WORK/Packages.all" | sha256sum | awk '{print $1}')
if [ "${UPDATE_SNAPSHOT:-0}" = "1" ]; then
    mkdir -p "$(dirname "$SNAPSHOT_PIN_FILE")"
    cat > "$SNAPSHOT_PIN_FILE" <<EOF
# M4-S08 reproducibility pin — sha256 of (Packages.${ARCH} || Packages.all)
# concatenated then re-hashed. Bump via UPDATE_SNAPSHOT=1 ./build-bootstrap.sh.
# Drift = upstream Termux apt repo changed; either bump pin (and rebuild) or
# the build fails with a clear message.
$ACTUAL_SNAPSHOT_SHA
EOF
    echo "    -> [UPDATE_SNAPSHOT=1] wrote new pin to $SNAPSHOT_PIN_FILE"
elif [ -f "$SNAPSHOT_PIN_FILE" ]; then
    EXPECTED_SNAPSHOT_SHA=$(grep -v '^#' "$SNAPSHOT_PIN_FILE" | tr -d '[:space:]' | head -c 64)
    if [ "$ACTUAL_SNAPSHOT_SHA" != "$EXPECTED_SNAPSHOT_SHA" ]; then
        echo "[!] Upstream Packages snapshot drift detected (M4-S08 pin mismatch)" >&2
        echo "    Pinned:   $EXPECTED_SNAPSHOT_SHA" >&2
        echo "    Actual:   $ACTUAL_SNAPSHOT_SHA" >&2
        echo "    Pin file: $SNAPSHOT_PIN_FILE" >&2
        echo "    To accept the upstream change and re-pin:" >&2
        echo "      UPDATE_SNAPSHOT=1 $0 $ARCH" >&2
        exit 1
    fi
    echo "    -> Snapshot pin OK ($EXPECTED_SNAPSHOT_SHA)"
else
    echo "    -> [no pin] M4-S08 not yet pinned; run UPDATE_SNAPSHOT=1 to create $SNAPSHOT_PIN_FILE"
fi

# ─── 2. Resolve dependencies ──────────────────────────────────────────────────
echo "[2/6] Resolving package dependencies (Python)..."
RESOLVED_LIST="$WORK/resolved.txt"
PYTHONHASHSEED=0 python3 - "$ARCH" "$PKG_LIST_FILE" "$WORK/Packages.$ARCH" "$WORK/Packages.all" "$RESOLVED_LIST" <<'PYEOF'
"""Resolve termux apt package dependencies recursively.

Reads the request list, walks Depends fields in the apt Packages index, emits
a flat ordered list of every package + transitive dep we need to download.

Heuristics:
  - Strip Termux's `-gnu` suffix (the M4 spec uses `coreutils-gnu`; upstream
    package is `coreutils`).
  - Pick the first alternative for `a | b` Depends.
  - Skip Pre-Depends (only matters for ordering during apt install, not for
    bootstrap zip assembly).
  - Drop version constraints `(>= 1.2)` — upstream repo only has one version.
"""
import sys, re

_arch, pkg_list_file, idx_arch, idx_all, out_file = sys.argv[1:6]

def parse_index(path):
    pkgs = {}
    try:
        with open(path) as f:
            blocks = f.read().split('\n\n')
    except FileNotFoundError:
        return pkgs
    for block in blocks:
        if not block.strip():
            continue
        meta, key = {}, None
        for line in block.split('\n'):
            if line.startswith((' ', '\t')) and key:
                meta[key] += '\n' + line.strip()
            elif ':' in line:
                key, _, val = line.partition(':')
                key = key.strip().lower()
                meta[key] = val.strip()
        if 'package' in meta:
            pkgs[meta['package']] = meta
    return pkgs

idx = {}
idx.update(parse_index(idx_all))
idx.update(parse_index(idx_arch))  # arch-specific overrides arch-independent

def parse_depends(d):
    if not d:
        return []
    out = []
    for chunk in d.split(','):
        chunk = chunk.strip()
        if not chunk:
            continue
        # First alternative for `a | b`.
        first = chunk.split('|')[0].strip()
        # Strip `(>= 1.2)` version constraint.
        name = re.sub(r'\s*\(.*\)\s*', '', first).strip()
        if name:
            out.append(name)
    return out

requested = []
with open(pkg_list_file) as f:
    for line in f:
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        # Strip -gnu suffix (termux canonical: coreutils, findutils, etc).
        if line.endswith('-gnu'):
            line = line[:-4]
        requested.append(line)

# Special: ralplan asks for `pkg`. Termux ships `pkg` as a script inside the
# `termux-tools` package; map it.
if 'pkg' in requested and 'pkg' not in idx:
    requested = [p if p != 'pkg' else 'termux-tools' for p in requested]

resolved = []
seen = set()

def resolve(name):
    if name in seen:
        return
    seen.add(name)
    meta = idx.get(name)
    if not meta:
        sys.stderr.write(f"[!] Package not in index: {name}\n")
        return
    for dep in parse_depends(meta.get('depends', '')):
        resolve(dep)
    resolved.append(name)

for r in requested:
    resolve(r)

# Print summary to stderr (visible in CI log) and ordered list to file.
sys.stderr.write(f"[*] Requested:  {len(requested)} ({' '.join(requested)})\n")
sys.stderr.write(f"[*] Resolved:   {len(resolved)} packages (incl. transitive deps)\n")
with open(out_file, 'w') as f:
    for name in resolved:
        meta = idx.get(name, {})
        f.write(f"{name}\t{meta.get('filename', '')}\t{meta.get('sha256', '')}\n")
PYEOF

if [ ! -s "$RESOLVED_LIST" ]; then
    echo "[!] Dependency resolution produced an empty list" >&2
    exit 1
fi
echo "    -> $(wc -l < "$RESOLVED_LIST") packages to download"

# ─── 3. Download .debs ────────────────────────────────────────────────────────
echo "[3/6] Downloading .deb files..."
while IFS=$'\t' read -r pkg filename sha256; do
    [ -z "$filename" ] && { echo "    [!] $pkg has no Filename — skip"; continue; }
    deb_path="$WORK/debs/$(basename "$filename")"
    if [ -f "$deb_path" ]; then continue; fi
    echo "    -> $pkg"
    curl -fsSL --retry 3 --retry-delay 2 "$REPO_BASE_URL/$filename" -o "$deb_path"
    if [ -n "$sha256" ]; then
        actual=$(sha256sum "$deb_path" | awk '{print $1}')
        if [ "$actual" != "$sha256" ]; then
            echo "[!] SHA256 mismatch on $pkg: expected $sha256, got $actual" >&2
            exit 1
        fi
    fi
done < "$RESOLVED_LIST"

# ─── 4. Extract .debs into rootfs ─────────────────────────────────────────────
echo "[4/6] Extracting .deb data archives..."
PYTHONHASHSEED=0 python3 - "$WORK/debs" "$WORK/rootfs" <<'PYEOF'
"""Extract data.tar.* from every .deb in <debs_dir> into <rootfs>.

Cross-platform (macOS BSD ar can't read Debian ar member names with trailing
slashes, hence we parse the ar format directly in Python). Stdlib only.
"""
import sys, os, tarfile, lzma, gzip, bz2, io
from pathlib import Path

debs_dir, rootfs = Path(sys.argv[1]), Path(sys.argv[2])

def iter_ar(deb_path):
    """Yield (name, data) for each member in a Debian .deb (ar archive)."""
    with open(deb_path, 'rb') as f:
        magic = f.read(8)
        if magic != b'!<arch>\n':
            raise RuntimeError(f"{deb_path}: not an ar archive (magic={magic!r})")
        while True:
            hdr = f.read(60)
            if not hdr:
                return
            if len(hdr) < 60:
                raise RuntimeError(f"{deb_path}: truncated ar header")
            # Member name: 16 bytes, padded with spaces, optional trailing slash
            # (System V variant) or `/` followed by digits (BSD long-name table).
            name = hdr[:16].rstrip(b' ').rstrip(b'/').decode('utf-8', 'replace')
            try:
                size = int(hdr[48:58].rstrip().decode())
            except ValueError:
                raise RuntimeError(f"{deb_path}: bad size in member {name!r}")
            data = f.read(size)
            # Padding to even byte boundary.
            if size & 1:
                f.read(1)
            yield name, data

decompressors = {
    '.xz':  lambda b: lzma.decompress(b),
    '.gz':  lambda b: gzip.decompress(b),
    '.bz2': lambda b: bz2.decompress(b),
}

count = 0
for deb in sorted(debs_dir.glob('*.deb')):
    found = False
    for name, data in iter_ar(deb):
        # Match data.tar.{xz,gz,bz2}; we ignore zst because tarfile/stdlib has
        # no zstd, but termux .debs are .xz so this never trips in practice.
        for ext, dec in decompressors.items():
            if name == 'data.tar' + ext:
                tar_bytes = dec(data)
                with tarfile.open(fileobj=io.BytesIO(tar_bytes)) as tf:
                    # Python 3.12+ enforces a tar extraction filter for security
                    # (PEP 706, CVE-2007-4559). We trust the upstream termux
                    # apt repo and need to allow absolute symlinks (.debs ship
                    # symlinks like `bzcmp -> /data/data/com.termux/files/usr/
                    # bin/bzdiff`); the strict `data` filter rejects these.
                    # `tar` filter is permissive enough.
                    extract_kwargs = {}
                    if hasattr(tarfile, 'data_filter'):
                        extract_kwargs['filter'] = 'tar'
                    tf.extractall(path=str(rootfs), **extract_kwargs)
                found = True
                break
        if found:
            break
    if not found:
        print(f"[!] No data.tar.* in {deb.name}", file=sys.stderr)
        sys.exit(1)
    count += 1

print(f"    -> Extracted {count} .deb data archives")
PYEOF

# Sanity: upstream debs should have placed everything under data/data/com.termux/.
if [ ! -d "$WORK/rootfs/data/data/$UPSTREAM_APP_ID" ]; then
    echo "[!] Expected $WORK/rootfs/data/data/$UPSTREAM_APP_ID after extraction; not found" >&2
    echo "    Tree:"
    find "$WORK/rootfs" -maxdepth 4 -type d | sed 's/^/      /'
    exit 1
fi

# ─── 5. Retarget prefix com.termux → dev.warp.mobile ──────────────────────────
echo "[5/6] Retargeting prefix to /data/data/$WARP_APP_ID/..."

# 5a. Rename the directory tree.
mv "$WORK/rootfs/data/data/$UPSTREAM_APP_ID" \
   "$WORK/rootfs/data/data/$WARP_APP_ID"

TARGET_PREFIX="$WORK/rootfs/data/data/$WARP_APP_ID/files/usr"
[ -d "$TARGET_PREFIX" ] || { echo "[!] Target prefix $TARGET_PREFIX missing"; exit 1; }

# 5b. Sed-rewrite text files. Treat anything `file(1)` calls text/script as
# rewriteable; binary patching is deferred to M4-S05.
echo "    -> sed-rewriting text files..."
COUNT_REWRITTEN=0
COUNT_TEXT_FILES=0
while IFS= read -r -d '' f; do
    # Skip symlinks — we'll dereference them in step 6.
    [ -L "$f" ] && continue
    # Quick MIME check — only rewrite text-like files.
    case $(file -b --mime-type "$f" 2>/dev/null) in
        text/*|application/json|application/xml|application/x-shellscript|\
        application/x-perl|application/x-python*|inode/x-empty)
            COUNT_TEXT_FILES=$((COUNT_TEXT_FILES+1))
            if grep -qF "$UPSTREAM_APP_ID" "$f" 2>/dev/null; then
                # Use a literal-string sed (no regex) to avoid surprises.
                # The dot in com.termux must not regex-match an arbitrary char,
                # so we use awk-like literal replacement via a python one-liner.
                python3 -c "
import sys
p = sys.argv[1]
s = open(p, 'rb').read()
new = s.replace(b'$UPSTREAM_APP_ID', b'$WARP_APP_ID')
if new != s:
    open(p, 'wb').write(new)
" "$f"
                COUNT_REWRITTEN=$((COUNT_REWRITTEN+1))
            fi
            ;;
    esac
done < <(find "$TARGET_PREFIX" -type f -print0)
echo "    -> Inspected $COUNT_TEXT_FILES text files; rewrote $COUNT_REWRITTEN"

# 5c. Patch ELF DT_RUNPATH on every ELF binary that references the upstream
# lib path. Codex M4-S03 round-4 blocking finding: shipping com.termux
# RUNPATH means dynamic linker won't find libs at /data/data/dev.warp.mobile/
# files/usr/lib unless LD_LIBRARY_PATH is set at every spawn. patchelf
# rewrites the in-binary RUNPATH so libraries resolve correctly without
# any runtime env-var workaround.
echo "    -> patching ELF DT_RUNPATH (com.termux → dev.warp.mobile)..."
COUNT_ELF_PATCHED=0
COUNT_ELF_INSPECTED=0
WARP_LIB="/data/data/$WARP_APP_ID/files/usr/lib"
while IFS= read -r -d '' f; do
    [ -L "$f" ] && continue
    # patchelf is fast on non-ELF rejects, but a `file` pre-filter skips
    # the bulk of non-binaries (configs, headers, etc) and avoids noisy
    # patchelf warnings.
    case $(file -b "$f" 2>/dev/null) in
        *ELF*executable*|*ELF*shared\ object*)
            COUNT_ELF_INSPECTED=$((COUNT_ELF_INSPECTED+1))
            current_rpath=$(patchelf --print-rpath "$f" 2>/dev/null || true)
            if [ -n "$current_rpath" ] && \
               printf '%s' "$current_rpath" | grep -qF "$UPSTREAM_APP_ID"; then
                # Replace com.termux with WARP_APP_ID, preserving any other
                # paths in the rpath colon-list.
                new_rpath=$(printf '%s' "$current_rpath" \
                    | python3 -c "import sys; sys.stdout.write(sys.stdin.read().replace('/data/data/$UPSTREAM_APP_ID/', '/data/data/$WARP_APP_ID/'))")
                if patchelf --set-rpath "$new_rpath" "$f" 2>/dev/null; then
                    COUNT_ELF_PATCHED=$((COUNT_ELF_PATCHED+1))
                fi
            fi
            ;;
    esac
done < <(find "$TARGET_PREFIX" -type f -print0)
echo "    -> Inspected $COUNT_ELF_INSPECTED ELF files; patched RUNPATH on $COUNT_ELF_PATCHED"

# 5d. Audit: count remaining files with literal com.termux. These are
# residual config-default strings inside ELF .rodata: zsh module_path,
# git libexec-path, OpenSSL CA cert path, terminfo, locale, dpkg/apt
# internal fallbacks. Overridable at runtime: zsh-specific paths via
# shell-array assignment in $ZDOTDIR/.zshenv (M4-S06); the rest via env
# vars at PTY spawn (M4-S06 user-shell side: HOME, ZDOTDIR, GIT_EXEC_PATH;
# M4-S07 package-manager side: SSL_CERT_FILE, TERMINFO, LOCPATH).
set +o pipefail
COUNT_REMAINING=$(find "$TARGET_PREFIX" -type f -exec grep -lF "$UPSTREAM_APP_ID" {} + 2>/dev/null | wc -l | tr -d ' ')
set -o pipefail
echo "    -> $COUNT_REMAINING files still contain '$UPSTREAM_APP_ID' as embedded string (M4-S06 env-var override at spawn)"

# ─── 6. Pack into bootstrap zip ───────────────────────────────────────────────
echo "[6/6] Creating bootstrap-$ARCH.zip..."

# The Termux extractor expects the zip rooted at $PREFIX, with symlinks stored
# in SYMLINKS.txt (lines: `target←linkname`) instead of as actual symlinks.
# M4-S08 reproducibility: normalize all file mtimes to SOURCE_DATE_EPOCH
# (default 2020-01-01 UTC = epoch 1577836800) so zip per-entry timestamps
# are stable across runs. patchelf + sed rewrites + python extractall all
# update mtime to "now"; without this normalization, two builds at the
# same Packages snapshot produce zips that differ only in mtime bytes.
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1577836800}"
echo "    -> normalizing mtimes to SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH (M4-S08)"
# Python utime works cross-platform (BSD touch -d doesn't accept @epoch).
# follow_symlinks=False is the equivalent of touch -h: set the link's own
# mtime, don't follow to the target (the target may not exist post-rename).
python3 -c "
import os, sys
ts = int('$SOURCE_DATE_EPOCH')
root = '$TARGET_PREFIX'
for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
    # Set dir mtimes too so zip dir entries are stable.
    try:
        os.utime(dirpath, (ts, ts), follow_symlinks=False)
    except (OSError, NotImplementedError):
        pass
    for name in filenames + dirnames:
        path = os.path.join(dirpath, name)
        try:
            os.utime(path, (ts, ts), follow_symlinks=False)
        except (OSError, NotImplementedError):
            # follow_symlinks=False not supported on some platforms for
            # non-symlinks; fall back to default which follows but the
            # target also gets mtime-normalized in the same walk pass.
            try:
                os.utime(path, (ts, ts))
            except OSError:
                pass
"

(cd "$TARGET_PREFIX"
    # M4-S08 reproducibility: collect symlinks into a list first, then
    # SORT alphabetically before writing SYMLINKS.txt. Without this, the
    # readdir order from `find -type l` is filesystem-dependent and
    # produces non-deterministic SYMLINKS.txt content (different sha
    # across rebuilds even at the same Packages snapshot).
    SYMTMP=$(mktemp -t warp-symlinks.XXXXXX)
    : > "$SYMTMP"
    REWRITTEN_SYMLINKS=0
    while IFS= read -r -d '' link; do
        # Strip leading ./ from the link path.
        rel="${link#./}"
        target=$(readlink "$link")
        # Rewrite absolute symlink targets that still point at the upstream
        # com.termux prefix — the directory rename in step 5a only fixes
        # PARENT paths, not symlink targets stored as-is in the inode.
        case "$target" in
            "/data/data/$UPSTREAM_APP_ID"/*)
                target="/data/data/$WARP_APP_ID${target#/data/data/$UPSTREAM_APP_ID}"
                REWRITTEN_SYMLINKS=$((REWRITTEN_SYMLINKS+1))
                ;;
        esac
        printf '%s\xe2\x86\x90%s\n' "$target" "$rel" >> "$SYMTMP"
        rm -f "$link"
    done < <(find . -type l -print0)
    LC_ALL=C sort "$SYMTMP" > SYMLINKS.txt
    rm -f "$SYMTMP"
    SYMCOUNT=$(wc -l < SYMLINKS.txt | tr -d ' ')
    echo "    -> $SYMCOUNT symlinks recorded in SYMLINKS.txt ($REWRITTEN_SYMLINKS retargeted, sorted)"

    # M4-S08 reproducibility: normalize SYMLINKS.txt mtime too. The earlier
    # mtime normalization pass runs BEFORE this file is created, so without
    # this second pass SYMLINKS.txt would carry "now" as its timestamp and
    # cause two builds to produce different zip headers.
    python3 -c "
import os
ts = int('$SOURCE_DATE_EPOCH')
os.utime('SYMLINKS.txt', (ts, ts))
"

    # M4-S08 reproducibility: zip from a SORTED file list so entry order is
    # deterministic. Plain `zip -r9` walks the tree in filesystem readdir
    # order which is platform-dependent. Pass via stdin using `-@`.
    # zip -X strips extended attributes for byte-stable output. -9 = max compression.
    rm -f "$OUT_DIR/bootstrap-$ARCH.zip"
    # Only files (not dirs) — zip preserves directory structure from file
    # paths and would otherwise emit "cannot repeat names" when both a dir
    # entry AND files in it are passed to `zip -@`.
    find . -type f 2>/dev/null \
        | LC_ALL=C sort \
        | zip -9 -X -q -@ "$OUT_DIR/bootstrap-$ARCH.zip"
)

# ─── Metadata ─────────────────────────────────────────────────────────────────
SIZE_BYTES=$(stat -c%s "$OUT_DIR/bootstrap-$ARCH.zip" 2>/dev/null || stat -f%z "$OUT_DIR/bootstrap-$ARCH.zip")
SIZE_MB=$(( SIZE_BYTES / 1024 / 1024 ))
SHA256=$(sha256sum "$OUT_DIR/bootstrap-$ARCH.zip" | awk '{print $1}')

cat > "$OUT_DIR/bootstrap-metadata.json" <<EOF
{
  "arch": "$ARCH",
  "size_bytes": $SIZE_BYTES,
  "size_mb": $SIZE_MB,
  "sha256": "$SHA256",
  "build_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "warp_app_id": "$WARP_APP_ID",
  "upstream_app_id": "$UPSTREAM_APP_ID",
  "upstream_repo": "$REPO_BASE_URL",
  "package_list_file": "$PKG_LIST_FILE",
  "package_count": $(wc -l < "$RESOLVED_LIST" | tr -d ' '),
  "text_files_inspected": $COUNT_TEXT_FILES,
  "text_files_rewritten": $COUNT_REWRITTEN,
  "elf_files_inspected": $COUNT_ELF_INSPECTED,
  "elf_runpath_patched": $COUNT_ELF_PATCHED,
  "files_with_upstream_app_id_remaining": $COUNT_REMAINING,
  "remaining_handling": "Residual com.termux strings are compile-time config defaults (zsh module_path, git libexec-path, OpenSSL CA path, terminfo, locale, dpkg/apt fallbacks). M4-S06 ships \$ZDOTDIR/.zshenv with shell-array module_path=(...) (zsh 5.9 ignores MODULE_PATH env var) plus env vars HOME/ZDOTDIR/GIT_EXEC_PATH at PTY spawn; M4-S07 covers SSL_CERT_FILE/TERMINFO/LOCPATH for the apt/dpkg toolchain."
}
EOF

echo
echo "[OK] Built $OUT_DIR/bootstrap-$ARCH.zip"
echo "     Size:   $SIZE_BYTES bytes ($SIZE_MB MB)"
echo "     SHA256: $SHA256"
echo "     Metadata: $OUT_DIR/bootstrap-metadata.json"
