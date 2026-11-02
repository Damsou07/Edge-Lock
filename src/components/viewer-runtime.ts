import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import {
  inspectGlb,
  isEmbeddedImageUri,
  MAX_PREVIEW_BYTES,
} from "./viewer-utils";

export function createPreviewRenderer({
  pixelRatio = Math.min(window.devicePixelRatio || 1, 1.5),
  preserveDrawingBuffer = false,
} = {}) {
  const renderer = new THREE.WebGLRenderer({
    antialias: true,
    alpha: false,
    preserveDrawingBuffer,
  });
  renderer.setPixelRatio(pixelRatio);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1;
  return renderer;
}

export function disposePreviewRenderer(renderer: THREE.WebGLRenderer) {
  renderer.dispose();
  renderer.forceContextLoss();
  renderer.domElement.remove();
}

export function createPreviewStage(renderer: THREE.WebGLRenderer) {
  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(42, 1, 0.01, 10_000);
  scene.background = new THREE.Color("#111d2b");
  scene.add(new THREE.HemisphereLight(0xffffff, 0x667080, 0.75));
  const key = new THREE.DirectionalLight(0xffffff, 2.2);
  key.position.set(4, 6, 5);
  scene.add(key);
  const fill = new THREE.DirectionalLight(0xe7f4ff, 0.8);
  fill.position.set(-4, 2, -3);
  scene.add(fill);

  // A local studio environment lights metallic PBR materials without replacing
  // their GLB textures, UVs, alpha modes or material settings.
  const room = new RoomEnvironment();
  const generator = new THREE.PMREMGenerator(renderer);
  let environment: THREE.WebGLRenderTarget;
  try {
    environment = generator.fromScene(room, 0.04);
    scene.environment = environment.texture;
  } finally {
    room.dispose();
    generator.dispose();
  }
  return {
    scene,
    camera,
    dispose() {
      scene.environment = null;
      environment.dispose();
      scene.clear();
    },
  };
}

export async function loadPreviewModel(url: string, signal?: AbortSignal) {
  signal?.throwIfAborted();
  const response = await fetch(url, { signal });
  if (!response.ok)
    throw new Error("Impossible de charger le fichier d’aperçu.");
  if (Number(response.headers.get("content-length")) > MAX_PREVIEW_BYTES)
    throw new Error("Cet aperçu dépasse 256 Mo. Ouvrez l’asset dans Blender.");
  const bytes = await response.arrayBuffer();
  inspectGlb(bytes);
  signal?.throwIfAborted();
  const manager = new THREE.LoadingManager();
  manager.setURLModifier((resource) => {
    if (!resource.startsWith("blob:") && !isEmbeddedImageUri(resource))
      throw new Error("Ressource externe interdite dans l’aperçu.");
    return resource;
  });
  const model = await new GLTFLoader(manager).parseAsync(bytes, "");
  if (signal?.aborted) {
    releaseModel(model.scene, ...model.scenes);
    signal.throwIfAborted();
  }
  return model;
}

export function framePreviewModel(
  root: THREE.Object3D,
  camera: THREE.PerspectiveCamera,
) {
  const box = new THREE.Box3().setFromObject(root);
  if (box.isEmpty())
    throw new Error("Cette scène ne contient aucune géométrie visible.");
  const target = box.getCenter(new THREE.Vector3());
  const radius = Math.max(box.getSize(new THREE.Vector3()).length() / 2, 0.01);
  if (!Number.isFinite(radius))
    throw new Error("Les dimensions de la géométrie sont invalides.");
  const verticalFov = THREE.MathUtils.degToRad(camera.fov / 2);
  const limitingFov = Math.min(
    verticalFov,
    Math.atan(Math.tan(verticalFov) * camera.aspect),
  );
  const distance = (radius / Math.sin(limitingFov)) * 1.2;
  camera.position
    .copy(target)
    .add(new THREE.Vector3(1, 0.7, 1.2).normalize().multiplyScalar(distance));
  camera.near = Math.max(radius / 1000, 0.0001);
  camera.far = Math.max(radius * 100, 10);
  camera.lookAt(target);
  camera.updateProjectionMatrix();
  return { target, radius };
}

export function releaseModel(...roots: THREE.Object3D[]) {
  const geometries = new Set<THREE.BufferGeometry>();
  const materials = new Set<THREE.Material>();
  const textures = new Set<THREE.Texture>();
  const skeletons = new Set<THREE.Skeleton>();
  const bitmaps = new Set<ImageBitmap>();
  for (const root of roots)
    root.traverse((object) => {
      if (
        object instanceof THREE.Mesh ||
        object instanceof THREE.Line ||
        object instanceof THREE.Points
      ) {
        geometries.add(object.geometry);
        for (const material of Array.isArray(object.material)
          ? object.material
          : [object.material])
          materials.add(material);
      }
      if (object instanceof THREE.SkinnedMesh) skeletons.add(object.skeleton);
    });
  for (const material of materials)
    for (const value of Object.values(material))
      if (value instanceof THREE.Texture) textures.add(value);
  for (const texture of textures) {
    const bitmap: unknown = texture.source.data;
    if (typeof ImageBitmap !== "undefined" && bitmap instanceof ImageBitmap)
      bitmaps.add(bitmap);
    texture.dispose();
  }
  bitmaps.forEach((bitmap) => bitmap.close());
  geometries.forEach((resource) => resource.dispose());
  materials.forEach((resource) => resource.dispose());
  skeletons.forEach((resource) => resource.dispose());
}
