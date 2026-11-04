import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import {
  loadPreviewModel,
  releaseModel,
  disposePreviewRenderer,
} from "./viewer-runtime";
import ModelViewer from "./ModelViewer";

let fullscreenElement: Element | null = null;
function enterFullscreen(element: Element) {
  fullscreenElement = element;
  document.dispatchEvent(new Event("fullscreenchange"));
  return Promise.resolve();
}
const requestFullscreen = vi.fn(function (this: Element) {
  return enterFullscreen(this);
});
const exitFullscreen = vi.fn(() => {
  fullscreenElement = null;
  document.dispatchEvent(new Event("fullscreenchange"));
  return Promise.resolve();
});

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
  fullscreenElement = null;
  requestFullscreen.mockImplementation(function (this: Element) {
    return enterFullscreen(this);
  });
  Object.defineProperty(document, "fullscreenElement", {
    configurable: true,
    get: () => fullscreenElement,
  });
  Object.defineProperty(HTMLElement.prototype, "requestFullscreen", {
    configurable: true,
    value: requestFullscreen,
  });
  Object.defineProperty(document, "exitFullscreen", {
    configurable: true,
    value: exitFullscreen,
  });
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});

function readyModel() {
  const scene = new THREE.Group();
  scene.add(
    new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshBasicMaterial()),
  );
  vi.mocked(loadPreviewModel).mockResolvedValue({
    scene,
    scenes: [scene],
    animations: [],
  } as unknown as GLTF);
}

it("bascule en plein écran sans recharger le modèle ni remplacer son canvas", async () => {
  readyModel();
  const rendered = render(<ModelViewer url="model.glb" label="TRAVAIL" />);
  await screen.findByText(/Glisser pour tourner/);
  const canvas = screen.getByLabelText("Aperçu 3D interactif");
  const button = screen.getByRole("button", {
    name: "Afficher l’aperçu en plein écran",
  });
  button.focus();
  fireEvent.click(button);
  expect(
    await screen.findByRole("button", { name: "Quitter le plein écran" }),
  ).toHaveAttribute("aria-pressed", "true");
  expect(fullscreenElement).toBe(canvas.closest(".model-viewer"));
  fireEvent.click(
    screen.getByRole("button", { name: "Quitter le plein écran" }),
  );
  await waitFor(() => expect(exitFullscreen).toHaveBeenCalledTimes(1));
  expect(screen.getByLabelText("Aperçu 3D interactif")).toBe(canvas);
  expect(loadPreviewModel).toHaveBeenCalledTimes(1);
  expect(disposePreviewRenderer).not.toHaveBeenCalled();
  expect(
    screen.getByRole("button", { name: "Afficher l’aperçu en plein écran" }),
  ).toHaveFocus();
  rendered.unmount();
});

it("suit la sortie native du plein écran et le quitte au démontage", async () => {
  readyModel();
  const rendered = render(<ModelViewer url="model.glb" label="TRAVAIL" />);
  await screen.findByText(/Glisser pour tourner/);
  fireEvent.click(
    screen.getByRole("button", { name: "Afficher l’aperçu en plein écran" }),
  );
  await screen.findByRole("button", { name: "Quitter le plein écran" });
  act(() => {
    fullscreenElement = null;
    document.dispatchEvent(new Event("fullscreenchange"));
  });
  expect(
    screen.getByRole("button", { name: "Afficher l’aperçu en plein écran" }),
  ).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(
    screen.getByRole("button", { name: "Afficher l’aperçu en plein écran" }),
  );
  await screen.findByRole("button", { name: "Quitter le plein écran" });
  rendered.unmount();
  expect(exitFullscreen).toHaveBeenCalledTimes(1);
  expect(disposePreviewRenderer).toHaveBeenCalledTimes(1);
});

it("quitte le plein écran avec Échap sans recréer la visionneuse", async () => {
  readyModel();
  render(<ModelViewer url="model.glb" label="TRAVAIL" />);
  await screen.findByText(/Glisser pour tourner/);
  const canvas = screen.getByLabelText("Aperçu 3D interactif");
  fireEvent.keyDown(document, { key: "Escape" });
  expect(exitFullscreen).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "Afficher l’aperçu en plein écran" }),
  );
  await screen.findByRole("button", { name: "Quitter le plein écran" });
  fireEvent.keyDown(document, { key: "Escape" });
  await waitFor(() => expect(exitFullscreen).toHaveBeenCalledTimes(1));
  expect(fullscreenElement).toBeNull();
  expect(screen.getByLabelText("Aperçu 3D interactif")).toBe(canvas);
  expect(loadPreviewModel).toHaveBeenCalledTimes(1);
  expect(disposePreviewRenderer).not.toHaveBeenCalled();
});

it("signale un refus de plein écran sans interrompre l’aperçu", async () => {
  readyModel();
  requestFullscreen.mockRejectedValueOnce(new Error("Accès refusé"));
  render(<ModelViewer url="model.glb" label="TRAVAIL" />);
  await screen.findByText(/Glisser pour tourner/);
  fireEvent.click(
    screen.getByRole("button", { name: "Afficher l’aperçu en plein écran" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent("Accès refusé");
  expect(screen.getByText(/Glisser pour tourner/)).toBeInTheDocument();
  expect(loadPreviewModel).toHaveBeenCalledTimes(1);
  expect(disposePreviewRenderer).not.toHaveBeenCalled();
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
