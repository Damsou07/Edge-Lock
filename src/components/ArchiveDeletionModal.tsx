import { useEffect, useRef, useState } from "react";
import type { ArchiveDeletionPlan, ArchiveDeletionReport } from "../contracts";
import type { RunJob } from "../lib/useJobs";
import { errorMessage, formatBytes } from "../lib/format";
import { Modal } from "./Modal";

export function ArchiveDeletionModal({
  assetId,
  runJob,
  onClose,
  onDeleted,
}: {
  assetId?: string;
  runJob: RunJob;
  onClose: () => void;
  onDeleted: (report: ArchiveDeletionReport) => void;
}) {
  const [plan, setPlan] = useState<ArchiveDeletionPlan | null>(null);
  const [result, setResult] = useState<ArchiveDeletionReport | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    const controller = new AbortController();
    void runJob<ArchiveDeletionPlan>(
      { kind: "archiveDeletionPlan", ...(assetId ? { assetId } : {}) },
      { signal: controller.signal },
    )
      .then((value) => {
        if (!controller.signal.aborted) setPlan(value);
      })
      .catch((reason: unknown) => {
        if (!controller.signal.aborted) setError(errorMessage(reason));
      });
    return () => {
      alive.current = false;
      controller.abort();
    };
  }, [assetId, runJob, revision]);
  const apply = async () => {
    if (!plan || busy) return;
    setBusy(true);
    setError("");
    try {
      const report = await runJob<ArchiveDeletionReport>({
        kind: "archiveDeletionApply",
        ...(assetId ? { assetId } : {}),
        planToken: plan.planToken,
      });
      if (!alive.current) return;
      setResult(report);
      onDeleted(report);
    } catch (reason) {
      if (alive.current) setError(errorMessage(reason));
    } finally {
      if (alive.current) setBusy(false);
    }
  };
  return (
    <Modal
      title={
        assetId
          ? "Supprimer cet asset archivé ?"
          : "Vider toutes les archives ?"
      }
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <div className="form-stack">
        {!assetId && (
          <p>
            <strong>
              Portée : toutes les archives de la bibliothèque, quels que soient
              les filtres.
            </strong>
          </p>
        )}
        <p>
          Cette action est définitive : les fichiers, variantes et versions des
          assets concernés seront supprimés. Enregistrez et fermez leurs
          fichiers dans Blender avant de confirmer.
        </p>
        {!plan && !error && <p role="status">Analyse des archives…</p>}
        {plan && !result && (
          <>
            <p>
              {plan.assetCount} asset{plan.assetCount > 1 ? "s" : ""},{" "}
              {plan.variantCount} variante{plan.variantCount > 1 ? "s" : ""} et{" "}
              {plan.snapshotCount} version{plan.snapshotCount > 1 ? "s" : ""}.
              Dossiers de travail : {formatBytes(plan.workspaceBytes)}.
            </p>
            <ul className="storage-version-list">
              {plan.assets.map((asset) => (
                <li key={asset.id}>{asset.name}</li>
              ))}
            </ul>
          </>
        )}
        {result && (
          <p role="status">
            {result.cleanupPending
              ? "Nettoyage à terminer : des fichiers restent à supprimer. Une reprise sera tentée à la réouverture de la bibliothèque."
              : `Suppression terminée. Espace libéré : ${formatBytes(result.releasedBytes)}.`}
          </p>
        )}
        {(result ?? plan)?.warnings.map((warning, index) => (
          <p className="inline-error" key={index}>
            {warning}
          </p>
        ))}
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        <footer className="modal-actions">
          <button
            className="button secondary"
            disabled={busy}
            onClick={onClose}
          >
            {result ? "Fermer" : "Annuler"}
          </button>
          {error && !result && (
            <button
              className="button secondary"
              onClick={() => {
                setError("");
                setPlan(null);
                setRevision((value) => value + 1);
              }}
            >
              Analyser à nouveau
            </button>
          )}
          {!result && (
            <button
              className="button danger"
              disabled={!plan || !plan.assetCount || busy || !!error}
              onClick={() => void apply()}
            >
              {busy ? "Suppression…" : "Supprimer définitivement"}
            </button>
          )}
        </footer>
      </div>
    </Modal>
  );
}
