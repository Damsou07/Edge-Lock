import { beforeEach, describe, expect, it, vi } from "vitest";
import * as THREE from "three";
import { renderThumbnail } from "./thumbnail-renderer";
import {
  createPreviewRenderer,
  createPreviewStage,
  disposePreviewRenderer,
  loadPreviewModel,
  releaseModel,
} from "./viewer-runtime";

vi.mock("./viewer-runtime", () => ({
  createPreviewRenderer: vi.fn(),
  createPreviewStage: vi.fn(),
  disposePreviewRenderer: vi.fn(),
  framePreviewModel: vi.fn(),
  loadPreviewModel: vi.fn(),
  releaseModel: vi.fn(),
}));

let disposeStage: ReturnType<typeof vi.fn<() => void>>;
let render: ReturnType<typeof vi.fn>;
let setSize: ReturnType<typeof vi.fn>;
let png: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.clearAllMocks();
  disposeStage = vi.fn();
  render = vi.fn();
  setSize = vi.fn();
  png = vi.fn((callback: BlobCallback) => {
    const bytes = new Uint8Array([137, 80, 78, 71]);
    const blob = new Blob([bytes], { type: "image/png" });
    Object.defineProperty(blob, "arrayBuffer", {
      value: async () => bytes.buffer,
    });
    callback(blob);
  });
  vi.mocked(createPreviewRenderer).mockReturnValue({
    domElement: { toBlob: png },
    render,
    setSize,
  } as unknown as THREE.WebGLRenderer);
  vi.mocked(createPreviewStage).mockReturnValue({
    scene: new THREE.Scene(),
    camera: new THREE.PerspectiveCamera(),
    dispose: disposeStage,
  });
  vi.mocked(loadPreviewModel).mockResolvedValue({
    scene: new THREE.Group(),
  } as Awaited<ReturnType<typeof loadPreviewModel>>);
});

describe("capture statique de bibliothèque", () => {
  it("produit un PNG 512×384 puis libère le contexte et le modèle", async () => {
    expect(await renderThumbnail("preview://one")).toEqual([137, 80, 78, 71]);
    expect(setSize).toHaveBeenCalledWith(512, 384, false);
    expect(render).toHaveBeenCalledTimes(1);
    expect(png).toHaveBeenCalledWith(expect.any(Function), "image/png");
    expect(disposeStage).toHaveBeenCalledTimes(1);
    expect(releaseModel).toHaveBeenCalledTimes(1);
    expect(disposePreviewRenderer).toHaveBeenCalledTimes(1);
  });

  it("n’alloue pas de contexte pour une requête déjà annulée", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      renderThumbnail("preview://one", controller.signal),
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(createPreviewRenderer).not.toHaveBeenCalled();
  });

  it("libère le contexte même lorsque le chargement échoue", async () => {
    vi.mocked(loadPreviewModel).mockRejectedValue(
      new Error("Modèle illisible"),
    );
    await expect(renderThumbnail("preview://one")).rejects.toThrow(
      "Modèle illisible",
    );
    expect(disposeStage).toHaveBeenCalledTimes(1);
    expect(disposePreviewRenderer).toHaveBeenCalledTimes(1);
  });

  it("refuse une capture annulée pendant l’encodage PNG et nettoie les ressources", async () => {
    const controller = new AbortController();
    png.mockImplementation((callback: BlobCallback) => {
      controller.abort();
      callback(new Blob());
    });
    await expect(
      renderThumbnail("preview://one", controller.signal),
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(disposeStage).toHaveBeenCalledTimes(1);
    expect(releaseModel).toHaveBeenCalledTimes(1);
    expect(disposePreviewRenderer).toHaveBeenCalledTimes(1);
  });
});
