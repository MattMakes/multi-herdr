# Bot balance loop: terms

This file is part of `godot-bot-balance-loop`. It defines the words that the skill and its references use.

- **Encounter**: 1 boss, mini boss or set fight that can end in a kill or a loss.
- **Steering**: 1 scripted bot line that stands for a player type (for example coached, tight, wide, wait, ram).
- **Band**: a target range for 1 measure of 1 encounter class (for example a first-try win rate of 60 to 80 %).
- **Meeting state**: the upgrade levels, resources and unlocks that a real player has when the encounter first rises.
- **Resource**: the thing whose loss ends a try (health, fuel, time, lives). Use your game's word.
- **Rise**: the moment an encounter appears in a run and the fight starts.
- **The import**: a headless editor import (`godot --headless --import` in Godot 4.3 and later, or `--editor --quit` before) that refreshes the class
  cache and resources after a merge.
- **Slot runner**: a small wrapper that caps how many game processes run at once and kills a process at a time limit.
