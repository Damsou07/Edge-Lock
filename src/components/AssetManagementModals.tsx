import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { api } from "../api";
import type { ArchiveDeletionReport, VariantDeletionPlan } from "../contracts";
import { errorMessage, formatBytes } from "../lib/format";
import type { RunJob } from "../lib/useJobs";
import { Modal } from "./Modal";

export interface RenameTarget {
  kind: "snapshot" | "variant";
  id: string;
  name: string;
}

export function RenameModal({
  target,
  onClose,
  onSaved,
}: {
  target: RenameTarget;
  onClose: () => void;
  onSaved: (name: string) => void;
}) {
  const [value, setValue] = useState(target.name);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const name = value.trim();
  const valid =
    name.length > 0 &&
    new TextEncoder().encode(name).length <= 240 &&
    !/\p{Cc}/u.test(name);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!valid || busy) return;
    setBusy(true);
    setError("");
    try {
      if (target.kind === "snapshot") await api.renameSnapshot(target.id, name);
      else await api.renameVariant(target.id, name);
      if (alive.current) onSaved(name);
    } catch (reason) {
      if (alive.current) setError(errorMessage(reason));
    } finally {
      if (alive.current) setBusy(false);
    }
  };
  return (
    <Modal
      title={
        target.kind === "snapshot"
          ? "Renommer la version"
          : "Renommer la variante"
      }
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <form className="form-stack" onSubmit={(event) => void submit(event)}>
        <label>
          {target.kind === "snapshot"
            ? "Nom de la version"
            : "Nom de la variante"}
          <input
            autoFocus
            required
            maxLength={240}
            value={value}
            disabled={busy}
            onChange={(event) => setValue(event.target.value)}
          />
        </label>
        {value && !valid && (
          <p className="field-help">
            Le nom doit contenir entre 1 et 240 octets UTF-8, sans caractère de
            contrôle.
          </p>
        )}
        {target.kind === "snapshot" && (
          <p className="field-help">
            Ce nom sera visible dans toutes les variantes qui partagent cette
            version.
          </p>
        )}
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        <footer className="modal-actions">
          <button
            type="button"
            className="button secondary"
            disabled={busy}
            onClick={onClose}
          >
            Annuler
          </button>
          <button className="button primary" disabled={!valid || busy}>
            {busy ? "Enregistrement…" : "Enregistrer"}
          </button>
        </footer>
      </form>
    </Modal>
  );
}

export function VariantDeletionModal({
  variantId,
  runJob,
  onClose,
  onDeleted,
}: {
  variantId: string;
  runJob: RunJob;
  onClose: () => void;
  onDeleted: (report: ArchiveDeletionReport) => void;
}) {
  const [plan, setPlan] = useState<VariantDeletionPlan | null>(null);
  const [result, setResult] = useState<ArchiveDeletionReport | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    const controller = new AbortController();
    void runJob<VariantDeletionPlan>(
      { kind: "variantDeletionPlan", variantId },
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
  }, [variantId, runJob, revision]);
  const apply = async () => {
    if (!plan || busy) return;
    setBusy(true);
    setError("");
    try {
      const report = await runJob<ArchiveDeletionReport>({
        kind: "variantDeletionApply",
        variantId,
        planToken: plan.planToken,
      });
      if (alive.current) {
        setResult(report);
        onDeleted(report);
      }
    } catch (reason) {
      if (alive.current) setError(errorMessage(reason));
    } finally {
      if (alive.current) setBusy(false);
    }
  };
  return (
    <Modal
      title="Supprimer cette variante ?"
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <div className="form-stack">
        <p>
          Cette action est définitive : le dossier de travail et les versions
          exclusives de cette variante seront supprimés. Les versions et objets
          partagés restent conservés. Fermez les applications qui modifient ces
          fichiers avant de confirmer.
        </p>
        {!plan && !error && <p role="status">Analyse de la variante…</p>}
        {plan && !result && (
          <>
            <strong>{plan.variantName}</strong>
            <p>
              {plan.snapshotCount} version{plan.snapshotCount > 1 ? "s" : ""}{" "}
              exclusive{plan.snapshotCount > 1 ? "s" : ""} à supprimer ;{" "}
              {plan.retainedSnapshotCount} version
              {plan.retainedSnapshotCount > 1 ? "s" : ""} partagée
              {plan.retainedSnapshotCount > 1 ? "s" : ""} conservée
              {plan.retainedSnapshotCount > 1 ? "s" : ""}. Dossier de travail :{" "}
              {plan.workspaceBytesComplete ? "" : "au moins "}
              {formatBytes(plan.workspaceBytes)}.
            </p>
          </>
        )}
        {result && (
          <p role="status">
            {result.cleanupPending
              ? "Variante supprimée ; nettoyage de certains fichiers différé. Une reprise sera tentée à la réouverture de la bibliothèque."
              : `Variante supprimée. Espace libéré : ${formatBytes(result.releasedBytes)}.`}
          </p>
        )}
        {(result ?? plan)?.warnings.map((warning, index) => (
          <p className="field-help" key={index}>
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
              disabled={!plan || busy || !!error}
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
