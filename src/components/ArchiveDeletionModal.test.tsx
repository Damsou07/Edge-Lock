import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import type { ArchiveDeletionPlan, JobRequest } from "../contracts";
import type { RunJob } from "../lib/useJobs";
import { ArchiveDeletionModal } from "./ArchiveDeletionModal";

const plan: ArchiveDeletionPlan = {
  planToken: "confirmed",
  assets: [{ id: "one", name: "Drone" }],
  assetIds: ["one"],
  variantIds: ["v"],
  assetCount: 1,
  variantCount: 1,
  snapshotCount: 3,
  workspaceBytes: 10,
  warnings: [],
};
beforeEach(() => {
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
it("analyse toutes les archives et attend une confirmation avant suppression", async () => {
  const deleted = vi.fn();
  const run = vi.fn(async (request: JobRequest) =>
    request.kind === "archiveDeletionPlan"
      ? plan
      : {
          deletedAssetIds: ["one"],
          deletedVariantIds: ["v"],
          releasedBytes: 10,
          cleanupPending: false,
          warnings: [],
        },
  );
  render(
    <ArchiveDeletionModal
      runJob={run as RunJob}
      onClose={vi.fn()}
      onDeleted={deleted}
    />,
  );
  await screen.findByText(/1 asset/);
  expect(run).toHaveBeenCalledTimes(1);
  expect(run).toHaveBeenCalledWith(
    { kind: "archiveDeletionPlan" },
    expect.objectContaining({ signal: expect.any(AbortSignal) }),
  );
  await userEvent.click(
    screen.getByRole("button", { name: "Supprimer définitivement" }),
  );
  await waitFor(() =>
    expect(run).toHaveBeenCalledWith({
      kind: "archiveDeletionApply",
      planToken: "confirmed",
    }),
  );
  expect(deleted).toHaveBeenCalledTimes(1);
});
it("confirme le retrait des archives avec un nettoyage différé informatif", async () => {
  const run = vi.fn(async (request: JobRequest) =>
    request.kind === "archiveDeletionPlan"
      ? plan
      : {
          deletedAssetIds: ["one"],
          deletedVariantIds: ["v"],
          releasedBytes: 0,
          cleanupPending: true,
          warnings: ["Fichier verrouillé"],
        },
  );
  render(
    <ArchiveDeletionModal
      assetId="one"
      runJob={run as RunJob}
      onClose={vi.fn()}
      onDeleted={vi.fn()}
    />,
  );
  await screen.findByText(/1 asset/);
  await userEvent.click(
    screen.getByRole("button", { name: "Supprimer définitivement" }),
  );
  expect(
    await screen.findByText(
      /Archives supprimées ; nettoyage de certains fichiers différé/,
    ),
  ).toHaveAttribute("role", "status");
  expect(screen.getByText("Fichier verrouillé")).toBeInTheDocument();
  expect(screen.getByText("Fichier verrouillé")).not.toHaveClass(
    "inline-error",
  );
  expect(
    screen.queryByRole("button", { name: "Supprimer définitivement" }),
  ).not.toBeInTheDocument();
});

it("présente une estimation partielle sans empêcher la confirmation", async () => {
  const run = vi
    .fn()
    .mockResolvedValue({ ...plan, workspaceBytesComplete: false });
  render(
    <ArchiveDeletionModal
      runJob={run as RunJob}
      onClose={vi.fn()}
      onDeleted={vi.fn()}
    />,
  );
  expect(
    await screen.findByText(/Dossiers de travail : au moins/),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Supprimer définitivement" }),
  ).toBeEnabled();
});
