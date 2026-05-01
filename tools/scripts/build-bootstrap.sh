#!/usr/bin/env bash
# build-bootstrap.sh — M4-S03 bootstrap zip builder for warp-mobile-android.
#
# Strategy (chosen per .omc/m4-artifacts/M4-S03-strategy.md):
#   - Download upstream Termux prebuilt .deb packages from packages-cf.termux.dev
#     (the same source termux's own CI uses).
#   - Extract them into a staging rootfs (still under /data/data/com.termux/files/usr).
#   - Retarget paths to /data/data/dev.warp.mobile/files/usr:
#       1. Rename the on-disk directory tree.
#       2. Sed-rewrite text files (shebangs, configs, scripts).
#       3. Leave ELF binaries untouched (length mismatch — handled at extract
#          time by M4-S05 atomic extractor / runtime $PREFIX env override).
#   - Pack the result into bootstrap-<arch>.zip in the format the Termux app
#     extractor expects (relative paths, SYMLINKS.txt sidecar).
#
# Why this script exists:
#   - Free + fast: runs on GitHub Actions ubuntu-latest in ~5 min and on any
#     dev laptop with bash + curl + python3 + zip.
#   - Reproducible: avoids termux-packages's docker source-compile path which
#     hits an Android SDK install bug inside the docker container on GHA's
#     14 GB-disk runners (.omc/m4-artifacts/M4-S03-execution-log.md run 3).
#   - Clone-and-build friendly: no Android SDK, no gradle, no rust toolchain —
#     just stock unix tools + python3.
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
for cmd in curl python3 zip unzip tar xz find sed grep awk file sha256sum; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "[!] Required command not found: $cmd" >&2
        echo "    Install via your package manager (apt/brew/pacman)." >&2
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
                    # Python 3.12+ introduced filter='data' to mitigate the
                    # CVE-2007-4559 path traversal class. We trust the upstream
                    # termux apt repo content but use the filter anyway.
                    extract_kwargs = {}
                    if hasattr(tarfile, 'data_filter'):
                        extract_kwargs['filter'] = 'data'
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

# 5c. Audit: count remaining files with literal com.termux. These are mostly
# ELF binaries and embedded library data — handled by M4-S05 at extract time.
# Use `set +o pipefail` momentarily because grep returns 1 when nothing matches
# and that's a normal outcome here (it would mean retargeting was 100% complete).
set +o pipefail
COUNT_REMAINING=$(find "$TARGET_PREFIX" -type f -exec grep -lF "$UPSTREAM_APP_ID" {} + 2>/dev/null | wc -l | tr -d ' ')
set -o pipefail
echo "    -> $COUNT_REMAINING files still contain '$UPSTREAM_APP_ID' (binaries; handled by M4-S05 at extract time)"

# ─── 6. Pack into bootstrap zip ───────────────────────────────────────────────
echo "[6/6] Creating bootstrap-$ARCH.zip..."

# The Termux extractor expects the zip rooted at $PREFIX, with symlinks stored
# in SYMLINKS.txt (lines: `target←linkname`) instead of as actual symlinks.
(cd "$TARGET_PREFIX"
    # Truncate any stale SYMLINKS.txt.
    : > SYMLINKS.txt
    while IFS= read -r -d '' link; do
        # Strip leading ./ from the link path.
        rel="${link#./}"
        target=$(readlink "$link")
        echo "${target}←${rel}" >> SYMLINKS.txt
        rm -f "$link"
    done < <(find . -type l -print0)
    SYMCOUNT=$(wc -l < SYMLINKS.txt | tr -d ' ')
    echo "    -> $SYMCOUNT symlinks recorded in SYMLINKS.txt"

    # zip -X strips extended attributes for byte-stable output. -9 = max compression.
    zip -r9 -X -q "$OUT_DIR/bootstrap-$ARCH.zip" .
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
  "files_with_upstream_app_id_remaining": $COUNT_REMAINING,
  "remaining_handling": "M4-S05 atomic extractor patches binaries at install time / runtime PREFIX env override"
}
EOF

echo
echo "[OK] Built $OUT_DIR/bootstrap-$ARCH.zip"
echo "     Size:   $SIZE_BYTES bytes ($SIZE_MB MB)"
echo "     SHA256: $SHA256"
echo "     Metadata: $OUT_DIR/bootstrap-metadata.json"
