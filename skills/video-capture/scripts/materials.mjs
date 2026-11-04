// materials.mjs - capture the brand materials of 1 product page. Node built-ins and Chrome over CDP.
// Usage: node "<skill-dir>/scripts/materials.mjs" <project-dir> <url> [--allow-network]
//        node "<skill-dir>/scripts/materials.mjs" --check <project-dir>
//        node "<skill-dir>/scripts/materials.mjs" --self-test
// Writes <project-dir>/materials/: first-screen.png, full-page.png, colors.json, fonts.json,
// wording.md, logo-<n>.<ext> and materials.md, the list of every file with its source URL.
// A local URL (file:, data:, localhost, 127.0.0.1, ::1) needs no flag. Any other URL, and any
// request to another host, needs --allow-network, which only the operator may pass.
import { spawn, spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const CHROME = process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const MAX_SHOT_HEIGHT = 16384; // Chrome cannot capture a taller image
const LOCAL_PROTOCOLS = ['file:', 'data:', 'about:', 'blob:'];
const LOCAL_HOSTS = ['localhost', '127.0.0.1', '[::1]'];
const MIME_EXT = { 'image/svg+xml': 'svg', 'image/png': 'png', 'image/jpeg': 'jpg', 'image/webp': 'webp',
  'image/gif': 'gif', 'image/avif': 'avif', 'image/x-icon': 'ico', 'image/vnd.microsoft.icon': 'ico' };
const ARG_RULES = [ // first match is the usage error
  [(o) => !o.projectDir || !o.url, () => 'materials.mjs needs a project folder and a URL.'],
  [(o) => !URL.canParse(o.url), (o) => `${o.url} is not a URL.`],
  [(o) => !isLocalUrl(o.url) && !o.allowNetwork,
    (o) => `${o.url} is not a local URL. Pass --allow-network only when the operator agrees to read this site.`],
  [() => !existsSync(CHROME), () => `Chrome not found at ${CHROME}. Set CHROME_PATH or install Google Chrome.`],
];
const LOGO_SOURCES = [ // first match reads the logo bytes
  [(logo) => logo.kind === 'svg', (logo) => ({ bytes: Buffer.from(svgFile(logo.markup)), ext: 'svg' })],
  [(logo) => logo.url.startsWith('data:'), (logo) => dataUrlBytes(logo.url)],
  [() => true, urlBytes],
];
const MODES = { '--self-test': selfTest, '--check': checkMode };
const pause = (ms) => new Promise((ok) => setTimeout(ok, ms));
const inOrder = (list, fn) => list.reduce((chain, x) => chain.then(() => fn(x)), Promise.resolve());
const fetchBase64 = (url) => `fetch(${JSON.stringify(url)}).then((r) => r.ok ? r.arrayBuffer() : Promise.reject(new Error('HTTP ' + r.status)))
  .then((b) => { let s = ''; for (const x of new Uint8Array(b)) s += String.fromCharCode(x); return btoa(s); })`;

process.exitCode = await (MODES[process.argv[2]] || captureMode)(process.argv.slice(2));

// ---- arguments and the network rule ----
function usage(message) {
  console.error(`${message}\nUsage: node materials.mjs <project-dir> <url> [--allow-network] | --check <project-dir> | --self-test`);
  return 2;
}

function notFlag(arg) {
  return !arg.startsWith('--');
}

function parseArgs(args) {
  const [projectDir, url] = args.filter(notFlag);
  return { projectDir, url, allowNetwork: args.includes('--allow-network') };
}

function isLocalUrl(url) {
  const u = new URL(url);
  return LOCAL_PROTOCOLS.includes(u.protocol) || LOCAL_HOSTS.includes(u.hostname) || u.hostname.endsWith('.localhost');
}

function ruleHits(value, [test]) {
  return test(value);
}

function argError(opts) {
  const hit = ARG_RULES.find(ruleHits.bind(null, opts));
  return hit ? hit[1](opts) : '';
}

function timelineMeta(file) {
  return existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')).meta || {} : {};
}

function viewport(projectDir) { // the timeline size, so a rebuilt interface matches the film frame
  const meta = timelineMeta(join(projectDir, 'timeline.json'));
  return { width: meta.width || 1920, height: meta.height || 1080 };
}

// ---- capture mode ----
function captureMode(args) {
  const opts = parseArgs(args), error = argError(opts);
  return error ? usage(error) : runCapture(opts);
}

function freshDir(dir) { // the folder is this script's output: replace it whole
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  return dir;
}

async function runCapture(opts) {
  const out = freshDir(join(opts.projectDir, 'materials'));
  const rows = await withChrome(collect.bind(null, opts, out));
  writeFileSync(join(out, 'materials.md'), materialsMd(opts, rows));
  return report(opts.projectDir, rows);
}

function report(projectDir, rows) {
  const fails = checkMaterials(projectDir);
  console.log(JSON.stringify({ files: rows.files.length, blocked: rows.blocked.length, not_saved: rows.notSaved.length, fails }));
  return fails.length ? 1 : 0;
}

async function collect(opts, out, page) {
  const rows = { files: [], blocked: page.blocked, notSaved: [], size: viewport(opts.projectDir), url: opts.url, out };
  await page.open(opts, rows.size);
  const found = await page.evaluate(pageScript());
  await shootFirstScreen(page, rows);
  await shootFullPage(page, rows);
  writeData(rows, found);
  await saveLogos(page, rows, found.logos);
  return rows;
}

function addRow(rows, file, what, source = rows.url) {
  rows.files.push({ file, source, what });
}

function writeText(rows, name, text, what) {
  writeFileSync(join(rows.out, name), text);
  addRow(rows, name, what);
}

function writeJson(rows, name, value, what) {
  writeText(rows, name, JSON.stringify(value, null, 2) + '\n', what);
}

function writeData(rows, found) {
  writeJson(rows, 'colors.json', { page: rows.url, colors: found.colors, css_variables: found.vars },
    'colours by painted area, with CSS custom properties of :root');
  writeJson(rows, 'fonts.json', { page: rows.url, ...found.fonts }, 'font families with weights and sizes, loaded faces and font file URLs');
  writeText(rows, 'wording.md', wordingMd(rows.url, found), 'title, description and visible text in page order');
}

async function saveShot(page, file, params) {
  const r = await page.send('Page.captureScreenshot', { format: 'png', ...params });
  writeFileSync(file, Buffer.from(r.data, 'base64'));
}

async function shootFirstScreen(page, rows) {
  await saveShot(page, join(rows.out, 'first-screen.png'), {});
  addRow(rows, 'first-screen.png', `first screen at ${rows.size.width}x${rows.size.height} CSS px, scale 1`);
}

function cutNote(height, full) {
  return height < full ? `, cut at ${height} px of ${full}` : '';
}

async function shootFullPage(page, rows) {
  const { cssContentSize } = await page.send('Page.getLayoutMetrics');
  const full = Math.ceil(cssContentSize.height), height = Math.min(MAX_SHOT_HEIGHT, full);
  const clip = { x: 0, y: 0, width: rows.size.width, height, scale: 1 };
  await saveShot(page, join(rows.out, 'full-page.png'), { clip, captureBeyondViewport: true });
  addRow(rows, 'full-page.png', `full page at ${rows.size.width}x${height} CSS px${cutNote(height, full)}`);
}

// ---- logos ----
async function saveLogos(page, rows, logos) {
  const types = await resourceTypes(page);
  await inOrder(uniqueLogos(logos).map(numbered), saveLogo.bind(null, page, rows, types));
}

function numbered(logo, i) {
  return { ...logo, n: i + 1 };
}

function firstOfKey(seen, logo) {
  const fresh = !seen.has(logo.key);
  seen.add(logo.key);
  return fresh;
}

function uniqueLogos(logos) {
  return logos.filter(firstOfKey.bind(null, new Set()));
}

function failure(e) {
  return { error: e.message };
}

async function saveLogo(page, rows, types, logo) {
  const got = await logoBytes(page, logo, types).catch(failure);
  return got.bytes ? keepLogo(rows, logo, got) : rows.notSaved.push(`${logo.url}: ${got.error}`);
}

function keepLogo(rows, logo, got) {
  const file = `logo-${logo.n}.${got.ext}`;
  writeFileSync(join(rows.out, file), got.bytes);
  addRow(rows, file, logo.what, logo.source);
}

function urlAndType(resource) {
  return [resource.url, resource.mimeType];
}

async function resourceTypes(page) { // url -> mimeType of every resource the page loaded
  const { frameTree } = await page.send('Page.getResourceTree');
  page.frameId = frameTree.frame.id;
  return new Map(frameTree.resources.map(urlAndType));
}

async function logoBytes(page, logo, types) {
  return LOGO_SOURCES.find(ruleHits.bind(null, logo))[1](logo, page, types);
}

async function urlBytes(logo, page, types) { // the copy Chrome holds, the local file, then a fetch by the page
  const bytes = (await loadedBytes(page, logo.url)) || fileBytes(logo.url) || (await fetchedBytes(page, logo.url));
  return { bytes, ext: MIME_EXT[types.get(logo.url)] || urlExt(logo.url) };
}

function svgFile(markup) {
  return markup.includes('xmlns=') ? markup : markup.replace('<svg', '<svg xmlns="http://www.w3.org/2000/svg"');
}

function notDataUrl() {
  throw new Error('not a data URL');
}

function dataUrlBytes(url) {
  const m = url.match(/^data:([^;,]*)((?:;[^;,]*)*),(.*)$/s) || notDataUrl();
  const bytes = m[2].includes(';base64') ? Buffer.from(m[3], 'base64') : Buffer.from(decodeURIComponent(m[3]));
  return { bytes, ext: MIME_EXT[m[1]] || 'bin' };
}

function resourceBuffer(r) {
  return Buffer.from(r.content, r.base64Encoded ? 'base64' : 'utf8');
}

function nothing() {
  return undefined;
}

function loadedBytes(page, url) { // undefined when the page did not load this URL
  return page.send('Page.getResourceContent', { frameId: page.frameId, url }).then(resourceBuffer, nothing);
}

function fileBytes(url) {
  return url.startsWith('file:') ? readFileSync(fileURLToPath(url)) : undefined;
}

async function fetchedBytes(page, url) { // the request goes through the same network rule as the page
  return Buffer.from(await page.evaluate(fetchBase64(url)), 'base64');
}

function urlExt(url) {
  const m = new URL(url).pathname.match(/\.(svg|png|jpe?g|webp|gif|ico|avif)$/i);
  return m ? m[1].toLowerCase().replace('jpeg', 'jpg') : 'bin';
}

// ---- in-page collection: these functions run inside the page, so each one uses only the others ----
function pageScript() {
  const fns = [collectPage, metaContent, shownElements, isShown, ownText, isTextNode, nodeText, toHex, hex2, addColor,
    colorsOf, colorOf, byWeight, roundColor, cssVars, readRules, isRootRule, addVars, isCustomProp, addVar, fontsOf,
    hasOwnText, addFont, addOnce, byChars, faceOf, entryName, isFontFile, wordsOf, addWord, logosOf, isMarked,
    isOuterSvg, imgLogo, iconLogo, urlLogo, svgLogo];
  return `(() => { ${fns.map(String).join('\n')}\nreturn collectPage(); })()`;
}

function collectPage() {
  const shown = shownElements();
  return { title: document.title, description: metaContent('meta[name="description"]'), ogTitle: metaContent('meta[property="og:title"]'),
    colors: colorsOf(shown), vars: cssVars(), fonts: fontsOf(shown), words: wordsOf(), logos: logosOf() };
}

function metaContent(sel) {
  return document.querySelector(sel)?.content || '';
}

function shownElements() {
  return [document.documentElement, ...document.documentElement.querySelectorAll('*')].filter(isShown);
}

function isShown(el) {
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== 'hidden';
}

function isTextNode(node) {
  return node.nodeType === 3;
}

function nodeText(node) {
  return node.textContent;
}

function ownText(el) {
  return [...el.childNodes].filter(isTextNode).map(nodeText).join('').trim();
}

function toHex(css) { // rgb() or rgba() to #rrggbb; '' when fully transparent
  const m = css.match(/rgba?\(([^)]+)\)/), v = m ? m[1].split(/[\s,/]+/).filter(Boolean) : [];
  return v.length >= 3 && +(v[3] ?? 1) > 0 ? '#' + v.slice(0, 3).map(hex2).join('') : '';
}

function hex2(v) {
  return Math.round(+v).toString(16).padStart(2, '0');
}

function addColor(map, hex, role, weight) {
  if (!hex || !(weight > 0)) return;
  const e = map.get(hex) || { hex, weight: 0, roles: {} };
  e.weight += weight;
  e.roles[role] = (e.roles[role] || 0) + weight;
  map.set(hex, e);
}

function colorsOf(shown) { // weight: painted CSS px^2 of background, text and border
  const map = new Map();
  shown.forEach(colorOf.bind(null, map));
  return [...map.values()].sort(byWeight).slice(0, 24).map(roundColor);
}

function colorOf(map, el) {
  const s = getComputedStyle(el), r = el.getBoundingClientRect(), size = parseFloat(s.fontSize);
  addColor(map, toHex(s.backgroundColor), 'background', r.width * r.height);
  addColor(map, toHex(s.color), 'text', ownText(el).length * size * size * 0.5);
  addColor(map, toHex(s.borderTopColor), 'border', parseFloat(s.borderTopWidth) * (r.width + r.height) * 2);
}

function byWeight(a, b) {
  return b.weight - a.weight;
}

function roundColor(e) {
  return { hex: e.hex, weight: Math.round(e.weight), roles: Object.keys(e.roles) };
}

function cssVars() { // custom properties set on :root or html in the readable style sheets
  const out = {};
  [...document.styleSheets].flatMap(readRules).filter(isRootRule).forEach(addVars.bind(null, out));
  return out;
}

function readRules(sheet) {
  let rules = [];
  try {
    rules = [...sheet.cssRules];
  } catch {
    // a cross-origin sheet throws on cssRules: it has no readable rules
  }
  return rules;
}

function isRootRule(rule) {
  return [':root', 'html'].includes(rule.selectorText);
}

function addVars(out, rule) {
  [...rule.style].filter(isCustomProp).forEach(addVar.bind(null, out, rule.style));
}

function isCustomProp(name) {
  return name.startsWith('--');
}

function addVar(out, style, name) {
  out[name] = style.getPropertyValue(name).trim();
}

function fontsOf(shown) {
  const map = new Map();
  shown.filter(hasOwnText).forEach(addFont.bind(null, map));
  const files = performance.getEntriesByType('resource').map(entryName).filter(isFontFile);
  return { families: [...map.values()].sort(byChars), faces: [...document.fonts].map(faceOf), files };
}

function hasOwnText(el) {
  return ownText(el).length > 0;
}

function addFont(map, el) {
  const s = getComputedStyle(el), e = map.get(s.fontFamily) || { family: s.fontFamily, weights: [], sizes: [], chars: 0 };
  addOnce(e.weights, s.fontWeight);
  addOnce(e.sizes, s.fontSize);
  e.chars += ownText(el).length;
  map.set(s.fontFamily, e);
}

function addOnce(list, v) {
  list.includes(v) || list.push(v);
}

function byChars(a, b) {
  return b.chars - a.chars;
}

function faceOf(f) {
  return { family: f.family, weight: f.weight, style: f.style, status: f.status };
}

function entryName(entry) {
  return entry.name;
}

function isFontFile(url) {
  return /\.(woff2?|ttf|otf)(\?|$)/i.test(url);
}

function wordsOf() {
  const seen = new Set(), out = [];
  document.querySelectorAll('h1,h2,h3,h4,h5,h6,p,li,a,button,label,figcaption,blockquote,th,td').forEach(addWord.bind(null, out, seen));
  return out;
}

function addWord(out, seen, el) {
  const text = (el.innerText || '').replace(/\s+/g, ' ').trim();
  const fresh = text && !seen.has(text) && isShown(el);
  fresh && seen.add(text) && out.push({ tag: el.tagName.toLowerCase(), text });
}

function logosOf() { // images and inline SVG marked logo or brand, and the page icons
  const mark = '[class*=logo i],[id*=logo i],[class*=brand i],[id*=brand i],[alt*=logo i],[aria-label*=logo i],[src*=logo i]';
  const imgs = [...document.querySelectorAll('img')].filter(isMarked.bind(null, mark)).map(imgLogo);
  const svgs = [...document.querySelectorAll('svg')].filter(isOuterSvg.bind(null, mark)).map(svgLogo);
  const icons = [...document.querySelectorAll('link[rel~=icon i],link[rel=apple-touch-icon i]')].map(iconLogo);
  return [...imgs, ...svgs, ...icons];
}

function isMarked(mark, el) {
  return !!el.closest(mark);
}

function isOuterSvg(mark, el) {
  return isMarked(mark, el) && !el.parentElement.closest('svg');
}

function imgLogo(el) {
  return urlLogo('logo image', el.currentSrc || el.src);
}

function iconLogo(el) {
  return urlLogo('page icon', el.href);
}

function urlLogo(what, url) { // the source of a data: asset is the page that holds it
  return { kind: 'url', url, key: url, source: url.startsWith('data:') ? location.href : url, what };
}

function svgLogo(el, i) {
  const page = location.href;
  return { kind: 'svg', markup: el.outerHTML, key: el.outerHTML, url: `${page} inline svg ${i + 1}`, source: page, what: 'inline SVG logo' };
}

// ---- Markdown files ----
function hasValue([, v]) {
  return !!v;
}

function headLine([k, v]) {
  return `- ${k}: ${v}`;
}

function wordLine(w) {
  return `- ${w.tag}: ${w.text}`;
}

function wordingMd(url, found) {
  const head = [['title', found.title], ['meta description', found.description], ['og:title', found.ogTitle]].filter(hasValue);
  const lines = [...head.map(headLine), ...found.words.map(wordLine)];
  return `# Wording\n\nSource: ${url}\n\n${lines.join('\n')}\n`;
}

function tableCell(text) {
  return String(text).replaceAll('|', '%7C');
}

function tableRow(r) {
  return `| \`${r.file}\` | ${tableCell(r.source)} | ${tableCell(r.what)} |`;
}

function bullet(x) {
  return `- ${x}`;
}

function mdList(title, items) {
  return items.length ? `\n${title}:\n\n${items.map(bullet).join('\n')}\n` : '';
}

function mdHead(opts, size) {
  const net = opts.allowNetwork ? 'allowed by --allow-network' : 'local only, other hosts blocked';
  return `# Materials\n\nPage: ${opts.url}\nViewport: ${size.width}x${size.height} CSS px, scale 1\nNetwork: ${net}\n\n`;
}

function materialsMd(opts, rows) {
  const table = `| File | Source URL | What |\n|---|---|---|\n${rows.files.map(tableRow).join('\n')}\n`;
  return mdHead(opts, rows.size) + table + mdList('Blocked requests (not loaded)', rows.blocked) + mdList('Logos not saved', rows.notSaved);
}

// ---- check mode: materials.md lists every file in materials/, and each row has a source URL ----
function checkMode(args) {
  return args[1] ? runCheck(args[1]) : usage('--check needs a project folder.');
}

function runCheck(projectDir) {
  const fails = checkMaterials(projectDir);
  console.log(JSON.stringify({ fails }));
  return fails.length ? 1 : 0;
}

function checkMaterials(projectDir) {
  const dir = join(projectDir, 'materials'), md = join(dir, 'materials.md');
  return existsSync(md) ? listFails(dir, tableRows(readFileSync(md, 'utf8'))) : ['materials/materials.md is missing'];
}

function rowFile(row) {
  return row.file;
}

function isUnlisted(listed, file) {
  return file !== 'materials.md' && !listed.has(file);
}

function unlistedFail(file) {
  return `materials/${file} is not listed in materials.md`;
}

function listFails(dir, rows) {
  const listed = new Set(rows.map(rowFile));
  const unlisted = readdirSync(dir).filter(isUnlisted.bind(null, listed)).map(unlistedFail);
  return [...unlisted, ...rows.flatMap(rowFails.bind(null, dir))];
}

function cellText(cell) {
  return cell.trim().replace(/^`|`$/g, '');
}

function cells(line) {
  return line.startsWith('|') ? line.split('|').slice(1, -1).map(cellText) : null;
}

function isBodyRow(c) {
  return !!c && c[0] !== 'File' && !/^-+$/.test(c[0]);
}

function asRow([file, source]) {
  return { file, source };
}

function tableRows(text) {
  return text.split('\n').map(cells).filter(isBodyRow).map(asRow);
}

function rowFails(dir, row) {
  const missing = existsSync(join(dir, row.file)) ? [] : [`materials/${row.file} is listed but missing`];
  const noUrl = /^(https?|file):\/\/\S+$/.test(row.source || '') ? [] : [`materials/${row.file} has no source URL`];
  return [...missing, ...noUrl];
}

// ---- Chrome and CDP ----
async function withChrome(fn) {
  const chrome = await startChrome(mkdtempSync(join(tmpdir(), 'materials-chrome-')), 3);
  return openPage(chrome).then(fn).finally(closeChrome.bind(null, chrome));
}

function chromeArgs(userDir) {
  return ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${userDir}`, '--hide-scrollbars',
    '--mute-audio', '--no-first-run', '--no-default-browser-check', '--force-color-profile=srgb', 'about:blank'];
}

function onChromeLog(log, ok, chunk) { // resolve with the DevTools URL once Chrome prints it
  log.text += chunk;
  const m = log.text.match(/DevTools listening on (ws:\/\/\S+)/);
  return m && ok(m[1]);
}

function onChromeExit(fail, code) {
  fail(new Error(`chrome exited ${code}`));
}

function watchChrome(p, ok, fail) { // resolves '' when Chrome does not print its DevTools URL in 15 s
  p.stderr.on('data', onChromeLog.bind(null, { text: '' }, ok));
  p.on('exit', onChromeExit.bind(null, fail));
  setTimeout(ok, 15000, '').unref();
}

async function startChrome(userDir, tries) {
  const p = spawn(CHROME, chromeArgs(userDir), { stdio: ['ignore', 'ignore', 'pipe'] });
  const url = await new Promise(watchChrome.bind(null, p));
  return url ? { p, url, userDir } : retryChrome(p, userDir, tries);
}

function retryChrome(p, userDir, tries) { // Chrome can hang in its start-up fork: kill it and start again
  spawnSync('pkill', ['-KILL', '-P', String(p.pid)]); // only the children of this stuck Chrome
  p.kill('SIGKILL');
  return tries > 1 ? startChrome(userDir, tries - 1) : Promise.reject(new Error('chrome did not print its DevTools URL in 15 s, 3 tries'));
}

function killMain(p, ok) {
  p.once('exit', ok);
  p.kill();
}

async function closeChrome(chrome) {
  chrome.ws?.close();
  await new Promise(killMain.bind(null, chrome.p));
  rmSync(chrome.userDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
}

function settle(cdp, m) { // a reply: resolve or reject the send that waits for it
  const p = cdp.pending.get(m.id);
  cdp.pending.delete(m.id);
  return m.error ? p.fail(new Error(m.error.message)) : p.ok(m.result);
}

function onWsMessage(cdp, e) {
  const m = JSON.parse(e.data);
  return m.id ? settle(cdp, m) : cdp.handlers[m.method]?.(m.params);
}

function enqueue(cdp, method, params, ok, fail) {
  cdp.pending.set(++cdp.id, { ok, fail });
  cdp.ws.send(JSON.stringify({ id: cdp.id, method, params, sessionId: cdp.sessionId }));
}

function cdpSend(cdp, method, params = {}) {
  return new Promise(enqueue.bind(null, cdp, method, params));
}

function wsOpen(ws, ok, fail) {
  ws.onopen = ok;
  ws.onerror = fail;
}

async function openPage(chrome) {
  const ws = new WebSocket(chrome.url), cdp = { ws, id: 0, pending: new Map(), handlers: {}, blocked: [] };
  chrome.ws = ws;
  await new Promise(wsOpen.bind(null, ws));
  ws.onmessage = onWsMessage.bind(null, cdp);
  cdp.send = cdpSend.bind(null, cdp);
  return attachTarget(cdp);
}

async function attachTarget(cdp) {
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
  cdp.sessionId = (await cdp.send('Target.attachToTarget', { targetId, flatten: true })).sessionId;
  return Object.assign(cdp, { evaluate: evaluate.bind(null, cdp), open: openUrl.bind(null, cdp) });
}

function pageThrew(d) {
  throw new Error(d.exception?.description || d.text);
}

async function evaluate(cdp, expression) {
  const r = await cdp.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  return r.exceptionDetails ? pageThrew(r.exceptionDetails) : r.result.value;
}

function block(cdp, requestId, url) {
  cdp.blocked.push(url);
  return cdp.send('Fetch.failRequest', { requestId, errorReason: 'BlockedByClient' });
}

function route(cdp, allowNetwork, { requestId, request }) { // local requests pass; others need --allow-network
  const pass = allowNetwork || isLocalUrl(request.url);
  return pass ? cdp.send('Fetch.continueRequest', { requestId }) : block(cdp, requestId, request.url);
}

async function openUrl(cdp, opts, size) {
  cdp.handlers['Fetch.requestPaused'] = route.bind(null, cdp, opts.allowNetwork);
  await cdp.send('Fetch.enable', { patterns: [{ urlPattern: '*' }] });
  await cdp.send('Page.enable');
  await cdp.send('Emulation.setDeviceMetricsOverride', { ...size, deviceScaleFactor: 1, mobile: false });
  await cdp.send('Page.navigate', { url: opts.url });
  await waitLoaded(cdp, 300);
  await cdp.evaluate('document.fonts.ready.then(() => Promise.all([...document.images].map((i) => i.decode().catch(() => {}))))');
  await pause(300); // late layout after fonts swap in
}

async function isLoaded(cdp) {
  return (await cdp.evaluate('document.readyState')) === 'complete';
}

async function waitLoaded(cdp, tries) { // poll document.readyState for up to 15 s of real time
  return tries === 0 || (await isLoaded(cdp)) ? undefined : pause(50).then(waitLoaded.bind(null, cdp, tries - 1));
}

// ---- self-test: a local fixture page, no network ----
// Function declarations only: the mode dispatch at the top runs before a constant down here is set.
function fixtureLogo() {
  return '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" fill="#ff5a1f"/></svg>';
}

function fixtureIcon() {
  return "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='8' height='8'%3E%3Ccircle r='4' cx='4' cy='4' fill='%230b1020'/%3E%3C/svg%3E";
}

function fixturePage() {
  return `<!doctype html><meta charset="utf-8"><title>Rocketdesk</title>
<meta name="description" content="Plan launches in one place."><link rel="icon" href="${fixtureIcon()}">
<style>:root{--brand:#ff5a1f;--ink:#0b1020}html,body{margin:0;background:#0b1020;color:#f5f7fa;font:20px/1.4 "Fixture Sans",Helvetica,sans-serif}
header{display:flex;gap:12px;padding:12px}button{background:#ff5a1f;color:#fff;border:0;padding:8px 16px;font:inherit}
.tall{height:2400px}</style>
<header><a class="brand" href="/"><svg width="32" height="32"><circle cx="16" cy="16" r="14" fill="#ff5a1f"/></svg></a>
<img src="logo.svg" alt="Rocketdesk logo"></header>
<h1>Launch faster</h1><p>Every launch plan in one view.</p><button>Start free</button>
<img src="https://cdn.example.test/hero.png" alt="hero"><div class="tall"></div><p>Made for launch teams.</p>`;
}

function selfTest() {
  const dir = mkdtempSync(join(tmpdir(), 'materials-selftest-'));
  process.on('exit', rmSync.bind(null, dir, { recursive: true, force: true })); // remove the temp dir on every exit
  const results = [...testRefusals(dir), ...testCapture(dir), ...testCheck(dir)];
  const passed = results.filter(Boolean).length;
  console.log(`materials self-test: ${passed} of ${results.length} checks pass`);
  return passed === results.length ? 0 : 1;
}

function outcome(ok, what) {
  console.log(`${ok ? 'pass' : 'FAIL'}: ${what}`);
  return ok;
}

function run(args, env = process.env) {
  return spawnSync(process.execPath, [fileURLToPath(import.meta.url), ...args], { encoding: 'utf8', env });
}

function testRefusals(dir) {
  const ext = run([join(dir, 'ext'), 'https://www.example.com/']);
  const noChrome = run([join(dir, 'nochrome'), 'file:///x.html'], { ...process.env, CHROME_PATH: join(dir, 'no-chrome') });
  return [
    outcome(ext.status === 2 && ext.stderr.includes('--allow-network'), 'an external URL without --allow-network exits 2 and names the flag'),
    outcome(!existsSync(join(dir, 'ext', 'materials')), 'a refused run writes no materials/ folder'),
    outcome(noChrome.status === 2 && noChrome.stderr.includes('Chrome not found'), 'a missing Chrome binary exits 2'),
  ];
}

function fixtureUrl(project, name) {
  return pathToFileURL(join(project, 'app', name)).href;
}

function writeFixture(project) {
  mkdirSync(join(project, 'app'), { recursive: true });
  writeFileSync(join(project, 'timeline.json'), JSON.stringify({ meta: { fps: 30, width: 640, height: 360 }, scenes: [] }));
  writeFileSync(join(project, 'app', 'logo.svg'), fixtureLogo());
  writeFileSync(join(project, 'app', 'index.html'), fixturePage());
}

function readOut(dir, name) {
  const file = join(dir, name);
  return existsSync(file) ? readFileSync(file, 'utf8') : '';
}

function readOutputs(project) {
  const dir = join(project, 'materials'), text = readOut.bind(null, dir);
  return { dir, colors: JSON.parse(text('colors.json') || '{}'), fonts: JSON.parse(text('fonts.json') || '{}'),
    md: text('materials.md'), words: text('wording.md'), files: existsSync(dir) ? readdirSync(dir) : [] };
}

function exitNote(r) {
  return r.status ? `: ${r.stderr.trim()}` : '';
}

function testCapture(dir) {
  const project = join(dir, 'site');
  writeFixture(project);
  const r = run([project, fixtureUrl(project, 'index.html')]), out = readOutputs(project);
  return [outcome(r.status === 0, `a local page captures with exit 0${exitNote(r)}`), ...testShots(out), ...testData(out), ...testLists(out, project)];
}

function pngSize(file) { // width and height from the IHDR chunk; 0 x 0 when the file is missing
  const b = existsSync(file) ? readFileSync(file) : Buffer.alloc(24);
  return { width: b.readUInt32BE(16), height: b.readUInt32BE(20) };
}

function testShots(out) {
  const first = pngSize(join(out.dir, 'first-screen.png')), full = pngSize(join(out.dir, 'full-page.png'));
  return [
    outcome(first.width === 640 && first.height === 360, 'first-screen.png is 640x360, the timeline size'),
    outcome(full.width === 640 && full.height > 2400, 'full-page.png holds the whole 2400 px tall page'),
  ];
}

function colorHex(c) {
  return c.hex;
}

function familyName(f) {
  return f.family;
}

function testData(out) {
  const hexes = (out.colors.colors || []).map(colorHex), families = (out.fonts.families || []).map(familyName).join(' | ');
  return [
    outcome(hexes.includes('#ff5a1f') && hexes.includes('#0b1020'), 'colors.json holds the brand colour and the page background'),
    outcome(out.colors.css_variables?.['--brand'] === '#ff5a1f', 'colors.json holds the :root custom property --brand'),
    outcome(families.includes('Fixture Sans'), 'fonts.json names the page font family'),
    outcome(out.words.includes('Launch faster') && out.words.includes('Plan launches in one place.'), 'wording.md holds the heading and the description'),
  ];
}

function isLogoFile(name) {
  return name.startsWith('logo-');
}

function testLists(out, project) {
  const logos = out.files.filter(isLogoFile);
  return [
    outcome(logos.length === 3, `3 logo files are saved (logo image, inline SVG, page icon): ${logos.join(', ')}`),
    outcome(out.md.includes('https://cdn.example.test/hero.png'), 'materials.md lists the blocked external request'),
    outcome(out.md.includes(`| ${fixtureUrl(project, 'logo.svg')} |`), 'materials.md gives the logo image its own source URL'),
    outcome(run(['--check', project]).status === 0, '--check passes on the captured folder'),
  ];
}

function checkFixture(dir, name, change) { // a copy of the captured folder with 1 defect
  const p = join(dir, name);
  cpSync(join(dir, 'site', 'materials'), join(p, 'materials'), { recursive: true });
  change(join(p, 'materials'));
  return run(['--check', p]);
}

function addNotes(m) {
  writeFileSync(join(m, 'notes.txt'), 'x');
}

function dropFullPage(m) {
  rmSync(join(m, 'full-page.png'));
}

function blankSource(m) {
  const md = join(m, 'materials.md');
  writeFileSync(md, readFileSync(md, 'utf8').replace(/\| `colors.json` \| \S+ \|/, '| `colors.json` | - |'));
}

function testCheck(dir) {
  const extra = checkFixture(dir, 'extra', addNotes), gone = checkFixture(dir, 'gone', dropFullPage);
  const noUrl = checkFixture(dir, 'nourl', blankSource);
  return [
    outcome(extra.status === 1 && extra.stdout.includes('notes.txt is not listed'), '--check fails on a file that materials.md does not list'),
    outcome(gone.status === 1 && gone.stdout.includes('full-page.png is listed but missing'), '--check fails on a listed file that is missing'),
    outcome(noUrl.status === 1 && noUrl.stdout.includes('colors.json has no source URL'), '--check fails on a row with no source URL'),
    outcome(run(['--check', join(dir, 'empty')]).status === 1, '--check fails when materials.md is missing'),
  ];
}
