#!/usr/bin/env bash
# real-argv smoke test: runs the compiled example CLIs and asserts on exit code
# and stdout/stderr content. not a unit test — this is "does it behave like a
# CLI when you actually run it."
set -u
G=target/debug/examples/grab
V=target/debug/examples/vault
pass=0; fail=0

# check <name> <expected_exit> <stream:out|err> <substr|--> -- <argv...>
check() {
  local name="$1" want_rc="$2" stream="$3" needle="$4"; shift 4
  [ "$1" = "--" ] && shift
  local out err rc
  out="$("$@" 2>/tmp/smoke.err)"; rc=$?
  err="$(cat /tmp/smoke.err)"
  local hay="$out"; [ "$stream" = "err" ] && hay="$err"
  local ok=1
  [ "$rc" = "$want_rc" ] || ok=0
  if [ "$needle" != "--" ] && ! grep -qF -- "$needle" <<<"$hay"; then ok=0; fi
  if [ "$ok" = 1 ]; then
    pass=$((pass+1)); printf '  ok   %-44s [rc=%s]\n' "$name" "$rc"
  else
    fail=$((fail+1))
    printf 'FAIL   %-44s want_rc=%s got_rc=%s\n' "$name" "$want_rc" "$rc"
    printf '         needle(%s)=%q\n' "$stream" "$needle"
    printf '         out=%q\n         err=%q\n' "$out" "$err"
  fi
}

echo "── grab: help / version ──"
check "grab --help"            0 out "Usage"        -- $G --help
check "grab -h"                0 out "--output"     -- $G -h
check "grab --version"         0 out "grab 0.1.0"   -- $G --version
check "grab -V"                0 out "grab 0.1.0"   -- $G -V

echo "── grab: valid parses ──"
check "positionals + default"  0 out 'jobs: 4'                   -- $G a b c
check "url vec captured"       0 out 'url: ["a", "b", "c"]'      -- $G a b c
check "short cluster -fvv"     0 out 'verbose: 2'                -- $G -fvv x
check "force via cluster"      0 out 'force: true'               -- $G -fvv x
check "attached short -o/tmp"  0 out 'output: Some("/tmp")'      -- $G -o/tmp x
check "long =value"            0 out 'jobs: 8'                   -- $G --jobs=8 x
check "spaced long value"      0 out 'output: Some("/d")'        -- $G --output /d --force -vv u
check "-- forces positional"   0 out 'url: ["--weird", "-x"]'    -- $G -- --weird -x
check "negative-num positional" 0 out 'url: ["-5"]'              -- $G -- -5

echo "── grab: errors (exit 2) ──"
check "unknown flag"           2 err "unrecognised"             -- $G --nope
check "missing opt value"      2 err "needs a value"            -- $G x --output
check "bad u32 value"          2 err "invalid value"            -- $G --jobs abc x

echo "── vault: help / version / auth gate ──"
check "vault --help"           0 out "Usage"          -- $V --help
check "vault lists subcmds"    0 out "namespace"      -- $V --help
check "whoami shown (rename)"  0 out "whoami"         -- $V --help
check "vault --version"        0 out "vault 0.1.0"    -- $V --version
check "bare vault -> auth err" 2 err "is required"    -- $V

# negative checks: hidden items must NOT surface in help
if $V --help 2>&1 | grep -qiF doctor; then
  fail=$((fail+1)); printf 'FAIL   %-44s (doctor leaked)\n' "hidden variant absent"
else pass=$((pass+1)); printf '  ok   %-44s\n' "hidden variant absent from help"; fi
if $V --help 2>&1 | grep -qiF debug-internals; then
  fail=$((fail+1)); printf 'FAIL   %-44s (debug-internals leaked)\n' "hidden field absent"
else pass=$((pass+1)); printf '  ok   %-44s\n' "hidden field absent from help"; fi

echo "── vault: required_group (auth) ──"
check "no auth rejected"       2 err "auth"            -- $V set k v
check "--token unlocks"        0 -- --                 -- $V --token t set k v
check "--key-file unlocks"     0 -- --                 -- $V --key-file f set k v
check "both auth -> conflict"  2 err "together"        -- $V --token t --key-file f set k v

echo "── vault: valid (silent unless -vv) ──"
check "set full opts"          0 -- --                 -- $V --token t set k v --kind token -t a -t b --lock
check "-vv echoes Set"         0 err "Set"             -- $V --token t -vv set k v
check "get + value-enum"       0 -- --                 -- $V --token t get k --format json
check "kebab enum certificate" 0 -- --                 -- $V --token t set k v --kind certificate
check "short override -K"      0 err "Token"           -- $V --token t -vv set k v -K token
check "default on_conflict"    0 err "Skip"            -- $V --token t -vv import f.txt
check "global opt + sub"       0 err 'namespace: "prod"' -- $V --token t --namespace prod -vv set k v
check "-D/--database override" 0 err 'db: Some("/x")'  -- $V --token t -D /x -vv whoami

echo "── vault: custom FromArg (ttl) ──"
check "ttl 2h -> Duration"     0 err 'Ttl(7200s)'      -- $V --token t -vv set k v --ttl 2h
check "ttl bad unit -> err"    2 err "use s/m/h/d"     -- $V --token t set k v --ttl 5x

echo "── vault: conflicts_with & group ──"
check "clip vs format"         2 err "together"        -- $V --token t get k --clip --format json
check "export dest group"      2 err "together"        -- $V --token t export -o f --stdout
check "export stdout alone ok" 0 -- --                 -- $V --token t export --stdout

echo "── vault: trailing args (exec) ──"
check "exec -- passthrough"    0 err 'command: ["ls", "-la"]' -- $V --token t -vv exec -- ls -la

echo "── vault: nested / rename / hidden ──"
check "namespace create"       0 -- --                 -- $V --token t namespace create ns1 --desc hi
check "nested rename echoes"   0 err 'Rename'          -- $V --token t -vv namespace rename a b
check "variant rename: ns ls"  0 -- --                 -- $V --token t namespace ls
check "hidden doctor parses"   0 -- --                 -- $V --token t doctor
check "unknown subcommand"     2 err "unknown subcommand" -- $V --token t frobnicate
check "missing required arg"   2 err "missing required"   -- $V --token t set k

echo "── vault: aliases & env ──"
check "arg alias --ns"         0 err 'namespace: "prod"' -- $V --token t --ns prod -vv whoami
check "command alias: delete"  0 -- --                 -- $V --token t delete k
check "command primary: rm"    0 -- --                 -- $V --token t rm k
export VAULT_DB=/e
check "env fills db"           0 err 'db: Some("/e")'  -- $V --token t -vv whoami
check "cli beats env"          0 err 'db: Some("/c")'  -- $V --token t -D /c -vv whoami
unset VAULT_DB

echo "── vault: value-enum errors ──"
check "bad enum lists choices" 2 err "possible values" -- $V --token t get k --format yaml
check "bad enum shows json"    2 err "json"            -- $V --token t get k --format yaml

echo "── unicode ──"
check "utf-8 args parse"       0 err 'café'            -- $V --token t -vv set café résumé

echo
echo "════════  $pass passed, $fail failed  ════════"
exit $fail
