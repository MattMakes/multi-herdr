Status: open, the operator decides (review finding R8-F2)

# macOS check sandbox and the LAN address of the host

Task T-301, ticket `docs/tickets/fleetd-finish/200-r8-f2-macos-check-sandbox-lan.md`. The check contract says "loopback and Unix sockets only" (`crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs:45`). This file records whether a Seatbelt profile can hold that rule on macOS, and the cost of the strict option. No product code and no test changed in this unit.

## What was tested

| Item | Value | Label |
|---|---|---|
| Host | macOS 26.5.1 (build 25F80), Darwin 25.5.0, aarch64 | verified |
| Base commit | `50fe4705` of `fleet/fleetd-finish` | verified |
| Present macOS rule for `Network::Local` | `(deny network-outbound (remote ip))(allow network-outbound (remote ip "localhost:*"))` in `crates/horch-platform/src/mac/sandbox.rs` | verified |
| Listeners | 1 TCP listener (Python, port 0) on each of 127.0.0.1, ::1, 192.168.1.54 (en0 LAN IPv4), fd0a:ad04:2f5d:4eed:49d:d983:7a3c:e3d0 (LAN IPv6), 192.168.139.3 (bridge) | verified |
| Probe | `/usr/bin/sandbox-exec -p '(version 1)(allow default)<rule>' /usr/bin/nc -z -G 2 <address> <port>` for each address | verified |

## Step 1: the 11 profile forms

Each profile starts with `(version 1)(allow default)`. "parse error" means `sandbox-exec` refuses the profile and runs nothing.

| Form | Rule after `(allow default)` | 127.0.0.1 | ::1 | LAN IPv4 | LAN IPv6 | bridge | Label |
|---|---|---|---|---|---|---|---|
| P-open | (none) | connect | connect | connect | connect | connect | verified |
| P0 present rule | `(deny network-outbound (remote ip))(allow network-outbound (remote ip "localhost:*"))` | connect | connect | connect | connect | connect | verified |
| P1 local ip | `(deny network-outbound (remote ip))(allow network-outbound (local ip "localhost:*"))` | connect | connect | connect | connect | connect | verified |
| P2a literal host | `(deny network-outbound (remote ip))(allow network-outbound (remote ip "127.0.0.1:*"))` | parse error | parse error | parse error | parse error | parse error | verified |
| P2b localhost minus LAN | `(deny network-outbound (remote ip))(allow network-outbound (remote ip "localhost:*"))(deny network-outbound (remote ip "192.168.1.54:*"))` | parse error | parse error | parse error | parse error | parse error | verified |
| P3 no IP | `(deny network-outbound (remote ip))` | refused | refused | refused | refused | refused | verified |
| P4 require-not | `(deny network-outbound (require-all (remote ip) (require-not (remote ip "localhost:*"))))` | refused | refused | refused | refused | refused | verified |
| P5 ip4 and ip6 | `(deny network-outbound (remote ip))(allow network-outbound (remote ip4 "localhost:*") (remote ip6 "localhost:*"))` | connect | connect | connect | connect | connect | verified |
| P6 tcp | `(deny network-outbound (remote ip))(allow network-outbound (remote tcp "localhost:*"))` | connect | connect | connect | connect | connect | verified |
| P7 interface | `(deny network-outbound (remote ip))(allow network-outbound (interface "lo0"))` | parse error | parse error | parse error | parse error | parse error | verified |
| P8 remote and local | `(deny network-outbound (remote ip))(allow network-outbound (require-all (remote ip "localhost:*") (local ip "localhost:*")))` | connect | connect | connect | connect | connect | verified |
| P9 bind limit | `(deny network-bind (local ip))(allow network-bind (local ip "localhost:*"))(deny network-outbound (remote ip))(allow network-outbound (remote ip "localhost:*"))` | connect | connect | connect | connect | connect | verified |

The parse error of P2a and P2b is `sandbox-exec: host must be * or localhost in network address`. The parse error of P7 is `sandbox-exec: unbound variable: interface`. (verified)

Findings:

1. Seatbelt `localhost` matches each address of this host: loopback, the LAN addresses and the bridge address. (verified)
2. Seatbelt accepts only `*` or `localhost` as the host of an IP filter, so no rule can name 127.0.0.1 or ::1. (verified)
3. No form in the table connects to loopback and refuses the LAN address. (verified)
4. The Apple profiles in `/System/Library/Sandbox/Profiles` and `/usr/share/sandbox` use only `"localhost:<port>"` and `"*:<port>"`. No Apple profile limits a program to loopback. (verified by `grep` of `remote ip`, `local ip`, `localhost`, `loopback`, `lo0` and `127.0.0.1`)
5. A Unix socket proxy with P3: a TCP client in the check cannot connect to 127.0.0.1, and a proxy outside the sandbox cannot catch that connect unless each check uses the Unix socket. For TCP clients this is equal to option (a). (not verified by a run; reasoned from P3)
6. A nested `sandbox-exec` fails in each outer Seatbelt profile with `sandbox-exec: sandbox_apply: Operation not permitted`. (verified)

## Measure of option (a)

Option (a) sets the macOS `Network::Local` rule to `(deny network-outbound (remote ip))`. Unix socket connections stay allowed. The measure ran with a temporary change to `crates/horch-platform/src/mac/sandbox.rs`. The change is reverted and is not committed.

### Tests of the sandbox: 3 fail (verified)

Run with the temporary change, through `scripts/dev-sandbox.sh`:

1. `horch-platform` `sandbox::tests::local_network_allows_loopback_and_refuses_an_off_host_address` (panics at `crates/horch-platform/src/sandbox.rs:374`: loopback does not connect)
2. `horch-core` `sr3_no_agent_push::verifier_check_sends_no_message_outside_the_fleet` (panics at `crates/horch-core/tests/sr3_no_agent_push.rs:461`)
3. `horch-e2e` `p4_verifier::sr3_check_that_connects_off_host_is_refused` (panics at `crates/horch-e2e/tests/p4_verifier.rs:629`)

### Tests that this repo runs as a task check: 21 fail (verified)

A task check of this repo runs `cargo test` in the check sandbox. These test files serve HTTP on 127.0.0.1 themselves. Each run was `scripts/dev-sandbox.sh /usr/bin/sandbox-exec -p '<profile>' cargo test --offline --locked --no-fail-fast ...`, once with the profile `(version 1)(allow default)` plus P0 and once with `(version 1)(allow default)` plus P3.

| Test file | P0 (present rule) | P3 (option a) |
|---|---|---|
| `horch` `r_cli_cmds_selfupdate` | 5 of 5 pass | 5 of 5 fail |
| `horch` `self_update_cli` | 5 of 5 pass | 5 of 5 fail |
| `horch-e2e` `update` | 14 of 14 pass | 11 of 14 fail |
| `horch-core` `worktree_jobs` | 25 of 34 fail | 25 of 34 fail |

`worktree_jobs` fails the same with both rules, because its tests start a nested `sandbox-exec` (finding 6). So it gives no measure of option (a). (verified)

The 21 tests:

1. `r_cli_cmds_selfupdate::a_failing_install_script_fails_the_update`
2. `r_cli_cmds_selfupdate::no_release_is_not_an_error`
3. `r_cli_cmds_selfupdate::an_older_release_is_up_to_date`
4. `r_cli_cmds_selfupdate::check_reports_a_newer_release_and_installs_nothing`
5. `r_cli_cmds_selfupdate::a_release_without_sha256sums_is_refused`
6. `self_update_cli::fleet_installs_a_newer_release_and_restarts_on_it`
7. `self_update_cli::self_update_refuses_an_archive_that_lacks_a_binary_and_installs_nothing`
8. `self_update_cli::self_update_refuses_a_wrong_digest_and_installs_nothing`
9. `self_update_cli::fleet_continues_on_this_version_when_the_update_fails`
10. `self_update_cli::self_update_verifies_and_runs_the_new_horch_install`
11. `update::arc_32_fleet_updates_each_cli_once_before_the_orchestrator_pane`
12. `update::arc_32_orchestration_updates_before_its_panes`
13. `update::arc_32_a_cpx_launch_updates_claude_not_cpx`
14. `update::arc_33_a_hung_updater_is_left_running_at_the_timeout`
15. `update::arc_33_a_failed_updater_gives_one_note_and_the_fleet_starts`
16. `update::arc_33_no_updater_gets_anthropic_api_key`
17. `update::arc_33_a_second_fleet_within_24_h_skips_the_updated_clis`
18. `update::arc_35_horch_update_exits_1_on_a_detached_or_busy_cli`
19. `update::arc_35_horch_update_runs_herdr_only_on_request`
20. `update::arc_36_each_updater_gets_the_package_manager_cooldown`
21. `update::arc_36_a_release_younger_than_the_cooldown_is_held_back`

Total: option (a) makes 24 tests fail that pass with the present rule. (verified for the files above)

Limit of the measure: the test files were found with a search for `TcpListener`, `TcpStream` and `127.0.0.1` in `crates/`. The full workspace did not run in an outer profile, so a test that reaches loopback in another way is not in the count. (not verified)

## The 3 options

1. **(a) No IP network for a macOS check.** The macOS `Network::Local` rule becomes P3: each IP connection is refused, and only Unix sockets connect. The contract text stays true on macOS. The 24 tests above fail when they run as a macOS task check, and the 3 sandbox tests need a new macOS expectation.
2. **(b) This host only.** The present rule stays. The contract text (`check_sandbox.rs:45`) and the design change to "this host only (loopback, each address of this host, and Unix sockets)". A check on macOS can still reach a service that listens on a LAN address of the host. Linux keeps loopback only (`--unshare-net`).
3. **(c) macOS check sandbox unsupported.** The macOS check sandbox gives `NotSupported`. Each macOS task check then holds on the `no_check_sandbox` needs-you item, so no macOS user gets a checked task.

Recommendation of the worker (T-301): option (b), because option (a) breaks the loopback test servers and option (c) stops each macOS user. Recommendation of the end review (`docs/specs/fleetd/reviews/end-review-a.md`, operator decision 3): keep the strict loopback-only contract and fix or disable the macOS sandbox. The operator decides.
