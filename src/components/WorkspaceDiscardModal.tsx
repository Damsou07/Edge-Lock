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
  const [confirmed, setConfirmed] = useState(false);
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
    if (!snapshot || !confirmed || busy || result) return;
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
      if (report.pending || report.reconfirmationRequired) return;
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
          Terminez vos écritures dans Blender et les autres applications. Si un
          fichier est verrouillé, Edge Lock peut reprendre la demande après sa
          libération.
        </p>
        {!result && (
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={confirmed}
              disabled={busy}
              onChange={(event) => setConfirmed(event.target.checked)}
            />{" "}
            Je confirme la perte des modifications non versionnées.
          </label>
        )}
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        {result?.pending && (
          <p role="status">
            Demande d’annulation enregistrée. Edge Lock vérifiera le dossier
            après la libération du verrou. Vous pouvez suivre la demande dans la
            fiche.
          </p>
        )}
        {result?.reconfirmationRequired && (
          <p role="status">
            Les fichiers ont été préservés. Confirmez de nouveau l’annulation
            depuis la fiche.
          </p>
        )}
        {result?.cleanupPending && (
          <p role="status">
            Modifications annulées. Le nettoyage de certains fichiers reprendra
            à la réouverture de la bibliothèque.
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
              disabled={!snapshot || !confirmed || busy}
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
