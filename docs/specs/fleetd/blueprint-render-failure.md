# Blueprint architecture render failure

Ticket: T-21. Plugin: `software@skill-marketplace` 1.0.0, source `MattMakes/skill-marketplace` commit `17be4c6`. Host: macOS (Darwin 25.6.0, arm64), Node v26.12.1.

## Cause

The artifact checker `skills/blueprint/scripts/check-render-output.mjs` writes its receipt with `console.log(...)` at line 276 and then calls `process.exit(ok ? 0 : 1)` at line 277. On macOS, Node writes `process.stdout` to a pipe asynchronously. `process.exit()` ends the process before the queued output is flushed, so only the bytes that fit in the pipe buffer (64 KiB) before exit reach the parent. The parent `skills/blueprint/bin/blueprint.mjs` starts the checker with `spawnSync(..., { stdio: 'pipe' })` (`runNode`, lines 51-58, called at line 908) and parses `check.stdout` at line 937. The checker exits 0, so the parent takes the success path and `JSON.parse` fails on the truncated text. Evidence:

1. The full receipt for the architecture candidate is 244170 bytes when stdout is a file, and it parses.
2. The stdout captured by `blueprint.mjs` in the real run is exactly 65536 bytes and ends inside a string (`... \"pull-request-opened\" (\"integration\" -> `). The parse error position 65308 is the start of that last unterminated string.
3. `node check-render-output.mjs cand-final.html | wc -c` printed 65536, 65536 and 131072 on 3 runs. The cut is always a multiple of 64 KiB and changes with reader timing. This is the signature of an unflushed async pipe write, not a fixed read size.
4. A standalone `spawnSync` with `stdio: 'pipe'` got 65536 bytes with status 0. The default `spawnSync` `maxBuffer` is 1 MiB, so it is not the limit.
5. The 7 sequence renders pass because their receipts are 1831 to 2287 bytes, below 1 pipe buffer.
6. With `process.exit` replaced by `process.exitCode`, 5 of 5 piped runs printed all 244170 bytes, and the export wrote `final.html`.

The guess in the ticket is correct in substance. The precise mechanism is `process.exit()` after an asynchronous pipe write on macOS, not a limit in the parent.

## Reproduction

```sh
mkdir -p /tmp/t21
cp -R ddd /tmp/t21/ddd
cp -R /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0 /tmp/t21/plugin
cd /tmp/t21
node plugin/shared/bin/ddd.mjs export blueprint ddd --deliver; echo "exit $?"
```

Output (exit 1):

```
wrote /private/tmp/t21/ddd/diagrams/final.architecture.json
wrote /private/tmp/t21/ddd/diagrams/final-F1.sequence.json
... (F2 to F7)
blueprint deliver architecture /private/tmp/t21/ddd/diagrams/final.architecture.json failed (exit 1); previous /private/tmp/t21/ddd/diagrams/final.html left untouched
  [delivery/receipt-invalid] Could not parse the successful artifact-check receipt: Unterminated string in JSON at position 65308 (line 186 column 178)
/private/tmp/t21/ddd/diagrams/final-F1.html  sha256 9779dd37...  errors 0
... (F2 to F7, errors 0)
```

The column (178 here, 100 in the original run) changes with the length of the absolute path in the receipt. The position 65308 does not change.

Measurement. An instrumented copy (`/tmp/t21/plugin-instr`) of `blueprint.mjs` saved `check.stdout` and the candidate HTML before line 937:

```sh
wc -c cap-final.html.txt                       # 65536
node plugin/skills/blueprint/scripts/check-render-output.mjs cand-final.html > full-receipt.json
wc -c full-receipt.json                        # 244170, parses
for i in 1 2 3; do node plugin/skills/blueprint/scripts/check-render-output.mjs cand-final.html | wc -c; done
                                               # 65536, 65536, 131072
```

## Fix

Set the exit code and let Node exit when stdout is drained. The diff is against `plugins/software/skills/blueprint/scripts/check-render-output.mjs` at commit `17be4c6`. That file is byte-identical to the installed copy.

```diff
--- a/plugins/software/skills/blueprint/scripts/check-render-output.mjs
+++ b/plugins/software/skills/blueprint/scripts/check-render-output.mjs
@@ -274,7 +274,7 @@
 
 const ok = checks.every((check) => check.ok) && composition.status !== 'fail';
 console.log(JSON.stringify({ ok, file: htmlPath, checks, composition }, null, 2));
-process.exit(ok ? 0 : 1);
+process.exitCode = ok ? 0 : 1;
 
 function collectArrows(fragment) {
   const arrows = [];
```

## Proof

Fixed copy `/tmp/t21/plugin`, fresh copy of `ddd/`:

```
$ node plugin/shared/bin/ddd.mjs export blueprint ddd --deliver; echo "exit $?"
wrote /private/tmp/t21/ddd/diagrams/final.architecture.json
wrote /private/tmp/t21/ddd/diagrams/final-F1.sequence.json
... (F2 to F7)
/private/tmp/t21/ddd/diagrams/final.html  sha256 ebd87dea14024f43c34a7bdab02db2966ad469595b1e925dde438961e42151d7  errors 0
/private/tmp/t21/ddd/diagrams/final-F1.html  sha256 9779dd3772d218ba77562c6c0e85858852d63f65a6654c8774579824d9dbcc6a  errors 0
/private/tmp/t21/ddd/diagrams/final-F2.html  sha256 3f3ae55f9dadf11282e73403ecbcaf7efd26f37edc6b1dcbb188f4d837be9205  errors 0
/private/tmp/t21/ddd/diagrams/final-F3.html  sha256 dffe5a3bfd18a6f998b6735b75bf51eb60f977fe63cd31f4718f546cca75970f  errors 0
/private/tmp/t21/ddd/diagrams/final-F4.html  sha256 8ae0387475f8345a3b1c0bd599e02c36c0999447b90c178dc9073aa06411f32a  errors 0
/private/tmp/t21/ddd/diagrams/final-F5.html  sha256 3a9b785e5d66ee67920cf8894927284aaf14760b1da14ee849ab8ad7b319d8c7  errors 0
/private/tmp/t21/ddd/diagrams/final-F6.html  sha256 bd3b6930f7b917cd2cc626971198add344d74208166cd620346aee4779392e60  errors 0
/private/tmp/t21/ddd/diagrams/final-F7.html  sha256 9fe39b244bc438c543c990289b8d01d723df8e721dedfce46c92dadb801993f6  errors 0
exit 0
$ ls -l ddd/diagrams/final.html
-rw-r--r--  1 MASCOTT  wheel  841611 Nov 12 11:59 ddd/diagrams/final.html
```

The F1 to F7 hashes are the same as in the failing run, so the fix does not change the sequence renders.

## Not determined

1. I did not run the fix on Linux. Node documents pipe writes as synchronous on Linux, so the bug probably does not occur there. I did not test this.
2. `check-render-output.mjs` line 31 has the same `console.error` then `process.exit(1)` pattern on the unreadable-file path. Its output is small, so I did not change it.
3. Other parsers of child stdout exist in `blueprint.mjs` (lines 403, 414, 916, 1732, 1838, 1890, 1907) and `bin/preview.mjs:176`. The callers of `check-render-output.mjs` get the same fix. I did not check if the script behind line 1838 has the same exit pattern.
4. I did not check if other plugin scripts that print large JSON and then call `process.exit` exist outside the blueprint skill.
