# Script: godot-procs.sh

Usage: `sh godot-procs.sh [path fragment]`

The process lister lists the running Godot processes with the pid, the elapsed time, the working folder and the command line. With a path fragment, it marks each process whose command line or working folder holds the fragment with `MINE`, and every other process with `OTHER`. It matches the process name, not the command text, so it never lists itself or a grep. Read each line before you stop a process, and stop only your own.

Copy the block into a file named `godot-procs.sh` before you run it.

```sh
#!/bin/sh
# Usage: godot-procs.sh [path fragment]
# Example: godot-procs.sh /work/my-game/.worktrees/feature-x
#
# Lists running Godot processes: pid, elapsed time, working folder and command line.
# With a path fragment, it marks each process whose command line or working folder holds the
# fragment with "MINE". Read each line before you stop a process; stop only your own.
# It matches the process name, not the command text, so it never lists itself or a grep.
case "${1:-}" in
	-h|--help) sed -n '2,8s/^# \{0,1\}//p' "$0"; exit 0 ;;
esac
frag=${1:-}
ps -axo pid=,etime=,comm= | while read -r pid etime comm; do
	case ${comm##*/} in
		godot*|Godot*) ;;
		*) continue ;;
	esac
	cmd=$(ps -o command= -p "$pid" 2>/dev/null)
	cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p')
	[ -z "$cwd" ] && cwd=$(readlink "/proc/$pid/cwd" 2>/dev/null)
	tag=OTHER
	if [ -n "$frag" ]; then
		case "$cmd $cwd" in
			*"$frag"*) tag=MINE ;;
		esac
	fi
	printf '%s pid=%s etime=%s cwd=%s cmd=%s\n' "$tag" "$pid" "$etime" "${cwd:-?}" "$cmd"
done
```

## Check the copy

This block is POSIX `sh` or GDScript, not Python, so do not check it with `python3`.
Run `sh -n godot-procs.sh`. Then `sh godot-procs.sh /tmp` lists 0 or more lines.
