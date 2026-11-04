import { StrictMode } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import { defaultExportConfig } from "../contracts";
import type {
  AssetDetail,
  ChangeSet,
  Inspection,
  JobRequest,
  PreviewResult,
  VariantDetail,
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
    onWorkspaceChanged: vi.fn(),
    onWorkspaceStatus: vi.fn(),
    getWorkspaceStatus: vi.fn(),
    requestWorkspaceSync: vi.fn(),
    chooseExportPath: vi.fn(),
    updateConfig: vi.fn(),
    openWorkspace: vi.fn(),
    openBlender: vi.fn(),
    copyText: vi.fn(),
    releasePreviewResources: vi.fn(),
  },
}));

const detail: AssetDetail = {
  asset: {
    id: "asset-1",
    name: "Drone",
    categoryId: null,
    tags: [],
    gameIds: [],
    archived: false,
    createdAt: "2026-11-01",
    updatedAt: "2026-11-01",
    workingBytes: 15,
    historyBytes: 10,
    triangles: 999,
    variantCount: 1,
  },
  variants: [
    {
      id: "variant-1",
      assetId: "asset-1",
      name: "Principale",
      workspacePath: "C:\\Vault\\workspaces\\one",
      headSnapshotId: "snapshot-1",
      workingBytes: 15,
      triangles: 12,
      config: { ...defaultExportConfig, primaryFile: "drone.blend" },
    },
  ],
};
const variant: VariantDetail = {
  variant: detail.variants[0]!,
  files: [{ path: "drone.blend", kind: "file", hash: "new", size: 15 }],
  snapshots: [
    {
      id: "snapshot-1",
      variantId: "variant-1",
      parentId: null,
      createdAt: "2026-11-01",
      message: "Première version",
      logicalBytes: 10,
      config: { ...defaultExportConfig, primaryFile: "drone.blend" },
      entries: [{ path: "drone.blend", kind: "file", hash: "old", size: 10 }],
    },
  ],
};
const changes: ChangeSet = {
  added: [],
  modified: ["drone.blend"],
  deleted: [],
  configChanged: false,
  workingBytes: 15,
};
const sourceInspection: Inspection = {
  geometry: { meshObjects: 1, vertices: 8, edges: 12, faces: 6, triangles: 12 },
  scenes: ["Scene"],
  collections: ["Asset"],
  dependencies: [],
  warnings: [],
  blenderVersion: "5.1.1",
};
const generatedPreview: PreviewResult = {
  url: "http://edge-preview.localhost/generated",
  inspection: sourceInspection,
  sourceLabel: "Aperçu généré depuis drone.blend",
  sourceFile: "drone.blend",
  sourceKind: "generatedGlb",
};
const onError = vi.fn();
const run = vi.fn<(request: JobRequest) => Promise<unknown>>();

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getAsset).mockResolvedValue(detail);
  vi.mocked(api.getVariant).mockResolvedValue(variant);
  vi.mocked(api.getSnapshot).mockResolvedValue(variant.snapshots[0]!);
  vi.mocked(api.listSnapshots).mockResolvedValue({
    items: variant.snapshots.map((snapshot) => ({ ...snapshot, entries: [] })),
    total: 1,
  });
  vi.mocked(api.onWorkspaceChanged).mockResolvedValue(() => {});
  vi.mocked(api.onWorkspaceStatus).mockResolvedValue(() => {});
  vi.mocked(api.getWorkspaceStatus).mockImplementation(async (variantId) => ({
    variantId,
    revision: 1,
    state: "ready",
    sourceChanged: true,
    canCreateSnapshot: true,
    artifactKey: "first",
    error: null,
  }));
  vi.mocked(api.chooseExportPath).mockResolvedValue("C:\\Export\\drone.glb");
  vi.mocked(api.openBlender).mockResolvedValue(undefined);
  vi.mocked(api.updateConfig).mockResolvedValue(undefined);
  vi.mocked(api.releasePreviewResources).mockResolvedValue(undefined);
  run.mockImplementation(async (request: JobRequest) =>
    request.kind === "changes" ? changes : generatedPreview,
  );
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(
    function (this: HTMLDialogElement) {
      this.setAttribute("open", "");
    },
  );
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (
    this: HTMLDialogElement,
  ) {
    this.removeAttribute("open");
  });
});

function renderDetail() {
  return render(
    <AssetDetailView
      id="asset-1"
      taxonomy={{ categories: [], games: [], tags: [] }}
      generation={0}
      onBack={vi.fn()}
      onChanged={vi.fn()}
      onError={onError}
      runJob={run as RunJob}
    />,
  );
}

describe("versions, aperçu et dépendances", () => {
  it("choisit indépendamment l’aperçu, le principal et la source d’export", async () => {
    vi.mocked(api.getVariant).mockResolvedValue({
      ...variant,
      files: [
        ...variant.files,
        { path: "alternate.blend", kind: "file", hash: "blend2", size: 20 },
        { path: "other.glb", kind: "file", hash: "glb2", size: 30 },
      ],
    });
    run.mockImplementation(async (request) =>
      request.kind === "changes"
        ? changes
        : {
            ...generatedPreview,
            sourceFile:
              request.kind === "preview" && request.previewFile
                ? request.previewFile
                : "drone.blend",
          },
    );
    renderDetail();
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.selectOptions(
      screen.getByLabelText("Source de l’aperçu"),
      "alternate.blend",
    );
    await waitFor(() =>
      expect(run).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: "preview",
          previewFile: "alternate.blend",
        }),
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      ),
    );
    expect(screen.getByLabelText("Source de l’aperçu")).toHaveValue(
      "alternate.blend",
    );
    expect(api.updateConfig).not.toHaveBeenCalled();
    await userEvent.selectOptions(
      screen.getByLabelText("Source de l’export"),
      "other.glb",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Exporter .glb" }),
    );
    await waitFor(() =>
      expect(run).toHaveBeenCalledWith(
        expect.objectContaining({ kind: "export", sourceFile: "other.glb" }),
      ),
    );
    await userEvent.selectOptions(
      screen.getByLabelText("Fichier principal"),
      "alternate.blend",
    );
    await waitFor(() =>
      expect(api.updateConfig).toHaveBeenCalledWith(
        "variant-1",
        expect.objectContaining({
          primaryFile: "alternate.blend",
          generatedGlbFile: null,
        }),
      ),
    );
  });

  it("permet de relancer la détection automatique du principal", async () => {
    renderDetail();
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.click(
      screen.getByRole("button", { name: "Re-détecter automatiquement" }),
    );
    await waitFor(() =>
      expect(api.updateConfig).toHaveBeenCalledWith(
        "variant-1",
        expect.objectContaining({ primaryFile: null, generatedGlbFile: null }),
      ),
    );
  });
  it("conserve les filtres au rafraîchissement et les réinitialise pour une autre version", async () => {
    const props = {
      id: "asset-1",
      taxonomy: { categories: [], games: [], tags: [] },
      generation: 0,
      onBack: vi.fn(),
      onChanged: vi.fn(),
      onError,
      runJob: run as RunJob,
    };
    const { rerender } = render(<AssetDetailView {...props} />);
    await screen.findByText("Modifications non versionnées");
    await userEvent.type(screen.getByLabelText("Taille minimum"), "12");
    const readsBeforeRefresh = vi.mocked(api.getVariant).mock.calls.length;
    rerender(<AssetDetailView {...props} generation={1} />);
    await waitFor(() =>
      expect(vi.mocked(api.getVariant).mock.calls.length).toBeGreaterThan(
        readsBeforeRefresh,
      ),
    );
    expect(screen.getByLabelText("Taille minimum")).toHaveValue(12);
    await userEvent.click(screen.getByRole("tab", { name: /Historique/ }));
    await userEvent.click(screen.getByRole("tab", { name: /Fichiers/ }));
    expect(screen.getByLabelText("Taille minimum")).toHaveValue(12);
    await userEvent.click(screen.getByRole("tab", { name: /Historique/ }));
    await userEvent.click(
      await screen.findByRole("button", { name: /Consulter/ }),
    );
    await userEvent.click(screen.getByRole("tab", { name: /Fichiers/ }));
    expect(screen.getByLabelText("Taille minimum")).toHaveValue(null);
    expect(
      await within(screen.getByRole("tabpanel")).findByText("10 o"),
    ).toBeInTheDocument();
  });
  it("affiche la géométrie de la variante et les tailles de la version sélectionnée", async () => {
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    expect(screen.getByText("Triangles").parentElement).toHaveTextContent("12");
    expect(screen.getByText("Triangles").parentElement).not.toHaveTextContent(
      "999",
    );
    await userEvent.click(screen.getByRole("tab", { name: /Historique/ }));
    await userEvent.click(
      await screen.findByRole("button", { name: /Consulter/ }),
    );
    expect(await screen.findByText("Cette version")).toBeInTheDocument();
    expect(screen.getByText("Cette version").parentElement).toHaveTextContent(
      "10 o",
    );
    await screen.findByLabelText("Aperçu 3D interactif");
    expect(screen.getByText("Triangles").parentElement).toHaveTextContent("12");
  });

  it("signale une modification externe sans créer de version automatiquement", async () => {
    renderDetail();
    expect(
      await screen.findByText("Modifications non versionnées"),
    ).toBeInTheDocument();
    expect(
      run.mock.calls.every(
        ([request]: [JobRequest]) => request.kind !== "snapshot",
      ),
    ).toBe(true);
    await userEvent.click(screen.getByRole("tab", { name: /Modifications/ }));
    expect(await screen.findByText("Modifié")).toBeInTheDocument();
  });

  it("exporte la version historique sélectionnée et protège sa configuration", async () => {
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(screen.getByRole("tab", { name: /Historique/ }));
    await userEvent.click(
      await screen.findByRole("button", { name: /Consulter/ }),
    );
    await waitFor(() =>
      expect(api.getSnapshot).toHaveBeenCalledWith("snapshot-1"),
    );
    await userEvent.click(screen.getByRole("tab", { name: /Fichiers/ }));
    expect(
      await within(screen.getByRole("tabpanel")).findByText("10 o"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("combobox", { name: "Fichier principal" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Ouvrir dans Blender" }),
    ).toBeDisabled();
    await userEvent.click(
      screen.getByRole("button", { name: "Exporter .glb" }),
    );
    await waitFor(() =>
      expect(run).toHaveBeenCalledWith({
        kind: "export",
        variantId: "variant-1",
        snapshotId: "snapshot-1",
        destination: "C:\\Export\\drone.glb",
        overwrite: false,
      }),
    );
  });

  it("ne remplace un export existant qu’après confirmation", async () => {
    run.mockImplementation(async (request: JobRequest) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "export" && !request.overwrite)
        throw { code: "destination_exists", message: "Fichier existant" };
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(
      screen.getByRole("button", { name: "Exporter .glb" }),
    );
    expect(
      await screen.findByRole("dialog", {
        name: "Remplacer le fichier existant ?",
      }),
    ).toBeInTheDocument();
    expect(
      run.mock.calls.filter(
        ([request]: [JobRequest]) =>
          request.kind === "export" && request.overwrite,
      ),
    ).toHaveLength(0);
    await userEvent.click(
      screen.getByRole("button", { name: "Remplacer et exporter" }),
    );
    await waitFor(() =>
      expect(run).toHaveBeenCalledWith(
        expect.objectContaining({ kind: "export", overwrite: true }),
      ),
    );
  });

  it("crée une variante depuis une version sans modifier le dossier courant", async () => {
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(
      screen.getByRole("button", { name: "Nouvelle variante" }),
    );
    await userEvent.type(
      screen.getByRole("textbox", { name: "Nom de la variante" }),
      "Désert",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Créer la variante" }),
    );
    await waitFor(() =>
      expect(run).toHaveBeenCalledWith({
        kind: "variant",
        snapshotId: "snapshot-1",
        name: "Désert",
      }),
    );
    expect(api.updateConfig).not.toHaveBeenCalled();
  });

  it("ne double pas la génération dans StrictMode", async () => {
    render(
      <StrictMode>
        <AssetDetailView
          id="asset-1"
          taxonomy={{ categories: [], games: [], tags: [] }}
          generation={0}
          onBack={vi.fn()}
          onChanged={vi.fn()}
          onError={onError}
          runJob={run as RunJob}
        />
      </StrictMode>,
    );
    await screen.findByLabelText("Aperçu 3D interactif");
    expect(
      run.mock.calls.filter(([request]) => request.kind === "preview"),
    ).toHaveLength(1);
    expect(api.releasePreviewResources).not.toHaveBeenCalled();
  });

  it("affiche les arêtes et ignore les dépendances secondaires dans les alertes", async () => {
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "preview")
        return {
          ...generatedPreview,
          inspection: {
            ...sourceInspection,
            dependencies: [
              {
                path: "C:\\Windows\\Fonts\\msyhbd.ttc",
                status: "external",
                kind: "font",
                renderRelevant: false,
              },
              {
                path: "essentials_brushes-gp_draw.blend",
                status: "missing",
                kind: "brush",
                renderRelevant: false,
              },
            ],
          },
        };
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    expect(
      await screen.findByLabelText("Aperçu 3D interactif"),
    ).toBeInTheDocument();
    expect(screen.getByText("12 arêtes")).toBeInTheDocument();
    expect(
      screen.queryByRole("region", { name: "Dépendances" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/Aperçu dégradé/)).not.toBeInTheDocument();
    expect(onError).not.toHaveBeenCalled();
  });

  it("ouvre le modèle et signale une texture essentielle absente dans Dépendances", async () => {
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "preview")
        return {
          ...generatedPreview,
          inspection: {
            ...sourceInspection,
            dependencies: [
              {
                path: "textures/albedo.png",
                status: "missing",
                kind: "image",
                renderRelevant: true,
              },
            ],
          },
        };
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    expect(
      await screen.findByLabelText("Aperçu 3D interactif"),
    ).toBeInTheDocument();
    expect(screen.getByText(/Aperçu dégradé/)).toBeInTheDocument();
    expect(
      within(screen.getByRole("region", { name: "Dépendances" })).getByText(
        "textures/albedo.png",
      ),
    ).toBeInTheDocument();
  });

  it("conserve une erreur d’aperçu locale et l’accès au fichier Blender", async () => {
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "preview")
        throw { code: "external", message: "Blender n’a pas produit de GLB." };
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Blender n’a pas produit de GLB.",
    );
    const open = screen.getByRole("button", { name: "Ouvrir dans Blender" });
    expect(open).toBeEnabled();
    await userEvent.click(open);
    expect(api.openBlender).toHaveBeenCalledWith("variant-1");
    expect(onError).not.toHaveBeenCalled();
    expect(
      run.mock.calls.filter(([request]) => request.kind === "preview"),
    ).toHaveLength(1);
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    expect(
      run.mock.calls.filter(([request]) => request.kind === "preview"),
    ).toHaveLength(2);
  });

  it("affiche un compte d’arêtes indisponible sans bloquer un ancien résultat", async () => {
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "preview")
        return {
          ...generatedPreview,
          inspection: {
            ...sourceInspection,
            geometry: { ...sourceInspection.geometry, edges: null },
          },
        };
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    expect(await screen.findByText("— arêtes")).toBeInTheDocument();
    expect(screen.getByLabelText("Aperçu 3D interactif")).toBeInTheDocument();
  });

  it("ignore un aperçu terminé après le changement de version", async () => {
    let finishPreview: (value: PreviewResult) => void = () => {};
    let finishSnapshot: (value: PreviewResult) => void = () => {};
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "preview")
        return new Promise<PreviewResult>((resolve) => {
          if (request.snapshotId) finishSnapshot = resolve;
          else finishPreview = resolve;
        });
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(screen.getByRole("tab", { name: /Historique/ }));
    await userEvent.click(
      await screen.findByRole("button", { name: /Consulter/ }),
    );
    await screen.findByText("Cette version");
    await waitFor(() =>
      expect(run).toHaveBeenCalledWith(
        {
          kind: "preview",
          variantId: "variant-1",
          snapshotId: "snapshot-1",
        },
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      ),
    );
    await act(async () =>
      finishSnapshot({
        ...generatedPreview,
        url: "http://edge-preview.localhost/snapshot",
      }),
    );
    await screen.findByLabelText("Aperçu 3D interactif");
    await act(async () => finishPreview(generatedPreview));
    expect(screen.getByLabelText("Aperçu 3D interactif")).toHaveAttribute(
      "data-url",
      "http://edge-preview.localhost/snapshot",
    );
    expect(api.releasePreviewResources).toHaveBeenCalledWith(["generated"]);
  });
});
