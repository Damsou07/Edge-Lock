import assert from "node:assert/strict";
import { test } from "node:test";
import { inflateSync } from "node:zlib";
import { mkdir, mkdtemp, readdir, readFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import {
  animatedTexturedTriangle,
  texturedStudioScene,
  prepareBlendFixture,
  saveMirrorFixture,
  runFixtureBlender,
} from "./e2e-fixtures.mjs";

test("studio regression fixture keeps two textured volumes and its oversized blue floor", () => {
  const glb = texturedStudioScene();
  assert.equal(glb.readUInt32LE(8), glb.length);
  const length = glb.readUInt32LE(12);
  const document = JSON.parse(glb.subarray(20, 20 + length).toString());
  assert.equal(document.nodes.length, 3);
  assert.equal(document.animations, undefined);
  for (const node of document.nodes.slice(0, 2)) {
    assert.ok(node.scale.every((value) => value > 1));
    const primitive = document.meshes[node.mesh].primitives[0];
    assert.equal(document.accessors[primitive.attributes.TEXCOORD_0].count, 24);
    assert.equal(
      document.materials[primitive.material].pbrMetallicRoughness
        .baseColorTexture.index,
      0,
    );
  }
  const floor = document.nodes[2];
  assert.deepEqual(floor.scale, [200, 0.06, 200]);
  assert.deepEqual(floor.translation, [0, 0.045, 0]);
  const primitive = document.meshes[floor.mesh].primitives[0];
  assert.deepEqual(
    document.materials[primitive.material].pbrMetallicRoughness.baseColorFactor,
    [0, 0, 1, 1],
  );
  assert.equal(document.images[0].mimeType, "image/png");
});

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

test(
  "desktop Blender fixture is source-only and saved Mirror remains editable",
  {
    skip: !process.env.EDGE_LOCK_E2E_BLENDER,
  },
  async () => {
    await mkdir(resolve(".test-data"), { recursive: true });
    const root = await mkdtemp(resolve(".test-data", "blend-fixture-"));
    const source = join(root, "source");
    const blender = resolve(process.env.EDGE_LOCK_E2E_BLENDER);
    await prepareBlendFixture(blender, source, join(root, "author"));
    assert.deepEqual(await readdir(source), ["model.blend"]);
    const blendFile = join(source, "model.blend");
    const original = await readFile(blendFile);
    assert.ok(original.length > 1024);
    await saveMirrorFixture(blender, blendFile, join(root, "edit"));
    assert.notDeepEqual(await readFile(blendFile), original);
    await runFixtureBlender(
      blender,
      join(root, "verify"),
      `
import bpy,json,sys
r=json.loads(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=r['blendFile'],use_scripts=False)
mesh=next(obj for obj in bpy.context.scene.objects if obj.type=='MESH')
assert len(mesh.modifiers)==1 and mesh.modifiers[0].type=='MIRROR'
assert any(image.packed_file for image in bpy.data.images)
assert len(bpy.data.actions)>0
`,
      { blendFile },
    );
    assert.deepEqual(await readdir(source), ["model.blend"]);
  },
);
