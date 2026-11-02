import { deflateSync } from "node:zlib";

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
