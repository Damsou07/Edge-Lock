import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { HistoryStorageModal, RestoreBackupModal } from "./HistoryStorageModal";
import type { RunJob } from "../lib/useJobs";
import type { StorageReport, Variant } from "../contracts";
import { defaultExportConfig } from "../contracts";

const report: StorageReport = {
  planToken: "verified-plan",
  logicalBytes: 1000,
  physicalBytes: 600,
  reclaimableBytes: 400,
  releasedBytes: 0,
  versions: [{ id: "old", message: "Ancienne version" }],
  warnings: [],
};

describe("gestion du stockage historique", () => {
  it("analyse puis attend une confirmation explicite avant de supprimer des sources", async () => {
    const job = vi
      .fn()
      .mockResolvedValueOnce(report)
      .mockResolvedValueOnce({ ...report, releasedBytes: 400 });
    const saved = vi.fn();
    render(
      <HistoryStorageModal
        variantId="v"
        action="compact"
        runJob={job as RunJob}
        onSaved={saved}
        onClose={vi.fn()}
      />,
    );
    await screen.findByText("Ancienne version", { exact: false });
    expect(job).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Confirmer" }));
    await waitFor(() => expect(saved).toHaveBeenCalledOnce());
    expect(job.mock.calls[1]?.[0]).toEqual({
      kind: "storageApply",
      variantId: "v",
      action: "compact",
      snapshotId: undefined,
      planToken: "verified-plan",
    });
    expect(screen.getByRole("status")).toHaveTextContent("Espace libéré");
  });

  it("ne propose pas de confirmer une conversion sans version éligible", async () => {
    const job = vi.fn().mockResolvedValue({
      ...report,
      versions: [],
      warnings: ["GLB invalide : sources conservées"],
    });
    render(
      <HistoryStorageModal
        variantId="v"
        action="compact"
        runJob={job as RunJob}
        onSaved={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    await screen.findByText("GLB invalide : sources conservées");
    expect(screen.getByRole("button", { name: "Confirmer" })).toBeDisabled();
  });

  it("conserve une erreur de plan périmé et permet une nouvelle analyse", async () => {
    const job = vi
      .fn()
      .mockResolvedValueOnce(report)
      .mockRejectedValueOnce(new Error("Le stockage a changé"))
      .mockResolvedValueOnce({ ...report, planToken: "new-plan" });
    render(
      <HistoryStorageModal
        variantId="v"
        action="deleteVersion"
        snapshotId="old"
        runJob={job as RunJob}
        onSaved={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    await screen.findByText("Ancienne version", { exact: false });
    await userEvent.click(screen.getByRole("button", { name: "Confirmer" }));
    await screen.findByRole("alert");
    expect(screen.getByRole("button", { name: "Confirmer" })).toBeDisabled();
    await userEvent.click(
      screen.getByRole("button", { name: "Analyser à nouveau" }),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Confirmer" })).toBeEnabled(),
    );
    expect(job).toHaveBeenCalledTimes(3);
  });

  it("demande la fermeture de Blender et envoie les identifiants attendus pour restaurer", async () => {
    const variant: Variant = {
      id: "v",
      assetId: "a",
      name: "Main",
      workspacePath: "C:/fixture",
      headSnapshotId: "head",
      backupSnapshotId: "backup",
      config: defaultExportConfig,
      workingBytes: 1,
      triangles: null,
    };
    const job = vi.fn().mockResolvedValue({});
    const restored = vi.fn();
    render(
      <RestoreBackupModal
        variant={variant}
        runJob={job as RunJob}
        onRestored={restored}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "Restaurer" })).toBeDisabled();
    await userEvent.click(screen.getByRole("checkbox"));
    await userEvent.click(screen.getByRole("button", { name: "Restaurer" }));
    await waitFor(() => expect(restored).toHaveBeenCalledOnce());
    expect(job).toHaveBeenCalledWith({
      kind: "restore",
      variantId: "v",
      expectedHead: "head",
      expectedBackup: "backup",
    });
  });
});
