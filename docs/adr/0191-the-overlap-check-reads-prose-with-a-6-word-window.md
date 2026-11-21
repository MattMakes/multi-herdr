# The overlap check reads prose with a 6-word window

`scripts/dev/overlap_check.py` reads prose (the repository's `.md` and
`.txt` files) with a 6-word n-gram window (`PROSE_N`) instead of the
8-word window it uses elsewhere. A phrase of 6 words is already enough to
be someone's wording, and a window of 8 let copied 6- and 7-word phrases
through.
