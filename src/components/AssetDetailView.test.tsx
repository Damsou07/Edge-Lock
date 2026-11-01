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
    chooseExportPath: vi.fn(),
    updateConfig: vi.fn(),
    openWorkspace: vi.fn(),
    openBlender: vi.fn(),
    copyText: vi.fn(),
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
  vi.mocked(api.chooseExportPath).mockResolvedValue("C:\\Export\\drone.glb");
  vi.mocked(api.openBlender).mockResolvedValue(undefined);
  run.mockImplementation(async (request: JobRequest) =>
    request.kind === "changes" ? changes : null,
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

describe("versions et travail en cours", () => {
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
    expect(screen.getByText("Triangles").parentElement).toHaveTextContent("—");
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

  it("préserve une configuration en cours de saisie lors du rafraîchissement des métriques", async () => {
    const props = {
      id: "asset-1",
      taxonomy: { categories: [], games: [], tags: [] },
      generation: 0,
      onBack: vi.fn(),
      onChanged: vi.fn(),
      onError,
      runJob: run as RunJob,
    };
    const rendered = render(<AssetDetailView {...props} />);
    await screen.findByText("Modifications non versionnées");
    await userEvent.type(
      screen.getByRole("textbox", { name: "Scène" }),
      "Export",
    );
    rendered.rerender(<AssetDetailView {...props} generation={1} />);
    await waitFor(() => expect(api.getVariant).toHaveBeenCalledTimes(2));
    expect(
      screen.getByRole("button", { name: "Exporter .glb" }),
    ).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Scène" })).toHaveValue(
      "Export",
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
});

describe("aperçu indépendant et dépendances", () => {
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
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
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
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
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

  it("choisit un GLB ambigu sans modifier la source puis réutilise ce choix", async () => {
    vi.mocked(api.getVariant).mockResolvedValue({
      ...variant,
      files: [
        ...variant.files,
        { path: "drone.glb", kind: "file", hash: "glb-1", size: 10 },
        { path: "other.glb", kind: "file", hash: "glb-2", size: 10 },
      ],
    });
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "preview") {
        if (!request.previewFile)
          throw {
            code: "preview_ambiguous",
            message: "Plusieurs GLB sont disponibles.",
            details: { candidates: ["drone.glb", "other.glb"] },
          };
        return {
          ...generatedPreview,
          sourceKind: "assetGlb",
          sourceFile: request.previewFile,
        };
      }
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    await userEvent.selectOptions(
      await screen.findByRole("combobox", { name: "GLB de prévisualisation" }),
      "other.glb",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Utiliser ce GLB" }),
    );
    expect(
      await screen.findByLabelText("Aperçu 3D interactif"),
    ).toBeInTheDocument();
    expect(api.updateConfig).not.toHaveBeenCalled();
    expect(
      screen.getByRole("combobox", { name: "Fichier principal" }),
    ).toHaveValue("drone.blend");
    await userEvent.click(
      screen.getAllByRole("button", { name: "Actualiser" })[0]!,
    );
    await waitFor(() =>
      expect(run).toHaveBeenLastCalledWith({
        kind: "preview",
        variantId: "variant-1",
        snapshotId: null,
        previewFile: "other.glb",
      }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Changer le GLB" }),
    );
    await userEvent.selectOptions(
      await screen.findByRole("combobox", { name: "GLB de prévisualisation" }),
      "drone.glb",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Utiliser ce GLB" }),
    );
    await waitFor(() =>
      expect(run).toHaveBeenLastCalledWith({
        kind: "preview",
        variantId: "variant-1",
        snapshotId: null,
        previewFile: "drone.glb",
      }),
    );
    expect(api.updateConfig).not.toHaveBeenCalled();
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
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Blender n’a pas produit de GLB.",
    );
    const open = screen.getByRole("button", { name: "Ouvrir dans Blender" });
    expect(open).toBeEnabled();
    await userEvent.click(open);
    expect(api.openBlender).toHaveBeenCalledWith("variant-1");
    expect(onError).not.toHaveBeenCalled();
  });

  it("préserve l’analyse de la source lorsqu’un GLB original sert d’aperçu", async () => {
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "inspect") return sourceInspection;
      if (request.kind === "preview")
        return {
          ...generatedPreview,
          sourceKind: "assetGlb",
          sourceFile: "drone.glb",
          inspection: {
            ...sourceInspection,
            scenes: [],
            collections: [],
            blenderVersion: "GLB 2.0",
            geometry: {
              meshObjects: 1,
              vertices: 4,
              edges: 5,
              faces: 2,
              triangles: 2,
            },
          },
        };
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(screen.getByRole("button", { name: "Analyser" }));
    expect(await screen.findByText("12 arêtes")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    expect(
      await screen.findByLabelText("Aperçu 3D interactif"),
    ).toBeInTheDocument();
    expect(screen.getByText("12 arêtes")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Scène" })).toHaveTextContent(
      "Scene",
    );
    expect(screen.getByText(/Analyse de la source/)).toBeInTheDocument();
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
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    expect(await screen.findByText("— arêtes")).toBeInTheDocument();
    expect(screen.getByLabelText("Aperçu 3D interactif")).toBeInTheDocument();
  });

  it("actualise les métriques de la source avec un nouvel aperçu généré", async () => {
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "inspect") return sourceInspection;
      if (request.kind === "preview")
        return {
          ...generatedPreview,
          inspection: {
            ...sourceInspection,
            geometry: { ...sourceInspection.geometry, edges: 24 },
          },
        };
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(screen.getByRole("button", { name: "Analyser" }));
    await screen.findByText("12 arêtes");
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    expect(await screen.findByText("24 arêtes")).toBeInTheDocument();
  });

  it("ignore un aperçu terminé après le changement de version", async () => {
    let finishPreview: (value: PreviewResult) => void = () => {};
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "preview")
        return new Promise<PreviewResult>((resolve) => {
          finishPreview = resolve;
        });
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    await userEvent.click(screen.getByRole("tab", { name: /Historique/ }));
    await userEvent.click(
      await screen.findByRole("button", { name: /Consulter/ }),
    );
    await screen.findByText("Cette version");
    await act(async () => finishPreview(generatedPreview));
    expect(
      screen.queryByLabelText("Aperçu 3D interactif"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("12 arêtes")).not.toBeInTheDocument();
  });

  it("abandonne le choix d’un GLB supprimé dans le travail courant", async () => {
    let deleted = false;
    vi.mocked(api.getVariant).mockImplementation(async () => ({
      ...variant,
      files: deleted
        ? variant.files
        : [
            ...variant.files,
            { path: "drone.glb", kind: "file", hash: "glb-1", size: 10 },
            { path: "other.glb", kind: "file", hash: "glb-2", size: 10 },
          ],
    }));
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "preview") {
        if (deleted) {
          if (request.previewFile)
            throw { code: "invalid", message: "GLB supprimé" };
          return generatedPreview;
        }
        if (!request.previewFile)
          throw {
            code: "preview_ambiguous",
            details: { candidates: ["drone.glb", "other.glb"] },
          };
        return {
          ...generatedPreview,
          sourceKind: "assetGlb",
          sourceFile: request.previewFile,
        };
      }
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    await userEvent.selectOptions(
      await screen.findByRole("combobox", { name: "GLB de prévisualisation" }),
      "other.glb",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Utiliser ce GLB" }),
    );
    await screen.findByLabelText("Aperçu 3D interactif");
    deleted = true;
    await userEvent.click(
      screen.getAllByRole("button", { name: "Actualiser" })[0]!,
    );
    await waitFor(() =>
      expect(run).toHaveBeenLastCalledWith({
        kind: "preview",
        variantId: "variant-1",
        snapshotId: null,
      }),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByTitle("drone.blend")).toBeInTheDocument();
    expect(api.updateConfig).not.toHaveBeenCalled();
  });

  it("conserve le choix GLB d’une version immuable après suppression dans le travail courant", async () => {
    vi.mocked(api.getSnapshot).mockResolvedValue({
      ...variant.snapshots[0]!,
      entries: [
        ...variant.snapshots[0]!.entries,
        { path: "saved-a.glb", kind: "file", hash: "one", size: 10 },
        { path: "saved-b.glb", kind: "file", hash: "two", size: 10 },
      ],
    });
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "preview") {
        if (!request.previewFile)
          throw {
            code: "preview_ambiguous",
            details: { candidates: ["saved-a.glb", "saved-b.glb"] },
          };
        return {
          ...generatedPreview,
          sourceKind: "assetGlb",
          sourceFile: request.previewFile,
        };
      }
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(screen.getByRole("tab", { name: /Historique/ }));
    await userEvent.click(
      await screen.findByRole("button", { name: /Consulter/ }),
    );
    await screen.findByText("Cette version");
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    await userEvent.selectOptions(
      await screen.findByRole("combobox", { name: "GLB de prévisualisation" }),
      "saved-b.glb",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Utiliser ce GLB" }),
    );
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.click(
      screen.getAllByRole("button", { name: "Actualiser" })[0]!,
    );
    await waitFor(() =>
      expect(run).toHaveBeenLastCalledWith({
        kind: "preview",
        variantId: "variant-1",
        snapshotId: "snapshot-1",
        previewFile: "saved-b.glb",
      }),
    );
    expect(api.getVariant).toHaveBeenCalledTimes(1);
  });

  it("nomme le .blend effectivement analysé lorsque le GLB principal a disparu", async () => {
    vi.mocked(api.getVariant).mockResolvedValue({
      ...variant,
      variant: {
        ...variant.variant,
        config: { ...variant.variant.config, primaryFile: "deleted.glb" },
      },
    });
    run.mockImplementation(async (request) =>
      request.kind === "changes"
        ? changes
        : request.kind === "preview"
          ? generatedPreview
          : null,
    );
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    expect(await screen.findByText(/Analyse de la source/)).toHaveTextContent(
      "drone.blend",
    );
    expect(screen.getByText(/Analyse de la source/)).not.toHaveTextContent(
      "deleted.glb",
    );
  });

  it("préserve l’analyse Blender après un changement externe et un aperçu GLB original", async () => {
    let workspaceChanged: (variantId: string) => void = () => {};
    vi.mocked(api.onWorkspaceChanged).mockImplementation(async (callback) => {
      workspaceChanged = callback;
      return () => {};
    });
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      if (request.kind === "inspect") return sourceInspection;
      if (request.kind === "preview")
        return {
          ...generatedPreview,
          sourceKind: "assetGlb",
          sourceFile: "drone.glb",
          inspection: {
            ...sourceInspection,
            scenes: [],
            collections: [],
            blenderVersion: "GLB 2.0",
            geometry: { ...sourceInspection.geometry, edges: 5 },
          },
        };
      return null;
    });
    renderDetail();
    await screen.findByText("Modifications non versionnées");
    await userEvent.click(screen.getByRole("button", { name: "Analyser" }));
    await screen.findByText("12 arêtes");
    await act(async () => workspaceChanged("variant-1"));
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher l’aperçu 3D" }),
    );
    await screen.findByLabelText("Aperçu 3D interactif");
    expect(screen.getByText("12 arêtes")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Scène" })).toHaveTextContent(
      "Scene",
    );
    expect(screen.getByText(/Analyse de la source/)).toHaveTextContent(
      "drone.blend",
    );
  });
});
