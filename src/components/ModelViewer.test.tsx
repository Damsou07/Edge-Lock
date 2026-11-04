import { act, fireEvent, render, screen } from "@testing-library/react";
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
vi.mock("./viewer-runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./viewer-runtime")>()),
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

it.each([false, true])(
  "présente une scène vide ou masquée sans erreur (masquée : %s)",
  async (hidden) => {
    const scene = new THREE.Group();
    if (hidden) {
      const mesh = new THREE.Mesh(
        new THREE.BoxGeometry(),
        new THREE.MeshBasicMaterial(),
      );
      mesh.visible = false;
      scene.add(mesh);
    }
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
    expect(
      await screen.findByText("Votre scène est vide."),
    ).toBeInTheDocument();
    expect(screen.queryByText("Aperçu indisponible")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Recentrer le modèle" }),
    ).toBeDisabled();
    rendered.unmount();
    expect(releaseModel).toHaveBeenCalledWith(scene, scene);
    expect(disposePreviewRenderer).toHaveBeenCalledTimes(1);
  },
);

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

it("conserve le diagnostic de perte du contexte même si le chargement finit ensuite", async () => {
  let complete!: (model: GLTF) => void;
  vi.mocked(loadPreviewModel).mockReturnValue(
    new Promise((resolve) => {
      complete = resolve;
    }),
  );
  const rendered = render(
    <ModelViewer url="large.glb" label="DOSSIER DE TRAVAIL" />,
  );
  const canvas = screen.getByLabelText("Aperçu 3D interactif");
  fireEvent(canvas, new Event("webglcontextlost", { cancelable: true }));
  expect(
    await screen.findByText(/Le contexte graphique a été perdu/),
  ).toBeInTheDocument();
  const scene = new THREE.Group();
  await act(async () => {
    complete({ scene, scenes: [scene], animations: [] } as unknown as GLTF);
  });
  expect(
    screen.getByText(/Le contexte graphique a été perdu/),
  ).toBeInTheDocument();
  expect(releaseModel).toHaveBeenCalledWith(scene, scene);
  rendered.unmount();
});
