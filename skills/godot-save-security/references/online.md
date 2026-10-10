# Online values: servers, platform services, plausibility and replays

> Back to [SKILL.md](../SKILL.md).

A value that reaches another player needs an owner that the player does not
control. A client MAC or signature proves only "a copy of the game sent
this". It does not prove "this happened".

proof: not run (needs a store SDK and a service). The API names below come
from the Steamworks and GodotSteam documentation, not from a run.

## 1. Who owns which value

| Value | Owner | Client role |
| --- | --- | --- |
| Public or friend leaderboard | Server, or a platform board with trusted writes | Send the run (seed, input log, score claim) |
| Stats and achievements other players see | Server-set platform stats | Show progress only |
| Currency or items with value, trades, purchases | Server | Ask; show the server's answer |
| Competitive multiplayer state | Authoritative server (`godot-dedicated-server`) | Send input |
| "Newest save" for an online reward | Server counter | Send the envelope counter |

## 2. Steam

- **Identity.** The client calls `ISteamUser::GetAuthTicketForWebApi(identity)`
  (GodotSteam exposes it). The game server checks the ticket with the
  `ISteamUserAuth/AuthenticateUserTicket` Web API. That call needs the
  publisher Web API key, so only a server makes it. Never put the
  publisher key in the client.
- **Leaderboards.** Set "Writes" to "Trusted" on every board that matters.
  The client cannot write a score; only the `SetLeaderboardScore` Web API
  can, from your server.
- **Stats and achievements.** Set "Set By" to "Official GS" for stats that
  must hold. Use the "Increment Only", "Max Change", "Min Value" and "Max
  Value" limits.
- **Steam Cloud.** A sync service, not an integrity service. The player can
  edit a file before sync. Use the envelope counter to pick the newer file
  in a sync conflict, and show both times to the player when the counters
  are equal.
- **Consoles.** The platform save APIs and the sandbox give strong
  practical protection. The certification rules for corrupt saves apply:
  use the recovery UX in [validation-and-recovery.md](validation-and-recovery.md).

## 3. Server checks on an upload

Treat every upload as untrusted input.

1. **Auth**: a valid platform ticket for the account that submits.
2. **Plausibility**: score per second below the design maximum; currency
   gains that match the drop tables; run time inside the possible range;
   an event order that the game can produce; counters that only go up.
   Take the limits from the same game data that the client uses.
3. **Rate limits**: a cap on submissions per account per hour.
4. **Keep the evidence**: store the seed and the input log of every top
   run, so a later analysis is possible.
5. **Modded and marked profiles**: the client sends the `modified` and
   "modded" flags. The server keeps such runs off the shared board.

## 4. Replay validation

The client uploads the seed and the input log. The server runs the
simulation again and computes the score itself.

- The scored simulation must be deterministic. Godot physics and float math
  do not give the same result on every CPU and platform. Design the scored
  part with a fixed step and integer or fixed-point logic when replay
  validation is a goal.
- Replay validation does not catch every cheat. In the 2020 Trackmania
  case, top players slowed the game with a tool. The replays held valid
  inputs. The community found the cheat by analysis of the input timing in
  the stored replays.
- Keep replays. Analysis after the fact finds what a live check misses.

## 5. Bans

Never ban on a client-side check. A false positive (disk damage, a cloud
conflict, a bad mod) then bans an honest player. A server decides bans,
from server evidence.
