import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { api } from "../api";
import { defaultExportConfig } from "../contracts";
import type {
  AssetDetail,
  ChangeSet,
  JobRequest,
  PreviewResult,
  VariantDetail,
  WorkspaceStatus,
} from "../contracts";
import type { RunJob } from "../lib/useJobs";
import { AssetDetailView } from "./AssetDetailView";

vi.mock("./ModelViewer", () => ({
  default: ({ url }: { url: string }) => (
    <div aria-label="Aperçu 3D interactif" data-url={url} />
  ),
}));
vi.mock("../api", () => ({
  api: {
    getAsset: vi.fn(),
    getVariant: vi.fn(),
    getSnapshot: vi.fn(),
    listSnapshots: vi.fn(),
    getFileSelections: vi.fn(),
    setFileSelection: vi.fn(),
    onWorkspaceChanged: vi.fn(),
    onWorkspaceStatus: vi.fn(),
    getWorkspaceStatus: vi.fn(),
    requestWorkspaceSync: vi.fn(),
    chooseExportPath: vi.fn(),
    openWorkspace: vi.fn(),
    openBlender: vi.fn(),
    copyText: vi.fn(),
    releasePreviewResources: vi.fn(),
  },
}));

const variant: VariantDetail = {
  variant: {
    id: "variant",
    assetId: "asset",
    name: "Principale",
    workspacePath: "C:\\Vault\\workspace",
    headSnapshotId: "head",
    workingBytes: 10,
    triangles: 12,
    config: { ...defaultExportConfig, primaryFile: "model.blend" },
  },
  files: [{ path: "model.blend", kind: "file", hash: "new", size: 10 }],
  snapshots: [],
};
const detail: AssetDetail = {
  asset: {
    id: "asset",
    name: "Kitsune",
    categoryId: null,
    tags: [],
    gameIds: [],
    archived: false,
    createdAt: "2026-11-01",
    updatedAt: "2026-11-01",
    workingBytes: 10,
    historyBytes: 10,
    triangles: 12,
    variantCount: 1,
  },
  variants: [variant.variant],
};
const preview: PreviewResult = {
  url: "http://edge-preview.localhost/first",
  sourceFile: "model.glb",
  sourceKind: "generatedGlb",
  sourceLabel: "Travail enregistré sur disque",
  inspection: {
    geometry: {
      meshObjects: 1,
      vertices: 8,
      edges: 12,
      faces: 6,
      triangles: 12,
    },
    scenes: [],
    collections: [],
    dependencies: [],
    warnings: [],
    blenderVersion: "GLB 2.0",
  },
};
const changed: ChangeSet = {
  added: [],
  modified: ["model.blend"],
  deleted: [],
  configChanged: false,
  workingBytes: 10,
};
const ready: WorkspaceStatus = {
  variantId: "variant",
  revision: 1,
  state: "ready",
  sourceChanged: true,
  canCreateSnapshot: true,
  artifactKey: "first",
  error: null,
};
let emitStatus: (status: WorkspaceStatus) => void;
const run = vi.fn<(request: JobRequest) => Promise<unknown>>();
const onChanged = vi.fn();
const onError = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  emitStatus = () => {};
  vi.mocked(api.getAsset).mockResolvedValue(detail);
  vi.mocked(api.getVariant).mockResolvedValue(variant);
  vi.mocked(api.getFileSelections).mockResolvedValue({
    previewFile: null,
    exportFile: null,
  });
  vi.mocked(api.setFileSelection).mockResolvedValue(undefined);
  vi.mocked(api.listSnapshots).mockResolvedValue({ items: [], total: 0 });
  vi.mocked(api.onWorkspaceChanged).mockResolvedValue(() => {});
  vi.mocked(api.onWorkspaceStatus).mockImplementation(async (callback) => {
    emitStatus = callback;
    return () => {};
  });
  vi.mocked(api.getWorkspaceStatus).mockResolvedValue(ready);
  vi.mocked(api.requestWorkspaceSync).mockResolvedValue(ready);
  vi.mocked(api.releasePreviewResources).mockResolvedValue(undefined);
  run.mockImplementation(async (request) =>
    request.kind === "changes" ? changed : preview,
  );
});

function renderDetail(generation = 0) {
  return render(
    <AssetDetailView
      id="asset"
      taxonomy={{ categories: [], games: [], tags: [] }}
      generation={generation}
      onBack={vi.fn()}
      onChanged={onChanged}
      onError={onError}
      runJob={run as RunJob}
    />,
  );
}

it("propose les sources et garde les modificateurs évalués automatiquement", async () => {
  renderDetail();
  await screen.findByLabelText("Aperçu 3D interactif");
  expect(screen.queryByText("Source de l’asset")).not.toBeInTheDocument();
  expect(
    screen.getByRole("combobox", { name: "Fichier principal" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("checkbox", { name: /modificateurs/ }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Changer le GLB" }),
  ).not.toBeInTheDocument();
});

it("ne permet aucune version pour un changement GLB seul", async () => {
  vi.mocked(api.getWorkspaceStatus).mockResolvedValue({
    ...ready,
    sourceChanged: false,
    canCreateSnapshot: false,
  });
  run.mockImplementation(async (request) =>
    request.kind === "changes"
      ? { ...changed, modified: ["model.glb"] }
      : preview,
  );
  renderDetail();
  await screen.findByLabelText("Aperçu 3D interactif");
  expect(
    screen.getByRole("button", { name: "Créer une version" }),
  ).toBeDisabled();
});

it("désactive la version dès que la comparaison avec la nouvelle version est vide", async () => {
  run.mockImplementation(async (request) =>
    request.kind === "changes" ? { ...changed, modified: [] } : preview,
  );
  renderDetail();
  await screen.findByLabelText("Aperçu 3D interactif");
  expect(
    screen.getByRole("button", { name: "Créer une version" }),
  ).toBeDisabled();
});

it("actualise automatiquement l’aperçu et les métriques après synchronisation", async () => {
  let currentPreview = preview;
  run.mockImplementation(async (request) =>
    request.kind === "changes" ? changed : currentPreview,
  );
  renderDetail();
  await screen.findByLabelText("Aperçu 3D interactif");
  await act(async () =>
    emitStatus({
      ...ready,
      revision: 2,
      state: "syncing",
      canCreateSnapshot: false,
    }),
  );
  expect(screen.getByLabelText("Aperçu 3D interactif")).toHaveAttribute(
    "data-url",
    preview.url,
  );
  expect(
    screen.getByRole("button", { name: "Créer une version" }),
  ).toBeDisabled();
  currentPreview = {
    ...preview,
    url: "http://edge-preview.localhost/second",
    inspection: {
      ...preview.inspection,
      geometry: { ...preview.inspection.geometry, triangles: 24 },
    },
  };
  await act(async () =>
    emitStatus({ ...ready, revision: 3, artifactKey: "second" }),
  );
  await waitFor(() =>
    expect(screen.getByLabelText("Aperçu 3D interactif")).toHaveAttribute(
      "data-url",
      currentPreview.url,
    ),
  );
  expect(screen.getByText("Triangles").parentElement).toHaveTextContent("24");
  expect(
    run.mock.calls.filter(([request]) => request.kind === "preview"),
  ).toHaveLength(2);
});

it("conserve l’aperçu et bloque la version après un échec de synchronisation", async () => {
  renderDetail();
  await screen.findByLabelText("Aperçu 3D interactif");
  await act(async () =>
    emitStatus({
      ...ready,
      revision: 2,
      state: "failed",
      canCreateSnapshot: false,
      error: { code: "operation_failed", message: "Export interrompu" },
    }),
  );
  expect(screen.getByLabelText("Aperçu 3D interactif")).toHaveAttribute(
    "data-url",
    preview.url,
  );
  expect(screen.getByRole("alert")).toHaveTextContent("Export interrompu");
  expect(
    screen.getByRole("button", { name: "Créer une version" }),
  ).toBeDisabled();
  await userEvent.click(
    screen.getByRole("button", { name: "Réessayer la vérification" }),
  );
  expect(api.requestWorkspaceSync).toHaveBeenCalledWith("variant");
});

it("ignore une lecture initiale antérieure à un événement reçu", async () => {
  let finish!: (status: WorkspaceStatus) => void;
  vi.mocked(api.getWorkspaceStatus).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  renderDetail();
  await waitFor(() =>
    expect(api.getWorkspaceStatus).toHaveBeenCalledWith("variant"),
  );
  await act(async () =>
    emitStatus({
      ...ready,
      revision: 2,
      state: "syncing",
      canCreateSnapshot: false,
      artifactKey: null,
    }),
  );
  await act(async () => finish(ready));
  expect(
    screen.getByRole("button", { name: "Créer une version" }),
  ).toBeDisabled();
  expect(
    run.mock.calls.filter(([request]) => request.kind === "preview"),
  ).toHaveLength(0);
});

it("ne recharge pas l’aperçu lors d’un rafraîchissement du catalogue", async () => {
  const rendered = renderDetail();
  await screen.findByLabelText("Aperçu 3D interactif");
  const reads = vi.mocked(api.getVariant).mock.calls.length;
  rendered.rerender(
    <AssetDetailView
      id="asset"
      taxonomy={{ categories: [], games: [], tags: [] }}
      generation={1}
      onBack={vi.fn()}
      onChanged={onChanged}
      onError={onError}
      runJob={run as RunJob}
    />,
  );
  await waitFor(() => expect(api.getVariant).toHaveBeenCalledTimes(reads + 1));
  expect(
    run.mock.calls.filter(([request]) => request.kind === "preview"),
  ).toHaveLength(1);
});

it("garde le modèle chargé lorsque la réconciliation retrouve le même artefact", async () => {
  let count = 0;
  run.mockImplementation(async (request) =>
    request.kind === "changes"
      ? changed
      : {
          ...preview,
          artifactKey: "stable",
          url: `http://edge-preview.localhost/token-${++count}`,
        },
  );
  renderDetail();
  await screen.findByLabelText("Aperçu 3D interactif");
  await act(async () => emitStatus({ ...ready, revision: 2 }));
  await waitFor(() =>
    expect(
      run.mock.calls.filter(([request]) => request.kind === "preview"),
    ).toHaveLength(2),
  );
  expect(screen.getByLabelText("Aperçu 3D interactif")).toHaveAttribute(
    "data-url",
    "http://edge-preview.localhost/token-1",
  );
  expect(api.releasePreviewResources).toHaveBeenCalledWith(["token-2"]);
});

it("actualise une source secondaire même si l’artefact principal ne change pas", async () => {
  vi.mocked(api.getVariant).mockResolvedValue({
    ...variant,
    files: [
      ...variant.files,
      { path: "other.glb", kind: "file", hash: "other", size: 20 },
    ],
  });
  renderDetail();
  await screen.findByLabelText("Aperçu 3D interactif");
  await userEvent.selectOptions(
    screen.getByLabelText("Source de l’aperçu"),
    "other.glb",
  );
  await waitFor(() =>
    expect(
      run.mock.calls.filter(([request]) => request.kind === "preview"),
    ).toHaveLength(2),
  );
  await act(async () => emitStatus({ ...ready, revision: 2 }));
  await waitFor(() =>
    expect(
      run.mock.calls.filter(([request]) => request.kind === "preview"),
    ).toHaveLength(3),
  );
  expect(
    run.mock.calls
      .filter(([request]) => request.kind === "preview")
      .at(-1)?.[0],
  ).toEqual(expect.objectContaining({ previewFile: "other.glb" }));
});

it("libère un ancien aperçu terminé après une nouvelle sauvegarde", async () => {
  let finish!: (value: PreviewResult) => void;
  let first = true;
  run.mockImplementation(async (request) => {
    if (request.kind === "changes") return changed;
    if (first) {
      first = false;
      return new Promise((resolve) => {
        finish = resolve;
      });
    }
    return { ...preview, url: "http://edge-preview.localhost/newest" };
  });
  renderDetail();
  await waitFor(() =>
    expect(run.mock.calls.some(([request]) => request.kind === "preview")).toBe(
      true,
    ),
  );
  await act(async () =>
    emitStatus({ ...ready, revision: 2, artifactKey: "newest" }),
  );
  await screen.findByLabelText("Aperçu 3D interactif");
  await act(async () => finish(preview));
  expect(screen.getByLabelText("Aperçu 3D interactif")).toHaveAttribute(
    "data-url",
    "http://edge-preview.localhost/newest",
  );
  expect(api.releasePreviewResources).toHaveBeenCalledWith(["first"]);
});

it("libère l’aperçu affiché et un résultat arrivé après fermeture", async () => {
  let finish!: (value: PreviewResult) => void;
  let previews = 0;
  run.mockImplementation(async (request) => {
    if (request.kind === "changes") return changed;
    if (++previews === 1) return preview;
    return new Promise((resolve) => {
      finish = resolve;
    });
  });
  const rendered = renderDetail();
  await screen.findByLabelText("Aperçu 3D interactif");
  await userEvent.click(
    screen.getAllByRole("button", { name: "Actualiser" })[0]!,
  );
  rendered.unmount();
  await act(async () =>
    finish({ ...preview, url: "http://edge-preview.localhost/late" }),
  );
  expect(api.releasePreviewResources).toHaveBeenCalledWith(["first"]);
  expect(api.releasePreviewResources).toHaveBeenCalledWith(["late"]);
});

it("conserve l’aperçu historique lors d’une sauvegarde du travail courant", async () => {
  const snapshot = {
    id: "head",
    variantId: "variant",
    parentId: null,
    createdAt: "2026-11-01",
    message: "Version initiale",
    logicalBytes: 10,
    config: variant.variant.config,
    entries: variant.files,
  };
  vi.mocked(api.listSnapshots).mockResolvedValue({
    items: [snapshot],
    total: 1,
  });
  vi.mocked(api.getSnapshot).mockResolvedValue(snapshot);
  run.mockImplementation(async (request) =>
    request.kind === "changes"
      ? changed
      : {
          ...preview,
          url: `http://edge-preview.localhost/${request.kind === "preview" && request.snapshotId ? "history" : "current"}`,
        },
  );
  renderDetail();
  await screen.findByLabelText("Aperçu 3D interactif");
  await userEvent.click(screen.getByRole("tab", { name: /Historique/ }));
  await userEvent.click(
    await screen.findByRole("button", { name: "Consulter" }),
  );
  await waitFor(() =>
    expect(screen.getByLabelText("Aperçu 3D interactif")).toHaveAttribute(
      "data-url",
      "http://edge-preview.localhost/history",
    ),
  );
  const count = run.mock.calls.filter(
    ([request]) => request.kind === "preview",
  ).length;
  await act(async () =>
    emitStatus({ ...ready, revision: 2, artifactKey: "newest" }),
  );
  expect(
    run.mock.calls.filter(([request]) => request.kind === "preview"),
  ).toHaveLength(count);
  expect(screen.getByLabelText("Aperçu 3D interactif")).toHaveAttribute(
    "data-url",
    "http://edge-preview.localhost/history",
  );
});

it("bloque aussi la validation d’une version déjà ouverte pendant la synchronisation", async () => {
  renderDetail();
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Créer une version" }),
    ).toBeEnabled(),
  );
  await userEvent.click(
    screen.getByRole("button", { name: "Créer une version" }),
  );
  await userEvent.type(
    screen.getByRole("textbox", { name: "Qu’avez-vous changé ?" }),
    "Miroir",
  );
  expect(
    screen.getByRole("button", { name: "Créer la version" }),
  ).toBeEnabled();
  await act(async () =>
    emitStatus({
      ...ready,
      revision: 2,
      state: "syncing",
      canCreateSnapshot: false,
    }),
  );
  expect(
    screen.getByRole("button", { name: "Créer la version" }),
  ).toBeDisabled();
});

it("préserve les projets non conformes et bloque les nouvelles versions", async () => {
  vi.mocked(api.getWorkspaceStatus).mockResolvedValue({
    ...ready,
    state: "nonconforming",
    canCreateSnapshot: false,
    artifactKey: null,
    error: { code: "invalid_project", message: "Plusieurs fichiers .blend." },
  });
  renderDetail();
  await screen.findByText("Projet non conforme");
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Plusieurs fichiers .blend.",
  );
  expect(
    screen.getByRole("button", { name: "Créer une version" }),
  ).toBeDisabled();
  expect(run.mock.calls.some(([request]) => request.kind === "preview")).toBe(
    false,
  );
  expect(api.requestWorkspaceSync).not.toHaveBeenCalled();
});
