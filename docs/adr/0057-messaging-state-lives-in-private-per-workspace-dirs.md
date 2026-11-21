# Messaging state lives in private per-workspace dirs under the state root

The role registry, briefs, spool and mail hold task text, reports and the
binaries a worker runs, so only this user may read or change them (#77).
`crates/horch-core/src/messaging/private.rs` creates every messaging dir
with mode 0700 and every file with mode 0600. An existing dir that another
user owns, or that others can write to, is refused: whoever can write it can
plant a brief or an `orchestrator.id`. An own dir that only its group can
write (the umask 002 of earlier releases made it so) is made 0700 and used.
The per-workspace mailbox (`crates/horch-core/src/messaging/mailbox.rs`),
the role to pane-id registry that makes `horch tell` possible plus the brief
a spawned worker reads to learn who it is, lives under horch's state folder
keyed by workspace id, not the system temp dir, so the OS cannot clear it
from under an idle fleet.

## Consequences

- A fleet started before the move to the state folder still has its
  registry in the system temp dir (`herdr-orchestration-<ws>`). The first
  use copies it once into the state folder, when this user owns it and
  others cannot write it (#77). After that it is never read. Only
  `Mailbox::unregister` deletes from the registry.
- `private::judge_shared` judges that old temp dir more strictly, because it
  is in a dir all users share: it must be a real dir, not a symlink, and its
  group must not be able to write it either. horch never changes its mode.
- `Mail::pending` refuses mail in a dir that another user owns or can
  write: it could hold lines planted for the orchestrator to read.
- `Mailbox::trusted` refuses to read a registry dir that another user owns
  or can write, and prints why: planted id files could redirect
  `horch tell` to another pane.
- `Mailbox::read_brief` refuses a brief in a dir that another user can
  write. A brief names the binaries the worker runs, so a planted brief
  would make the worker run arbitrary programs.
- `messaging::file_safe` maps a name to 1 file or directory name: ASCII
  letters, digits and `-` stay, every other character (`/`, `:`, `.`, `_`,
  a space) becomes `_`. `.` is replaced on purpose, not kept, so a
  workspace named `..` cannot reach a parent directory when it becomes a
  mail or message directory. Channel sockets use the same rule, so every
  name horch derives from a workspace, role or pane id is mapped 1 way.
- `Mailbox::resolve` finds the workspace from `HORCH_WORKSPACE_ID`, which a
  registered pane hands to its children, so they take the cheap path.
  Otherwise it falls back to the pane's own `HERDR_PANE_ID`. herdr exports
  an internal id there (`p_2`), which `herdr pane get` upgrades to the
  public workspace id. That fallback lets these commands also work from an
  unregistered pane.
- `Mailbox::try_unregister` drops a role's registration and brief, and the
  discovery files its launch left (launch marker, harvest log). Every file
  is tried, and the error names the first removal that failed. The
  discovery files go first: once the id and brief are gone the role name
  can start again, and its new launch writes a new marker that this call
  must not delete.
