# Phase skills verification notes

1. Configuration enablement is not discovery. Codex `skills.config` accepts an external path but does not add it to the loader's roots. A native `skills/list` probe caught this before implementation relied on it. Preserve native catalog checks alongside argv tests.
2. Validate before side effects. A bad selected skill originally reached the worker only after the ledger marked it live. Spawn now validates the resolved phase/catalog before allocating the role or recording work. Keep the negative-selection regression.
3. Positional delimiters matter. Prime daemon options previously followed `--`, becoming prompt content. Add runtime options before the builder appends the delimiter and assert that order.
4. Isolation claims need runtime evidence. OpenCode and Codex retain ambient skills beyond the fleet catalog. Documentation and estimates now distinguish the materialized fleet catalog from complete model context.
5. Check host prerequisites separately. pi's loaders worked under the shell's old Node while its bundled CLI failed before parsing. Supported Node fixed the version check without changing the integration.
