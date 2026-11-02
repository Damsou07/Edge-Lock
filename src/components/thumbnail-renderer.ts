import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import { Box3 } from "three";
import {
  createPreviewRenderer,
  createPreviewStage,
  disposePreviewRenderer,
  framePreviewModel,
  loadPreviewModel,
  releaseModel,
} from "./viewer-runtime";

export async function renderThumbnail(
  url: string,
  signal?: AbortSignal,
): Promise<number[]> {
  signal?.throwIfAborted();
  const renderer = createPreviewRenderer({
    pixelRatio: 1,
    preserveDrawingBuffer: true,
  });
  let stage: ReturnType<typeof createPreviewStage> | undefined;
  let model: GLTF | undefined;
  try {
    renderer.setSize(512, 384, false);
    stage = createPreviewStage(renderer);
    stage.camera.aspect = 512 / 384;
    model = await loadPreviewModel(url, signal);
    signal?.throwIfAborted();
    stage.scene.add(model.scene);
    if (!new Box3().setFromObject(model.scene).isEmpty())
      framePreviewModel(model.scene, stage.camera);
    renderer.render(stage.scene, stage.camera);
    const png = await new Promise<Blob>((resolve, reject) => {
      renderer.domElement.toBlob((blob) => {
        if (blob) resolve(blob);
        else reject(new Error("Impossible de créer la miniature de l’asset."));
      }, "image/png");
    });
    signal?.throwIfAborted();
    const bytes = await png.arrayBuffer();
    signal?.throwIfAborted();
    return Array.from(new Uint8Array(bytes));
  } finally {
    if (model) releaseModel(model.scene, ...(model.scenes ?? []));
    stage?.dispose();
    disposePreviewRenderer(renderer);
  }
}
