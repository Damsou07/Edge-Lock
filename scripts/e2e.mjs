// Real Windows WebView2 / Tauri backend. Every file is generated under .test-data.
import assert from "node:assert/strict";
import { mkdir, writeFile, readFile, mkdtemp, access } from "node:fs/promises";
import { spawn } from "node:child_process";
import { resolve, join } from "node:path";
import { createHash } from "node:crypto";

const root = resolve(".");
await mkdir(".test-data", { recursive: true });
const testRoot = await mkdtemp(resolve(".test-data", "desktop-"));
const settings = join(testRoot, "settings");
const library = join(testRoot, "library");
const source = join(testRoot, "source é");
await mkdir(source);
await mkdir(settings);
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
async function job(requestValue) {
  const id = await invoke("start_job", { request: requestValue });
  const result = await until(async () => {
    const job = await invoke("get_job", { id });
    return ["completed", "failed", "cancelled"].includes(job.status)
      ? job
      : null;
  }, 120_000);
  if (result.status !== "completed")
    throw Object.assign(
      new Error(result.error?.message ?? result.status),
      result.error,
    );
  return result.result;
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
