# Parse /proc/<pid>/stat fields after the last parenthesis

On Linux, `/proc/<pid>/stat` fields are counted from after the last `)`,
starting at field 3. Field 2, the command, is in parentheses and may hold
spaces, so splitting the whole line on whitespace misreads every later
field. `procid::imp::parse_stat` (`crates/horch-core/src/procid.rs`) reads
the start time, field 22, this way. `parent_pid`
(`crates/horch/src/cmd/spawn.rs`) reads the parent, field 4, the same way.
