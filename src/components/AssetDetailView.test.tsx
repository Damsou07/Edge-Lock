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

  it("préserve une configuration en cours de saisie lors du rafraîchissement des métriques", async () => {
    run.mockImplementation(async (request) =>
      request.kind === "changes"
        ? changes
        : { ...generatedPreview, sourceKind: "assetGlb" },
    );
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
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.type(
      screen.getByRole("textbox", { name: "Scène" }),
      "Scene",
    );
    rendered.rerender(<AssetDetailView {...props} generation={1} />);
    await waitFor(() => expect(api.getVariant).toHaveBeenCalledTimes(2));
    expect(
      screen.getByRole("button", { name: "Exporter .glb" }),
    ).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Scène" })).toHaveValue("Scene");
    expect(api.updateConfig).not.toHaveBeenCalled();
    expect(
      run.mock.calls.filter(([request]) => request.kind === "preview"),
    ).toHaveLength(1);
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

  it("attend les réglages de la nouvelle variante avant d’afficher son aperçu", async () => {
    const alternate = {
      ...variant.variant,
      id: "variant-2",
      name: "Désert",
      config: { ...variant.variant.config, primaryFile: "desert.blend" },
    };
    vi.mocked(api.getAsset).mockResolvedValue({
      ...detail,
      variants: [...detail.variants, alternate],
    });
    let finishVariant: (value: VariantDetail) => void = () => {};
    vi.mocked(api.getVariant).mockImplementation(async (id) => {
      if (id === "variant-1") return variant;
      return new Promise((resolve) => {
        finishVariant = resolve;
      });
    });
    run.mockImplementation(async (request) =>
      request.kind === "changes"
        ? changes
        : {
            ...generatedPreview,
            url: `http://edge-preview.localhost/${"variantId" in request ? request.variantId : "unused"}`,
          },
    );
    renderDetail();
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Variante" }),
      "variant-2",
    );
    expect(
      screen.queryByLabelText("Aperçu 3D interactif"),
    ).not.toBeInTheDocument();
    expect(
      run.mock.calls.filter(([request]) => request.kind === "preview"),
    ).toHaveLength(1);
    await act(async () => finishVariant({ ...variant, variant: alternate }));
    expect(
      await screen.findByLabelText("Aperçu 3D interactif"),
    ).toHaveAttribute("data-url", "http://edge-preview.localhost/variant-2");
    expect(run).toHaveBeenCalledWith({
      kind: "preview",
      variantId: "variant-2",
      snapshotId: null,
    });
    expect(
      screen.getByRole("combobox", { name: "Fichier principal" }),
    ).toHaveValue("desert.blend");
    expect(api.releasePreviewResources).toHaveBeenCalledWith(["variant-1"]);
  });

  it("charge automatiquement une seule fois et résiste au rafraîchissement global des métriques", async () => {
    run.mockImplementation(async (request) =>
      request.kind === "changes" ? changes : generatedPreview,
    );
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
    expect(
      await screen.findByLabelText("Aperçu 3D interactif"),
    ).toHaveAttribute("data-url", generatedPreview.url);
    rendered.rerender(<AssetDetailView {...props} generation={1} />);
    await waitFor(() => expect(api.getVariant).toHaveBeenCalledTimes(2));
    expect(
      run.mock.calls.filter(([request]) => request.kind === "preview"),
    ).toHaveLength(1);
    await userEvent.click(
      screen.getAllByRole("button", { name: "Actualiser" })[0]!,
    );
    expect(
      run.mock.calls.filter(([request]) => request.kind === "preview"),
    ).toHaveLength(2);
  });

  it("enregistre automatiquement les choix et recharge une seule fois sans bouton manuel", async () => {
    let savedConfig = variant.variant.config;
    vi.mocked(api.getVariant).mockImplementation(async () => ({
      ...variant,
      variant: { ...variant.variant, config: savedConfig },
    }));
    vi.mocked(api.updateConfig).mockImplementation(async (_, value) => {
      savedConfig = value;
    });
    run.mockImplementation(async (request) =>
      request.kind === "changes" ? changes : generatedPreview,
    );
    renderDetail();
    await screen.findByLabelText("Aperçu 3D interactif");
    expect(
      screen.queryByRole("button", { name: "Enregistrer les réglages" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/Ces réglages définissent la source/),
    ).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: "Appliquer les modificateurs à l’export",
      }),
    );
    await waitFor(() =>
      expect(
        run.mock.calls.filter(([request]) => request.kind === "preview"),
      ).toHaveLength(2),
    );
    expect(api.updateConfig).toHaveBeenCalledWith("variant-1", {
      ...variant.variant.config,
      applyModifiers: true,
    });
    expect(
      screen.getByRole("checkbox", {
        name: "Appliquer les modificateurs à l’export",
      }),
    ).toBeChecked();
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Fichier principal" }),
      "drone.blend",
    );
    expect(api.updateConfig).toHaveBeenCalledTimes(1);
  });

  it("enregistre immédiatement la scène et la collection choisies", async () => {
    let savedConfig = variant.variant.config;
    vi.mocked(api.getVariant).mockImplementation(async () => ({
      ...variant,
      variant: { ...variant.variant, config: savedConfig },
    }));
    vi.mocked(api.updateConfig).mockImplementation(async (_, value) => {
      savedConfig = value;
    });
    renderDetail();
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Scène" }),
      "Scene",
    );
    await waitFor(() =>
      expect(api.updateConfig).toHaveBeenCalledWith("variant-1", {
        ...variant.variant.config,
        scene: "Scene",
      }),
    );
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Collection" }),
      "Asset",
    );
    await waitFor(() =>
      expect(api.updateConfig).toHaveBeenLastCalledWith("variant-1", {
        ...variant.variant.config,
        scene: "Scene",
        collection: "Asset",
      }),
    );
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Scène" }),
      "",
    );
    await waitFor(() =>
      expect(api.updateConfig).toHaveBeenLastCalledWith(
        "variant-1",
        variant.variant.config,
      ),
    );
    expect(api.updateConfig).toHaveBeenCalledTimes(3);
  });

  it("enregistre les champs texte au blur ou avec Entrée, sans écrire à chaque frappe", async () => {
    let savedConfig = variant.variant.config;
    vi.mocked(api.getVariant).mockImplementation(async () => ({
      ...variant,
      variant: { ...variant.variant, config: savedConfig },
    }));
    vi.mocked(api.updateConfig).mockImplementation(async (_, value) => {
      savedConfig = value;
    });
    run.mockImplementation(async (request) =>
      request.kind === "changes"
        ? changes
        : { ...generatedPreview, sourceKind: "assetGlb" },
    );
    renderDetail();
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.type(
      screen.getByRole("textbox", { name: "Scène" }),
      "Scene",
    );
    expect(api.updateConfig).not.toHaveBeenCalled();
    await userEvent.tab();
    await waitFor(() =>
      expect(api.updateConfig).toHaveBeenCalledWith("variant-1", {
        ...variant.variant.config,
        scene: "Scene",
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Collection" })).toBeEnabled(),
    );
    await userEvent.type(
      screen.getByRole("textbox", { name: "Collection" }),
      "Asset{Enter}",
    );
    await waitFor(() =>
      expect(api.updateConfig).toHaveBeenLastCalledWith("variant-1", {
        ...variant.variant.config,
        scene: "Scene",
        collection: "Asset",
      }),
    );
    expect(api.updateConfig).toHaveBeenCalledTimes(2);
  });

  it("bloque les réglages et opérations pendant la sauvegarde et utilise la configuration canonique", async () => {
    let finish: () => void = () => {};
    vi.mocked(api.updateConfig).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    renderDetail();
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Fichier principal" }),
      "",
    );
    expect(api.updateConfig).toHaveBeenCalledWith("variant-1", {
      ...variant.variant.config,
      primaryFile: null,
    });
    for (const name of ["Fichier principal", "Scène", "Collection"])
      expect(screen.getByLabelText(name)).toBeDisabled();
    for (const name of [
      "Exporter .glb",
      "Ouvrir dans Blender",
      "Créer une version",
      "Analyser la source",
    ])
      expect(screen.getByRole("button", { name })).toBeDisabled();
    expect(
      screen.getByRole("checkbox", {
        name: "Appliquer les modificateurs à l’export",
      }),
    ).toBeDisabled();
    await act(async () => finish());
    await waitFor(() =>
      expect(
        screen.getByRole("combobox", { name: "Fichier principal" }),
      ).toHaveValue("drone.blend"),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Exporter .glb" }),
      ).toBeEnabled(),
    );
  });

  it("conserve le brouillon après échec et permet de réessayer la sauvegarde", async () => {
    let savedConfig = variant.variant.config;
    vi.mocked(api.getVariant).mockImplementation(async () => ({
      ...variant,
      variant: { ...variant.variant, config: savedConfig },
    }));
    vi.mocked(api.updateConfig)
      .mockRejectedValueOnce(new Error("Disque indisponible"))
      .mockImplementation(async (_, value) => {
        savedConfig = value;
      });
    renderDetail();
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: "Appliquer les modificateurs à l’export",
      }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Disque indisponible",
    );
    expect(
      screen.getByRole("checkbox", {
        name: "Appliquer les modificateurs à l’export",
      }),
    ).toBeChecked();
    expect(
      screen.getByRole("button", { name: "Exporter .glb" }),
    ).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    await waitFor(() =>
      expect(screen.queryByRole("alert")).not.toBeInTheDocument(),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Exporter .glb" }),
      ).toBeEnabled(),
    );
    expect(api.updateConfig).toHaveBeenCalledTimes(2);
  });

  it("termine la sauvegarde capturée sans recharger l’ancienne variante après navigation", async () => {
    const alternate = {
      ...variant.variant,
      id: "variant-2",
      name: "Désert",
      config: { ...variant.variant.config, primaryFile: "desert.blend" },
    };
    vi.mocked(api.getAsset).mockResolvedValue({
      ...detail,
      variants: [...detail.variants, alternate],
    });
    vi.mocked(api.getVariant).mockImplementation(async (id) =>
      id === "variant-1" ? variant : { ...variant, variant: alternate },
    );
    let finish: () => void = () => {};
    vi.mocked(api.updateConfig).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    renderDetail();
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: "Appliquer les modificateurs à l’export",
      }),
    );
    expect(api.updateConfig).toHaveBeenCalledWith("variant-1", {
      ...variant.variant.config,
      applyModifiers: true,
    });
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Variante" }),
      "variant-2",
    );
    await waitFor(() =>
      expect(
        screen.getByRole("combobox", { name: "Fichier principal" }),
      ).toHaveValue("desert.blend"),
    );
    const oldReads = vi
      .mocked(api.getVariant)
      .mock.calls.filter(([id]) => id === "variant-1").length;
    await act(async () => finish());
    await waitFor(() =>
      expect(
        screen.getByRole("combobox", { name: "Fichier principal" }),
      ).toBeEnabled(),
    );
    expect(
      vi.mocked(api.getVariant).mock.calls.filter(([id]) => id === "variant-1"),
    ).toHaveLength(oldReads);
    expect(
      screen.getByRole("combobox", { name: "Fichier principal" }),
    ).toHaveValue("desert.blend");
    expect(
      screen.getByRole("checkbox", {
        name: "Appliquer les modificateurs à l’export",
      }),
    ).not.toBeChecked();
  });

  it("n’installe aucun résultat ni watcher après la fermeture de la fiche pendant une sauvegarde", async () => {
    let finish: () => void = () => {};
    vi.mocked(api.updateConfig).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const rendered = renderDetail();
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: "Appliquer les modificateurs à l’export",
      }),
    );
    expect(api.updateConfig).toHaveBeenCalledTimes(1);
    rendered.unmount();
    const reads = vi.mocked(api.getVariant).mock.calls.length;
    await act(async () => finish());
    expect(api.getVariant).toHaveBeenCalledTimes(reads);
  });

  it("ignore un ancien aperçu terminé pendant la sauvegarde automatique", async () => {
    let finishPreview: (value: PreviewResult) => void = () => {};
    let finishSave: () => void = () => {};
    run.mockImplementation(async (request) =>
      request.kind === "changes"
        ? changes
        : new Promise((resolve) => {
            finishPreview = resolve;
          }),
    );
    vi.mocked(api.updateConfig).mockImplementation(
      () =>
        new Promise((resolve) => {
          finishSave = resolve;
        }),
    );
    renderDetail();
    await waitFor(() =>
      expect(
        run.mock.calls.filter(([request]) => request.kind === "preview"),
      ).toHaveLength(1),
    );
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: "Appliquer les modificateurs à l’export",
      }),
    );
    await act(async () => finishPreview(generatedPreview));
    expect(
      screen.queryByLabelText("Aperçu 3D interactif"),
    ).not.toBeInTheDocument();
    expect(api.releasePreviewResources).toHaveBeenCalledWith(["generated"]);
    await act(async () => finishSave());
  });

  it.each(["choix GLB", "actualisation des fichiers"])(
    "ignore une ancienne configuration chargée par %s après la sauvegarde automatique",
    async (origin) => {
      const stale = {
        ...variant,
        files: [
          ...variant.files,
          {
            path: "alternate.glb",
            kind: "file" as const,
            hash: "glb",
            size: 10,
          },
        ],
      };
      let savedConfig = variant.variant.config;
      let delayNextRead = false;
      let readWaiting = false;
      let finishRead: (value: VariantDetail) => void = () => {};
      vi.mocked(api.getVariant).mockImplementation(async () => {
        if (delayNextRead) {
          delayNextRead = false;
          readWaiting = true;
          return new Promise((resolve) => {
            finishRead = resolve;
          });
        }
        return { ...stale, variant: { ...stale.variant, config: savedConfig } };
      });
      vi.mocked(api.updateConfig).mockImplementation(async (_, value) => {
        savedConfig = value;
      });
      let previewCalls = 0;
      run.mockImplementation(async (request) => {
        if (request.kind === "changes") return changes;
        previewCalls += 1;
        if (origin === "choix GLB" && previewCalls === 1)
          throw {
            code: "preview_ambiguous",
            details: { candidates: ["alternate.glb"] },
          };
        return generatedPreview;
      });
      renderDetail();
      if (origin === "choix GLB") {
        await userEvent.selectOptions(
          await screen.findByRole("combobox", {
            name: "GLB de prévisualisation",
          }),
          "alternate.glb",
        );
        delayNextRead = true;
        await userEvent.click(
          screen.getByRole("button", { name: "Utiliser ce GLB" }),
        );
      } else {
        await screen.findByLabelText("Aperçu 3D interactif");
        delayNextRead = true;
        await userEvent.click(
          screen
            .getAllByRole("button", { name: "Actualiser" })
            .find((button) => button.classList.contains("refresh-files"))!,
        );
      }
      await waitFor(() => expect(readWaiting).toBe(true));
      await userEvent.click(
        screen.getByRole("checkbox", {
          name: "Appliquer les modificateurs à l’export",
        }),
      );
      await waitFor(() =>
        expect(
          screen.getByRole("button", { name: "Exporter .glb" }),
        ).toBeEnabled(),
      );
      await act(async () => finishRead(stale));
      expect(
        screen.getByRole("button", { name: "Exporter .glb" }),
      ).toBeEnabled();
      expect(
        screen.getByRole("checkbox", {
          name: "Appliquer les modificateurs à l’export",
        }),
      ).toBeChecked();
      expect(previewCalls).toBe(2);
    },
  );

  it("libère les ressources affichées et les réponses arrivées après fermeture", async () => {
    let finish: (value: PreviewResult) => void = () => {};
    run.mockImplementation(async (request) => {
      if (request.kind === "changes") return changes;
      return new Promise<PreviewResult>((resolve) => {
        finish = resolve;
      });
    });
    const rendered = renderDetail();
    await waitFor(() =>
      expect(run).toHaveBeenCalledWith({
        kind: "preview",
        variantId: "variant-1",
        snapshotId: null,
      }),
    );
    await act(async () => finish(generatedPreview));
    await screen.findByLabelText("Aperçu 3D interactif");
    await userEvent.click(
      screen.getAllByRole("button", { name: "Actualiser" })[0]!,
    );
    rendered.unmount();
    await act(async () =>
      finish({
        ...generatedPreview,
        url: "http://edge-preview.localhost/late",
      }),
    );
    expect(api.releasePreviewResources).toHaveBeenCalledWith(["generated"]);
    expect(api.releasePreviewResources).toHaveBeenCalledWith(["late"]);
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
    await userEvent.click(
      screen.getByRole("button", { name: "Analyser la source" }),
    );
    expect(await screen.findByText("12 arêtes")).toBeInTheDocument();
    await userEvent.click(
      screen.getAllByRole("button", { name: "Actualiser" })[0]!,
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
    await userEvent.click(
      screen.getByRole("button", { name: "Analyser la source" }),
    );
    await screen.findByText("12 arêtes");
    await userEvent.click(
      screen.getAllByRole("button", { name: "Actualiser" })[0]!,
    );
    expect(await screen.findByText("24 arêtes")).toBeInTheDocument();
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
      expect(run).toHaveBeenCalledWith({
        kind: "preview",
        variantId: "variant-1",
        snapshotId: "snapshot-1",
      }),
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
    await userEvent.click(
      screen.getByRole("button", { name: "Analyser la source" }),
    );
    await screen.findByText("12 arêtes");
    await act(async () => workspaceChanged("variant-1"));
    await userEvent.click(
      screen.getAllByRole("button", { name: "Actualiser" })[0]!,
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
