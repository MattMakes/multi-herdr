# Physics interpolation

Use this when a project turns on physics interpolation and the scene tree holds SubViewports and a
moving player node.

1. List every node that moves the player or follows it (cameras, markers, trails).
2. Write the tests first: the player subtree interpolates, and no other code moves the player
   outside a physics tick.
3. Set the interpolation mode on each SubViewport that holds no player node to off, before
   `add_child`. Do not trust the root setting to reach SubViewports.
4. Add a test that walks every SubViewport of a run and checks its mode.
5. Call `reset_physics_interpolation()` on the moved node after each teleport. Prove the reset test:
   remove the line and check that the test fails.
6. Place screen marks and effects from the interpolated transform (for example
   `get_global_transform_interpolated()`), not from the physics position.
7. When older camera tests fail with interpolation on, decide with the owner whether the test runner
   turns it off. Do not weaken the tests in silence.
8. Measure frame pacing in a quiet window: 3 takes, medians.
