import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Snapshot, Variant, WorkspaceDiscardReport } from "../contracts";
import { errorMessage, formatDate } from "../lib/format";
import type { RunJob } from "../lib/useJobs";
import { Modal } from "./Modal";

export function WorkspaceDiscardModal({
  variant,
  runJob,
  onClose,
  onDiscarded,
}: {
  variant: Variant;
  runJob: RunJob;
  onClose: () => void;
  onDiscarded: (report: WorkspaceDiscardReport) => void;
}) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [closed, setClosed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<WorkspaceDiscardReport | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    let active = true;
    void api
      .getSnapshot(variant.headSnapshotId)
      .then((value) => {
        if (active) setSnapshot(value);
      })
      .catch((reason: unknown) => {
        if (active) setError(errorMessage(reason));
      });
    return () => {
      active = false;
      alive.current = false;
    };
  }, [variant.headSnapshotId]);

  const discard = async () => {
    if (!snapshot || !closed || busy || result) return;
    setBusy(true);
    setError("");
    try {
      const report = await runJob<WorkspaceDiscardReport>({
        kind: "discardChanges",
        variantId: variant.id,
        expectedHead: variant.headSnapshotId,
      });
      if (!alive.current) return;
      setResult(report);
      onDiscarded(report);
      if (!report.cleanupPending) onClose();
    } catch (reason) {
      if (alive.current) setError(errorMessage(reason));
    } finally {
      if (alive.current) setBusy(false);
    }
  };

  return (
    <Modal
      title="Annuler les modifications ?"
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <div className="form-stack">
        <p>
          La variante <strong>{variant.name}</strong> retrouvera sa dernière
          version créée. Les fichiers ajoutés et toutes les modifications non
          versionnées seront définitivement supprimés, et les fichiers supprimés
          seront rétablis. Les réglages du projet seront également restaurés.
          Aucun nouveau backup ni aucune version ne sera créé.
        </p>
        {snapshot ? (
          <p>
            Version du {formatDate(snapshot.createdAt)} :{" "}
            <strong>{snapshot.message}</strong>
          </p>
        ) : (
          !error && <p role="status">Chargement de la version…</p>
        )}
        <p>
          Fermez Blender et les autres applications qui modifient ce dossier.
          Les changements encore en mémoire dans ces applications et les
          ressources externes au dossier ne sont pas gérés par Edge Lock.
        </p>
        {!result && (
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={closed}
              disabled={busy}
              onChange={(event) => setClosed(event.target.checked)}
            />{" "}
            J’ai fermé les applications qui modifient ce dossier et je confirme
            la perte des modifications non versionnées.
          </label>
        )}
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        {result && (
          <p role="status">
            Modifications annulées ; le nettoyage de certains fichiers est
            différé. Une reprise sera tentée à la réouverture de la
            bibliothèque.
          </p>
        )}
        {result?.warnings.map((warning, index) => (
          <p className="field-help" key={index}>
            {warning}
          </p>
        ))}
        <footer className="modal-actions">
          <button
            type="button"
            className="button secondary"
            disabled={busy}
            onClick={onClose}
          >
            {result ? "Fermer" : "Conserver mes modifications"}
          </button>
          {!result && (
            <button
              type="button"
              className="button danger"
              disabled={!snapshot || !closed || busy}
              onClick={() => void discard()}
            >
              {busy
                ? "Annulation des modifications…"
                : "Supprimer les modifications"}
            </button>
          )}
        </footer>
      </div>
    </Modal>
  );
}
