# The shape of the diag tree

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

The shape of the diagnostic tree is deliberate:

- **Hub and spokes, 1 layer.** Each spoke is 1 upgradeable stat. There are no gates, so the only choice is the stat.
- **Caps that grow.** Every spoke caps at a step (for example 5). When a share of the spokes (for example 80 %) is at
  the cap, every cap rises by the step. The cap forces breadth. The slack lets a bot skip 1 spoke and still progress,
  which makes leave-one-out measurement possible.
- **1 pooled currency, 1 price curve.** Every currency counts toward 1 price, and the Nth level costs the same on any
  spoke. So the data compares the value of stats, not their prices. A buy spends the rarest currency first, and the
  trace records every currency, so the report shows when each rare currency really arrives.
