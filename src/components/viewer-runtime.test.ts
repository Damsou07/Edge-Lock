import { afterEach, describe, expect, it, vi } from "vitest";
import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import {
  framePreviewModel,
  loadPreviewModel,
  releaseModel,
} from "./viewer-runtime";

function documentBytes() {
  const json = new TextEncoder().encode('{"asset":{"version":"2.0"}}');
  const length = Math.ceil(json.length / 4) * 4;
  const bytes = new Uint8Array(20 + length).fill(32);
  const view = new DataView(bytes.buffer);
  view.setUint32(0, 0x46546c67, true);
  view.setUint32(4, 2, true);
  view.setUint32(8, bytes.length, true);
  view.setUint32(12, length, true);
  view.setUint32(16, 0x4e4f534a, true);
  bytes.set(json, 20);
  return bytes.buffer;
}

function studioScene() {
  const scene = new THREE.Group();
  const floor = new THREE.Mesh(new THREE.BoxGeometry(200, 0.06, 200));
  floor.position.y = 0.03;
  const subject = new THREE.Group();
  for (const x of [-2, 2]) {
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(4, 2, 1));
    mesh.position.set(x, 1.06, 0);
    subject.add(mesh);
  }
  scene.add(subject, floor);
  return { scene, subject, floor };
}

async function decodeScene(
  scene: THREE.Group,
  animations: THREE.AnimationClip[] = [],
) {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(new Response(documentBytes())),
  );
  vi.spyOn(GLTFLoader.prototype, "parseAsync").mockResolvedValue({
    scene,
    scenes: [scene],
    animations,
  } as unknown as GLTF);
  return loadPreviewModel("http://edge-preview.localhost/studio");
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("ressources communes des aperçus", () => {
  it("explique un échec réel d’allocation pendant la lecture", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        arrayBuffer: () =>
          Promise.reject(new RangeError("Array buffer allocation failed")),
      }),
    );
    await expect(loadPreviewModel("large.glb")).rejects.toThrow(
      /Mémoire insuffisante/,
    );
  });
  it("explique un échec d’allocation signalé par le protocole local", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(new Response(null, { status: 507 })),
    );
    await expect(loadPreviewModel("large.glb")).rejects.toThrow(
      /Mémoire insuffisante/,
    );
  });
  it("ne refuse pas le chargement selon la taille annoncée du GLB", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(documentBytes(), {
          headers: { "content-length": String(300 * 1024 * 1024) },
        }),
      ),
    );
    const scene = new THREE.Group();
    vi.spyOn(GLTFLoader.prototype, "parseAsync").mockResolvedValue({
      scene,
      scenes: [scene],
      animations: [],
    } as unknown as GLTF);
    await expect(loadPreviewModel("model.glb")).resolves.toHaveProperty(
      "scene",
      scene,
    );
  });
  it("masque le sol disproportionné au décodage et cadre le sujet sans altérer ses ressources", async () => {
    const { scene, subject, floor } = studioScene();
    const geometry = floor.geometry;
    const material = floor.material;
    const children = scene.children.slice();
    await decodeScene(scene);
    expect(floor.visible).toBe(false);
    expect(floor.geometry).toBe(geometry);
    expect(floor.material).toBe(material);
    expect(scene.children).toEqual(children);
    const camera = new THREE.PerspectiveCamera(42, 4 / 3);
    const expected = new THREE.Box3().setFromObject(subject);
    const framed = framePreviewModel(scene, camera);
    expect(
      framed.target.distanceTo(expected.getCenter(new THREE.Vector3())),
    ).toBeLessThan(1e-6);
    expect(framed.radius).toBeCloseTo(
      expected.getSize(new THREE.Vector3()).length() / 2,
    );
    floor.visible = true;
    framePreviewModel(scene, camera);
    expect(floor.visible).toBe(true); // Preparation is not repeated on recenter.
  });

  it("détecte le sol en coordonnées monde sous un parent transformé", async () => {
    const { scene, floor } = studioScene();
    const parent = new THREE.Group();
    parent.position.set(12, -4, 6);
    parent.rotation.y = Math.PI / 3;
    parent.scale.setScalar(3);
    parent.add(scene);
    await decodeScene(parent);
    expect(floor.visible).toBe(false);
    expect(parent.position.toArray()).toEqual([12, -4, 6]);
    expect(parent.scale.toArray()).toEqual([3, 3, 3]);
  });

  it.each([
    "sol seul",
    "un seul accessoire",
    "sol normal",
    "mur vertical",
    "sujet plat",
    "sujet hors emprise",
    "sol au-dessus du sujet",
    "deux sols",
    "descendant rendable",
    "morph",
    "instances",
    "animation",
    "parent animé",
  ])("conserve la scène ambiguë : %s", async (scenario) => {
    const { scene, subject, floor } = studioScene();
    const animations: THREE.AnimationClip[] = [];
    if (scenario === "sol seul") scene.remove(subject);
    if (scenario === "un seul accessoire") subject.remove(subject.children[1]!);
    if (scenario === "sol normal") floor.scale.set(0.05, 1, 0.05);
    if (scenario === "mur vertical") floor.rotation.z = Math.PI / 2;
    if (scenario === "sujet plat") subject.scale.y = 0.001;
    if (scenario === "sujet hors emprise") subject.position.x = 300;
    if (scenario === "sol au-dessus du sujet") floor.position.y = 2;
    if (scenario === "deux sols") scene.add(floor.clone());
    if (scenario === "descendant rendable") floor.add(subject);
    if (scenario === "morph") {
      floor.geometry.morphAttributes.position = [
        floor.geometry.getAttribute("position").clone(),
      ];
    }
    if (scenario === "instances") {
      const instances = new THREE.InstancedMesh(
        floor.geometry,
        floor.material,
        1,
      );
      instances.position.copy(floor.position);
      scene.remove(floor);
      scene.add(instances);
      await decodeScene(scene);
      expect(instances.visible).toBe(true);
      return;
    }
    if (scenario === "animation" || scenario === "parent animé") {
      const target = scenario === "animation" ? floor : scene;
      animations.push(
        new THREE.AnimationClip("motion", 1, [
          new THREE.VectorKeyframeTrack(
            `${target.uuid}.position`,
            [0, 1],
            [0, 0, 0, 0, 1, 0],
          ),
        ]),
      );
    }
    const model = await decodeScene(scene, animations);
    expect(floor.visible).toBe(true);
    expect(model.animations).toBe(animations);
  });

  it("ignore les géométries et descendants invisibles lors du cadrage", () => {
    const scene = new THREE.Group();
    const visible = new THREE.Mesh(new THREE.BoxGeometry(2, 2, 2));
    const hidden = new THREE.Group();
    hidden.visible = false;
    hidden.add(new THREE.Mesh(new THREE.BoxGeometry(200, 200, 200)));
    visible.add(hidden);
    scene.add(visible);
    const framed = framePreviewModel(scene, new THREE.PerspectiveCamera());
    expect(framed.radius).toBeCloseTo(Math.sqrt(3));
    scene.visible = false;
    expect(() =>
      framePreviewModel(scene, new THREE.PerspectiveCamera()),
    ).toThrow(/géométrie/);
    expect(() =>
      framePreviewModel(visible, new THREE.PerspectiveCamera()),
    ).toThrow(/géométrie/);
  });

  it("cadre les matrices des instances, y compris après modification", () => {
    const mesh = new THREE.InstancedMesh(
      new THREE.BoxGeometry(2, 2, 2),
      new THREE.MeshBasicMaterial(),
      2,
    );
    mesh.setMatrixAt(0, new THREE.Matrix4().makeTranslation(-5, 0, 0));
    mesh.setMatrixAt(1, new THREE.Matrix4().makeTranslation(5, 0, 0));
    const camera = new THREE.PerspectiveCamera();
    expect(framePreviewModel(mesh, camera).radius).toBeCloseTo(
      Math.sqrt(152) / 2,
    );
    mesh.setMatrixAt(1, new THREE.Matrix4().makeTranslation(15, 0, 0));
    expect(framePreviewModel(mesh, camera).target.x).toBeCloseTo(5);
  });

  it("recentre un skin sur sa pose courante", () => {
    const geometry = new THREE.BoxGeometry(2, 2, 2);
    const count = geometry.getAttribute("position").count;
    geometry.setAttribute(
      "skinIndex",
      new THREE.Uint16BufferAttribute(new Uint16Array(count * 4), 4),
    );
    const weights = new Float32Array(count * 4);
    for (let i = 0; i < count; i++) weights[i * 4] = 1;
    geometry.setAttribute(
      "skinWeight",
      new THREE.Float32BufferAttribute(weights, 4),
    );
    const mesh = new THREE.SkinnedMesh(geometry, new THREE.MeshBasicMaterial());
    const bone = new THREE.Bone();
    mesh.add(bone);
    mesh.bind(new THREE.Skeleton([bone]));
    const camera = new THREE.PerspectiveCamera();
    expect(framePreviewModel(mesh, camera).target.x).toBeCloseTo(0);
    bone.position.x = 4;
    expect(framePreviewModel(mesh, camera).target.x).toBeCloseTo(4);
  });

  it("cadre les objets sans modifier leurs matériaux, UV ou transparence", () => {
    const texture = new THREE.Texture();
    const material = new THREE.MeshStandardMaterial({
      map: texture,
      transparent: true,
      opacity: 0.4,
    });
    const geometry = new THREE.BoxGeometry(2, 4, 3);
    const uv = geometry.getAttribute("uv").array.slice();
    const mesh = new THREE.Mesh(geometry, material);
    mesh.position.set(12, 8, -4);
    const camera = new THREE.PerspectiveCamera(42, 4 / 3, 0.01, 10_000);
    const framed = framePreviewModel(mesh, camera);
    expect(framed.target.toArray()).toEqual([12, 8, -4]);
    expect(camera.position.distanceTo(framed.target)).toBeGreaterThan(
      framed.radius,
    );
    expect(mesh.material).toBe(material);
    expect(material.map).toBe(texture);
    expect(material.opacity).toBe(0.4);
    expect(geometry.getAttribute("uv").array).toEqual(uv);
  });

  it.each([0.5, 4 / 3, 1039 / 419])(
    "conserve le modèle entier dans le champ avec une marge resserrée (aspect %s)",
    (aspect) => {
      const mesh = new THREE.Mesh(new THREE.BoxGeometry(8.6, 2.8, 1.7));
      mesh.position.set(12, 3, -4);
      const camera = new THREE.PerspectiveCamera(42, aspect);
      framePreviewModel(mesh, camera);
      camera.updateMatrixWorld(true);
      const box = new THREE.Box3().setFromObject(mesh);
      for (const x of [box.min.x, box.max.x])
        for (const y of [box.min.y, box.max.y])
          for (const z of [box.min.z, box.max.z]) {
            const projected = new THREE.Vector3(x, y, z).project(camera);
            expect(Math.abs(projected.x)).toBeLessThan(1);
            expect(Math.abs(projected.y)).toBeLessThan(1);
            expect(Math.abs(projected.z)).toBeLessThan(1);
          }
    },
  );

  it("refuse les scènes vides plutôt que de publier une image vide", () => {
    expect(() =>
      framePreviewModel(new THREE.Group(), new THREE.PerspectiveCamera()),
    ).toThrow(/géométrie/);
  });

  it("libère une seule fois les ressources partagées entre plusieurs objets", () => {
    const texture = new THREE.Texture();
    const material = new THREE.MeshStandardMaterial({
      map: texture,
      emissiveMap: texture,
    });
    const geometry = new THREE.BoxGeometry();
    const root = new THREE.Group();
    root.add(
      new THREE.Mesh(geometry, material),
      new THREE.Mesh(geometry, material),
    );
    const disposeTexture = vi.spyOn(texture, "dispose");
    const disposeMaterial = vi.spyOn(material, "dispose");
    const disposeGeometry = vi.spyOn(geometry, "dispose");
    releaseModel(root);
    expect(disposeTexture).toHaveBeenCalledTimes(1);
    expect(disposeMaterial).toHaveBeenCalledTimes(1);
    expect(disposeGeometry).toHaveBeenCalledTimes(1);
  });

  it("nettoie le modèle si la sélection est annulée pendant son décodage", async () => {
    const controller = new AbortController();
    const mesh = new THREE.Mesh(
      new THREE.BoxGeometry(),
      new THREE.MeshStandardMaterial(),
    );
    const scene = new THREE.Group();
    scene.add(mesh);
    const dispose = vi.spyOn(mesh.geometry, "dispose");
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(new Response(documentBytes())),
    );
    vi.spyOn(GLTFLoader.prototype, "parseAsync").mockImplementation(
      async () => {
        controller.abort();
        return { scene, scenes: [scene] } as GLTF;
      },
    );
    await expect(
      loadPreviewModel("http://edge-preview.localhost/test", controller.signal),
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(dispose).toHaveBeenCalledTimes(1);
  });
});
