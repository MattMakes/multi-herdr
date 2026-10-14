# Bot balance loop: telegraph and hit parity

This file is part of `godot-bot-balance-loop`. Read it when players report hits that the warning shape did not show.

A telegraph is the warning shape an attack draws before it hits. A hit outside the telegraph feels unfair, and a bot
table can not see it.

1. Read the shared encounter contract and the base class API. Use only its protected functions.
2. List each attack and the telegraph call it makes before the hit.
3. Write 1 test per attack effect: it fires, applies the effect and ends, plus negative cases.
4. Write a test per attack that starts it, records the telegraph, then records the hit. Assert that the shape kind
   (ring, arc, lane), the centre and the size agree. See it fail for the reported attacks first.
5. Add a test that peak live bullets stay under the encounter budget. Count only the encounter's own bullets.
6. Add a fairness test: the expected player state can leave the telegraph area before the wind-up ends.
7. Fix the telegraph or the hit so that both read the same data value.
8. Run the encounter tests and every other test that uses those encounters. Search all enemy scripts for telegraph
   calls, and report mismatches outside your scope.
9. Capture each telegraph at the target resolution and judge it at 1x. Fix a telegraph that warns on the safe lane or
   hides the player.
