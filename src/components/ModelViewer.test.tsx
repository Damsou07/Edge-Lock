import { render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import {
  loadPreviewModel,
  releaseModel,
  disposePreviewRenderer,
} from "./viewer-runtime";
import ModelViewer from "./ModelViewer";

vi.mock("three/addons/controls/OrbitControls.js", () => ({
  OrbitControls: class {
    target = new THREE.Vector3();
    addEventListener() {}
    update() {}
    dispose() {}
  },
}));
vi.mock("./viewer-runtime", () => ({
  createPreviewRenderer: () => ({
    domElement: document.createElement("canvas"),
    setSize: vi.fn(),
    render: vi.fn(),
  }),
  createPreviewStage: () => ({
    scene: new THREE.Scene(),
    camera: new THREE.PerspectiveCamera(),
    dispose: vi.fn(),
  }),
  framePreviewModel: () => ({ target: new THREE.Vector3(), radius: 1 }),
  loadPreviewModel: vi.fn(),
  releaseModel: vi.fn(),
  disposePreviewRenderer: vi.fn(),
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});

it("présente une scène vide sans erreur et libère ses ressources", async () => {
  const scene = new THREE.Group();
  vi.mocked(loadPreviewModel).mockResolvedValue({
    scene,
    scenes: [scene],
    animations: [],
  } as unknown as GLTF);
  const rendered = render(
    <ModelViewer
      url="http://edge-preview.localhost/empty"
      label="DOSSIER DE TRAVAIL"
    />,
  );
  expect(await screen.findByText("Votre scène est vide.")).toBeInTheDocument();
  expect(screen.queryByText("Aperçu indisponible")).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Recentrer le modèle" }),
  ).toBeDisabled();
  rendered.unmount();
  expect(releaseModel).toHaveBeenCalledWith(scene, scene);
  expect(disposePreviewRenderer).toHaveBeenCalledTimes(1);
});

it("affiche normalement la géométrie et recharge le modèle quand l’URL change", async () => {
  const scene = new THREE.Group();
  scene.add(
    new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshBasicMaterial()),
  );
  vi.mocked(loadPreviewModel).mockResolvedValue({
    scene,
    scenes: [scene],
    animations: [],
  } as unknown as GLTF);
  const rendered = render(
    <ModelViewer
      url="http://edge-preview.localhost/one"
      label="DOSSIER DE TRAVAIL"
    />,
  );
  await screen.findByText(/Glisser pour tourner/);
  rendered.rerender(
    <ModelViewer
      url="http://edge-preview.localhost/two"
      label="DOSSIER DE TRAVAIL"
    />,
  );
  await screen.findByText(/Glisser pour tourner/);
  expect(loadPreviewModel).toHaveBeenCalledTimes(2);
  expect(disposePreviewRenderer).toHaveBeenCalledTimes(1);
});
