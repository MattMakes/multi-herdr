# S6 release-sandbox: an OS-enforced boundary for app-release-preparer

Plan: `ai_docs/plans/finish/s6-release-sandbox.md`. Worker: opus-65. Date: 2026-10-04.
Claude Code: 2.1.289. Host: macOS 26.5.1, Xcode 26.6. asc source: 5.9.1
(`/tmp/App-Store-Connect-CLI-5.9.1`).

## Result

`app-release-preparer` now runs with Claude Code's Bash sandbox (Seatbelt on
macOS, bubblewrap on Linux). horch writes the sandbox block into the
`--settings` overlay of the launch. The block comes from a new teammate field,
`sandbox:`. horch adds 5 keys that the teammate cannot turn off. A pane that
runs `sh -c 'unset ASC_CONFIG_PATH ASC_BYPASS_KEYCHAIN; asc ...'` stays inside
the same OS boundary, because the boundary applies to the process, not to the
command string.

## Commits

- `a2de49f`: the `sandbox:` field, the settings merge, the host check, the
  roster checks and the template entry. The merge commit `6058563` sits on
  top of it (another worker's merge; it changed none of these lines).
- The next commit: the teammate block, the persona, the README section and
  the launch test for the shipped teammate.

## Sources

- Sandboxing: https://code.claude.com/docs/en/sandboxing (fetched 2026-10-04).
- Settings reference: https://code.claude.com/docs/en/settings-reference,
  section "Sandbox settings" and `permissions.blockReadsOutsideWorkingDirectories`.
- Seatbelt profile: `sandbox-runtime`, `src/sandbox/macos-sandbox-utils.ts`
  (main, fetched 2026-10-04). Claude Code builds its sandbox on this package.
- asc 5.9.1: `internal/asc/client_core.go:23` (`BaseURL =
  "https://api.appstoreconnect.apple.com"`), `internal/asc/notary.go:27`,
  `internal/auth/keychain.go:102-198` (99designs/keyring, service `asc`),
  `internal/config/config.go:18,253,281` (`~/.asc`, `ASC_CONFIG_PATH`).

## Facts from the docs that drive the design

1. The sandbox wraps Bash, PowerShell and Monitor commands and their child
   processes. The built-in Read, Edit, Write, WebFetch and WebSearch tools
   are outside it. "A `denyRead` entry doesn't stop the Read tool"
   (sandboxing, "What runs outside the sandbox").
2. `sandbox.failIfUnavailable: true` makes Claude Code exit at startup when
   the sandbox cannot start. Without it, Claude Code runs commands
   unsandboxed (settings reference, `sandbox.failIfUnavailable`).
3. `sandbox.allowUnsandboxedCommands: false` makes Claude Code ignore
   `dangerouslyDisableSandbox`. A `false` given with `--settings` makes the
   sandbox admin-required, so a repository's `.claude/settings.json` cannot
   add `excludedCommands`, `allowWrite`, `allowedDomains` or similar entries
   (sandboxing, "Repository settings under an admin-required sandbox";
   Claude Code 2.1.285 or later).
4. `excludedCommands` takes matching commands out of the sandbox. It is an
   escape hatch, not a boundary (settings reference).
5. `network.strictAllowlist: true`, from `--settings` or user settings,
   denies every host outside `allowedDomains` instead of asking. It also
   refuses auto mode's per-command allowed domains (sandboxing, "Per-command
   allowed domains in auto mode"; Claude Code 2.1.219 or later).
6. `permissions.blockReadsOutsideWorkingDirectories: true` makes the Read,
   Grep, Glob and LSP tools refuse paths outside the working directories.
   With the sandbox on, it also removes read access to the home directory
   from sandboxed commands, then re-opens the working directories and the
   parts of `~/.claude` that commands need (settings reference, "Sandboxed
   commands under the block"; Claude Code 2.1.257 or later). `allowRead`
   entries from `--settings` still re-open paths under the block.
7. `sandbox.credentials.envVars` entries with `"mode": "deny"` unset the
   variable before each sandboxed command. There is no built-in list
   (sandboxing, "Protect credentials").
8. Seatbelt allows the Mach services `com.apple.SecurityServer` and
   `com.apple.securityd.xpc` (`macos-sandbox-utils.ts:986-1000,1123`).
   Settings can add Mach services but cannot remove these.

## Design

### The field

`sandbox:` in teammate frontmatter is the Claude `sandbox` settings object,
written as YAML. horch copies it into the `--settings` overlay as `sandbox`,
then sets these keys whatever the file says:

| Key | Value | Why |
| --- | --- | --- |
| `sandbox.enabled` | `true` | The field means "sandbox this pane". |
| `sandbox.failIfUnavailable` | `true` | Fail closed when the host cannot sandbox (fact 2). |
| `sandbox.allowUnsandboxedCommands` | `false` | No retry outside the sandbox, and admin-required (fact 3). |
| `sandbox.network.strictAllowlist` | `true` | No host outside the list, in every permission mode (fact 5). |
| `permissions.blockReadsOutsideWorkingDirectories` | `true` | The Read tool cannot read what the sandbox denies to Bash (facts 1 and 6). |

`horch teammates --check` rejects:

- `sandbox:` on any agent other than `claude`;
- a value that contradicts a forced key (`enabled: false`,
  `failIfUnavailable: false`, `allowUnsandboxedCommands: true`,
  `network.strictAllowlist: false`);
- `excludedCommands` with any entry, and `filesystem.disabled: true` (fact 4);
- `sandbox:` together with `settings:` (a settings file replaces horch's
  overlay, so the sandbox block would be lost).

The launch also refuses, with a clear error, in 2 cases:

- `settings` names a file and the teammate has `sandbox:`;
- the host has no sandbox: on Linux, `bwrap` or `socat` is not on `PATH`; on
  macOS, `/usr/bin/sandbox-exec` is missing; any other OS. Claude Code would
  also exit (`failIfUnavailable`), but horch says why before the pane opens.

### The teammate's block

- Writes: the project directory and the session temp directory (Claude's
  defaults), plus `/private/var/folders` for the per-user Darwin temp and
  cache directories that `xcodebuild` writes (see probe 4). The persona
  passes `-derivedDataPath build/DerivedData`, `-archivePath build/...` and
  `-clonedSourcePackagesDirPath build/SourcePackages`, so build output stays
  in the project.
- Reads: the home directory is closed by `blockReadsOutsideWorkingDirectories`.
  `allowRead` re-opens `~/.config/horch/asc` (the config and the key),
  `~/Library/Developer`, `~/Library/Preferences`, `~/Library/Caches` and
  `~/Library/MobileDevice/Provisioning Profiles` for `xcodebuild`.
- `denyRead` names the credential stores explicitly as well, so they stay
  closed if a later Claude Code changes the block: `~/.asc`,
  `~/Library/Keychains`, `~/.config` (the narrower `~/.config/horch/asc`
  allow re-opens that one directory), and altool's key directories
  `~/.appstoreconnect`, `~/private_keys`, `~/.private_keys`.
- Environment: `credentials.envVars` denies asc's credential variables
  (`ASC_KEY_ID`, `ASC_ISSUER_ID`, `ASC_PRIVATE_KEY`, `ASC_PRIVATE_KEY_PATH`,
  `ASC_PROFILE`, the `ASC_WEB_*` login variables and the `ASC_ADS_*`,
  `ASC_STOREKIT_*` key variables). A key in the operator's shell does not
  reach asc.
- Network: `allowedDomains: [api.appstoreconnect.apple.com]`. asc sends
  every API call there (`client_core.go:23`). Build uploads go to upload
  hosts that the API names at run time, and altool and notarytool use other
  hosts, so every upload fails at the proxy. `xcodebuild archive` and
  `-exportArchive` with `method: app-store-connect` and `destination:
  export` need no network when the signing assets are local. Swift package
  resolution needs the package hosts; the project resolves packages before
  the assignment, or the operator adds the hosts to the teammate file.
  `-allowProvisioningUpdates` needs `developerservices2.apple.com`, which
  is not allowed, so that flag fails even when the assignment names it.
- `enableWeakerNetworkIsolation` stays off. Probe 3 shows that a Go TLS
  client (asc is Go) reaches the API without it on this host.

### Uploads

The agent never uploads. The deny list adds `asc builds upload`,
`asc distribute apply`, `asc distribute publish`, `asc notarization submit`,
`xcrun altool`, `altool`, `xcrun notarytool submit` and `notarytool submit`
as a second line. The OS line is the network allowlist. The persona writes
the exact upload command for a human to run.

### What still depends on the key's role

The sandbox stops the pane from using any key except the configured one.
It does not limit what that key can do through `api.appstoreconnect.apple.com`.
A submit, a price change or a metadata push is one API call to the allowed
host. The deny list blocks the asc subcommands, but `curl` with a JWT signed
from the readable key can still make the call. So the key's role must be
Developer, as before: Developer cannot submit apps or edit pricing.

## Probes (dummy files only, all removed after)

Each probe ran `env -u ANTHROPIC_API_KEY claude -p --model sonnet --settings
<json>` in `.worktrees/_scratch/probe`, with the sandbox block above and the
dummy files `~/.asc/horch-probe-dummy.txt`,
`~/.config/horch/asc/horch-probe-dummy.txt` and
`~/Downloads/horch-probe/AuthKey_PROBE.p8`. No real credential was read.

1. Filesystem, with `denyRead: [~/.asc, ~/Library/Keychains, ~/.config]` and
   `allowRead: [~/.config/horch/asc]`:
   - `sh -c "cat ~/.asc/horch-probe-dummy.txt"`: exit 1, `Operation not permitted`.
   - `sh -c "unset ASC_CONFIG_PATH; cat $HOME/.asc/horch-probe-dummy.txt"`: exit 1, `Operation not permitted`.
   - `cat ~/.config/horch/asc/horch-probe-dummy.txt`: exit 0, `DUMMY-HORCH-ASC`.
   - `ls ~/Library/Keychains`: exit 1, `Operation not permitted`.
   - `gh api ...`: `open ~/.config/gh/config.yml: operation not permitted`.
2. Keychain: outside the sandbox, `security list-keychains` prints
   `login.keychain-db` and `System.keychain`. Inside the sandbox it prints
   only `/Library/Keychains/System.keychain`. The login keychain is not in
   the search list, so a keyring lookup by asc does not find the operator's
   keychain profiles. I did not add an item to the login keychain to test a
   positive lookup (the permission classifier refused it).
3. Network, with `allowedDomains: [api.appstoreconnect.apple.com]` and
   `strictAllowlist: true`, both values of `enableWeakerNetworkIsolation`:
   - `curl https://example.com`: exit 56, `deny network-outbound example.com:443 (host is not on the allow list)`.
   - `curl https://api.appstoreconnect.apple.com/v1/apps`: HTTP 401 (reached).
   - a Go `net/http` client to the same URL: HTTP 401 (reached).
4. Home-wide read block, with `denyRead: ["~/"]` and the `allowRead` list
   above, then with `permissions.blockReadsOutsideWorkingDirectories: true`:
   - `cat ~/Downloads/horch-probe/AuthKey_PROBE.p8`: denied (Bash).
   - Read tool on the same file: refused, "the
     permissions.blockReadsOutsideWorkingDirectories setting blocks reads
     outside the working directories".
   - `cat ~/.config/horch/asc/horch-probe-dummy.txt`: exit 0.
   - `xcodebuild -version`: exit 0, `Xcode 26.6`.
   - `xcodebuild ... -derivedDataPath ../build/DerivedData build` on a
     1-target Swift package: `IDELogStore` saves under `build/DerivedData/Logs`
     and the result bundle in `/var/folders/.../T/` failed with `Operation
     not permitted`. Atomic saves stage in the Darwin temp directory, so the
     block adds `/private/var/folders` to `allowWrite`. I could not run the
     build again with that entry: the permission classifier refused the
     probe. So a full `xcodebuild archive` inside the sandbox is NOT
     verified.

Command 6 of probe 1 (`echo $ASC_PRIVATE_KEY` with a dummy value) needed
approval in `-p` mode, so the env deny is verified from the docs only.

## Teammate and persona changes

- `teammates/app-release-preparer.md`: the `sandbox:` block from "The
  teammate's block"; WebFetch and WebSearch denied; 6 upload patterns added
  to `disallowed_tools`; the brief description says "Never uploads or
  submits".
- The persona no longer uploads. It writes the exact `asc builds upload
  --app <APP_ID> --ipa build/export/<name>.ipa` command for a human. It keeps
  build output under `build/`, never passes `-allowProvisioningUpdates`, and
  reports `BLOCKED:` when the sandbox blocks a command.
- `README.md`, section "App Store Connect key for app-release-preparer":
  states the 3 lines (sandbox, key role, deny list), what the sandbox
  enforces, and what it does not cover. The old sentence "the pane cannot
  use your own key" was false before this unit; it is gone.

## Limits

- Code signing: the login keychain is outside the sandbox (probe 2), so
  `xcodebuild archive` cannot use a signing identity from it. The operator
  has 2 choices: a dedicated keychain with only the distribution identity,
  stored under `~/.config/horch/asc/` and added to the user search list
  (not verified here), or a human runs the signed archive and export. The
  README says this.
- Processes outside the sandbox: hooks, MCP servers, the status line and
  the Edit and Write tools run with the operator's access (sandboxing,
  "What runs outside the sandbox"). The teammate has `mcp_servers: {}` and
  denies WebFetch and WebSearch. Edit and Write stay: the pane edits
  version files. They cannot read a credential.
- The key itself is readable inside the sandbox, because asc must read it.
  The pane can only send it to `api.appstoreconnect.apple.com`.
- A key file outside the home directory (for example under `/Volumes` or
  `/tmp`) is readable. Keep the key under `~/.config/horch/asc/`.
- Claude Code 2.1.285 or later is needed for every rule above.
