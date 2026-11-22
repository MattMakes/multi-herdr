# Dependency changes for Astryx

Two packages matter:

- `@astryxdesign/core` is the component library.
- `@astryxdesign/cli` provides the `astryx` program.

They belong together in a project, with one and the same version number.

`<X.Y.Z>` below is a placeholder. This file names no real version on purpose.

## Ground rules

1. **Propose, then wait.** Describe the change to the requester and act only
   on an approving reply. In a fleet, the proposal is a message to the
   orchestrator.
2. **Exact numbers.** Write each version as a plain number such as
   `<X.Y.Z>`. No `^`, no `~`, no `>=`, no `*`, no tag such as `latest` in the
   dependency list.
3. **One number for both.** The library and the CLI package carry an
   identical version.
4. **Project only.** Never pass a global flag to `npm`. Never install
   anything outside the project folder.
5. **No setup run on the side.** Adding packages does not include
   `astryx init`. That command runs only when the requester asks for it.
6. **`npm` only.** If the project's lock file or scripts show that some other
   package manager is in charge, do not run it. Give the requester the exact
   package names and version to add.

## Case A: a new project that should use Astryx

1. If no project folder exists, the proposal includes the folder: its name
   and where it goes.
2. Ask which version to use. Do not pick a number from memory. If the
   requester answers "the newest" or asks you to look, you may run
   `npm view @astryxdesign/core version` and put the number it prints into
   the proposal.
3. Send the proposal and wait.
4. After a yes: create the folder, set the project up the way the request
   describes (a `package.json` must exist), then run from the project root:

       npm pkg set "dependencies.@astryxdesign/core=<X.Y.Z>" "dependencies.@astryxdesign/cli=<X.Y.Z>"  # writes the two entries and fetches nothing; ask the requester to fetch them, then go on with step 5

5. Open the dependency list and confirm both entries are exact and equal.
   Run the presence check again; the CLI should now be found in the project.

Proposal wording you can adapt:

> This app needs a project folder and two packages before I can build UI
> with Astryx. I propose: folder `<name>`; `@astryxdesign/core` and
> `@astryxdesign/cli`, both fixed at `<X.Y.Z>`. I will not run
> `astryx init` unless you ask. Shall I go ahead?

## Case B: the library is listed, the CLI package is not

1. Read the library's entry in the dependency list.
2. If the entry is a plain number, that number is the version to propose for
   `@astryxdesign/cli`, written the same exact way.
3. If the entry is a range, read the number that is really installed from the
   `package.json` under `node_modules/@astryxdesign/core/`. Propose that
   exact number for the CLI package, and mention the range so the requester
   can decide otherwise.
4. After a yes:

       npm pkg set "dependencies.@astryxdesign/cli=<X.Y.Z>"  # writes the entry and fetches nothing; ask the requester to fetch the package, then go on with step 5

5. Confirm the new entry, then run the presence check again.

Proposal wording you can adapt:

> The project has `@astryxdesign/core` at `<X.Y.Z>` but no
> `@astryxdesign/cli`, so the `astryx` program is not available here. I
> propose adding `@astryxdesign/cli` fixed at `<X.Y.Z>`. Until you answer I
> can go on by reading the package files in `node_modules`.

## Case C: an existing app that does not use Astryx

Do nothing about Astryx. Work with the UI library the app already has, and
do not bring Astryx up yourself.

Only a request that asks for Astryx changes this. Then propose both packages
as in case A, without the folder part, and wait for the answer.

## Case D: the two packages carry different versions

1. Report both numbers and where you read them.
2. Propose bringing them to one number. Which of the two numbers wins is the
   requester's call.
3. Change nothing until the reply. Lookups made in the meantime may not
   match the installed library, so say that when you report CLI answers.

## Case E: the proposal is turned down

Add no package. Continue with what is in the project:

- Library installed, CLI missing: read the package files under
  `node_modules/@astryxdesign/` for component facts.
- No Astryx at all: do the UI work with the library the project uses.

Do not ask again in the same task unless the requester reopens the question.

## When the registry cannot be reached

`npm view` talks to `registry.npmjs.org`. If that lookup fails for lack of
network, report the failure and ask the requester for the number instead.
