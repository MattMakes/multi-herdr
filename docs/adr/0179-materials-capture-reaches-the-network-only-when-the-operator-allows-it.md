# Materials capture reaches the network only when the operator allows it

`skills/video-capture/scripts/materials.mjs` captures the brand materials
of 1 product page with Node built-ins and Chrome over CDP. By default it
loads only local URLs (`file:`, `data:`, `localhost`, `127.0.0.1`,
`::1`). Any other URL, and any request to another host, needs
`--allow-network`, which only the operator may pass. A teammate cannot
make the capture reach the network on its own.

## Consequences

- Usage: `node "<skill-dir>/scripts/materials.mjs" <project-dir> <url>
  [--allow-network]`, `--check <project-dir>` and `--self-test`.
- It writes `<project-dir>/materials/`: `first-screen.png`,
  `full-page.png`, `colors.json`, `fonts.json`, `wording.md`,
  `logo-<n>.<ext>` and `materials.md`, the list of every file with its
  source URL.
