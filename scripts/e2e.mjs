// Real Windows WebView2 / Tauri backend. All mutations stay under .test-data;
// optional personal fixtures are read-only inputs imported into the disposable library.
import assert from "node:assert/strict";
import {
  mkdir,
  writeFile,
  readFile,
  mkdtemp,
  access,
  readdir,
  realpath,
  lstat,
  copyFile,
} from "node:fs/promises";
import { createReadStream } from "node:fs";
import { spawn } from "node:child_process";
import { resolve, join, relative, isAbsolute, sep } from "node:path";
import { createHash } from "node:crypto";
import { prepareBlendFixture, saveMirrorFixture } from "./e2e-fixtures.mjs";

const root = resolve(".");
await mkdir(".test-data", { recursive: true });
const testRoot = await mkdtemp(resolve(".test-data", "desktop-"));
const settings = join(testRoot, "settings");
const library = join(testRoot, "library");
const source = join(testRoot, "source é");
const blender = resolve(
  process.env.EDGE_LOCK_E2E_BLENDER ??
    "C:/Program Files/Blender Foundation/Blender 5.1/blender.exe",
);
await access(blender);
await mkdir(source);
await mkdir(settings);
await writeFile(
  join(settings, "settings.json"),
  JSON.stringify({ libraryPath: null, blenderPath: blender }),
);
await mkdir(join(source, "empty"));
await prepareBlendFixture(blender, source, join(testRoot, "fixture-blender"));
await writeFile(join(source, "notes.txt"), "original");
const sourceOriginal = await fileManifest(source);
const executable = resolve(
  process.env.EDGE_LOCK_E2E_EXE ?? "target/debug/edge-lock.exe",
);
const driver = resolve(
  process.env.TAURI_DRIVER ?? ".tools/tauri-driver/bin/tauri-driver.exe",
);
const native = resolve(
  process.env.EDGE_DRIVER ?? ".tools/webdriver/msedgedriver.exe",
);
await Promise.all([executable, driver, native].map((file) => access(file)));
// Release intentionally ignores the test settings override. Refuse it before
// launching anything, so a custom executable can never use personal settings.
assert.ok(
  (await readFile(executable)).includes(Buffer.from("EDGE_LOCK_TEST_DATA_DIR")),
  "The desktop test requires a debug build with isolated settings support; release binaries are refused.",
);
const port = Number(process.env.EDGE_LOCK_DRIVER_PORT ?? 4444);
const base = `http://127.0.0.1:${port}`;
const child = spawn(
  driver,
  [
    "--port",
    String(port),
    "--native-port",
    String(port + 1),
    "--native-driver",
    native,
  ],
  {
    cwd: root,
    windowsHide: true,
    env: {
      ...process.env,
      EDGE_LOCK_TEST_DATA_DIR: settings,
      WEBVIEW2_USER_DATA_FOLDER: join(testRoot, "webview"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  },
);
let driverLog = "";
child.stdout.on(
  "data",
  (value) => (driverLog = (driverLog + value).slice(-50_000)),
);
child.stderr.on(
  "data",
  (value) => (driverLog = (driverLog + value).slice(-50_000)),
);
let session;
const report = { testRoot, startedAt: new Date().toISOString(), checks: [] };
const canonicalConfig = {
  primaryFile: null,
  scene: null,
  collection: null,
  applyModifiers: false,
};
async function request(method, path, data) {
  const response = await fetch(base + path, {
    method,
    headers: { "Content-Type": "application/json" },
    body: data === undefined ? undefined : JSON.stringify(data),
    signal: AbortSignal.timeout(120_000),
  });
  const result = await response.json();
  if (!response.ok || result.value?.error)
    throw new Error(`${method} ${path}: ${JSON.stringify(result)}`);
  return result.value;
}
async function until(check, timeout = 30_000) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try {
      const value = await check();
      if (value) return value;
    } catch (error) {
      last = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw last ?? new Error("Timeout waiting for desktop state");
}
async function script(code, args = []) {
  return request("POST", `/session/${session}/execute/sync`, {
    script: code,
    args,
  });
}
async function invoke(command, args = {}) {
  const result = await request("POST", `/session/${session}/execute/async`, {
    script:
      "const done=arguments[arguments.length-1];window.__TAURI_INTERNALS__.invoke(arguments[0],arguments[1]).then(value=>done({value})).catch(error=>done({error}));",
    args: [command, args],
  });
  if (result.error)
    throw Object.assign(
      new Error(result.error.message ?? JSON.stringify(result.error)),
      result.error,
    );
  return result.value;
}
async function job(requestValue, timeout = 120_000) {
  const id = await invoke("start_job", { request: requestValue });
  return awaitJob(id, timeout);
}
async function awaitJob(id, timeout) {
  const result = await until(async () => {
    const job = await invoke("get_job", { id });
    return ["completed", "failed", "cancelled"].includes(job.status)
      ? job
      : null;
  }, timeout);
  if (result.status !== "completed")
    throw Object.assign(
      new Error(result.error?.message ?? result.status),
      result.error,
    );
  return result.result;
}
async function concurrentPreviews(requestValue) {
  const result = await request("POST", `/session/${session}/execute/async`, {
    script:
      "const done=arguments[arguments.length-1]; const request=arguments[0]; Promise.all([0,1].map(()=>window.__TAURI_INTERNALS__.invoke('start_job',{request}))).then(ids=>done({value:ids})).catch(error=>done({error}));",
    args: [requestValue],
  });
  if (result.error)
    throw Object.assign(new Error(result.error.message), result.error);
  const ids = result.value;
  assert.equal(ids.length, 2);
  assert.notEqual(ids[0], ids[1]);
  return Promise.all(ids.map((id) => awaitJob(id, 300_000)));
}
async function clickText(text) {
  const element = await request("POST", `/session/${session}/element`, {
    using: "xpath",
    value: `//button[contains(normalize-space(.),${JSON.stringify(text)})]`,
  });
  await request(
    "POST",
    `/session/${session}/element/${element["element-6066-11e4-a52e-4f735466cecf"]}/click`,
    {},
  );
}
async function newSession() {
  const result = await request("POST", "/session", {
    capabilities: {
      alwaysMatch: { "tauri:options": { application: executable } },
    },
  });
  session = result.sessionId;
  await request("POST", `/session/${session}/timeouts`, { script: 120_000 });
  await until(() =>
    script(
      'return !!window.__TAURI_INTERNALS__ && !!document.querySelector(".app-shell");',
    ),
  );
}
async function screenshot(name) {
  const base64 = await request("GET", `/session/${session}/screenshot`);
  await writeFile(join(testRoot, name), Buffer.from(base64, "base64"));
  return base64;
}

async function clickSelector(selector) {
  const element = await request("POST", `/session/${session}/element`, {
    using: "css selector",
    value: selector,
  });
  await request(
    "POST",
    `/session/${session}/element/${element["element-6066-11e4-a52e-4f735466cecf"]}/click`,
    {},
  );
}

let viewportEmulated = false;
async function viewport(width, height) {
  if (viewportEmulated) {
    await request("POST", `/session/${session}/ms/cdp/execute`, {
      cmd: "Emulation.clearDeviceMetricsOverride",
      params: {},
    });
    viewportEmulated = false;
  }
  for (let attempt = 0; attempt < 3; attempt++) {
    const size = await script(
      "return {width: innerWidth, height: innerHeight};",
    );
    if (size.width === width && size.height === height) return;
    const rect = await request("GET", `/session/${session}/window/rect`);
    await request("POST", `/session/${session}/window/rect`, {
      width: rect.width + width - size.width,
      height: rect.height + height - size.height,
    });
  }
  // Windows rounds the minimum native window width at fractional DPI. Keep
  // the CSS test viewport exact without changing production window limits.
  await request("POST", `/session/${session}/ms/cdp/execute`, {
    cmd: "Emulation.setDeviceMetricsOverride",
    params: { width, height, deviceScaleFactor: 0, mobile: false },
  });
  viewportEmulated = true;
  assert.deepEqual(
    await script("return {width: innerWidth, height: innerHeight};"),
    { width, height },
  );
}

async function assertTheme() {
  await until(() =>
    script(`return Array.from(document.fonts).some(font =>
    font.family.includes('Space Grotesk') && font.status === 'loaded');`),
  );
  const theme = await script(`
    const logo = document.querySelector('.brand img');
    const brandText = document.querySelector('.brand > div');
    const sidebar = document.querySelector('.sidebar').getBoundingClientRect();
    const dot = document.querySelector('.live-dot');
    const main = document.querySelector('.main-area');
    return {
      viewport: {width: innerWidth, height: innerHeight},
      font: getComputedStyle(document.body).fontFamily,
      background: getComputedStyle(document.documentElement).getPropertyValue('--bg').trim(),
      logo: { loaded: logo.complete && logo.naturalWidth > 0, width: logo.getBoundingClientRect().width },
      brandFits: Array.from(brandText.children).every(el => {
        const text = document.createRange();
        text.selectNodeContents(el);
        return text.getBoundingClientRect().right <= sidebar.right;
      }),
      dot: dot ? getComputedStyle(dot).backgroundColor : null,
      overflow: document.documentElement.scrollWidth > innerWidth || main.scrollWidth > main.clientWidth,
    };
  `);
  assert.match(theme.font, /Space Grotesk/);
  assert.equal(theme.background, "#172a3e");
  assert.equal(theme.logo.loaded, true);
  assert.ok(
    theme.logo.width >
      (theme.viewport.width <= 860
        ? 46
        : theme.viewport.width <= 1080
          ? 50
          : 56),
    "The logo must be larger than before at each sidebar breakpoint.",
  );
  assert.equal(
    theme.brandFits,
    true,
    "The enlarged logo and wordmark must fit the sidebar.",
  );
  assert.equal(theme.dot, "rgb(238, 38, 143)");
  assert.equal(
    theme.overflow,
    false,
    "The document and main area must not overflow horizontally.",
  );
  return theme;
}

async function assertBlueButton(selector, capture) {
  await script(
    "document.querySelector(arguments[0]).scrollIntoView({block: 'center'});",
    [selector],
  );
  await request("POST", `/session/${session}/actions`, {
    actions: [
      {
        type: "pointer",
        id: "theme",
        parameters: { pointerType: "mouse" },
        actions: [
          { type: "pointerMove", duration: 0, origin: "viewport", x: 2, y: 2 },
        ],
      },
    ],
  });
  const read = () =>
    script(
      `
    const el = document.querySelector(arguments[0]), s = getComputedStyle(el), r = el.getBoundingClientRect();
    return { gradient: s.backgroundImage, color: s.color, width: r.width, height: r.height,
      x: Math.round(r.x+r.width/2), y: Math.round(r.y+r.height/2),
      disabled: el.disabled, outline: s.outlineStyle };
  `,
      [selector],
    );
  const idle = await read();
  assert.match(idle.gradient, /linear-gradient/);
  assert.equal(idle.disabled, false);
  await request("POST", `/session/${session}/actions`, {
    actions: [
      {
        type: "pointer",
        id: "theme",
        parameters: { pointerType: "mouse" },
        actions: [
          {
            type: "pointerMove",
            duration: 0,
            origin: "viewport",
            x: idle.x,
            y: idle.y,
          },
        ],
      },
    ],
  });
  // Sample during and after the transition: the gradient must never disappear.
  for (let index = 0; index < 4; index++) {
    const hovered = await read();
    assert.equal(hovered.gradient, idle.gradient);
    assert.equal(hovered.color, idle.color);
    assert.equal(hovered.width, idle.width);
    assert.equal(hovered.height, idle.height);
  }
  await screenshot(capture);
  await request("POST", `/session/${session}/actions`, {
    actions: [
      {
        type: "pointer",
        id: "theme",
        parameters: { pointerType: "mouse" },
        actions: [{ type: "pointerDown", button: 0 }],
      },
    ],
  });
  const active = await read();
  assert.equal(active.gradient, idle.gradient);
  assert.equal(active.width, idle.width);
  assert.equal(active.height, idle.height);
  await screenshot(capture.replace("hover", "active"));
  // Release outside the button so the style check never launches an export or Blender.
  await request("POST", `/session/${session}/actions`, {
    actions: [
      {
        type: "pointer",
        id: "theme",
        parameters: { pointerType: "mouse" },
        actions: [
          { type: "pointerMove", duration: 0, origin: "viewport", x: 2, y: 2 },
          { type: "pointerUp", button: 0 },
        ],
      },
    ],
  });
  await script("document.querySelector(arguments[0]).focus();", [selector]);
  await request("POST", `/session/${session}/actions`, {
    actions: [
      {
        type: "key",
        id: "theme-keyboard",
        actions: [
          { type: "keyDown", value: "\uE012" },
          { type: "keyUp", value: "\uE012" },
        ],
      },
    ],
  });
  assert.notEqual((await read()).outline, "none");
  return idle;
}

async function testVisualRefreshAndCanonicalRules() {
  report.visualRefresh = {};
  for (const [width, height] of [
    [1440, 940],
    [1024, 700],
    [860, 700],
  ]) {
    await viewport(width, height);
    report.visualRefresh[width] = await assertTheme();
    await screenshot(`ui-${width}-detail.png`);
    await clickText("Fichiers");
    await script(
      "document.querySelector('.asset-bottom-panel').scrollIntoView({block:'end'});",
    );
    await screenshot(`ui-${width}-table.png`);
    await clickText("Organiser");
    await screenshot(`ui-${width}-modal.png`);
    assert.equal(
      await script(
        "const r=document.querySelector('dialog[open]').getBoundingClientRect(); return r.left>=0 && r.right<=innerWidth;",
      ),
      true,
    );
    await clickSelector('dialog[open] [aria-label="Fermer"]');
    await clickText("Retour à la bibliothèque");
    await waitForThumbnail("Éclaireur");
    await assertTheme();
    await screenshot(`ui-${width}-grid.png`);
    await clickSelector('[aria-label="Vue liste"]');
    await screenshot(`ui-${width}-list.png`);
    await assertTheme();
    await clickSelector('[aria-label="Vue grille"]');
    await clickText("Éclaireur");
    await waitForModel("model.glb");
  }
  await viewport(1440, 940);
  await assertBlueButton(".blender-button", "ui-blender-hover.png");
  assert.equal(
    await script('return !!document.querySelector(".config-form");'),
    false,
  );
  report.checks.push(
    "Thème à 1440/1024/860 px : police locale, logo, grille/liste/tableau/modales ; aucun ancien menu de source ou choix des modificateurs",
  );
}

async function testAutomaticMirror(variant) {
  assertWithin(await realpath(testRoot), await realpath(variant.workspacePath));
  await until(() =>
    script(
      `const button = Array.from(document.querySelectorAll('button')).find(el => el.textContent.includes('Créer une version')); return button?.disabled && document.querySelector('.workspace-state')?.textContent.includes('À jour');`,
    ),
  );
  await assert.rejects(
    () =>
      job({
        kind: "snapshot",
        variantId: variant.id,
        message: "Version vide refusée",
      }),
    /Aucune modification/,
  );
  const blendFile = join(variant.workspacePath, "model.blend");
  const glbFile = join(variant.workspacePath, "model.glb");
  const beforeGlb = await fileHash(glbFile);
  await saveMirrorFixture(blender, blendFile, join(testRoot, "fixture-edit"));
  const savedBlend = await fileHash(blendFile);
  // No refresh/job call here: only the filesystem save may trigger generation.
  await until(async () => (await fileHash(glbFile)) !== beforeGlb, 300_000);
  await until(
    () =>
      script(`
    const metric = Array.from(document.querySelectorAll('.asset-metrics > div')).find(el => el.querySelector('span')?.textContent === 'Triangles');
    const button = Array.from(document.querySelectorAll('button')).find(el => el.textContent.includes('Créer une version'));
    return metric?.querySelector('strong')?.textContent.trim() === '2' && button && !button.disabled && !!document.querySelector('.viewer-help');
  `),
    300_000,
  );
  assert.equal(
    await fileHash(blendFile),
    savedBlend,
    "Synchronization must not save or apply modifiers in the .blend",
  );
  await screenshot("11-mirror-autosync.png");
  report.checks.push(
    "Sauvegarde réelle d’un Mirror : GLB et géométrie d’aperçu passent de 1 à 2 triangles automatiquement, .blend inchangé par Edge Lock, version vide refusée",
  );
}

async function testStrictImportRules(variant) {
  const invalidRoot = join(testRoot, "invalid-projects");
  await mkdir(invalidRoot);
  for (const [name, files] of [
    ["GLB seul", ["only.glb"]],
    ["Deux sources", ["first.blend", "nested/second.blend"]],
    ["Deux GLB", ["model.blend", "first.glb", "second.glb"]],
  ]) {
    const directory = join(invalidRoot, name);
    await mkdir(directory);
    for (const file of files) {
      const target = join(directory, file);
      await mkdir(resolve(target, ".."), { recursive: true });
      await copyFile(
        join(
          variant.workspacePath,
          file.endsWith(".blend") ? "model.blend" : "model.glb",
        ),
        target,
      );
    }
    const before = await fileManifest(directory);
    await assert.rejects(
      () =>
        job({
          kind: "import",
          source: directory,
          name,
          config: canonicalConfig,
        }),
      /non conforme/i,
    );
    assert.deepEqual(await fileManifest(directory), before);
  }
  report.checks.push(
    "Imports GLB seul, plusieurs .blend ou plusieurs GLB refusés sans modifier leurs fichiers",
  );
}

async function previewPixels(selector, name) {
  await until(() =>
    script(
      `
      const element = document.querySelector(arguments[0]);
      if (!element || element.getBoundingClientRect().width <= 0) return false;
      const results = element.closest('.asset-results');
      if (results?.getAttribute('aria-busy') === 'true' || results?.classList.contains('is-loading')) return false;
      // The gallery fades while catalog results load; wait for the transition too.
      for (let parent = element; parent; parent = parent.parentElement)
        if (Number(getComputedStyle(parent).opacity) < 0.999) return false;
      return true;
    `,
      [selector],
    ),
  );
  const base64 = await screenshot(name);
  const result = await request("POST", `/session/${session}/execute/async`, {
    script: `
      const [base64, selector, done] = arguments;
      const element = document.querySelector(selector);
      if (!element) { done({ error: 'Missing texture capture region: ' + selector }); return; }
      const rect = element.getBoundingClientRect();
      const image = new Image();
      image.onerror = () => done({ error: 'Unable to decode the WebDriver screenshot' });
      image.onload = () => {
        try {
          const scaleX = image.naturalWidth / innerWidth;
          const scaleY = image.naturalHeight / innerHeight;
          const x = Math.max(0, Math.floor(rect.left * scaleX));
          const y = Math.max(0, Math.floor(rect.top * scaleY));
          const width = Math.min(image.naturalWidth - x, Math.ceil(rect.width * scaleX));
          const height = Math.min(image.naturalHeight - y, Math.ceil(rect.height * scaleY));
          if (width <= 0 || height <= 0) throw new Error('Texture region is outside the viewport');
          const canvas = document.createElement('canvas');
          canvas.width = width; canvas.height = height;
          const context = canvas.getContext('2d');
          context.drawImage(image, x, y, width, height, 0, 0, width, height);
          const pixels = context.getImageData(0, 0, width, height).data;
          let red = 0, green = 0, blue = 0;
          let textureMin = width, textureMax = -1, brightMin = width, brightMax = -1;
          let subjectMin = width, subjectMax = -1;
          for (let i = 0; i < pixels.length; i += 4) {
            const [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]];
            const isRed = r > 60 && r > g * 1.5 && r > b * 1.5;
            // ACES and studio image-based lighting brighten saturated greens.
            // White/missing textures still fail both chromatic dominance checks.
            const isGreen = g > 60 && g > r * 1.12 && g > b * 1.25;
            if (isRed) red++;
            if (isGreen) green++;
            const px = (i / 4) % width, py = Math.floor(i / 4 / width);
            // Exclude the viewer badge, controls and help text from framing checks.
            if (py < height * 0.2 || py > height * 0.8) continue;
            if (b > 60 && b > r * 1.5 && b > g * 1.5) blue++;
            if (isRed || isGreen) {
              textureMin = Math.min(textureMin, px); textureMax = Math.max(textureMax, px);
            }
            const isBright = r > 160 && g > 160 && b > 160;
            if (isBright) {
              brightMin = Math.min(brightMin, px); brightMax = Math.max(brightMax, px);
            }
            if (isRed || isGreen || isBright) {
              subjectMin = Math.min(subjectMin, px); subjectMax = Math.max(subjectMax, px);
            }
          }
          done({ red, green, blue, width, height,
            textureWidthRatio: Math.max(0, textureMax - textureMin + 1) / width,
            brightWidthRatio: Math.max(0, brightMax - brightMin + 1) / width,
            subjectWidthRatio: Math.max(0, subjectMax - subjectMin + 1) / width });
        } catch (error) { done({ error: String(error) }); }
      };
      image.src = 'data:image/png;base64,' + base64;
    `,
    args: [base64, selector],
  });
  assert.ok(!result.error, result.error);
  return result;
}

async function assertTexturePixels(selector, name) {
  const result = await previewPixels(selector, name);
  assert.ok(
    result.red > 20 && result.green > 20,
    `Embedded texture missing in ${selector}: ${JSON.stringify(result)}`,
  );
  return result;
}

async function assertPreviewFraming(selector, name, textured = false) {
  const result = textured
    ? await assertTexturePixels(selector, name)
    : await previewPixels(selector, name);
  const ratio = textured ? result.textureWidthRatio : result.subjectWidthRatio;
  assert.ok(
    ratio >= 0.25,
    `The subject must occupy at least 25% of the preview width: ${JSON.stringify(result)}`,
  );
  if (textured)
    assert.ok(
      result.blue <= 5,
      `The blue studio floor must be hidden: ${JSON.stringify(result)}`,
    );
  return result;
}

async function waitForThumbnail(name) {
  return until(
    () =>
      script(
        `
    const card = Array.from(document.querySelectorAll('.asset-card')).find(element => element.innerText.includes(arguments[0]));
    card?.scrollIntoView({ block: 'nearest' });
    const image = card?.querySelector('.asset-thumbnail img');
    return image?.complete && image.naturalWidth > 0 && !card.querySelector('.thumbnail-status') ? image.currentSrc : null;
  `,
        [name],
      ),
    300_000,
  );
}

async function resourceAccessible(url) {
  const result = await request("POST", `/session/${session}/execute/async`, {
    script: `const [url, done] = arguments; const fetchResource = window.__edgeLockE2EAudit?.fetch ?? window.fetch.bind(window); fetchResource(url, {cache:'no-store'}).then(response => done(response.ok)).catch(() => done(false));`,
    args: [url],
  });
  return result;
}

async function installNavigationAudit() {
  await script(`
    const contexts = new Set(), blobs = new Set();
    const audit = window.__edgeLockE2EAudit = { contexts, blobs, modelUrls: [], maxContexts: 0, createdContexts: 0, fetch: window.fetch.bind(window) };
    const getContext = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function(type, ...args) {
      const context = getContext.call(this, type, ...args);
      if (context && ['webgl', 'webgl2', 'experimental-webgl'].includes(type) && !contexts.has(context)) {
        contexts.add(context); audit.createdContexts++;
        audit.maxContexts = Math.max(audit.maxContexts, contexts.size);
        this.addEventListener('webglcontextlost', () => contexts.delete(context), { once: true });
      }
      return context;
    };
    const createUrl = URL.createObjectURL.bind(URL), revokeUrl = URL.revokeObjectURL.bind(URL);
    URL.createObjectURL = object => { const url = createUrl(object); blobs.add(url); return url; };
    URL.revokeObjectURL = url => { blobs.delete(url); revokeUrl(url); };
    window.fetch = async (...args) => {
      const response = await audit.fetch(...args);
      if (response.headers.get('content-type')?.includes('model/gltf-binary')) audit.modelUrls.push(response.url || String(args[0]));
      return response;
    };
    return true;
  `);
}

async function navigationSnapshot() {
  return script(`
    const audit = window.__edgeLockE2EAudit;
    const memory = performance.memory;
    return {
      canvases: document.querySelectorAll('.viewer-canvas canvas').length,
      contexts: audit.contexts.size,
      maxContexts: audit.maxContexts,
      createdContexts: audit.createdContexts,
      blobUrls: audit.blobs.size,
      latestModelUrl: audit.modelUrls.at(-1) ?? null,
      heap: memory ? { used: memory.usedJSHeapSize, total: memory.totalJSHeapSize, limit: memory.jsHeapSizeLimit } : null,
    };
  `);
}

function timings(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return {
    median: sorted[Math.floor(sorted.length / 2)],
    p95: sorted[Math.ceil(sorted.length * 0.95) - 1],
  };
}

async function testNavigationPerformance() {
  const warmLibraryStarted = performance.now();
  await clickText("Retour à la bibliothèque");
  await waitForThumbnail("Éclaireur");
  report.previewPerformance.libraryWarmFirstMs = Math.round(
    performance.now() - warmLibraryStarted,
  );
  await installNavigationAudit();
  const before = await navigationSnapshot();
  const samples = [];
  for (let index = 0; index < 10; index++) {
    const detailStarted = performance.now();
    await clickText("Éclaireur");
    await waitForModel("model.glb");
    const detailMs = Math.round(performance.now() - detailStarted);
    const active = await navigationSnapshot();
    assert.equal(active.canvases, 1);
    assert.equal(active.contexts, 1);
    assert.ok(
      active.latestModelUrl,
      "The detail must fetch its model automatically.",
    );
    assert.equal(await resourceAccessible(active.latestModelUrl), true);
    const galleryStarted = performance.now();
    await clickText("Retour à la bibliothèque");
    await waitForThumbnail("Éclaireur");
    await until(async () => (await navigationSnapshot()).contexts === 0);
    await until(async () => !(await resourceAccessible(active.latestModelUrl)));
    const idle = await navigationSnapshot();
    assert.equal(idle.canvases, 0);
    assert.equal(idle.blobUrls, 0, "Embedded-image blob URLs must be revoked.");
    assert.ok(
      idle.maxContexts <= 1,
      "A static gallery must not keep WebGL contexts alive.",
    );
    samples.push({
      detailMs,
      libraryMs: Math.round(performance.now() - galleryStarted),
      ...idle,
    });
  }
  const after = await navigationSnapshot();
  report.previewPerformance.navigation = {
    cycles: 10,
    detailWarmMs: timings(samples.map((sample) => sample.detailMs)),
    libraryWarmMs: timings(samples.map((sample) => sample.libraryMs)),
    maxContexts: after.maxContexts,
    idleContexts: after.contexts,
    liveBlobUrls: after.blobUrls,
    releasedModelUrls: 10,
    resourceRegistryTotal:
      "Non exposé par l’API ; les dix URL de modèles deviennent inaccessibles après navigation.",
    memoryBefore: before.heap,
    memoryAfter: after.heap,
    memoryStatus:
      before.heap && after.heap
        ? "Tas JavaScript mesuré"
        : "Indisponible : cette WebView2 n’expose pas performance.memory",
    usedHeapDelta:
      before.heap && after.heap ? after.heap.used - before.heap.used : null,
    memoryLimit:
      "performance.memory mesure le tas JavaScript lorsque disponible, pas la RAM totale WebView2 ni la mémoire GPU. Aucun GC forcé.",
    samples,
  };
  report.checks.push(
    "Dix allers-retours : un contexte WebGL dans la fiche, zéro dans la galerie, URL modèles libérées et aucune URL blob conservée",
  );
}

async function reloadWithNavigationAudit(timeout) {
  const previousOrigin = await script("return performance.timeOrigin;");
  await script("window.location.reload(); return true;");
  await until(
    () =>
      script("return performance.timeOrigin !== arguments[0];", [
        previousOrigin,
      ]),
    timeout,
  );
  await installNavigationAudit();
}

async function galleryPageSnapshot() {
  return script(`
    const cards = Array.from(document.querySelectorAll('.asset-card'));
    const viewport = document.querySelector('.main-area')?.getBoundingClientRect();
    const visible = cards.filter(card => {
      const rect = card.getBoundingClientRect();
      return rect.bottom > Math.max(0, viewport?.top ?? 0) && rect.top < Math.min(innerHeight, viewport?.bottom ?? innerHeight) && rect.right > 0 && rect.left < innerWidth;
    });
    return {
      cards: cards.length,
      fixtureCards: cards.filter(card => card.innerText.includes('Performance galerie')).length,
      visible: visible.length,
      visibleImagesReady: visible.filter(card => { const image = card.querySelector('.asset-thumbnail img'); return image?.complete && image.naturalWidth > 0; }).length,
      visibleValidating: visible.filter(card => card.querySelector('.thumbnail-status')).length,
      canvases: document.querySelectorAll('canvas').length,
    };
  `);
}

async function testGalleryPagePerformance() {
  const started = performance.now();
  const deadline = started + 900_000;
  const remaining = () => {
    const duration = Math.round(deadline - performance.now());
    assert.ok(
      duration > 0,
      "Timeout preparing the disposable gallery benchmark.",
    );
    return duration;
  };
  const fixtures = [];
  report.galleryPagePerformance = {
    fixtureAssets: 50,
    pageSize: 36,
    conditions:
      "50 imports séquentiels du même .blend synthétique texturé avec génération canonique réelle ; première visite de ces assets. Les temps incluent Blender et le polling WebDriver.",
  };
  try {
    for (let index = 0; index < 50; index++) {
      const imported = await job(
        {
          kind: "import",
          source,
          name: `Performance galerie ${String(index).padStart(2, "0")}`,
          config: {
            primaryFile: "model.blend",
            scene: null,
            collection: null,
            applyModifiers: false,
          },
        },
        remaining(),
      );
      fixtures.push(imported.asset);
    }
    report.galleryPagePerformance.importsMs = Math.round(
      performance.now() - started,
    );
    const coldStarted = performance.now();
    await reloadWithNavigationAudit(remaining());
    await until(async () => {
      const state = await galleryPageSnapshot();
      return state.cards === 36 && state.fixtureCards === 36;
    }, remaining());
    const coldListMs = Math.round(performance.now() - coldStarted);
    await until(async () => {
      const state = await galleryPageSnapshot();
      return (
        state.visible > 0 &&
        state.visibleImagesReady === state.visible &&
        state.visibleValidating === 0
      );
    }, remaining());
    const coldImagesMs = Math.round(performance.now() - coldStarted);
    await until(
      async () => (await navigationSnapshot()).contexts === 0,
      remaining(),
    );
    const coldPage = await galleryPageSnapshot();
    const coldAudit = await navigationSnapshot();
    assert.equal(coldPage.canvases, 0);
    assert.ok(coldAudit.maxContexts <= 1);
    assert.equal(coldAudit.contexts, 0);
    report.galleryPagePerformance.cold = {
      listMs: coldListMs,
      visibleImagesReadyMs: coldImagesMs,
      ...coldPage,
      maxContexts: coldAudit.maxContexts,
      idleContexts: coldAudit.contexts,
    };

    const warmStarted = performance.now();
    await reloadWithNavigationAudit(remaining());
    await until(async () => {
      const state = await galleryPageSnapshot();
      return state.cards === 36 && state.fixtureCards === 36;
    }, remaining());
    const warmListMs = Math.round(performance.now() - warmStarted);
    // Cached images should become visible without waiting for source revalidation.
    await until(async () => {
      const state = await galleryPageSnapshot();
      return state.visible > 0 && state.visibleImagesReady === state.visible;
    }, remaining());
    const warmImagesMs = Math.round(performance.now() - warmStarted);
    const warmPage = await galleryPageSnapshot();
    await until(
      async () => (await navigationSnapshot()).contexts === 0,
      remaining(),
    );
    const warmAudit = await navigationSnapshot();
    assert.equal(warmPage.canvases, 0);
    assert.ok(warmAudit.maxContexts <= 1);
    assert.equal(warmAudit.contexts, 0);
    report.galleryPagePerformance.warm = {
      listMs: warmListMs,
      visibleCachedImagesMs: warmImagesMs,
      ...warmPage,
      maxContexts: warmAudit.maxContexts,
      idleContexts: warmAudit.contexts,
    };
    report.checks.push(
      "Galerie de 50 assets synthétiques : page de 36 cartes, vignettes visibles froides/chaudes, aucun canvas de carte et un contexte transitoire au maximum observé",
    );
  } finally {
    // Retain every disposable fixture and its history; archiving only removes
    // them from the active gallery used by the remaining desktop scenarios.
    for (const asset of fixtures) {
      await invoke("update_asset", {
        id: asset.id,
        name: asset.name,
        categoryId: asset.categoryId,
        tags: asset.tags,
        gameIds: asset.gameIds,
        archived: true,
      });
    }
    report.galleryPagePerformance.archivedFixtures = fixtures.length;
    report.galleryPagePerformance.totalMs = Math.round(
      performance.now() - started,
    );
    await script("window.location.reload(); return true;");
    await waitForThumbnail("Éclaireur");
  }
}

async function testDataUriTexture() {
  const dataSource = join(testRoot, "source data-uri");
  await prepareBlendFixture(
    blender,
    dataSource,
    join(testRoot, "fixture-data-uri"),
    { dataUri: true },
  );
  const original = await fileManifest(dataSource);
  const imported = await job({
    kind: "import",
    source: dataSource,
    name: "Texture data URI — test",
    config: {
      primaryFile: "model.blend",
      scene: null,
      collection: null,
      applyModifiers: false,
    },
  });
  try {
    await script("window.location.reload(); return true;");
    await waitForThumbnail("Texture data URI");
    report.texturePixels.dataUriThumbnail = await assertTexturePixels(
      '.asset-card:has(img[alt*="Texture data URI"]) .asset-thumbnail',
      "09-data-uri-library.png",
    );
    assert.equal(
      await script('return document.querySelectorAll("canvas").length;'),
      0,
    );
    await clickText("Texture data URI");
    await waitForModel("model.glb");
    report.texturePixels.dataUriViewer = await assertTexturePixels(
      ".viewer-canvas",
      "10-data-uri-viewer.png",
    );
    await clickText("Retour à la bibliothèque");
    assert.deepEqual(await fileManifest(dataSource), original);
    report.checks.push(
      "Texture PNG en data URI réellement affichée dans la vignette et la fiche automatique, pixels rouge/vert vérifiés et source préservée",
    );
  } finally {
    const asset = imported.asset;
    await invoke("update_asset", {
      id: asset.id,
      name: asset.name,
      categoryId: asset.categoryId,
      tags: asset.tags,
      gameIds: asset.gameIds,
      archived: true,
    });
    await script("window.location.reload(); return true;");
    await waitForThumbnail("Éclaireur");
  }
}

async function testStudioFloor() {
  const directory = join(testRoot, "source studio-floor");
  await prepareBlendFixture(
    blender,
    directory,
    join(testRoot, "fixture-studio-floor"),
    { studioFloor: true },
  );
  const original = await fileManifest(directory);
  const imported = await job({
    kind: "import",
    source: directory,
    name: "Socle de studio — cadrage",
    config: canonicalConfig,
  });
  const glb = join(imported.variants[0].workspacePath, "model.glb");
  const originalGlb = await fileHash(glb);
  try {
    await script("window.location.reload(); return true;");
    await waitForThumbnail("Socle de studio");
    const thumbnail = await assertPreviewFraming(
      '.asset-card:has(img[alt*="Socle de studio"]) .asset-thumbnail',
      "12-studio-floor-library.png",
      true,
    );
    await clickText("Socle de studio");
    await waitForModel("model.glb");
    const viewer = await assertPreviewFraming(
      ".viewer-canvas",
      "13-studio-floor-viewer.png",
      true,
    );
    report.studioFloor = {
      thumbnail,
      viewer,
      sourcePreserved: true,
      glbPreserved: true,
    };
    report.checks.push(
      "Socle synthétique 200 × 0,06 × 200 masqué dans la vignette et la fiche ; deux volumes texturés couvrent au moins 25 % de la largeur ; source et GLB conservés",
    );
    await clickText("Retour à la bibliothèque");
  } finally {
    assert.deepEqual(await fileManifest(directory), original);
    assert.equal(
      await fileHash(glb),
      originalGlb,
      "Previewing must not rewrite the GLB",
    );
    const asset = imported.asset;
    await invoke("update_asset", {
      id: asset.id,
      name: asset.name,
      categoryId: asset.categoryId,
      tags: asset.tags,
      gameIds: asset.gameIds,
      archived: true,
    });
    await script("window.location.reload(); return true;");
    await waitForThumbnail("Éclaireur");
  }
}

async function fileHash(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function fileManifest(directory, exclude = () => false) {
  const entries = [];
  const visit = async (current, prefix = "") => {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const path = join(current, entry.name);
      const name = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (exclude(name)) continue;
      const before = await lstat(path);
      assert.ok(!before.isSymbolicLink(), `Fixture contains a link: ${name}`);
      if (before.isDirectory()) {
        entries.push({ path: name, kind: "directory" });
        await visit(path, name);
      } else {
        assert.ok(before.isFile(), `Fixture contains a special file: ${name}`);
        const hash = await fileHash(path);
        const after = await lstat(path);
        assert.equal(
          after.size,
          before.size,
          `File changed while hashing: ${name}`,
        );
        assert.equal(
          after.mtimeMs,
          before.mtimeMs,
          `File changed while hashing: ${name}`,
        );
        entries.push({
          path: name,
          kind: "file",
          size: before.size,
          hash,
          modifiedAt: before.mtimeMs,
        });
      }
    }
  };
  await visit(directory);
  return entries.sort((a, b) => a.path.localeCompare(b.path, "en"));
}

function assertWithin(directory, target) {
  const path = relative(directory, target);
  assert.ok(
    path && !isAbsolute(path) && !path.split(sep).includes(".."),
    `Disposable target escapes the intended directory: ${target}`,
  );
}

async function waitForModel(sourceFile) {
  await until(
    () =>
      script(
        'const canvas=document.querySelector("canvas"); const caption=document.querySelector(".preview-caption > span"); return !!canvas && canvas.clientWidth>0 && canvas.clientHeight>0 && !!document.querySelector(".viewer-help") && caption?.title===arguments[0];',
        [sourceFile],
      ),
    300_000,
  );
}

async function testKitsune() {
  const personalSource = await realpath(
    resolve(process.env.EDGE_LOCK_E2E_KITSUNE_SOURCE),
  );
  const original = await fileManifest(personalSource);
  report.kitsune = { source: personalSource };
  try {
    const asset = await job(
      {
        kind: "import",
        source: personalSource,
        name: "Sniper Kitsune — synchronisation",
        config: canonicalConfig,
      },
      600_000,
    );
    const variant = asset.variants[0];
    assertWithin(
      await realpath(testRoot),
      await realpath(variant.workspacePath),
    );
    const blend = original.find(
      (entry) => entry.kind === "file" && /\.blend$/i.test(entry.path),
    );
    assert.ok(blend, "Kitsune must have exactly one editable Blender source");
    assert.equal(
      await fileHash(join(variant.workspacePath, blend.path)),
      blend.hash,
    );
    const before = await fileManifest(variant.workspacePath);
    const previews = await concurrentPreviews({
      kind: "preview",
      variantId: variant.id,
    });
    assert.equal(previews[0].artifactKey, previews[1].artifactKey);
    assert.ok(previews[0].inspection.geometry.triangles > 0);
    await invoke("release_preview_resources", {
      tokens: previews.map((preview) => preview.resourceToken),
    });
    await script("window.location.reload(); return true;");
    await waitForThumbnail("Sniper Kitsune");
    report.kitsune.thumbnail = await assertPreviewFraming(
      '.asset-card:has(img[alt*="Sniper Kitsune"]) .asset-thumbnail',
      "14-kitsune-library.png",
    );
    await clickText("Sniper Kitsune");
    await waitForModel(previews[0].sourceFile);
    report.kitsune.viewer = await assertPreviewFraming(
      ".viewer-canvas",
      "15-kitsune-viewer.png",
    );
    await clickText("Retour à la bibliothèque");
    assert.deepEqual(await fileManifest(variant.workspacePath), before);
    report.kitsune.geometry = previews[0].inspection.geometry;
    report.checks.push(
      "Kitsune facultatif : sniper visible sur au moins 25 % de la largeur en vignette et fiche, GLB canonique et source personnelle préservés",
    );
  } finally {
    assert.deepEqual(
      await fileManifest(personalSource),
      original,
      "Every personal Kitsune file and its modification time must remain unchanged",
    );
    report.kitsune.sourcePreserved = true;
  }
}

async function testEmptyLibraryStorage() {
  const stableManifest = async () =>
    fileManifest(
      library,
      (name) => name === "library.lock" || name.startsWith("library.sqlite3"),
    );
  const before = await stableManifest();
  await clickText("Optimisation du stockage");
  await until(() =>
    script(
      "return [...document.querySelectorAll('h1')].some(h=>h.textContent==='Optimisation du stockage');",
    ),
  );
  await clickText("Lancer le diagnostic");
  await until(() =>
    script(
      "return [...document.querySelectorAll('button')].some(b=>b.textContent.includes('Actualiser le diagnostic')&&!b.disabled);",
    ),
  );
  const diagnostic = await job({ kind: "storageDiagnostic" });
  assert.equal(diagnostic.complete, true);
  assert.equal(diagnostic.purgeAvailable, true);
  for (const key of ["objects", "workspaces", "staging", "backups"]) {
    assert.equal(
      diagnostic.categories.find((item) => item.key === key).bytes,
      0,
    );
  }
  assert.equal(
    diagnostic.categories.reduce((total, item) => total + item.bytes, 0),
    diagnostic.total.bytes,
  );
  assert.equal(
    diagnostic.categories.reduce((total, item) => total + item.fileCount, 0),
    diagnostic.total.fileCount,
  );
  assert.deepEqual(
    await stableManifest(),
    before,
    "The empty-library diagnostic must not create or change managed storage files outside the locked catalog",
  );
  await screenshot("storage-empty-diagnostic.png");
  for (const action of ["recompress", "purge"]) {
    const plan = await job({ kind: "storagePlan", action });
    assert.equal(plan.reclaimableBytes, 0);
    const result = await job({
      kind: "storageApply",
      action,
      planToken: plan.planToken,
    });
    assert.equal(result.releasedBytes, 0);
  }
  await clickText("Tous les assets");
  await until(() =>
    script(
      "return document.body.innerText.includes('Importer mon premier asset');",
    ),
  );
  report.checks.push(
    "Stockage global sur bibliothèque vide : diagnostic manuel, inventaire additif, fichiers gérés inchangés hors SQLite auxiliaire, compression et purge sans variante",
  );
}

async function testHistoryStorage() {
  const asset = await job({
    kind: "import",
    source,
    name: "Historique Zstd",
    config: canonicalConfig,
  });
  const variant = asset.variants[0];
  assertWithin(await realpath(testRoot), await realpath(variant.workspacePath));
  const first = variant.headSnapshotId;
  const originalGlb = await fileHash(join(variant.workspacePath, "model.glb"));
  await writeFile(join(variant.workspacePath, "notes.txt"), "version deux");
  await writeFile(
    join(variant.workspacePath, "rapport.txt"),
    "rapport conservé\n".repeat(20000),
  );
  const second = await job({
    kind: "snapshot",
    variantId: variant.id,
    message: "Deuxième état complet",
  });
  await writeFile(join(variant.workspacePath, "notes.txt"), "version trois");
  const third = await job({
    kind: "snapshot",
    variantId: variant.id,
    message: "Troisième état complet",
  });
  assert.equal(
    (await invoke("get_snapshot", { id: first })).storage.kind,
    "preview",
  );
  const exported = join(testRoot, "historical-consultation.glb");
  await job({
    kind: "export",
    variantId: variant.id,
    snapshotId: first,
    destination: exported,
    overwrite: false,
  });
  assert.equal(await fileHash(exported), originalGlb);
  await script("location.reload();");
  await waitForThumbnail("Historique Zstd");
  await clickText("Historique Zstd");
  await waitForModel("model.glb");
  await clickText("Historique");
  await until(() =>
    script("return document.body.innerText.includes('Consultation seule');"),
  );
  assert.equal(
    await script(
      "return [...document.querySelectorAll('.history-storage-actions button')].some(b=>/Compresser avec Zstd|Libérer les objets inutilisés/.test(b.textContent));",
    ),
    false,
    "Library-wide actions must no longer appear in an asset's history",
  );
  await script(
    "document.querySelector('.history-storage-actions').scrollIntoView({block:'center'});",
  );
  await screenshot("storage-history.png");
  await script(
    "const row=[...document.querySelectorAll('.history-row')].find(r=>r.textContent.includes(arguments[0])); [...row.querySelectorAll('button')].find(b=>b.textContent.includes('Consulter')).click();",
    [first.slice(0, 8)],
  );
  await waitForModel("model.glb");
  assert.equal(
    await script(
      "return [...document.querySelectorAll('button')].find(b=>b.textContent.includes('Nouvelle variante')).disabled;",
    ),
    true,
  );
  await clickText("Revenir au travail en cours");
  await writeFile(
    join(variant.workspacePath, "notes.txt"),
    "modification non versionnée",
  );
  await clickText("Restaurer le backup");
  await until(() =>
    script(
      "return !!document.querySelector('dialog[open] input[type=checkbox]');",
    ),
  );
  assert.equal(
    await script(
      "return document.querySelector('dialog[open] .button.primary').disabled;",
    ),
    true,
  );
  await screenshot("storage-restore-confirmation.png");
  await clickSelector('dialog[open] input[type="checkbox"]');
  const checkboxSize = await script(
    "const box=document.querySelector('dialog[open] input[type=checkbox]').getBoundingClientRect(); return {width:box.width,height:box.height};",
  );
  assert.equal(checkboxSize.width, checkboxSize.height);
  await screenshot("storage-restore-confirmed.png");
  await clickSelector("dialog[open] .button.primary");
  await until(
    async () =>
      (await invoke("get_variant", { id: variant.id })).variant
        .headSnapshotId !== third.id,
  );
  await until(() => script("return !document.querySelector('dialog[open]');"));
  assert.equal(
    await readFile(join(variant.workspacePath, "notes.txt"), "utf8"),
    "version deux",
  );
  const restored = (await invoke("get_variant", { id: variant.id })).variant;
  assert.equal(
    (await invoke("get_snapshot", { id: restored.backupSnapshotId })).storage
      .previewTrusted,
    false,
  );
  await job({
    kind: "restore",
    variantId: variant.id,
    expectedHead: restored.headSnapshotId,
    expectedBackup: restored.backupSnapshotId,
  });
  assert.equal(
    await readFile(join(variant.workspacePath, "notes.txt"), "utf8"),
    "modification non versionnée",
  );
  const plan = await job({
    kind: "storagePlan",
    variantId: variant.id,
    action: "releaseBackup",
  });
  await job({
    kind: "storageApply",
    variantId: variant.id,
    action: "releaseBackup",
    planToken: plan.planToken,
  });
  assert.equal(
    (await invoke("get_variant", { id: variant.id })).variant.backupSnapshotId,
    null,
  );
  const deletion = await job({
    kind: "storagePlan",
    variantId: variant.id,
    action: "deleteVersion",
    snapshotId: first,
  });
  await job({
    kind: "storageApply",
    variantId: variant.id,
    action: "deleteVersion",
    snapshotId: first,
    planToken: deletion.planToken,
  });
  assert.equal(
    (await invoke("get_snapshot", { id: first })).storage.kind,
    "deleted",
  );
  await clickText("Optimisation du stockage");
  await clickText("Lancer le diagnostic");
  await until(() =>
    script(
      "return [...document.querySelectorAll('button')].some(b=>b.textContent.includes('Actualiser le diagnostic')&&!b.disabled);",
    ),
  );
  const diagnostic = await job({ kind: "storageDiagnostic" });
  assert.ok(diagnostic.total.bytes > 0);
  assert.ok(diagnostic.logicalObjectBytes > 0);
  assert.ok(
    diagnostic.categories.find((item) => item.key === "objects").bytes > 0,
  );
  await screenshot("storage-library-diagnostic.png");
  await clickText("Compresser avec Zstd");
  await until(() =>
    script(
      "return document.querySelector('dialog[open]')?.textContent.includes('Espace récupérable');",
    ),
  );
  assert.ok(
    await script(
      "return document.querySelector('dialog[open]').textContent.includes('Portée : toute la bibliothèque');",
    ),
  );
  await screenshot("storage-compression-plan.png");
  await clickSelector("dialog[open] .button.primary");
  await until(() =>
    script(
      "return document.querySelector('dialog[open]')?.textContent.includes('Opération terminée');",
    ),
  );
  await screenshot("storage-compression-result.png");
  await clickSelector('dialog[open] [aria-label="Fermer"]');
  await until(() =>
    script(
      "return document.body.innerText.includes('Stockage modifié : actualisez le diagnostic pour obtenir les nouveaux volumes.');",
    ),
  );
  await screenshot("storage-diagnostic-stale.png");
  await clickText("Libérer les objets inutilisés");
  await until(() =>
    script(
      "return document.querySelector('dialog[open]')?.textContent.includes('Espace récupérable');",
    ),
  );
  assert.ok(
    await script(
      "return document.querySelector('dialog[open]').textContent.includes('Portée : toute la bibliothèque');",
    ),
  );
  await screenshot("storage-purge-plan.png");
  await clickSelector("dialog[open] .button.primary");
  await until(() =>
    script(
      "return document.querySelector('dialog[open]')?.textContent.includes('Opération terminée');",
    ),
  );
  await clickSelector('dialog[open] [aria-label="Fermer"]');
  assert.equal(
    (await invoke("get_snapshot", { id: second.id })).storage.kind,
    "preview",
  );
  report.checks.push(
    "Historique Zstd : rotation, consultation/export GLB exact, protections UI, restauration avec travail non versionné, retour arrière, libération du backup et suppression ; diagnostic, recompression et purge confirmées depuis le menu global",
  );
}

try {
  await until(async () => {
    const response = await fetch(base + "/status", {
      signal: AbortSignal.timeout(1500),
    });
    return response.ok;
  });
  await newSession();
  await until(() =>
    script('return document.body.innerText.includes("Choisir un dossier");'),
  );
  await viewport(1440, 940);
  await assertTheme();
  await screenshot("01-onboarding.png");
  const opened = await invoke("open_library", { path: library });
  assert.equal(opened.assetCount, 0);
  await script("window.location.reload(); return true;");
  await until(() =>
    script(
      'return document.body.innerText.includes("Importer mon premier asset");',
    ),
  );
  await testEmptyLibraryStorage();
  // The native listener is installed asynchronously after the library renders.
  // Retry the harmless event until the import form proves it is ready.
  await until(async () => {
    if (await script('return !!document.querySelector("dialog[open] input");'))
      return true;
    await invoke("plugin:event|emit_to", {
      target: { kind: "Webview", label: "main" },
      event: "tauri://drag-drop",
      payload: { paths: [source], position: { x: 30, y: 30 } },
    });
    return false;
  });
  assert.equal(
    await script('return document.querySelector("dialog[open] input").value;'),
    source,
  );
  assert.equal((await invoke("get_bootstrap")).library.assetCount, 0);
  await clickText("Annuler");
  report.checks.push(
    "Événement de dépôt Tauri : formulaire prérempli et aucune copie sans confirmation",
  );
  const imported = await job({
    kind: "import",
    source,
    name: "Éclaireur — prototype",
    config: {
      primaryFile: "model.blend",
      scene: null,
      collection: null,
      applyModifiers: false,
    },
  });
  const main = imported.variants[0];
  const first = main.headSnapshotId;
  assert.equal(
    (await readdir(source)).filter((name) => /\.glb$/i.test(name)).length,
    0,
  );
  assert.equal(
    (await readdir(main.workspacePath)).filter((name) => /\.glb$/i.test(name))
      .length,
    1,
  );
  await testStrictImportRules(main);
  assert.equal(await readFile(join(source, "notes.txt"), "utf8"), "original");
  await access(join(main.workspacePath, "empty"));
  report.checks.push(
    "Import réel, noms Unicode, source et dossier vide conservés",
  );
  await invoke("save_category", {
    id: null,
    name: "Personnages",
    parentId: null,
  });
  await invoke("save_game", { id: null, name: "Horizon bleu" });
  const taxonomy = await invoke("get_taxonomy");
  await invoke("update_asset", {
    id: imported.asset.id,
    name: imported.asset.name,
    categoryId: taxonomy.categories[0].id,
    tags: ["robot", "prototype"],
    gameIds: [taxonomy.games[0].id],
    archived: false,
  });
  const libraryFirstStarted = performance.now();
  await script("window.location.reload(); return true;");
  await until(() => script('return !!document.querySelector(".asset-card");'));
  await waitForThumbnail("Éclaireur");
  assert.equal(
    await script('return document.querySelectorAll("canvas").length;'),
    0,
    "Library cards must contain static images, not WebGL canvases.",
  );
  report.previewPerformance = {
    libraryFirstImageMs: Math.round(performance.now() - libraryFirstStarted),
    conditions:
      "Premier affichage dans cette WebView ; le GLB du premier détail peut déjà avoir été préparé par la galerie. Les temps incluent navigation et polling WebDriver.",
  };
  report.texturePixels = {
    thumbnail: await assertTexturePixels(".asset-thumbnail", "02-library.png"),
  };
  const detailFirstStarted = performance.now();
  await clickText("Éclaireur");
  await until(() =>
    script('return !!document.querySelector(".workspace-panel");'),
  );
  await waitForModel("model.glb");
  report.previewPerformance.automaticDetailFirstMs = Math.round(
    performance.now() - detailFirstStarted,
  );
  assert.ok(
    await script(
      "return !!document.querySelector('[aria-label=\"Lire l’animation\"]');",
    ),
  );
  report.texturePixels.viewer = await assertTexturePixels(
    ".viewer-canvas",
    "03-viewer.png",
  );
  await testVisualRefreshAndCanonicalRules();
  await testAutomaticMirror(main);
  report.checks.push(
    "Texture PNG rouge et verte réellement visible dans la vignette statique et dans l’aperçu automatique, contrôlée par pixels des captures WebDriver",
  );
  const play = await request("POST", `/session/${session}/element`, {
    using: "css selector",
    value: '[aria-label="Lire l’animation"]',
  });
  await request(
    "POST",
    `/session/${session}/element/${play["element-6066-11e4-a52e-4f735466cecf"]}/click`,
    {},
  );
  await until(() =>
    script("return Number(document.querySelector(arguments[0]).value)>0.05;", [
      '[aria-label="Position dans l’animation"]',
    ]),
  );
  const pause = await request("POST", `/session/${session}/element`, {
    using: "css selector",
    value: '[aria-label="Mettre en pause"]',
  });
  await request(
    "POST",
    `/session/${session}/element/${pause["element-6066-11e4-a52e-4f735466cecf"]}/click`,
    {},
  );
  const canvasBox = await script(
    'const r=document.querySelector("canvas").getBoundingClientRect();return {x:Math.round(r.x+r.width/2),y:Math.round(r.y+r.height/2)};',
  );
  const beforeOrbit = await request("GET", `/session/${session}/screenshot`);
  await request("POST", `/session/${session}/actions`, {
    actions: [
      {
        type: "pointer",
        id: "orbit",
        parameters: { pointerType: "mouse" },
        actions: [
          {
            type: "pointerMove",
            duration: 0,
            origin: "viewport",
            ...canvasBox,
          },
          { type: "pointerDown", button: 0 },
          {
            type: "pointerMove",
            duration: 250,
            origin: "viewport",
            x: canvasBox.x + 80,
            y: canvasBox.y + 20,
          },
          { type: "pointerUp", button: 0 },
        ],
      },
    ],
  });
  assert.notEqual(
    await request("GET", `/session/${session}/screenshot`),
    beforeOrbit,
  );
  await request("POST", `/session/${session}/actions`, {
    actions: [
      {
        type: "wheel",
        id: "zoom",
        actions: [
          {
            type: "scroll",
            duration: 150,
            origin: "viewport",
            ...canvasBox,
            deltaX: 0,
            deltaY: 150,
          },
        ],
      },
    ],
  });
  const center = await request("POST", `/session/${session}/element`, {
    using: "css selector",
    value: '[aria-label="Recentrer le modèle"]',
  });
  await request(
    "POST",
    `/session/${session}/element/${center["element-6066-11e4-a52e-4f735466cecf"]}/click`,
    {},
  );
  report.checks.push(
    "Visionneuse WebGL réelle, chargement local, rotation/zoom/recentrage, lecture/pause animation",
  );
  await testNavigationPerformance();
  await testDataUriTexture();
  await testStudioFloor();
  if (process.env.EDGE_LOCK_E2E_SKIP_GALLERY_BENCHMARK === "1")
    report.galleryPagePerformance = {
      skipped: true,
      reason:
        "EDGE_LOCK_E2E_SKIP_GALLERY_BENCHMARK=1 : benchmark de 50 imports ignoré explicitement.",
    };
  else await testGalleryPagePerformance();
  await writeFile(join(main.workspacePath, "notes.txt"), "version 2");
  await writeFile(join(main.workspacePath, "rapport.txt"), "nouveau");
  const changes = await job({ kind: "changes", variantId: main.id });
  assert.deepEqual(changes.modified, ["model.blend", "notes.txt"]);
  assert.deepEqual(changes.added, ["rapport.txt"]);
  const second = await job({
    kind: "snapshot",
    variantId: main.id,
    message: "Textures et rapport — v2",
  });
  assert.equal(second.parentId, first);
  const branch = await job({
    kind: "variant",
    snapshotId: first,
    name: "Texture désert",
  });
  assert.equal(
    await readFile(join(branch.workspacePath, "notes.txt"), "utf8"),
    "original",
  );
  await writeFile(
    join(branch.workspacePath, "notes.txt"),
    "variante indépendante",
  );
  assert.equal(
    await readFile(join(main.workspacePath, "notes.txt"), "utf8"),
    "version 2",
  );
  report.checks.push(
    "Détection modifications, version manuelle, variante indépendante depuis v1",
  );
  const output = join(testRoot, "export.glb");
  await job({
    kind: "export",
    variantId: main.id,
    snapshotId: first,
    destination: output,
    overwrite: false,
  });
  const exported = await readFile(output);
  const exportedJson = JSON.parse(
    exported.subarray(20, 20 + exported.readUInt32LE(12)).toString(),
  );
  const primitive = exportedJson.meshes[0].primitives[0];
  assert.equal(
    exportedJson.accessors[primitive.indices].count,
    3,
    "Historical export must keep the pre-Mirror geometry",
  );
  await assert.rejects(
    () =>
      job({
        kind: "export",
        variantId: main.id,
        snapshotId: first,
        destination: output,
        overwrite: false,
      }),
    (error) => error.code === "destination_exists",
  );
  assert.equal(
    await readFile(join(main.workspacePath, "notes.txt"), "utf8"),
    "version 2",
  );
  report.checks.push(
    "Export historique vérifié, remplacement refusé sans consentement, travail courant intact",
  );
  if (process.env.EDGE_LOCK_E2E_KITSUNE_SOURCE) await testKitsune();
  const created = await job({ kind: "create", name: "Nouveau projet Blender" });
  await access(join(created.variants[0].workspacePath, "main.blend"));
  await access(join(created.variants[0].workspacePath, "main.glb"));
  assert.equal(created.variants[0].triangles, 0);
  if (await script('return !!document.querySelector(".workspace-panel");'))
    await clickText("Retour à la bibliothèque");
  await waitForThumbnail("Éclaireur");
  const thumbnailCacheBeforeRestart = (
    await fileManifest(join(library, "cache", "thumbnails"))
  ).filter((entry) => entry.kind === "file" && entry.path.endsWith(".png"));
  assert.ok(thumbnailCacheBeforeRestart.length > 0);
  await request("DELETE", `/session/${session}`);
  session = undefined;
  await newSession();
  const saved = await invoke("get_asset", { id: imported.asset.id });
  assert.equal(saved.variants.length, 2);
  assert.equal(
    saved.variants.find((variant) => variant.id === main.id).config
      .applyModifiers,
    true,
  );
  assert.deepEqual(saved.asset.tags, ["prototype", "robot"]);
  const history = await invoke("list_snapshots", {
    variantId: main.id,
    offset: 0,
    limit: 50,
  });
  assert.equal(history.total, 2);
  await until(() => script('return !!document.querySelector(".asset-card");'));
  await waitForThumbnail("Éclaireur");
  assert.equal(
    await script('return document.querySelectorAll("canvas").length;'),
    0,
  );
  const thumbnailCacheAfterRestart = await fileManifest(
    join(library, "cache", "thumbnails"),
  );
  for (const previous of thumbnailCacheBeforeRestart) {
    assert.deepEqual(
      thumbnailCacheAfterRestart.find((entry) => entry.path === previous.path),
      previous,
      "Restart must reuse persisted thumbnail PNGs without rewriting them.",
    );
  }
  const cached = await invoke("get_asset_thumbnails", {
    assetIds: [imported.asset.id],
  });
  assert.ok(
    cached[imported.asset.id]?.resourceToken,
    "The persisted thumbnail must be available through the cache API after restart.",
  );
  try {
    assert.equal(await resourceAccessible(cached[imported.asset.id].url), true);
  } finally {
    await invoke("release_preview_resources", {
      tokens: [cached[imported.asset.id].resourceToken],
    });
  }
  assert.equal(
    await resourceAccessible(cached[imported.asset.id].url),
    false,
    "An explicitly released thumbnail handle must become inaccessible.",
  );
  report.texturePixels.thumbnailAfterRestart = await assertTexturePixels(
    '.asset-card:has(img[alt*="Éclaireur"]) .asset-thumbnail',
    "08-library-restarted.png",
  );
  await clickText("Éclaireur");
  await until(() =>
    script('return !!document.querySelector(".workspace-panel");'),
  );
  await clickText("Historique");
  await until(() =>
    script('return document.body.innerText.includes("Textures et rapport");'),
  );
  await screenshot("04-history.png");
  await clickText("Consulter");
  await until(() =>
    script('return !!document.querySelector(".history-banner");'),
  );
  await waitForModel("model.glb");
  assert.equal(
    await script('return document.querySelector(".blender-button").disabled;'),
    true,
  );
  assert.equal(
    await script('return !document.querySelector(".config-form");'),
    true,
  );
  await screenshot("ui-history-readonly.png");
  report.checks.push(
    "Redémarrage : standard GLB conservé, vignette texturée persistante réutilisée, classement, variantes et historique conservés",
  );
  assert.deepEqual(
    await fileManifest(source),
    sourceOriginal,
    "The imported source folder must remain unchanged throughout the desktop scenarios",
  );
  await testHistoryStorage();
  report.status = "passed";
  console.log(JSON.stringify(report, null, 2));
} catch (error) {
  report.status = "failed";
  report.error = String(error?.stack ?? error);
  if (session) {
    try {
      await screenshot("failure.png");
      report.body = await script("return document.body.innerText;");
      report.desktopMetadata = await script(
        "return window.__TAURI_INTERNALS__.metadata;",
      );
    } catch {}
  }
  console.error(report.error);
  process.exitCode = 1;
} finally {
  if (session) {
    try {
      await request("DELETE", `/session/${session}`);
    } catch {}
  }
  child.kill();
  await writeFile(
    join(testRoot, "report.json"),
    JSON.stringify(report, null, 2),
  );
  await writeFile(join(testRoot, "driver.log"), driverLog);
  console.log(`Rapport : ${join(testRoot, "report.json")}`);
}
