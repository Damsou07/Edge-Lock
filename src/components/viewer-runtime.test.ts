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

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("ressources communes des aperçus", () => {
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
