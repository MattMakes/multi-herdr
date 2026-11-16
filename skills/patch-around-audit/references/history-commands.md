# History and comment sweeps: commands

Plain `git` and `grep`. Adjust paths and the time window to the scope. Run
them from the repository root. Each prints candidates, not findings: read
the code before you judge.

## Contents

- Comment sweep
- When was this comment written, and what changed around it since
- Functions that grow around a fixed core
- Files and functions with repeated fixes
- A fix that follows a fix
- Tests that pin a workaround
- No history available

## Comment sweep

Comment lines that state a rule, a reason or a history. The first group
matches the comment leaders of most languages. The pattern uses POSIX
classes only: `\b` and `\s` match nothing in `git grep -E` on some
systems, and the command then prints no hits and no error.

```sh
git grep -nIiE \
  '(//|#|/\*|^[[:space:]]*\*|--|<!--|""").*(^|[^[:alnum:]_])(must|never|always|do not|don.t|keep|legacy|backward|compat|workaround|work around|hack|temporary|for now|special[- ]case|for some reason|somehow|previously|used to|no longer|see above|important|careful|beware)($|[^[:alnum:]_])' \
  -- . ':!vendor' ':!node_modules' ':!*.lock' ':!*.min.*'
```

A sweep that prints nothing is a result to distrust. Before you write
"no candidates", run the command on a line you know should match.

Too many hits is normal. Narrow in 2 ways: keep the hits inside functions
that the history sweep also names, and read the densest files first:

```sh
git grep -cIiE '<same pattern>' -- . | sort -t: -k2 -rn | head -20
```

Instruction files and prompts hold anchors too. Read `CLAUDE.md`,
`AGENTS.md`, `README` sections on behaviour, and any prompt files for
rules about the code in scope.

## When was this comment written, and what changed around it since

The commit that introduced a comment:

```sh
git log -S'exact words from the comment' --format='%h %ad %s' --date=short -- path/to/file
```

The age of each line in a function. A comment line much older than the
lines around it has outlived its code:

```sh
git blame -L <start>,<end> --date=short path/to/file
```

## Functions that grow around a fixed core

The history of 1 function, oldest first. Read the patches in order and
watch which lines never change:

```sh
git log --reverse -L :<function_name>:path/to/file --format='--- %h %ad %s' --date=short
git log --reverse -L <start>,<end>:path/to/file --format='--- %h %ad %s' --date=short
```

Commits that only add lines to a file (adds, few or no deletes) are the
shape of a patch. Columns: added, deleted, file:

```sh
git log --numstat --format='%h %ad %s' --date=short -- path/to/file
```

## Files and functions with repeated fixes

Files that fix commits touch most, in the last year:

```sh
git log --since='1 year ago' -i -E \
  --grep='fix|bug|workaround|hotfix|regress|revert|again|still|actually|edge case|special case|guard|handle' \
  --name-only --format= | sort | uniq -c | sort -rn | head -30
```

Then, for a hot file, which function the fix commits land in: git prints
the enclosing function in each hunk header.

```sh
git log --since='1 year ago' -i -E --grep='fix|bug|workaround|regress|again' -p -- path/to/file \
  | grep '^@@' | sed 's/^@@[^@]*@@ *//' | sort | uniq -c | sort -rn | head
```

## A fix that follows a fix

Messages that admit the earlier fix did not hold:

```sh
git log -i -E --grep='again|still|really|actually|properly|follow[- ]up|another|also handle|missed|fix.*fix|^Revert' \
  --format='%h %ad %s' --date=short
```

Two fix commits on the same lines within days of each other are a
candidate even when the messages do not say so.

## Tests that pin a workaround

Skipped or expected-failure tests, and tests whose name or comment
mentions the workaround:

```sh
git grep -nIiE 'skip|xfail|ignore\]|todo\(|\.only|pending|known (bug|issue)|workaround|legacy|compat' -- '*test*' '*spec*'
```

For a site you are building, find the tests that call the function and
read what they assert. A test added in the same commit as a patch
usually pins that patch.

## No history available

`git rev-list --count HEAD` under about 5, or 1 huge first commit, means
the history sweep has nothing to read. Say so in the report's Method
section. The comment and shape sweeps still work; verdicts lean on
checking the claim against the code, and patches are listed by location
without a commit.
