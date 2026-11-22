# Script: find-tool

A presence check for the `astryx` CLI. It stands in for any kind of automatic
install: it tells you whether the program can be found, and that is all it
does.

- Language: POSIX `sh`, shell builtins only.
- Input: an optional bare program name. With no name it looks for `astryx`.
- Output on stdout: `found` and, on a second line, the location; or
  `not found`.
- Exit code: 0 for found, 1 for not found, 2 for wrong usage.
- It reads the directories named in `PATH`. It opens no network connection,
  installs nothing, writes no file and never starts the program it looks for.
- Empty `PATH` entries are skipped, so the working directory is never searched
  by accident.

How to use it in a task: pass the first code block below to `sh` on standard
input, with the program name as the argument (`sh -s astryx`). Nothing has to
be saved to disk. To count a copy that is installed as a project dependency,
run it from the project root with the project's `node_modules/.bin` folder
placed at the front of `PATH` for that one command.

How to test it: save the first block as `find-tool.sh` and the second as
`find-tool-test.sh` in a folder under `$TMPDIR`, then run
`sh find-tool-test.sh find-tool.sh` there. The test prints one line per case
and ends with a summary line.

## The script

```sh
#!/bin/sh
# find-tool: say whether a program can be found on the program search path.
#
#   sh find-tool.sh [name]        name defaults to "astryx"
#   sh -s [name] < find-tool.sh   same, with the script on standard input
#
# stdout: "found" and then a line with the location, or "not found".
# exit:   0 found, 1 not found, 2 wrong usage.
#
# The lookup reads the directories listed in PATH and nothing else. It makes
# no network call, installs nothing, writes no file and never starts the
# program it looks for. Only shell builtins are used, so the result does not
# depend on which other programs PATH can reach.

if [ "$#" -gt 1 ]; then
  echo "usage: find-tool [program-name]" >&2
  exit 2
fi

name=${1:-astryx}

case $name in
  */* | -*)
    echo "find-tool: give a bare program name, not a path or an option" >&2
    exit 2
    ;;
esac

search_path=${PATH:-}
saved_ifs=$IFS
IFS=:
set -f    # no wildcard expansion while PATH is split into entries
for dir in $search_path; do
  # An empty entry would mean "the working directory". Skip it on purpose.
  [ -n "$dir" ] || continue
  candidate=$dir/$name
  if [ -f "$candidate" ] && [ -x "$candidate" ]; then
    IFS=$saved_ifs
    printf '%s\n%s\n' "found" "$candidate"
    exit 0
  fi
done
set +f
IFS=$saved_ifs

echo "not found"
exit 1
```

## The test

```sh
#!/bin/sh
# Test for find-tool.sh.
#
#   sh find-tool-test.sh path/to/find-tool.sh
#
# Everything it creates sits in one folder under $TMPDIR and is removed, file
# by file, at exit. It needs no network. Exit 0 when every case passes, 1
# otherwise.

script=${1:?usage: find-tool-test.sh path/to/find-tool.sh}
: "${TMPDIR:?TMPDIR must be set}"

case $script in
  /*) ;;
  *) script=$PWD/$script ;;
esac
[ -f "$script" ] || { echo "no such script: $script" >&2; exit 2; }

# PATH is replaced for every run of the script, so the shell is named in full.
shell=$(command -v sh) || { echo "cannot locate sh" >&2; exit 2; }

work=$TMPDIR/find-tool-test.$$
mkdir -p "$work" || exit 2

# Cleanup names each file and folder it made. Nothing is removed recursively.
cleanup() {
  rm -f "$work/bin/astryx" "$work/other/astryx" "$work/plain/astryx" \
    "$work/marker"
  rmdir "$work/dirs/astryx" "$work/dirs" "$work/plain" "$work/empty" \
    "$work/other" "$work/bin" "$work" 2>/dev/null
}
trap cleanup EXIT

mkdir "$work/bin" "$work/other" "$work/empty" "$work/plain" "$work/dirs" \
  "$work/dirs/astryx" || exit 2

# A stand-in program. If anything ever starts it, it leaves a marker behind.
make_program() {
  printf '#!/bin/sh\necho started > "%s"\n' "$work/marker" > "$1"
  chmod +x "$1"
}
make_program "$work/bin/astryx"
make_program "$work/other/astryx"
printf 'not a program\n' > "$work/plain/astryx"    # no execute bit

nl='
'
count=0
failed=0

# run <PATH value> [arguments]: sets got_out and got_rc
run() {
  run_path=$1
  shift
  got_out=$(PATH=$run_path "$shell" "$script" "$@" 2>/dev/null)
  got_rc=$?
}

# expect <label> <exit code> <first line> [<second line>]
expect() {
  count=$((count + 1))
  first=${got_out%%"$nl"*}
  second=
  case $got_out in
    *"$nl"*) second=${got_out#*"$nl"} ;;
  esac
  if [ "$got_rc" -eq "$2" ] && [ "$first" = "$3" ] &&
    { [ "$#" -lt 4 ] || [ "$second" = "$4" ]; }; then
    echo "ok $count - $1"
  else
    echo "not ok $count - $1 (exit $got_rc, output: $got_out)"
    failed=$((failed + 1))
  fi
}

run "$work/bin" astryx
expect "program on the search path" 0 "found" "$work/bin/astryx"

run "$work/bin"
expect "name defaults to astryx" 0 "found" "$work/bin/astryx"

run "$work/empty" astryx
expect "absent program" 1 "not found"

run "$work/empty:$work/bin" astryx
expect "found in a later entry" 0 "found" "$work/bin/astryx"

run "$work/other:$work/bin" astryx
expect "first matching entry wins" 0 "found" "$work/other/astryx"

run "$work/plain" astryx
expect "file without an execute bit" 1 "not found"

run "$work/dirs" astryx
expect "directory that carries the name" 1 "not found"

run "" astryx
expect "empty search path" 1 "not found"

run "$work/bin" some-other-name
expect "a different name" 1 "not found"

run "$work/bin" bin/astryx
expect "path given instead of a name" 2 ""

run "$work/bin" -g
expect "option given instead of a name" 2 ""

run "$work/bin" astryx extra
expect "too many arguments" 2 ""

got_out=$(cd "$work/bin" && PATH=: "$shell" "$script" astryx 2>/dev/null)
got_rc=$?
expect "empty entries do not search the working directory" 1 "not found"

got_out=$(PATH=$work/bin "$shell" -s astryx < "$script" 2>/dev/null)
got_rc=$?
expect "script read from standard input" 0 "found" "$work/bin/astryx"

count=$((count + 1))
if [ -e "$work/marker" ]; then
  echo "not ok $count - the program was started during a lookup"
  failed=$((failed + 1))
else
  echo "ok $count - the program is never started"
fi

echo "$((count - failed)) of $count passed"
[ "$failed" -eq 0 ]
```
