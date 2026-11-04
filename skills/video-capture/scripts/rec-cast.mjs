// node rec-cast.mjs <out.cast> -- <command> [args...]: run a command, save its output and timing as asciicast v2
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
const [out, , cmd, ...args] = process.argv.slice(2);
const t0 = performance.now(), ev = [];
const p = spawn(cmd, args, { env: { ...process.env, FORCE_COLOR: '1', CLICOLOR_FORCE: '1' } });
for (const s of [p.stdout, p.stderr]) s.on('data', (d) => ev.push([+((performance.now() - t0) / 1000).toFixed(3), 'o', d.toString().replace(/\r?\n/g, '\r\n')]));
p.on('close', (code) => {
  writeFileSync(out, [JSON.stringify({ version: 2, width: 100, height: 30 }), ...ev.map((e) => JSON.stringify(e))].join('\n') + '\n');
  process.exit(code ?? 1);
});
