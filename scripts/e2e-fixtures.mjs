import { deflateSync } from "node:zlib";
import { mkdir, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { join } from "node:path";
import { BoxGeometry } from "three";

/** Run only disposable fixture authoring in a private Blender process. */
export async function runFixtureBlender(
  blender,
  temporaryRoot,
  script,
  payload,
) {
  await Promise.all(
    ["config", "scripts", "extensions", "temp"].map((name) =>
      mkdir(join(temporaryRoot, name), { recursive: true }),
    ),
  );
  return new Promise((resolve, reject) => {
    const child = spawn(
      blender,
      [
        "--background",
        "--factory-startup",
        "--disable-autoexec",
        "--offline-mode",
        "--python-exit-code",
        "1",
        "--python-expr",
        script,
        "--",
        JSON.stringify(payload),
      ],
      {
        cwd: temporaryRoot,
        windowsHide: true,
        env: {
          ...process.env,
          BLENDER_USER_CONFIG: join(temporaryRoot, "config"),
          BLENDER_USER_SCRIPTS: join(temporaryRoot, "scripts"),
          BLENDER_USER_EXTENSIONS: join(temporaryRoot, "extensions"),
          TEMP: join(temporaryRoot, "temp"),
          TMP: join(temporaryRoot, "temp"),
          TMPDIR: join(temporaryRoot, "temp"),
        },
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    let log = "";
    const capture = (chunk) => {
      log = (log + chunk).slice(-32_000);
    };
    child.stdout.on("data", capture);
    child.stderr.on("data", capture);
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error("Disposable Blender fixture timed out"));
    }, 120_000);
    child.on("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      if (code === 0) resolve(log);
      else
        reject(
          new Error(`Disposable Blender fixture failed (${code}): ${log}`),
        );
    });
  });
}

/** A single editable .blend with a packed texture; animated by default, no GLB. */
export async function prepareBlendFixture(
  blender,
  directory,
  temporaryRoot,
  options = {},
) {
  await mkdir(directory, { recursive: true });
  await mkdir(temporaryRoot, { recursive: true });
  const seed = join(temporaryRoot, "seed.glb");
  await writeFile(
    seed,
    options.studioFloor
      ? texturedStudioScene()
      : animatedTexturedTriangle(options),
  );
  await runFixtureBlender(
    blender,
    temporaryRoot,
    `
import bpy,json,sys
from pathlib import Path
r=json.loads(sys.argv[-1])
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=r['seed'])
for image in bpy.data.images:
    if image.has_data and not image.packed_file: image.pack()
bpy.context.preferences.filepaths.save_version=0
bpy.context.preferences.filepaths.file_preview_type='NONE'
bpy.ops.wm.save_as_mainfile(filepath=str(Path(r['directory'])/'model.blend'),check_existing=False)
`,
    { seed, directory },
  );
}

/** Simulate the user's Save after adding a Mirror; only call on disposable workspaces. */
export async function saveMirrorFixture(blender, blendFile, temporaryRoot) {
  await runFixtureBlender(
    blender,
    temporaryRoot,
    `
import bpy,json,sys
r=json.loads(sys.argv[-1])
bpy.ops.wm.open_mainfile(filepath=r['blendFile'],use_scripts=False)
mesh=next(obj for obj in bpy.context.scene.objects if obj.type=='MESH')
modifier=mesh.modifiers.new('Edge Lock regression Mirror','MIRROR')
modifier.use_mirror_merge=False
bpy.context.preferences.filepaths.save_version=0
bpy.context.preferences.filepaths.file_preview_type='NONE'
bpy.ops.wm.save_as_mainfile(filepath=r['blendFile'],check_existing=False)
`,
    { blendFile },
  );
}

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++)
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(type, data) {
  const chunk = Buffer.alloc(data.length + 12);
  chunk.writeUInt32BE(data.length);
  chunk.write(type, 4);
  data.copy(chunk, 8);
  chunk.writeUInt32BE(
    crc32(chunk.subarray(4, 8 + data.length)),
    8 + data.length,
  );
  return chunk;
}

function contrastingPng() {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(8, 0);
  header.writeUInt32BE(8, 4);
  header[8] = 8;
  header[9] = 6; // RGBA, one unfiltered row followed by eight pixels.
  const pixels = Buffer.alloc(8 * (1 + 8 * 4));
  for (let y = 0; y < 8; y++)
    for (let x = 0; x < 8; x++) {
      const offset = y * 33 + 1 + x * 4;
      pixels[offset] = x < 4 ? 255 : 16;
      pixels[offset + 1] = x < 4 ? 16 : 255;
      pixels[offset + 2] = 16;
      pixels[offset + 3] = 255;
    }
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk("IHDR", header),
    pngChunk("IDAT", deflateSync(pixels)),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

/** A white material can only show red and green when the embedded texture loads. */
export function animatedTexturedTriangle({ dataUri = false } = {}) {
  const texture = contrastingPng();
  const binary = Buffer.alloc(
    Math.ceil((100 + (dataUri ? 0 : texture.length)) / 4) * 4,
  );
  [-0.8, 0, 0, 0.8, 0, 0, 0, 1.4, 0].forEach((v, i) =>
    binary.writeFloatLE(v, i * 4),
  );
  [0, 1, 2].forEach((v, i) => binary.writeUInt16LE(v, 36 + i * 2));
  binary.writeFloatLE(0, 44);
  binary.writeFloatLE(2, 48);
  [0, 0, 0, 0, 0.5, 0].forEach((v, i) => binary.writeFloatLE(v, 52 + i * 4));
  [0, 0, 1, 0, 0.5, 1].forEach((v, i) => binary.writeFloatLE(v, 76 + i * 4));
  if (!dataUri) texture.copy(binary, 100);
  const gltf = {
    asset: { version: "2.0", generator: "Edge Lock disposable E2E fixture" },
    scene: 0,
    scenes: [{ nodes: [0] }],
    nodes: [{ mesh: 0, name: "Éclaireur" }],
    meshes: [
      {
        primitives: [
          {
            attributes: { POSITION: 0, TEXCOORD_0: 4 },
            indices: 1,
            material: 0,
          },
        ],
      },
    ],
    materials: [
      {
        doubleSided: true,
        pbrMetallicRoughness: {
          baseColorFactor: [1, 1, 1, 1],
          baseColorTexture: { index: 0 },
          metallicFactor: 0,
          roughnessFactor: 1,
        },
      },
    ],
    textures: [{ sampler: 0, source: 0 }],
    samplers: [
      { magFilter: 9728, minFilter: 9728, wrapS: 33071, wrapT: 33071 },
    ],
    images: [
      dataUri
        ? { uri: `data:image/png;base64,${texture.toString("base64")}` }
        : { bufferView: 5, mimeType: "image/png" },
    ],
    buffers: [{ byteLength: binary.length }],
    bufferViews: [
      { buffer: 0, byteOffset: 0, byteLength: 36 },
      { buffer: 0, byteOffset: 36, byteLength: 6 },
      { buffer: 0, byteOffset: 44, byteLength: 8 },
      { buffer: 0, byteOffset: 52, byteLength: 24 },
      { buffer: 0, byteOffset: 76, byteLength: 24 },
      ...(dataUri
        ? []
        : [{ buffer: 0, byteOffset: 100, byteLength: texture.length }]),
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
      { bufferView: 4, componentType: 5126, count: 3, type: "VEC2" },
    ],
    animations: [
      {
        name: "Flottement",
        samplers: [{ input: 2, output: 3, interpolation: "LINEAR" }],
        channels: [{ sampler: 0, target: { node: 0, path: "translation" } }],
      },
    ],
  };
  return encodeGlb(gltf, binary);
}

/** Two textured volumes touching a huge blue studio slab reproduce the framing bug. */
export function texturedStudioScene() {
  const geometry = new BoxGeometry();
  const parts = [
    geometry.getAttribute("position").array,
    geometry.getAttribute("normal").array,
    geometry.getAttribute("uv").array,
    geometry.index.array,
  ].map((array) =>
    Buffer.from(array.buffer, array.byteOffset, array.byteLength),
  );
  parts.push(contrastingPng());
  const bufferViews = [];
  let offset = 0;
  for (const part of parts) {
    bufferViews.push({
      buffer: 0,
      byteOffset: offset,
      byteLength: part.length,
    });
    offset += Math.ceil(part.length / 4) * 4;
  }
  const binary = Buffer.alloc(offset);
  parts.forEach((part, index) =>
    part.copy(binary, bufferViews[index].byteOffset),
  );
  geometry.dispose();
  const primitive = (material) => ({
    primitives: [
      {
        attributes: { POSITION: 0, NORMAL: 1, TEXCOORD_0: 2 },
        indices: 3,
        material,
      },
    ],
  });
  return encodeGlb(
    {
      asset: {
        version: "2.0",
        generator: "Edge Lock studio regression fixture",
      },
      scene: 0,
      scenes: [{ nodes: [0, 1, 2] }],
      nodes: [
        { mesh: 0, translation: [-2.1, 1.475, 0], scale: [4, 2.8, 1.7] },
        { mesh: 0, translation: [2.1, 1.475, 0], scale: [4, 2.8, 1.7] },
        { mesh: 1, translation: [0, 0.045, 0], scale: [200, 0.06, 200] },
      ],
      meshes: [primitive(0), primitive(1)],
      materials: [
        {
          pbrMetallicRoughness: {
            baseColorFactor: [1, 1, 1, 1],
            baseColorTexture: { index: 0 },
            metallicFactor: 0,
            roughnessFactor: 1,
          },
        },
        {
          pbrMetallicRoughness: {
            baseColorFactor: [0, 0, 1, 1],
            metallicFactor: 0,
            roughnessFactor: 1,
          },
        },
      ],
      textures: [{ sampler: 0, source: 0 }],
      samplers: [{ magFilter: 9728, minFilter: 9728 }],
      images: [{ bufferView: 4, mimeType: "image/png" }],
      buffers: [{ byteLength: binary.length }],
      bufferViews,
      accessors: [
        {
          bufferView: 0,
          componentType: 5126,
          count: 24,
          type: "VEC3",
          min: [-0.5, -0.5, -0.5],
          max: [0.5, 0.5, 0.5],
        },
        { bufferView: 1, componentType: 5126, count: 24, type: "VEC3" },
        { bufferView: 2, componentType: 5126, count: 24, type: "VEC2" },
        { bufferView: 3, componentType: 5123, count: 36, type: "SCALAR" },
      ],
    },
    binary,
  );
}

function encodeGlb(gltf, binary) {
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
