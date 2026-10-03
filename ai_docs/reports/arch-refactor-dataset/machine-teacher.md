# U08 machine-teacher report

Commits: `B2: Add machine probe`, `B6: Add inert System One teacher seam`.

## Public API

- `runtime::machine`: `Known<T>` (serde value or null), `GpuClass`, `MachineSnapshot`, `ProbeBins`, `probe(path, bins, fixture)`.
- `teacher`: `DecisionModel`, `TeacherRef` (`TeacherRef::none()`), `inert::Inert`, `system_one` serde types and `API`.

## Tests added: 10

- `nfr_10_machine_probe_cfg_paths`, `machine_fixture_roundtrip`, `machine_probe_live_is_sane`
- `machine_bad_fixture_is_all_unknown`, `machine_fixture_rejects_unknown_fields`, `machine_parsers`, `machine_probe_missing_commands_give_unknown`
- `exp_01_inert_returns_none`, `exp_01_system_one_serde_shape`, `exp_01_teacher_has_no_http`

## Decisions and gotchas

- All `libc::` use and all probe commands sit in `mod sys` under `cfg(unix)`. A `cfg(not(unix))` twin returns `Unknown`. The NFR-10 test checks byte ranges, so keep that order.
- `GpuClass` serializes as `"apple_silicon"`, `{"nvidia":{"count":2}}`, `"none"`, `"unknown"`.
- Linux without `nvidia-smi` reports `GpuClass::None`. macOS with no `sysctl` brand reports `Unknown`.
- `RLIM_INFINITY` reports `Unknown`. `RLIMIT_NPROC` is read only on linux and macos.
- A fixture that is unreadable or invalid gives an all-Unknown snapshot (os and arch "unknown"). It never falls back to a live probe.
- No `std::env::var` call exists in the new files.
- SPEC-TODO(System One criteria semantics) is on `Question::criteria`.

## Gate state

- 3 horch crate tests fail on the base (spc_04_render_goldens, spc_04_render_every_group_and_window_fits, the_template_documents_exactly_the_teammate_fields). `scripts/verify-telemetry-e2e.sh` exits 101.
