import { useEffect, useState } from "react";
import type { StorageAction, StorageReport, Variant } from "../contracts";
import type { RunJob } from "../lib/useJobs";
import { errorMessage, formatBytes } from "../lib/format";
import { Modal } from "./Modal";

const labels: Record<StorageAction, string> = {
  recompress: "Compresser le stockage avec Zstd",
  compact: "Convertir l’ancien historique en consultation seule",
  releaseBackup: "Libérer le backup complet",
  deleteVersion: "Supprimer cette version",
  purge: "Libérer les objets historiques inutilisés",
};

export function HistoryStorageModal({
  variantId,
  action,
  snapshotId,
  runJob,
  onSaved,
  onClose,
}: {
  variantId?: string;
  action: StorageAction;
  snapshotId?: string;
  runJob: RunJob;
  onSaved: () => void;
  onClose: () => void;
}) {
  const [report, setReport] = useState<StorageReport | null>(null);
  const [result, setResult] = useState<StorageReport | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    void runJob<StorageReport>(
      { kind: "storagePlan", variantId, action, snapshotId },
      { signal: controller.signal },
    )
      .then((value) => {
        if (!controller.signal.aborted) setReport(value);
      })
      .catch((reason: unknown) => {
        if (!controller.signal.aborted) setError(errorMessage(reason));
      });
    return () => controller.abort();
  }, [variantId, action, snapshotId, runJob, revision]);
  const apply = async () => {
    if (!report) return;
    setBusy(true);
    setError("");
    try {
      setResult(
        await runJob<StorageReport>({
          kind: "storageApply",
          variantId,
          action,
          snapshotId,
          planToken: report.planToken,
        }),
      );
      onSaved();
    } catch (reason) {
      setError(errorMessage(reason));
      onSaved();
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title={labels[action]}
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <div className="form-stack">
        {(action === "recompress" || action === "purge") && (
          <p>
            <strong>Portée : toute la bibliothèque</strong>
          </p>
        )}
        {action === "recompress" && (
          <p>
            Compression sans perte des objets de la bibliothèque. Le contenu des
            versions reste identique.
          </p>
        )}
        {action === "compact" && (
          <p>
            Les versions listées conserveront leur GLB texturé et leurs
            métadonnées. Leurs dossiers Blender ne seront plus restaurables. Les
            versions courantes et les backups restent protégés.
          </p>
        )}
        {action === "releaseBackup" && (
          <p>
            Ce retour arrière complet sera abandonné. Son aperçu restera
            consultable. La prochaine création de version pourra créer un
            nouveau backup.
          </p>
        )}
        {action === "deleteVersion" && (
          <p>
            Le GLB et les métadonnées visibles de cette version seront retirés
            de l’historique. Cette action est définitive.
          </p>
        )}
        {action === "purge" && (
          <p>
            Seuls les objets internes libérés par des versions supprimées ou
            converties sont concernés. Les fichiers des workspaces sont
            conservés.
          </p>
        )}
        {!report && !error && <p role="status">Analyse du stockage…</p>}
        {report && !result && (
          <>
            <p>
              Objets de la bibliothèque : {formatBytes(report.logicalBytes)} de
              contenu unique avant compression,{" "}
              {formatBytes(report.physicalBytes)} sur disque.
            </p>
            <p>
              <strong>
                Espace récupérable : {formatBytes(report.reclaimableBytes)}
              </strong>
            </p>
            {report.versions.length > 0 && (
              <ul className="storage-version-list">
                {report.versions.map((version) => (
                  <li key={version.id}>
                    {version.message} <code>{version.id.slice(0, 8)}</code>
                  </li>
                ))}
              </ul>
            )}
            {report.versions.length === 0 && action === "compact" && (
              <p>Aucune version éligible à la conversion.</p>
            )}
          </>
        )}
        {result && (
          <p role="status">
            Opération terminée. Espace libéré :{" "}
            {formatBytes(result.releasedBytes)}.
          </p>
        )}
        {(result ?? report)?.warnings.map((warning, index) => (
          <p className="inline-error" key={index}>
            {warning}
          </p>
        ))}
        {error && (
          <p role="alert" className="inline-error">
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
              disabled={busy}
              onClick={() => {
                setError("");
                setReport(null);
                setRevision((value) => value + 1);
              }}
            >
              Analyser à nouveau
            </button>
          )}
          {!result && (
            <button
              className="button primary"
              disabled={
                busy ||
                !report ||
                !!error ||
                (action === "compact" && report.versions.length === 0)
              }
              onClick={() => void apply()}
            >
              {busy ? "Traitement…" : "Confirmer"}
            </button>
          )}
        </footer>
      </div>
    </Modal>
  );
}

export function RestoreBackupModal({
  variant,
  runJob,
  onRestored,
  onClose,
}: {
  variant: Variant;
  runJob: RunJob;
  onRestored: () => void;
  onClose: () => void;
}) {
  const [closed, setClosed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const restore = async () => {
    if (!variant.backupSnapshotId || !closed) return;
    setBusy(true);
    try {
      await runJob({
        kind: "restore",
        variantId: variant.id,
        expectedHead: variant.headSnapshotId,
        expectedBackup: variant.backupSnapshotId,
      });
      onRestored();
      onClose();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title="Restaurer le backup complet"
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <div className="form-stack">
        <p>
          Le dossier de travail sera remplacé par le backup. Son état enregistré
          sur disque sera conservé comme nouveau backup, même si vous n’en avez
          pas créé de version.
        </p>
        <p>
          Les changements encore en mémoire dans Blender et les ressources
          externes au dossier ne sont pas capturés.
        </p>
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={closed}
            disabled={busy}
            onChange={(event) => setClosed(event.target.checked)}
          />{" "}
          J’ai enregistré et fermé ce projet dans Blender et les autres
          applications qui le modifient.
        </label>
        {error && (
          <p role="alert" className="inline-error">
            {error}
          </p>
        )}
        <footer className="modal-actions">
          <button
            className="button secondary"
            disabled={busy}
            onClick={onClose}
          >
            Annuler
          </button>
          <button
            className="button primary"
            disabled={busy || !closed}
            onClick={() => void restore()}
          >
            {busy ? "Restauration…" : "Restaurer"}
          </button>
        </footer>
      </div>
    </Modal>
  );
}
