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
  unlink,
} from "node:fs/promises";
import { createReadStream } from "node:fs";
import { spawn } from "node:child_process";
import { resolve, join, relative, isAbsolute, sep } from "node:path";
import { createHash } from "node:crypto";
import { animatedTexturedTriangle as animatedTriangle } from "./e2e-fixtures.mjs";

const root = resolve(".");
await mkdir(".test-data", { recursive: true });
const testRoot = await mkdtemp(resolve(".test-data", "desktop-"));
const settings = join(testRoot, "settings");
const library = join(testRoot, "library");
const source = join(testRoot, "source é");
const unavailableBlender = join(testRoot, "unavailable-blender.exe");
await mkdir(source);
await mkdir(settings);
if (process.env.EDGE_LOCK_E2E_KITSUNE_SOURCE) {
  await assert.rejects(() => access(unavailableBlender), { code: "ENOENT" });
  await writeFile(
    join(settings, "settings.json"),
    JSON.stringify({ libraryPath: null, blenderPath: unavailableBlender }),
  );
}
await mkdir(join(source, "empty"));
await writeFile(join(source, "model.glb"), animatedTriangle());
await writeFile(join(source, "notes.txt"), "original");
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

async function assertTexturePixels(selector, name) {
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
          let red = 0, green = 0;
          for (let i = 0; i < pixels.length; i += 4) {
            const [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]];
            if (r > 60 && r > g * 1.5 && r > b * 1.5) red++;
            // ACES and studio image-based lighting brighten saturated greens.
            // White/missing textures still fail both chromatic dominance checks.
            if (g > 60 && g > r * 1.12 && g > b * 1.25) green++;
          }
          done({ red, green, width, height });
        } catch (error) { done({ error: String(error) }); }
      };
      image.src = 'data:image/png;base64,' + base64;
    `,
    args: [base64, selector],
  });
  assert.ok(!result.error, result.error);
  assert.ok(
    result.red > 20 && result.green > 20,
    `Embedded texture missing in ${selector}: ${JSON.stringify(result)}`,
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
  const deadline = started + 180_000;
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
      "50 imports séquentiels du même petit GLB synthétique texturé ; première visite de ces assets, contenu GLB partagé avec la fixture initiale. Les temps incluent le polling WebDriver. Le maximum WebGL est celui observé après installation de l’instrumentation dans chaque nouveau document.",
  };
  try {
    for (let index = 0; index < 50; index++) {
      const imported = await job(
        {
          kind: "import",
          source,
          name: `Performance galerie ${String(index).padStart(2, "0")}`,
          config: {
            primaryFile: "model.glb",
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
  await mkdir(dataSource);
  const glb = animatedTriangle({ dataUri: true });
  await writeFile(join(dataSource, "data-uri.glb"), glb);
  const imported = await job({
    kind: "import",
    source: dataSource,
    name: "Texture data URI — test",
    config: {
      primaryFile: "data-uri.glb",
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
    await waitForModel("data-uri.glb");
    report.texturePixels.dataUriViewer = await assertTexturePixels(
      ".viewer-canvas",
      "10-data-uri-viewer.png",
    );
    await clickText("Retour à la bibliothèque");
    assert.deepEqual(await readFile(join(dataSource, "data-uri.glb")), glb);
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

async function fileHash(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function fileManifest(directory) {
  const entries = [];
  const visit = async (current, prefix = "") => {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const path = join(current, entry.name);
      const name = prefix ? `${prefix}/${entry.name}` : entry.name;
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

function contentManifest(entries) {
  return entries.map(({ modifiedAt: _modifiedAt, ...entry }) => entry);
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
  assert.ok(
    process.env.EDGE_LOCK_E2E_BLENDER,
    "Kitsune validation requires EDGE_LOCK_E2E_BLENDER.",
  );
  const original = await fileManifest(personalSource);
  const glbs = original.filter(
    (entry) => entry.kind === "file" && /\.glb$/i.test(entry.path),
  );
  assert.ok(
    glbs.length > 0,
    "Kitsune fixture must include its original GLB to test preview priority.",
  );
  const beforeSettings = await invoke("get_bootstrap");
  assert.equal(
    beforeSettings.settings.blenderPath,
    unavailableBlender,
    "The original GLB must work with an unavailable Blender executable.",
  );
  report.kitsune = {
    source: personalSource,
    sourceFileCount: original.filter((entry) => entry.kind === "file").length,
  };
  try {
    const asset = await job(
      {
        kind: "import",
        source: personalSource,
        name: "Sniper Kitsune — dépendances",
        config: {
          primaryFile: null,
          scene: null,
          collection: null,
          applyModifiers: false,
        },
      },
      300_000,
    );
    const variant = asset.variants[0];
    assert.ok(
      /\.blend$/i.test(variant.config.primaryFile),
      "Kitsune must retain its Blender primary source.",
    );
    const workspace = await realpath(variant.workspacePath);
    assertWithin(await realpath(testRoot), workspace);
    assertWithin(await realpath(library), workspace);
    assert.notEqual(
      workspace,
      personalSource,
      "The imported workspace must be separate from personal files.",
    );
    const importedFiles = await fileManifest(workspace);
    assert.deepEqual(
      contentManifest(importedFiles),
      contentManifest(original),
      "Import must preserve every Kitsune source file.",
    );
    const originalConfig = structuredClone(variant.config);
    const previewRequest = { kind: "preview", variantId: variant.id };
    const directStarted = performance.now();
    let direct;
    try {
      direct = await job(previewRequest, 300_000);
    } catch (error) {
      if (error.code !== "preview_ambiguous") throw error;
      assert.ok(error.details?.candidates?.includes(glbs[0].path));
      previewRequest.previewFile = glbs[0].path;
      direct = await job(previewRequest, 300_000);
    }
    report.kitsune.originalGlbMs = Math.round(
      performance.now() - directStarted,
    );
    assert.equal(direct.sourceKind, "assetGlb");
    assert.ok(glbs.some((entry) => entry.path === direct.sourceFile));
    assert.ok(direct.inspection.geometry.triangles > 0);
    assert.equal(
      (await invoke("get_bootstrap")).settings.blenderPath,
      unavailableBlender,
    );
    assert.equal(
      (await invoke("get_variant", { id: variant.id })).variant.triangles,
      variant.triangles,
      "An adjacent GLB must not overwrite Blender source metrics.",
    );
    const cache = join(library, "cache", "previews");
    const directCache = await fileManifest(cache);
    const directWarm = await concurrentPreviews(previewRequest);
    for (const result of directWarm) {
      assert.equal(result.sourceKind, "assetGlb");
      assert.equal(result.sourceFile, direct.sourceFile);
    }
    assert.deepEqual(
      await fileManifest(cache),
      directCache,
      "Concurrent warm original-GLB previews must reuse the cached files.",
    );
    await script("window.location.reload(); return true;");
    await until(() =>
      script('return document.body.innerText.includes("Sniper Kitsune");'),
    );
    await clickText("Sniper Kitsune");
    await until(() =>
      script('return !!document.querySelector(".workspace-panel");'),
    );
    if (previewRequest.previewFile) {
      await until(() =>
        script(
          'return !!document.querySelector(".preview-source-choice select");',
        ),
      );
      await script(
        'const select=document.querySelector(".preview-source-choice select"); const setter=Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,"value").set; setter.call(select, arguments[0]); select.dispatchEvent(new Event("change",{bubbles:true})); return true;',
        [previewRequest.previewFile],
      );
      await clickText("Utiliser ce GLB");
    }
    await waitForModel(direct.sourceFile);
    await screenshot("06-kitsune-original-glb.png");
    assert.match(
      await script(
        'return document.querySelector(".inspection-note").innerText;',
      ),
      /arêtes/,
    );

    // Only unlink GLBs in the canonical, imported workspace. Personal inputs and
    // immutable history objects remain untouched; the existing .blend is never saved.
    for (const entry of glbs) {
      const target = await realpath(join(workspace, entry.path));
      assertWithin(workspace, target);
      assertWithin(await realpath(testRoot), target);
      assert.ok(/\.glb$/i.test(target));
      assert.ok((await lstat(target)).isFile());
      await unlink(target);
    }
    delete previewRequest.previewFile;
    const expectedWorkspace = importedFiles.filter(
      (entry) => !glbs.some((glb) => glb.path === entry.path),
    );
    assert.deepEqual(await fileManifest(workspace), expectedWorkspace);
    await invoke("set_blender_path", {
      path: resolve(process.env.EDGE_LOCK_E2E_BLENDER),
    });
    const generatedStarted = performance.now();
    const generated = await job(previewRequest, 300_000);
    report.kitsune.generatedMs = Math.round(
      performance.now() - generatedStarted,
    );
    assert.equal(generated.sourceKind, "generatedGlb");
    assert.equal(generated.sourceFile, variant.config.primaryFile);
    assert.ok(generated.inspection.geometry.edges > 0);
    assert.ok(generated.inspection.geometry.triangles > 0);
    const font = generated.inspection.dependencies.find((dependency) =>
      /msyhbd\.ttc/i.test(dependency.path),
    );
    const brush = generated.inspection.dependencies.find((dependency) =>
      /essentials_brushes-gp_draw\.blend/i.test(dependency.path),
    );
    assert.ok(font, "Kitsune's external font must be identified.");
    assert.ok(brush, "Kitsune's missing brush library must be identified.");
    assert.equal(font.status, "external");
    assert.equal(brush.status, "missing");
    assert.equal(font.renderRelevant, false);
    assert.equal(brush.renderRelevant, false);
    const generatedCache = await fileManifest(cache);
    const warmStarted = performance.now();
    const warm = await concurrentPreviews(previewRequest);
    report.kitsune.warmMs = Math.round(performance.now() - warmStarted);
    report.kitsune.concurrentPreviewJobs = warm.length;
    for (const result of warm) {
      assert.equal(result.sourceKind, "generatedGlb");
      assert.deepEqual(result.inspection, generated.inspection);
    }
    assert.deepEqual(
      await fileManifest(cache),
      generatedCache,
      "Concurrent warm Blender previews must not rewrite the GLB or its metadata.",
    );
    await script("window.location.reload(); return true;");
    await until(() =>
      script('return document.body.innerText.includes("Sniper Kitsune");'),
    );
    await clickText("Sniper Kitsune");
    await until(() =>
      script('return !!document.querySelector(".workspace-panel");'),
    );
    await waitForModel(generated.sourceFile);
    const warningText = await script(
      'return Array.from(document.querySelectorAll(".inspection-warnings,.preview-notice")).map(element=>element.innerText).join("\\n");',
    );
    assert.doesNotMatch(
      warningText,
      /msyhbd\.ttc|essentials_brushes-gp_draw\.blend/i,
    );
    const counts = await script(
      'return document.querySelector(".inspection-note").innerText;',
    );
    assert.match(counts, /arêtes/);
    await screenshot("07-kitsune-generated-glb.png");
    assert.deepEqual(
      await fileManifest(workspace),
      expectedWorkspace,
      "Blender preview must never modify imported source files.",
    );
    const after = await invoke("get_variant", { id: variant.id });
    assert.deepEqual(after.variant.config, originalConfig);
    assert.equal(after.variant.headSnapshotId, variant.headSnapshotId);
    const snapshot = await invoke("get_snapshot", {
      id: variant.headSnapshotId,
    });
    for (const glb of glbs)
      assert.ok(snapshot.entries.some((entry) => entry.path === glb.path));
    assert.ok(
      (await readdir(cache)).every((name) => !name.startsWith(".preview-tmp-")),
      "Preview temporary directories must be cleaned.",
    );
    report.kitsune.geometry = generated.inspection.geometry;
    report.kitsune.secondaryDependencies = [font, brush].map(
      ({ path, kind, status, renderRelevant }) => ({
        path,
        kind,
        status,
        renderRelevant,
      }),
    );
    report.checks.push(
      "Sniper Kitsune importé dans une bibliothèque jetable : GLB original sans Blender, conversion réelle avec police/brosse non bloquantes, cache réutilisé, arêtes et rendu WebGL capturés",
    );
  } finally {
    assert.deepEqual(
      await fileManifest(personalSource),
      original,
      "Every personal Kitsune file and its modification time must remain unchanged.",
    );
    report.kitsune.sourcePreserved = true;
  }
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
  await screenshot("01-onboarding.png");
  const opened = await invoke("open_library", { path: library });
  assert.equal(opened.assetCount, 0);
  await script("window.location.reload(); return true;");
  await until(() =>
    script(
      'return document.body.innerText.includes("Importer mon premier asset");',
    ),
  );
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
      primaryFile: "model.glb",
      scene: null,
      collection: null,
      applyModifiers: false,
    },
  });
  const main = imported.variants[0];
  const first = main.headSnapshotId;
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
  await testGalleryPagePerformance();
  await writeFile(join(main.workspacePath, "notes.txt"), "version 2");
  await writeFile(join(main.workspacePath, "rapport.txt"), "nouveau");
  const changes = await job({ kind: "changes", variantId: main.id });
  assert.deepEqual(changes.modified, ["notes.txt"]);
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
  assert.deepEqual(
    await readFile(output),
    await readFile(join(source, "model.glb")),
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
  if (
    process.env.EDGE_LOCK_E2E_BLEND_SOURCE &&
    process.env.EDGE_LOCK_E2E_BLENDER
  ) {
    const blendSource = resolve(process.env.EDGE_LOCK_E2E_BLEND_SOURCE);
    const before = createHash("sha256")
      .update(await readFile(join(blendSource, "main.blend")))
      .digest("hex");
    await invoke("set_blender_path", {
      path: resolve(process.env.EDGE_LOCK_E2E_BLENDER),
    });
    const blenderAsset = await job({
      kind: "import",
      source: blendSource,
      name: "Robot Blender — test",
      config: {
        primaryFile: null,
        scene: null,
        collection: null,
        applyModifiers: false,
      },
    });
    const blenderVariant = blenderAsset.variants[0];
    assert.equal(blenderVariant.config.primaryFile, "main.blend");
    await invoke("update_config", {
      variantId: blenderVariant.id,
      config: { ...blenderVariant.config, collection: "Asset" },
    });
    const blenderGalleryStarted = performance.now();
    await script("window.location.reload(); return true;");
    await waitForThumbnail("Robot Blender");
    report.blenderPreviewPerformance = {
      galleryFirstConversionAndImageMs: Math.round(
        performance.now() - blenderGalleryStarted,
      ),
      conditions:
        "Premier aperçu de cet import Blender préparé par la galerie avant toute analyse explicite ; durée jusqu’à la vignette complète, polling WebDriver inclus.",
    };
    assert.equal(
      await script('return document.querySelectorAll("canvas").length;'),
      0,
    );
    const blenderWarmStarted = performance.now();
    const derived = await job({
      kind: "preview",
      variantId: blenderVariant.id,
    });
    report.blenderPreviewPerformance.warmPreviewJobMs = Math.round(
      performance.now() - blenderWarmStarted,
    );
    report.blenderPreviewPerformance.sourceKind = derived.sourceKind;
    assert.ok(derived.url.startsWith("http://edge-preview.localhost/"));
    await invoke("release_preview_resources", {
      tokens: [new URL(derived.url).pathname.slice(1)],
    });
    const inspection = await job({
      kind: "inspect",
      variantId: blenderVariant.id,
    });
    assert.equal(inspection.geometry.triangles, 12);
    const blenderOutput = join(testRoot, "blender-export.glb");
    await job({
      kind: "export",
      variantId: blenderVariant.id,
      destination: blenderOutput,
      overwrite: false,
    });
    assert.equal(
      (await readFile(blenderOutput)).subarray(0, 4).toString(),
      "glTF",
    );
    assert.equal(
      createHash("sha256")
        .update(await readFile(join(blendSource, "main.blend")))
        .digest("hex"),
      before,
    );
    const created = await job({
      kind: "create",
      name: "Nouveau projet Blender",
    });
    await access(join(created.variants[0].workspacePath, "main.blend"));
    await script("window.location.reload(); return true;");
    await until(() =>
      script('return document.body.innerText.includes("Robot Blender");'),
    );
    await clickText("Robot Blender");
    await until(() =>
      script('return !!document.querySelector(".workspace-panel");'),
    );
    await waitForModel("main.blend");
    assert.ok(
      await script(
        "return !!document.querySelector('[aria-label=\"Lire l’animation\"]');",
      ),
    );
    await screenshot("05-blender-viewer.png");
    report.checks.push(
      "Blender réel via Tauri : détection principale, collection12 triangles, aperçu/export, source inchangée et création native de projet",
    );
  }
  const thumbnailCacheBeforeRestart = (
    await fileManifest(join(library, "cache", "thumbnails"))
  ).filter((entry) => entry.kind === "file" && entry.path.endsWith(".png"));
  assert.ok(thumbnailCacheBeforeRestart.length > 0);
  await request("DELETE", `/session/${session}`);
  session = undefined;
  await newSession();
  const saved = await invoke("get_asset", { id: imported.asset.id });
  assert.equal(saved.variants.length, 2);
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
  report.checks.push(
    "Redémarrage : vignette texturée persistante réutilisée sans réécriture, classement, variantes et historique conservés",
  );
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
