# Threat model: who tampers, and what a client check can do

> Back to [SKILL.md](../SKILL.md).

Write the threat model before you pick a technique. The model decides how
much protection each asset needs. Most single-player saves need validation
and backups, not cryptography.

## 1. Who tampers, and why

| Actor | Goal | Typical method | Harm | Can a client check stop it? |
| --- | --- | --- | --- | --- |
| Curious single player | More currency, unlock all, skip the grind | A text editor on the save; a public save editor | Low: only that player | Detect only. A determined player wins. |
| Leaderboard cheater | A top score | Edit the score file; slow the game; edit memory; forge the upload | High: honest players lose rank | No. A server must validate. |
| Economy or trading cheater | Items or currency with real value | Duplicate items; replay requests; edit the client | High: market and revenue | No. A server must own the state. |
| Achievement hunter | Platform achievements | Achievement-unlock tools; edit stats | Medium: global unlock rates become false | No. Use server-set stats. |
| Save sharer | Give a finished save to a friend | Copy the save folder | Low to medium | Partly: bind the save to the install or account. |
| Modder | Change balance or content | Edit data JSON; add a PCK | Positive or neutral when the game supports mods | Do not stop it in single-player. Mark the run as modded. |
| Rollback player | Undo a loss; use a reward again | Keep a copy of the save; put it back | Low offline; high with online rewards | Detect only. A server enforces. |
| Malware or another user | Steal tokens or personal data | Read files in the user profile | High for credentials | Yes, for the player's own secrets: OS keystore. |
| Accident (no attacker) | None | Crash during a write; full disk; cloud conflict; bad mod | High: lost progress | Yes: atomic writes, backups, validation. |

The last row causes most "bad save" reports. Design for it first.

## 2. What a client-side check can do

**Can:**

- Detect an edit by a person who does not have the key.
- Raise the effort above "open the file in a text editor".
- Detect accidental damage.
- Stop a copied save from verifying on another install or account.
- Mark a profile as "modified", so it stays off shared boards.

**Cannot:**

- Keep a key secret from the owner of the machine. The game must use the
  key, so the key is in memory or in the binary. Public tools (GDRE Tools)
  recover Godot 2, 3 and 4 projects, and decrypt a PCK when
  they have the key.
- Stop memory editors or a changed game binary. Native code and anti-cheat
  software raise the cost. They do not make the client trusted.

## 3. When only a server works

Use a server, or a platform feature that is a server, when a false value
harms **another** player or the business:

1. Global or friend leaderboards.
2. Stats and achievements that other players see.
3. Currency or items with real value, that players trade or buy.
4. Competitive multiplayer state.
5. Anti-rollback that must hold. Only a counter outside the player's
   control proves "this is the newest save".

[online.md](online.md) gives the server and Steam options.

## 4. Write the table for your game

Make 1 row per file or data set. Fill every column. The last column picks
the technique from the SKILL.md table.

| Asset | Path | Who writes it | Who reads it | Who is harmed by an edit | Decision |
| --- | --- | --- | --- | --- | --- |
| Meta save | `user://save.json` | Game | Game | Only the player | Validation, backups, recovery |
| Settings | `user://settings.cfg` | Game | Game | Nobody | Validation |
| Balance data | `res://data/*.json` | Developer | Game | Nobody offline | Nothing now |
| Best time | in the meta save | Game | Game | Nobody while local | Nothing now; server before a public board |

Record the designer's choice for a bad tag on a single-player save:
`REFUSE` (try the backups) or `LOAD_MARKED` (load it and mark the
profile). `LOAD_MARKED` suits most single-player games.

## 5. Worked example: a single-player prototype

A small Godot roguelite has a JSON meta save with an atomic write and 1
backup, a `ConfigFile` settings file with a per-key sanitizer, local
telemetry logs, balance JSON in `res://data/`, and a local best time. No
value reaches another player. The decision:

**Now:**

1. No encryption and no HMAC. The cost (keys, false alarms, support) is
   larger than the benefit, and a readable save helps playtests.
2. Clamp every save field to the ranges in the game data. Drop unknown
   ids. Reject non-finite numbers: Godot's JSON parser turns `1e999` into
   `inf`.
3. Add a second backup, a quarantine copy, a player-visible recovery
   message and a read-only branch for a newer save.
4. Keep `encrypt_pck=false` and the balance data as plain JSON.
5. Keep telemetry local. A future upload is untrusted input.

**Before a store release with achievements:** add the HMAC envelope as a
marker, keyed per install plus account id. Use the envelope counter to pick
between 2 conflicting cloud files.

**Before a shared leaderboard or daily runs:** a trusted board, a small
server that checks the auth ticket, the run seed and input log, and
plausibility limits from the game rules.
