# mzd-tweaks CMU runtime (POSIX sh, BusyBox on the CMU).
# Generated run.sh files embed this verbatim after their configuration block; the
# per-bundle part only calls these functions. Install logic studied from
# MZD-AIO-TI's 00_start.txt (GPL-3.0), rewritten.
#
# Host testing: MZD_ROOT prefixes every CMU path, MZD_SIMULATE=1 skips hardware
# actions (watchdog GPIO, remounts, dialogs, reboot).

R="${MZD_ROOT:-}"
SIM="${MZD_SIMULATE:-0}"
MYDIR=$(cd "$(dirname "$0")" && pwd)
LOG="$MYDIR/mzd-tweaks.log"
TMPF="${TMPDIR:-/tmp}/mzd-tweaks.$$"
FAILED=0

log() { echo "$*" >>"$LOG"; echo "$*" >&2; }
notify() {
  [ "$SIM" = 1 ] && return 0
  if [ -x /jci/tools/jci-dialog ]; then
    killall -q jci-dialog
    /jci/tools/jci-dialog --info --title="mzd-tweaks" --text="$1" --no-cancel &
  fi
  return 0
}
die() { log "ABORT: $*"; notify "MZD-TWEAKS ABORTED: $*"; rm -f "$TMPF"; exit 1; }
fail() { log "FAIL: $*"; FAILED=1; }

count_lit() {
  n=$(grep -cF -- "$2" "$R$1" 2>/dev/null)
  echo "${n:-0}"
}

read_fw() {
  vi="$R/jci/version.ini"
  [ -f "$vi" ] || return 1
  FW_VER=$(tr -d '\r' <"$vi" | grep '^JCI_SW_VER=' | sed 's/^.*_\([^_]*\)"$/\1/')
  FW_PATCH=$(tr -d '\r' <"$vi" | grep '^JCI_SW_VER_PATCH=' | sed 's/^.*"\([^"]*\)"$/\1/')
}

check_fw() {
  read_fw || die "cannot read /jci/version.ini"
  log "firmware on CMU: $FW_VER$FW_PATCH (bundle built for $EXPECT_VER$EXPECT_PATCH)"
  [ "$FW_VER" = "$EXPECT_VER" ] || die "firmware mismatch: CMU has $FW_VER$FW_PATCH, bundle is for $EXPECT_VER$EXPECT_PATCH"
  if [ -n "$EXPECT_PATCH" ] && [ "$FW_PATCH" != "$EXPECT_PATCH" ]; then
    die "firmware patch mismatch: CMU has $FW_VER$FW_PATCH, bundle is for $EXPECT_VER$EXPECT_PATCH"
  fi
}

require_file() {
  if [ -f "$R$1" ]; then log "ok: $1 exists"; else fail "missing file $1"; fi
}

# require_literal PATH LITERAL WANT(any|N) APPLIED_LITERAL HINT
require_literal() {
  if [ ! -f "$R$1" ]; then fail "missing file $1"; return; fi
  n=$(count_lit "$1" "$2")
  if [ -n "$4" ] && [ "$n" -eq 0 ] && [ "$(count_lit "$1" "$4")" -gt 0 ]; then
    log "ok: already applied in $1"; return
  fi
  if { [ "$3" = any ] && [ "$n" -gt 0 ]; } || [ "$n" = "$3" ]; then
    log "ok: anchor found in $1"; return
  fi
  fail "anchor in $1 found on $n line(s), expected $3: $2"
  [ -n "$5" ] && log "  hint: $5"
}

backup_path() {
  if [ "$BACKUP_STRATEGY" = sibling ]; then echo "$R$1.mzd-orig"; else echo "$R$BACKUP_DIR$1"; fi
}

require_backup() {
  if [ -s "$(backup_path "$1")" ]; then log "ok: backup of $1 exists"; else fail "no backup of $1 on the CMU"; fi
}

# Bytes a new backup of PATH would take (0 when one already exists).
backup_cost() {
  if [ -s "$(backup_path "$1")" ] || [ ! -f "$R$1" ]; then echo 0; else wc -c <"$R$1"; fi
}

# need_space MOUNT BYTES RESERVE_KIB
need_space() {
  d="$R$1"
  while [ ! -d "$d" ]; do d=$(dirname "$d"); done
  have=$(df -k "$d" | tail -n 1 | awk '{ print $(NF-2) }')
  need=$(( ($2 + 1023) / 1024 + $3 ))
  if [ "$have" -ge "$need" ]; then
    log "ok: $1 has ${have}KiB free, needs ${need}KiB"
  else
    fail "not enough space on $1: ${have}KiB free, needs ${need}KiB"
  fi
}

# Never overwrites an existing backup. Seeds from a legacy MZD-AIO "<file>.org" when
# present, since that holds the factory file and the live one may already be modified.
backup_file() {
  dst=$(backup_path "$1")
  if [ -s "$dst" ]; then
    log "backup exists: $1"
  else
    seed="$R$1"
    if [ -s "$R$1.org" ]; then seed="$R$1.org"; log "seeding backup of $1 from legacy $1.org"; fi
    mkdir -p "$(dirname "$dst")" && cp -p "$seed" "$dst" || die "backup failed: $1"
    [ -s "$dst" ] && [ "$(wc -c <"$dst")" -eq "$(wc -c <"$seed")" ] || die "backup incomplete: $1"
    log "backed up $1"
  fi
  mkdir -p "$MYDIR/backups$(dirname "$1")" && cp -p "$dst" "$MYDIR/backups$1" ||
    log "warn: could not copy backup of $1 to USB"
}

restore_file() {
  src=$(backup_path "$1")
  [ -s "$src" ] || die "no backup to restore for $1"
  cat "$src" >"$R$1" || die "restore failed: $1"
  log "restored $1"
}

# Rewrite a file through TMPF without changing its inode or permissions.
commit_tmp() {
  cat "$TMPF" >"$R$1" || die "write failed: $1"
  rm -f "$TMPF"
}

remove_block() {
  b="/* mzd-tweaks:$2 BEGIN */"
  e="/* mzd-tweaks:$2 END */"
  [ "$(count_lit "$1" "$b")" -gt 0 ] || { log "no $2 block in $1"; return 0; }
  B="$b" E="$e" awk 'BEGIN { b = ENVIRON["B"]; e = ENVIRON["E"] }
    $0 == b { skip = 1; next }
    skip && $0 == e { skip = 0; next }
    !skip { print }' "$R$1" >"$TMPF" || die "awk failed on $1"
  commit_tmp "$1"
  log "removed $2 block from $1"
}

append_block() {
  remove_block "$1" "$2"
  [ -n "$(tail -c 1 "$R$1")" ] && echo >>"$R$1"
  cat "$MYDIR/$3" >>"$R$1" || die "append failed: $1"
  log "added $2 block to $1"
}

replace_literal() {
  if [ "$(count_lit "$1" "$2")" -gt 0 ]; then
    F="$2" T="$3" awk 'BEGIN { f = ENVIRON["F"]; t = ENVIRON["T"]; n = length(f) }
      { s = $0; o = ""
        while ((i = index(s, f)) > 0) { o = o substr(s, 1, i - 1) t; s = substr(s, i + n) }
        print o s }' "$R$1" >"$TMPF" || die "awk failed on $1"
    commit_tmp "$1"
    log "replaced literal in $1"
  elif [ "$(count_lit "$1" "$3")" -gt 0 ]; then
    log "already replaced in $1"
  else
    die "literal vanished from $1 after preflight"
  fi
}

install_file() {
  cat "$MYDIR/$2" >"$R$1" || die "install failed: $1"
  log "installed $1"
}

run_stock() {
  p="$1"
  shift
  if [ -x "$R$p" ]; then
    "$R$p" "$@" >>"$LOG" 2>&1 || log "warn: $p $* exited non-zero"
    log "ran $p $*"
  else
    log "skip: $p not present"
  fi
}

prepare_hw() {
  [ "$SIM" = 1 ] && return 0
  echo 1 >"/sys/class/gpio/Watchdog Disable/value"
  mount -o rw,remount / || die "cannot remount / read-write"
  mount -o rw,remount /tmp/mnt/resources
  return 0
}

disarm() {
  mv "$MYDIR/run.sh" "$MYDIR/run.sh.done" 2>/dev/null && log "renamed run.sh to run.sh.done"
  sync
}
