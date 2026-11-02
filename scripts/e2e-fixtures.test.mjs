import assert from "node:assert/strict";
import { test } from "node:test";
import { inflateSync } from "node:zlib";
import { animatedTexturedTriangle } from "./e2e-fixtures.mjs";

test("desktop fixture embeds a contrasting PNG used by its UV-mapped material", () => {
  const glb = animatedTexturedTriangle();
  assert.equal(glb.subarray(0, 4).toString(), "glTF");
  assert.equal(glb.readUInt32LE(8), glb.length);
  const jsonLength = glb.readUInt32LE(12);
  const document = JSON.parse(glb.subarray(20, 20 + jsonLength).toString());
  const primitive = document.meshes[0].primitives[0];
  assert.equal(document.accessors[primitive.attributes.TEXCOORD_0].count, 3);
  const material = document.materials[primitive.material].pbrMetallicRoughness;
  assert.deepEqual(material.baseColorFactor, [1, 1, 1, 1]);
  const image =
    document.images[document.textures[material.baseColorTexture.index].source];
  assert.equal(image.mimeType, "image/png");
  const view = document.bufferViews[image.bufferView];
  const png = glb.subarray(
    28 + jsonLength + view.byteOffset,
    28 + jsonLength + view.byteOffset + view.byteLength,
  );
  assert.deepEqual([...png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);
  let position = 8;
  const compressed = [];
  while (position < png.length) {
    const length = png.readUInt32BE(position);
    if (png.subarray(position + 4, position + 8).toString() === "IDAT")
      compressed.push(png.subarray(position + 8, position + 8 + length));
    position += length + 12;
  }
  const pixels = inflateSync(Buffer.concat(compressed));
  assert.deepEqual([...pixels.subarray(1, 4)], [255, 16, 16]);
  assert.deepEqual([...pixels.subarray(17, 20)], [16, 255, 16]);
});

test("data URI fixture loads its texture through a PNG data URL without an image bufferView", () => {
  const glb = animatedTexturedTriangle({ dataUri: true });
  const jsonLength = glb.readUInt32LE(12);
  const document = JSON.parse(glb.subarray(20, 20 + jsonLength).toString());
  assert.match(document.images[0].uri, /^data:image\/png;base64,/);
  assert.equal(document.images[0].bufferView, undefined);
  const png = Buffer.from(document.images[0].uri.split(",")[1], "base64");
  assert.deepEqual([...png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);
  assert.equal(document.bufferViews.length, 5);
  assert.equal(document.buffers[0].byteLength, 100);
});
