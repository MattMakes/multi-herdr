# U08 machine-teacher: machine probe and the inert System One seam

Unit slug: `machine-teacher`. Branch: `ard/machine-teacher`.
Phases: B2 (machine probe), B6 (teacher seam).
Requirements covered: NFR-10, EXP-01.

## GOAL

Two standalone modules exist with tests: `runtime/machine.rs` reports the
machine's resources (disk, memory, CPU, GPU class, limits) for preflight, and
`teacher/` holds the inert System One decision-model seam. Nothing calls them yet.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: "Operator decisions" OD4, "Hard constraints",
  §2 "teacher", §3 "B2" (the `runtime/machine.rs` bullet), §3 "B6" (the
  teacher bullet), §4 EXP-01 and NFR-10 rows.
- `libc` is already a unix dependency of `horch-core`. Use it for `statvfs`
  and `getrlimit`. Add no crate.
- This module runs on macOS (the operator) and Linux. Windows must compile and
  report `Unknown` values.
- Domain rule: no `std::env` reads. The fixture file path
  (`HORCH_MACHINE_FILE`) is a parameter; phase A2 wires the env var later.
- Parallel unit U05 (A1) adds other modules; phase A2 later adds
  `runtime/{context,paths,bins,process,fault}.rs` to the same `runtime/` dir.
  Create `runtime/mod.rs` with only `pub mod machine;`.

## FILES

own:
- `crates/horch-core/src/runtime/mod.rs` (new)
- `crates/horch-core/src/runtime/machine.rs` (new)
- `crates/horch-core/src/teacher/mod.rs` (new)
- `crates/horch-core/src/teacher/system_one.rs` (new)
- `crates/horch-core/src/teacher/inert.rs` (new)
- `crates/horch-core/src/lib.rs` (add `pub mod runtime; pub mod teacher;`)
- `crates/horch-core/tests/fixtures/machine/*.json` (new)
- `ai_docs/reports/arch-refactor-dataset/machine-teacher.md`

do not touch: every other file.

## STEPS

1. Create the worktree (conventions §2).
2. `runtime/machine.rs`:
   - Types (serde, `deny_unknown_fields` on input):
     ```
     pub enum Known<T> { Known(T), Unknown }   // serde: value or null
     pub struct MachineSnapshot {
         pub os: String,                // "macos" | "linux" | "windows" | other
         pub arch: String,              // from `uname -m` on unix, else std::env::consts::ARCH
         pub cpus: Known<u32>,          // std::thread::available_parallelism
         pub mem_total_bytes: Known<u64>,
         pub mem_available_bytes: Known<u64>,
         pub disk_free_bytes: Known<u64>,   // statvfs on the probed path
         pub disk_total_bytes: Known<u64>,
         pub gpu: GpuClass,             // AppleSilicon | Nvidia { count } | None | Unknown
         pub max_open_files: Known<u64>,    // getrlimit RLIMIT_NOFILE soft
         pub max_processes: Known<u64>,     // getrlimit RLIMIT_NPROC soft
     }
     ```
     `std::env::consts::ARCH` is a compile-time constant, not an env read; it
     is allowed.
   - `pub fn probe(path: &Path, bins: &ProbeBins, fixture: Option<&Path>) -> MachineSnapshot`.
     If `fixture` is `Some`, read the snapshot from that JSON file and return
     it (no probing). `ProbeBins` holds the paths of `sysctl`, `vm_stat`,
     `uname`, `nvidia-smi` (callers pass them; default constructor uses the
     bare names).
   - macOS: memory total from `sysctl -n hw.memsize`; available from
     `vm_stat` (free + inactive + speculative pages × page size from the
     `vm_stat` header). Apple Silicon when `uname -m` is `arm64` and
     `sysctl -n machdep.cpu.brand_string` starts with `Apple`.
   - Linux: memory from `/proc/meminfo` (`MemTotal`, `MemAvailable`). NVIDIA
     when `nvidia-smi -L` exits 0 (count lines starting with `GPU `).
   - Every external command runs with a 2 s timeout (spawn, poll `try_wait`,
     kill on timeout) and with `ANTHROPIC_API_KEY` removed. Any failure gives
     `Unknown`, never an error.
   - `cfg(windows)`: every probe returns `Unknown`; `os` is `"windows"`.
   - Tests:
     - `nfr_10_machine_probe_cfg_paths`: scan `machine.rs` source; assert every
       `libc::` use and every unix-only command is inside a `cfg(unix)` (or
       `cfg(target_os = ...)`) item, and a `cfg(windows)` or `cfg(not(unix))`
       path exists.
     - `machine_fixture_roundtrip`: 2 fixtures under
       `crates/horch-core/tests/fixtures/machine/` (`mac-m5-128g.json`,
       `linux-nvidia-2gpu.json`) load exactly.
     - `machine_probe_live_is_sane` (unix): a live probe of the temp dir gives
       cpus ≥ 1 and disk_free ≤ disk_total when both are known.
3. `teacher/system_one.rs`: serde types for the System One API (OD4), exactly:
   ```
   pub const API: &str = "systemone/v1";
   pub struct DecisionRequest { pub api: String, pub model: String, pub state: serde_json::Value,
                                pub questions: BTreeMap<String, Question> }
   pub struct Question { #[serde(rename = "type")] pub kind: QuestionKind, pub instructions: String,
                         #[serde(default, skip_serializing_if = "Vec::is_empty")] pub options: Vec<String>,
                         #[serde(default, skip_serializing_if = "Option::is_none")] pub criteria: Option<serde_json::Value> }
   pub enum QuestionKind { Choice, Score, Noul }     // serde lowercase
   pub struct DecisionResponse { pub answers: BTreeMap<String, Answer>,
                                 #[serde(default)] pub usage: Option<serde_json::Value> }
   pub struct Answer { pub choice: Option<String>, pub probabilities: BTreeMap<String, f64>,
                       pub confidence: Option<f64> }
   ```
   Mark `criteria` with `SPEC-TODO(System One criteria semantics)`.
4. `teacher/mod.rs`: `pub trait DecisionModel { fn id(&self) -> &str; fn decide(&self, req: &DecisionRequest) -> Option<DecisionResponse>; }`
   and `pub struct TeacherRef { pub id: String, pub probabilities: Option<BTreeMap<String, f64>> }`
   with `TeacherRef::none()` → `{ id: "none", probabilities: None }` (serialized
   as `{"id":"none","probabilities":null}`).
5. `teacher/inert.rs`: `pub struct Inert;` whose `decide` returns `None` and
   `id()` returns `"none"`.
6. Tests: `exp_01_inert_returns_none`, `exp_01_system_one_serde_shape` (a
   request and a response round-trip; the JSON keys match the shapes above;
   `type` values are `choice`, `score`, `noul`; `TeacherRef::none()`
   serializes exactly to `{"id":"none","probabilities":null}`).
   Also assert in a test that `teacher/` source contains no `http`, `reqwest`,
   `ureq`, `TcpStream` or `curl` (part of EXP-01: no HTTP).
7. Run the gate. Commit: `B2: Add machine probe`, `B6: Add inert System One teacher seam`.
8. Write and commit the report. Follow conventions §6 to finish.

## DONE WHEN

- The named tests pass on macOS. The crate compiles for the host.
- No `std::env::var` call in your files (`grep -n 'env::var' ...` is empty).
- The full gate is green.

## REPORT

- `horch note` after each module.
- `horch done` summary: public API and gotchas.
