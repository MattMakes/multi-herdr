"""Run a block check once for each input: a helper for the build and parse tests.

`check()` of csharp_blocks_check.py runs dotnet, and `check()` of
gdscript_blocks_check.py runs Godot. Each result depends only on the code
of the blocks, in order, not on the markdown file or line. The tests run
main() more than once on the same blocks: a copy of the sample under
another name, or the same file with another baseline. `once(check)` runs
the real check for the first call with a list of blocks; a later call with
the same code gets a copy of the result fields.
"""

from __future__ import annotations

import copy

# The fields that say where a block is; a check does not set them.
PLACE = {"path", "line", "code"}


def once(check):
    done = {}

    def run(blocks, *args, **kwargs):
        key = tuple(b.code for b in blocks)
        if key not in done:
            result = check(blocks, *args, **kwargs)
            fields = [{k: v for k, v in vars(b).items() if k not in PLACE} for b in blocks]
            done[key] = (result, copy.deepcopy(fields))
        result, fields = done[key]
        for b, f in zip(blocks, fields):
            vars(b).update(copy.deepcopy(f))
        return result

    return run
