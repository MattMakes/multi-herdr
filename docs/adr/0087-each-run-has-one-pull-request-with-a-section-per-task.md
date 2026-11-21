# Each run has 1 pull request with a section per landed task

The pr step (`taskledger::finish::pr`, #37) keeps the run's 1 pull request,
from the run branch into the main branch, with a section for every landed
task. Each task owns the text between `<!-- horch:task T-<n> -->` and
`<!-- /horch:task T-<n> -->`. Text outside every marker is never changed.
Body updates run under the repository's landing lock, so 2 workers never
overwrite each other's section.

## Consequences

- GitHub updates a PR's `headRefOid` some seconds after a push.
  `pr::HeadWait::GITHUB` reads the PR head up to 6 times, 5 s apart. When the
  remote run branch holds the landed commit but the head still lags, the step
  passes with a note instead of failing.
- `gh pr create` refuses a head with no commit ahead of the base. When the
  landed commit is already an ancestor of the base branch, `pr::step` returns
  not-applicable instead of calling `gh`.
