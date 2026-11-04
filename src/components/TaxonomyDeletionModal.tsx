import { useEffect, useState } from "react";
import { api } from "../api";
import { errorMessage } from "../lib/format";
import { Modal } from "./Modal";

export interface TaxonomyDeletionTarget {
  kind: "category" | "game";
  id: string;
  name: string;
}
export function TaxonomyDeletionModal({
  target,
  onClose,
  onDeleted,
}: {
  target: TaxonomyDeletionTarget;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const [count, setCount] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    const filter = {
      search: "",
      categoryId: target.kind === "category" ? target.id : null,
      gameId: target.kind === "game" ? target.id : null,
      tag: null,
      sort: "name" as const,
      offset: 0,
      limit: 1,
    };
    void Promise.all(
      [false, true].map((archived) => api.listAssets({ ...filter, archived })),
    )
      .then((pages) => {
        if (active)
          setCount(pages.reduce((total, page) => total + page.total, 0));
      })
      .catch((reason: unknown) => {
        if (active) setError(errorMessage(reason));
      });
    return () => {
      active = false;
    };
  }, [target]);
  const apply = async () => {
    setBusy(true);
    setError("");
    try {
      if (target.kind === "category") await api.deleteCategory(target.id);
      else await api.deleteGame(target.id);
      onDeleted();
      onClose();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title={`Supprimer ${target.name} ?`}
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <div className="form-stack">
        <p>
          {target.kind === "category"
            ? "La catégorie et toutes ses sous-catégories seront supprimées. Leurs assets seront placés dans les archives, avec leurs fichiers et leurs versions."
            : "Le projet de jeu sera supprimé. Tous ses assets seront archivés, même s’ils appartiennent à d’autres projets ; leurs autres associations seront conservées."}
        </p>
        {count === null ? (
          <p role="status">Analyse des assets concernés…</p>
        ) : (
          <p>
            {count} asset{count > 1 ? "s" : ""} concerné{count > 1 ? "s" : ""}.
          </p>
        )}
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
            className="button danger"
            disabled={busy || count === null}
            onClick={() => void apply()}
          >
            Supprimer et archiver
          </button>
        </footer>
      </div>
    </Modal>
  );
}
