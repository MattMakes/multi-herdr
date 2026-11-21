# Agent CLI updates wait out a 7 day release cooldown

The agent CLI update step installs a release only once it is `DEFAULT_DAYS`
(7) days old, set by `HORCH_UPDATE_COOLDOWN_DAYS` (ARC-36,
`crates/horch-core/src/harness/cooldown.rs`). A new release is the moment a
hijacked package does its harm, and most are caught and pulled within days.
The rule works at 2 levels: the CLI itself, and what its updater installs
with it.

## Consequences

- Before an updater runs, horch reads the publish time of the CLI's newest
  release from the npm registry. A release younger than the cooldown holds
  the updater back, and the CLI keeps the build it has.
- A CLI whose release dates horch cannot read is held back too: an age that
  cannot be checked does not pass. Antigravity and Prime publish no npm
  package, so `npm_package` returns `None` for them: horch knows no public
  record of their release dates.
- `npm_package` uses each CLI's npm package as the record even for native
  and Homebrew installs, because those builds carry the same version
  numbers.
- `child_env` gives the updater the cooldown of each package manager that
  reads it from the environment, so a dependency it installs is as old as
  the CLI. Each value replaces the operator's value for the updater only.
  - npm `before`: works in every npm version. `min-release-age` exists only
    from npm 11.10, and a `before` date from the same source wins over it
    anyway.
  - pnpm `minimumReleaseAge`, in minutes.
  - yarn `npmMinimalAgeGate`, a duration.
  - uv `exclude-newer` and pip `--uploaded-prior-to`: the cutoff as a date,
    which every uv version and every pip with the option read.
- bun reads its cooldown only from `bunfig.toml` or a flag, and Homebrew
  and the native installers have none, so their dependencies get no
  cutoff.
