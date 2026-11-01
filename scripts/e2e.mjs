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
    await clickText("Afficher l’aperçu 3D");
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
    await clickText("Afficher l’aperçu 3D");
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
  await script("window.location.reload(); return true;");
  await until(() => script('return !!document.querySelector(".asset-card");'));
  await screenshot("02-library.png");
  await clickText("Éclaireur");
  await until(() =>
    script('return !!document.querySelector(".workspace-panel");'),
  );
  await clickText("Afficher l’aperçu 3D");
  await until(
    () =>
      script(
        'return !!document.querySelector("canvas") && !!document.querySelector(arguments[0]);',
        ['[aria-label="Lire l’animation"]'],
      ),
    60_000,
  );
  await screenshot("03-viewer.png");
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
    const inspection = await job({
      kind: "inspect",
      variantId: blenderVariant.id,
    });
    assert.equal(inspection.geometry.triangles, 12);
    const derived = await job({
      kind: "preview",
      variantId: blenderVariant.id,
    });
    assert.ok(derived.url.startsWith("http://edge-preview.localhost/"));
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
    await clickText("Afficher l’aperçu 3D");
    await until(
      () =>
        script(
          'return !!document.querySelector("canvas") && !!document.querySelector(arguments[0]);',
          ['[aria-label="Lire l’animation"]'],
        ),
      60_000,
    );
    await screenshot("05-blender-viewer.png");
    report.checks.push(
      "Blender réel via Tauri : détection principale, collection12 triangles, aperçu/export, source inchangée et création native de projet",
    );
  }
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
    "Redémarrage : classement, variantes et historique conservés, historique visible",
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

function animatedTriangle() {
  const binary = Buffer.alloc(76);
  [-0.8, 0, 0, 0.8, 0, 0, 0, 1.4, 0].forEach((v, i) =>
    binary.writeFloatLE(v, i * 4),
  );
  [0, 1, 2].forEach((v, i) => binary.writeUInt16LE(v, 36 + i * 2));
  binary.writeFloatLE(0, 44);
  binary.writeFloatLE(2, 48);
  [0, 0, 0, 0, 0.5, 0].forEach((v, i) => binary.writeFloatLE(v, 52 + i * 4));
  const gltf = {
    asset: { version: "2.0", generator: "Edge Lock disposable E2E fixture" },
    scene: 0,
    scenes: [{ nodes: [0] }],
    nodes: [{ mesh: 0, name: "Éclaireur" }],
    meshes: [
      {
        primitives: [{ attributes: { POSITION: 0 }, indices: 1, material: 0 }],
      },
    ],
    materials: [
      {
        doubleSided: true,
        pbrMetallicRoughness: {
          baseColorFactor: [0.12, 0.55, 0.95, 1],
          metallicFactor: 0.15,
          roughnessFactor: 0.5,
        },
      },
    ],
    buffers: [{ byteLength: binary.length }],
    bufferViews: [
      { buffer: 0, byteOffset: 0, byteLength: 36 },
      { buffer: 0, byteOffset: 36, byteLength: 6 },
      { buffer: 0, byteOffset: 44, byteLength: 8 },
      { buffer: 0, byteOffset: 52, byteLength: 24 },
    ],
    accessors: [
      {
        bufferView: 0,
        componentType: 5126,
        count: 3,
        type: "VEC3",
        min: [-0.8, 0, 0],
        max: [0.8, 1.4, 0],
      },
      { bufferView: 1, componentType: 5123, count: 3, type: "SCALAR" },
      {
        bufferView: 2,
        componentType: 5126,
        count: 2,
        type: "SCALAR",
        min: [0],
        max: [2],
      },
      { bufferView: 3, componentType: 5126, count: 2, type: "VEC3" },
    ],
    animations: [
      {
        name: "Flottement",
        samplers: [{ input: 2, output: 3, interpolation: "LINEAR" }],
        channels: [{ sampler: 0, target: { node: 0, path: "translation" } }],
      },
    ],
  };
  const json = Buffer.from(JSON.stringify(gltf));
  const padded = Buffer.alloc(Math.ceil(json.length / 4) * 4, 0x20);
  json.copy(padded);
  const header = Buffer.alloc(20);
  header.write("glTF");
  header.writeUInt32LE(2, 4);
  header.writeUInt32LE(28 + padded.length + binary.length, 8);
  header.writeUInt32LE(padded.length, 12);
  header.writeUInt32LE(0x4e4f534a, 16);
  const binaryHeader = Buffer.alloc(8);
  binaryHeader.writeUInt32LE(binary.length, 0);
  binaryHeader.writeUInt32LE(0x004e4942, 4);
  return Buffer.concat([header, padded, binaryHeader, binary]);
}
