# Seeded chunk content

Use this when an endless or procedural world places content or solid obstacles per chunk from a
seed, the same every run, without blocked paths or load hitches.

1. Write the placement rules as data. Give one source the ownership of the data and its checks.
2. Write a generator that maps (seed, chunk coordinates) to sites. Oversample candidates a little.
   Drop candidates in passages, clearings and spawn areas. Cap the live colliders.
3. Test determinism: the same seed gives the same sites.
4. Test the rate or density over many chunks (for example 10,000) against the target range.
5. Test each exclusion rule over all loaded sites, not one chosen chunk.
6. Key any noise on world position, not on the chunk seed. Chunk-keyed noise shows seams at chunk
   borders.
7. Load sites only when chunks enter the active window. Load assets on a worker thread ahead of the
   view; a synchronous load costs a full frame or more. Reuse existing collision for simple shapes.
8. Run a perf probe with 3 trials, before and after.
9. Find a site near the start with a scratch script. Capture it and one scene per biome or region.
