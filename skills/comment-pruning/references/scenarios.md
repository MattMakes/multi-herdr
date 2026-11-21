# Worked scenarios

Each scenario gives a situation and the result that a correct run produces.
Use them to check your own run. The ids match the tests in
`script-prunecheck.md` where a mechanical part exists.

## t1: plain clean-up

Situation: the scope is a single billing module. It starts with a licence
header. Inside are a comment that retells a summing loop, a divider made of
symbols, and 3 lines of a former tax formula parked as comments.

Result: the header is untouched. The 3 other comment groups are gone. The
pruner's report names the module, gives the count and has an empty flag
list. The audit accepts. The final report says restored: none, reruns: 0.

## t2: external fact versus own surprise

Situation: 1 comment says the payment provider sends amounts in the
smallest currency unit. A second remark admits our in-house retry routine
quietly drops its first failure.

Result: the provider note stays (keep list, external fact). The retry note
goes away; that retry routine receives a reshape flag carrying 1 line of reason.
The auditor leaves the flag open for fixing and does not bring the comment
back.

## t3: suppressions

Situation: a type-checker suppression sits on a call that may receive an
absent customer record. A line-length suppression sits elsewhere.

Result: line length is style only, hence its directive survives. The directive
for the type checker goes, with a fix flag put on that call. The auditor then
changes the code in the scope so that an absent record is dealt with where
it arises (no masking guard), and lists the fix in the report.

## t4: the pruner edits code, twice

Situation: the first pass renames a function. The repeated pass alters the
body of a function.

Result: the first pass is rejected and undone, and the pass is requested
once more with the failure named. The second pass is rejected as well. The
run fails. The report shows reruns: 1 and the rejection under open work.

## t5: no scope named

Situation: the skill is called without arguments on a feature branch. 2
files have committed changes, and a third has an edit that is not committed.

Result: the scope is the changes of all 3 files, measured against `main`.

## t6: constraint comment, approved

Situation: a comment forbids changing the field order of an export record,
since a partner reads the fields by position. The offer of a test that pins
the order is approved.

Result: the test goes in first. The comment is deleted after that. The
report shows offers: 1 and enforcements added: 1.

## t7: constraint comment, unattended, no approval up front

Situation: the comment of t6, in an unattended run whose caller approved
nothing beforehand.

Result: nothing is added. The run continues. The comment is deleted (this
build's default for an offer that is not approved). The report lists the
constraint as open, not enforced, and outlines the pinning test needed.

## t8: cause beyond the scope

Situation: the scope holds a workaround under a flag; its origin is a common date
helper that sits beyond the scope and hands back local time.

Result: the least fix that fits in the scope is made. The shared helper is
not edited. The report names the helper under open work.
