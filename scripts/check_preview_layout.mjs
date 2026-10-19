#!/usr/bin/env node
// Layout regression check for the standalone HTML preview, not the native Slint UI.
// Uses only Node built-ins and a locally installed Chromium browser.
// Usage: node scripts/check_preview_layout.mjs [--screenshots]
// Table/code scrolling is intentional; text enlargement is distinct from browser zoom.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const output = join(root, 'target', 'preview-layout');
mkdirSync(output, { recursive: true });
const browser = process.env.CHROMIUM_PATH || [
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  '/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome',
].find(existsSync);
if (!browser) throw new Error('Set CHROMIUM_PATH to a locally installed Chromium executable.');

const source = readFileSync(join(root, 'design', 'dashboard.html'), 'utf8');
const sizes = [[320,568],[375,667],[480,800],[760,600],[800,600],[900,560],
  [1024,768],[1120,700],[1200,800],[1280,720],[1440,900],[1920,1080]];

function measure() {
  const round = value => Math.round(value * 100) / 100;
  const rect = element => element.getBoundingClientRect();
  const visible = element => element.getClientRects().length && getComputedStyle(element).visibility !== 'hidden';
  const label = element => element.id || element.className?.baseVal || element.className || element.tagName;
  const failures = [];
  if (document.documentElement.scrollWidth > innerWidth + 1) failures.push(`document overflow ${document.documentElement.scrollWidth - innerWidth}px`);
  const containers = [...document.querySelectorAll('.card,.metric,.row-card,.setting,.dialog,.welcome,.hero-top,.donut,.legend-row,.quick-action,.event,.ai-item,.search,.btn,.dialog-head,.dialog-foot,.details')].filter(visible);
  for (const container of containers) {
    const bounds = rect(container);
    const scrollsVertically = container.matches('.dialog') && ['auto','scroll'].includes(getComputedStyle(container).overflowY);
    if (scrollsVertically && container.scrollWidth > container.clientWidth + 1) failures.push('dialog horizontal overflow');
    for (const element of container.querySelectorAll('*')) {
      if (!visible(element) || element.closest('.table-wrap,pre')) continue;
      const item = rect(element);
      if (item.left < bounds.left - 1 || item.right > bounds.right + 1 || (!scrollsVertically && (item.top < bounds.top - 1 || item.bottom > bounds.bottom + 1))) {
        failures.push(`${label(container)} clips ${label(element)}`);
        break;
      }
    }
  }
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  const texts = [];
  const clips = new WeakMap();
  const textClip = element => {
    if (!element) return { left: -Infinity, top: -Infinity, right: Infinity, bottom: Infinity };
    if (clips.has(element)) return clips.get(element);
    const clip = { ...textClip(element.parentElement) };
    const style = getComputedStyle(element), bounds = rect(element);
    const clipsAxis = value => ['auto','scroll','hidden','clip'].includes(value);
    if (clipsAxis(style.overflowX)) {
      clip.left = Math.max(clip.left, bounds.left + element.clientLeft);
      clip.right = Math.min(clip.right, bounds.left + element.clientLeft + element.clientWidth);
    }
    if (clipsAxis(style.overflowY)) {
      clip.top = Math.max(clip.top, bounds.top + element.clientTop);
      clip.bottom = Math.min(clip.bottom, bounds.top + element.clientTop + element.clientHeight);
    }
    clips.set(element, clip);
    return clip;
  };
  while (walker.nextNode()) {
    const node = walker.currentNode;
    if (!node.textContent.trim() || !visible(node.parentElement) || node.parentElement.closest('script,style,.sr-only')) continue;
    const range = document.createRange();
    range.selectNodeContents(node);
    const clip = textClip(node.parentElement);
    for (const original of range.getClientRects()) {
      const bounds = { left: Math.max(original.left, clip.left), top: Math.max(original.top, clip.top),
        right: Math.min(original.right, clip.right), bottom: Math.min(original.bottom, clip.bottom) };
      if (bounds.right > bounds.left && bounds.bottom > bounds.top) texts.push({ bounds, node });
    }
  }
  for (let i = 0; i < texts.length; i++) for (let j = i + 1; j < texts.length; j++) {
    const a = texts[i], b = texts[j];
    if (a.node === b.node || !!a.node.parentElement.closest('.dialog') !== !!b.node.parentElement.closest('.dialog')) continue;
    if (Math.min(a.bounds.right, b.bounds.right) - Math.max(a.bounds.left, b.bounds.left) > 1 &&
        Math.min(a.bounds.bottom, b.bounds.bottom) - Math.max(a.bounds.top, b.bounds.top) > 1) {
      failures.push(`text overlap: ${a.node.textContent.trim().slice(0,35)} / ${b.node.textContent.trim().slice(0,35)}`);
    }
  }
  const controls = [...document.querySelectorAll('button,input,[role="button"]')].filter(visible);
  for (const element of controls) {
    const bounds = rect(element);
    if (bounds.width < 24 || bounds.height < 24) failures.push(`small target ${label(element)} ${round(bounds.width)}×${round(bounds.height)}`);
  }
  const components = [...document.querySelectorAll('button,input,th,td,.titlebar,aside,nav,.hero,.welcome,.search,.body,.metrics,.metric,.overview,.insights,.card,.card-header,.event,.storage-visual,.donut,.legend,.review-note,.quick-action,.table-tools,.filters,.ai-item,.footer,.page-card,.row-card,.callout,.settings-grid,.setting,.shortcuts,.dialog,.dialog-head,.dialog-foot,.details,.detail-path,.toast')].filter(visible).map(element => {
    const bounds = rect(element), style = getComputedStyle(element);
    return { component: label(element), x: round(bounds.x), y: round(bounds.y),
      width: round(bounds.width), height: round(bounds.height), padding: style.padding,
      margin: style.margin, gap: style.gap, fontSize: style.fontSize, lineHeight: style.lineHeight };
  });
  return { failures: [...new Set(failures)], components, textFragments: texts.length };
}

const harness = `<!doctype html><meta charset="utf-8"><title>Preview layout checks</title><body>
<iframe id="preview" style="border:0;display:block"></iframe><pre id="result">pending</pre>
<script>
const frame = document.getElementById('preview');
frame.onload = () => {
  try {
    const results = [];
    const capture = scenario => {
      const win = frame.contentWindow;
      const fonts = scenario === 'text-200' ? [...frame.contentDocument.querySelectorAll('body *')].map(element => [element, element.style.fontSize, parseFloat(win.getComputedStyle(element).fontSize)]) : [];
      for (const [element,,size] of fonts) element.style.fontSize = (size * 2) + 'px';
      const result = win.eval('(' + ${JSON.stringify(measure.toString())} + ')()');
      for (const [element,original] of fonts) element.style.fontSize = original;
      return result;
    };
    for (const scenario of ['demo','long-text','empty','text-200']) {
    frame.contentWindow.eval('simulation=false;data=structuredClone(FIXTURE);');
    if (scenario === 'long-text') frame.contentWindow.eval(
      "data.projects.forEach(p=>{p.name='Projekt'+('Langname'.repeat(12));p.path='C:/dev/'+('LangerProjektpfad'.repeat(120));});" +
      "data.resources.forEach(r=>{r.name='Ressource'+('LangerName'.repeat(12));r.evidence='Ausführliche Herkunftsbeschreibung. '.repeat(12);});" +
      "data.ai.forEach(a=>{a.client='Client'+('Langname'.repeat(8));a.label='Lange Beschreibung einer geschützten Konfiguration. '.repeat(8);});" +
      "data.activity.forEach(a=>{a.title='Aktivität'+('LangerName'.repeat(8));a.detail='Ein ausführlicher Ereignishinweis. '.repeat(8);});");
    if (scenario === 'empty') frame.contentWindow.eval('simulation=true;data={projects:[],resources:[],ai:[],activity:[]};');
    for (const [width,height] of ${JSON.stringify(sizes)}) {
      frame.style.width = width + 'px'; frame.style.height = height + 'px';
      for (const screen of ['overview','projects','packages','ai','resources','review','activity','settings']) {
        frame.contentWindow.eval('page=' + JSON.stringify(screen) + ';render();');
        results.push({scenario,width,height,screen,...capture(scenario)});
      }
      for (const [screen,action] of [['project-dialog','data.projects.length?projectDetail(data.projects[0].id):register()'],['register-dialog',"simulation=true;register()"],['cli-dialog',"action('cli')"]]) {
        frame.contentWindow.eval(action);
        if (frame.contentDocument.getElementById('modal').classList.contains('hidden')) throw new Error('Dialog did not open: ' + screen);
        results.push({scenario,width,height,screen,...capture(scenario)});
        frame.contentWindow.eval('closeModal();simulation=' + (scenario === 'empty'));
      }
    }
    }
    document.getElementById('result').textContent = JSON.stringify({sizes:${JSON.stringify(sizes)},results});
  } catch(error) { document.getElementById('result').textContent = JSON.stringify({error:String(error)}); }
};
frame.srcdoc = ${JSON.stringify(source).replaceAll('</script', '<\\/script')};
</script>`;
const harnessPath = join(output, 'harness.html');
writeFileSync(harnessPath, harness);
const run = spawnSync(browser, ['--headless', '--disable-gpu', '--no-first-run',
  '--disable-background-networking', '--disable-extensions', '--disable-sync',
  `--user-data-dir=${join(output, 'browser-profile')}`, '--dump-dom', '--virtual-time-budget=15000',
  pathToFileURL(harnessPath).href], { encoding: 'utf8', maxBuffer: 20 * 1024 * 1024, timeout: 60000, windowsHide: true });
if (run.error) throw run.error;
const match = run.stdout.match(/<pre id="result">([\s\S]*?)<\/pre>/);
if (!match || match[1] === 'pending') throw new Error(`Browser returned no measurements (exit ${run.status}): ${run.stderr.slice(-1000)}`);
const report = JSON.parse(match[1].replaceAll('&lt;', '<').replaceAll('&gt;', '>').replaceAll('&amp;', '&'));
if (report.error) throw new Error(report.error);
writeFileSync(join(output, 'measurements.json'), JSON.stringify(report, null, 2));
const failures = report.results.filter(result => result.failures.length);
for (const result of failures) console.log(`${result.scenario} ${result.width}×${result.height} ${result.screen}: ${result.failures.join('; ')}`);
console.log(`${report.results.length} preview cases; ${failures.length} failed. Full geometry: target/preview-layout/measurements.json`);
if (process.argv.includes('--screenshots')) {
  for (const width of [375,1440]) {
    const snapshotPath = join(output, `snapshot-${width}.html`);
    writeFileSync(snapshotPath, `<!doctype html><meta charset="utf-8"><body style="margin:0"><iframe id="preview" style="border:0;display:block;width:${width}px;height:1100px"></iframe><script>document.getElementById('preview').srcdoc=${JSON.stringify(source).replaceAll('</script', '<\\/script')};</script>`);
    const screenshot = spawnSync(browser, ['--headless', '--disable-gpu', '--no-first-run',
      '--disable-background-networking', '--disable-extensions', '--disable-sync',
      `--user-data-dir=${join(output, 'browser-profile')}`, `--window-size=${width},1100`,
      '--force-device-scale-factor=1', '--virtual-time-budget=2000',
      `--screenshot=${join(output, `preview-${width}.png`)}`, pathToFileURL(snapshotPath).href],
      { encoding: 'utf8', timeout: 30000, windowsHide: true });
    if (screenshot.error || screenshot.status !== 0) throw screenshot.error || new Error(`Screenshot failed: ${screenshot.stderr.slice(-1000)}`);
  }
}
process.exitCode = failures.length ? 1 : 0;
