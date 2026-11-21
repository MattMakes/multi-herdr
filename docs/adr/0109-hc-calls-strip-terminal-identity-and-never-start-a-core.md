# hc calls strip terminal identity and never start a core

`HcApp` (`crates/horch-core/src/workspace/hc.rs`) runs every `hc` command
with `HC_TERMINAL_ID` and `HC_SCOPE` removed from the child environment.
Otherwise `hc send` run from inside an agent terminal wraps the delivered
text as `<hc from="...">...</hc>` instead of sending it plain.

## Consequences

- `HcApp::server_reachable` is true only when the `herdr-core` socket exists
  and `hc server status` exits 0. Checking the socket first guarantees the
  reachability probe never starts a core as a side effect.
- `place::address_key` (`crates/horch-core/src/workspace/place.rs`) sorts hc
  terminal addresses in creation order by parsing the suffix after `t` as
  base 36. hc suffixes are alphanumeric (`t17`, `t1c`, `t1j`, `t1k`, `t1m`,
  `t1n`, `t1p`, `t20`), and base 36 preserves monotone creation order across
  decimal, hex, Crockford base-32 and base-36 digit alphabets.
