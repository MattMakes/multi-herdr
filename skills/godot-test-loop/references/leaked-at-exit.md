# Leaked at exit

A clean exit is part of a pass. A suite that prints "leaked at exit" or RID leak lines is not clean.

1. Note the leak count. Run the suite with `--verbose` to list the leaked objects by type.
2. Run the same suite on the parent commit in an isolated export (export, import, run) with the
   same engine build. If the parent is clean, the leak belongs to your branch.
3. Search for the leaked type. Check static caches and preloads that hold nodes. Check that every
   test frees its nodes, meshes and materials.
4. Bisect with the filter: by folder, then by file. The filter matches file paths, not test names.
5. Change 1 cause at a time. Restore every experiment that does not change the count.
6. Fix the owner: free the node, create it only when used, or break the cycle. Run the full suite
   until no leak line prints.

A known trap: a static function whose return type is its own class can form a reference cycle.
Return an untyped value and let callers cast. Another: a headless "leak" can come from the
placeholder renderer; confirm a render leak in a window run.
