import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import { defaultExportConfig } from "../contracts";
import type {
  AssetDetail,
  ChangeSet,
  JobRequest,
  VariantDetail,
} from "../contracts";
import type { RunJob } from "../lib/useJobs";
import { AssetDetailView } from "./AssetDetailView";

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
