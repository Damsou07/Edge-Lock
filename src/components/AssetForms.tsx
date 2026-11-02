import { useState } from "react";
import type { FormEvent } from "react";
import { api } from "../api";
import type { Asset, Settings, Taxonomy } from "../contracts";
import { defaultExportConfig } from "../contracts";
import { errorMessage } from "../lib/format";
import type { RunJob } from "../lib/useJobs";
import { Icon } from "./Icon";
import { Modal } from "./Modal";

export function CreateAssetModal({
  mode,
  initialSource = "",
  runJob,
  onClose,
}: {
  mode: "import" | "create";
  initialSource?: string;
  runJob: RunJob;
  onClose: () => void;
}) {
  const [name, setName] = useState(
    () => initialSource.split(/[\\/]/).filter(Boolean).at(-1) ?? "",
  );
  const [source, setSource] = useState(initialSource);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const choose = async () => {
    try {
      const path = await api.chooseFolder();
      if (path) {
        setSource(path);
        if (!name) setName(path.split(/[\\/]/).filter(Boolean).at(-1) ?? "");
      }
    } catch (reason) {
      setError(errorMessage(reason));
    }
  };
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      if (mode === "import")
        await runJob({
          kind: "import",
          source,
          name: name.trim(),
          config: defaultExportConfig,
        });
      else await runJob({ kind: "create", name: name.trim() });
      onClose();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title={mode === "import" ? "Importer un asset" : "Créer un asset"}
      subtitle={
        mode === "import"
          ? "Tout votre dossier, réuni dans la bibliothèque."
          : "Un espace de travail prêt à ouvrir dans Blender."
      }
      onClose={onClose}
    >
      <form onSubmit={(event) => void submit(event)} className="form-stack">
        {mode === "import" && (
          <>
            <label>
              Dossier source
              <div className="input-action">
                <input
                  value={source}
                  onChange={(event) => setSource(event.target.value)}
                  required
                  placeholder="C:\\Mes créations\\Robot"
                />
                <button
                  type="button"
                  className="button secondary"
                  onClick={() => void choose()}
                >
                  <Icon name="folder" />
                  Parcourir
                </button>
              </div>
            </label>
            <p className="field-help">
              Une copie complète est créée dans Edge Lock. Le dossier source
              reste intact. Un seul fichier .blend est requis ; son GLB est
              généré automatiquement avec les modificateurs.
            </p>
          </>
        )}
        <label>
          Nom de l’asset
          <input
            autoFocus
            value={name}
            onChange={(event) => setName(event.target.value)}
            required
            maxLength={120}
            placeholder="Ex. Robot explorateur"
          />
        </label>
        {mode === "create" && (
          <div className="callout">
            <Icon name="folder" />
            <p>
              Edge Lock crée le dossier de l’asset, sa variante principale et un
              fichier Blender vide. Configurez Blender dans les réglages si
              nécessaire.
            </p>
          </div>
        )}
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        <footer className="modal-actions">
          <button className="button secondary" type="button" onClick={onClose}>
            {busy ? "Fermer" : "Annuler"}
          </button>
          <button
            className="button primary"
            disabled={
              busy || !name.trim() || (mode === "import" && !source.trim())
            }
          >
            <Icon
              name={busy ? "refresh" : mode === "import" ? "import" : "plus"}
              className={busy ? "spinning" : ""}
            />
            {busy
              ? "En cours…"
              : mode === "import"
                ? "Importer le dossier"
                : "Créer l’asset"}
          </button>
        </footer>
      </form>
    </Modal>
  );
}

export function TaxonomyModal({
  kind,
  taxonomy,
  onClose,
  onSaved,
}: {
  kind: "category" | "game";
  taxonomy: Taxonomy;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState("");
  const [parent, setParent] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    try {
      if (kind === "category")
        await api.saveCategory(name.trim(), parent || null);
      else await api.saveGame(name.trim());
      onSaved();
      onClose();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title={
        kind === "category" ? "Nouvelle catégorie" : "Nouveau projet de jeu"
      }
      onClose={onClose}
    >
      <form className="form-stack" onSubmit={(event) => void submit(event)}>
        <label>
          Nom
          <input
            autoFocus
            required
            maxLength={120}
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={
              kind === "category" ? "Ex. Personnages" : "Ex. Echoes of Mars"
            }
          />
        </label>
        {kind === "category" && (
          <label>
            Catégorie parente
            <select
              value={parent}
              onChange={(event) => setParent(event.target.value)}
            >
              <option value="">Aucune — à la racine</option>
              {taxonomy.categories.map((category) => (
                <option key={category.id} value={category.id}>
                  {category.name}
                </option>
              ))}
            </select>
          </label>
        )}
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        <footer className="modal-actions">
          <button type="button" className="button secondary" onClick={onClose}>
            Annuler
          </button>
          <button className="button primary" disabled={busy || !name.trim()}>
            Créer
          </button>
        </footer>
      </form>
    </Modal>
  );
}

export function MetadataModal({
  asset,
  taxonomy,
  onClose,
  onSaved,
}: {
  asset: Asset;
  taxonomy: Taxonomy;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(asset.name);
  const [category, setCategory] = useState(asset.categoryId ?? "");
  const [tags, setTags] = useState(asset.tags.join(", "));
  const [games, setGames] = useState(asset.gameIds);
  const [archived, setArchived] = useState(asset.archived);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    try {
      await api.updateAsset({
        id: asset.id,
        name: name.trim(),
        categoryId: category || null,
        tags: [
          ...new Set(
            tags
              .split(",")
              .map((tag) => tag.trim())
              .filter(Boolean),
          ),
        ],
        gameIds: games,
        archived,
      });
      onSaved();
      onClose();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title="Organiser l’asset"
      subtitle="Les fichiers et les versions conservent leurs chemins."
      onClose={onClose}
    >
      <form className="form-stack" onSubmit={(event) => void submit(event)}>
        <label>
          Nom
          <input
            autoFocus
            required
            maxLength={120}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <label>
          Catégorie
          <select
            value={category}
            onChange={(event) => setCategory(event.target.value)}
          >
            <option value="">Sans catégorie</option>
            {taxonomy.categories.map((item) => (
              <option key={item.id} value={item.id}>
                {item.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Tags
          <input
            value={tags}
            onChange={(event) => setTags(event.target.value)}
            placeholder="robot, sci-fi, personnage"
          />
          <span className="field-help">Séparez les tags par des virgules.</span>
        </label>
        <fieldset>
          <legend>Projets de jeu</legend>
          {taxonomy.games.length ? (
            <div className="checkbox-grid">
              {taxonomy.games.map((game) => (
                <label className="checkbox-label" key={game.id}>
                  <input
                    type="checkbox"
                    checked={games.includes(game.id)}
                    onChange={(event) =>
                      setGames(
                        event.target.checked
                          ? [...games, game.id]
                          : games.filter((id) => id !== game.id),
                      )
                    }
                  />
                  {game.name}
                </label>
              ))}
            </div>
          ) : (
            <p className="muted small">
              Créez d’abord un projet dans la barre latérale.
            </p>
          )}
        </fieldset>
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={archived}
            onChange={(event) => setArchived(event.target.checked)}
          />
          Archiver cet asset{" "}
          <span className="optional">Les données sont conservées.</span>
        </label>
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        <footer className="modal-actions">
          <button className="button secondary" type="button" onClick={onClose}>
            Annuler
          </button>
          <button className="button primary" disabled={busy || !name.trim()}>
            Enregistrer
          </button>
        </footer>
      </form>
    </Modal>
  );
}

export function SettingsModal({
  settings,
  libraryPath,
  onClose,
  onChanged,
  version,
}: {
  settings: Settings;
  libraryPath: string | null;
  onClose: () => void;
  onChanged: (path: string) => void;
  version: string;
}) {
  const [path, setPath] = useState(settings.blenderPath ?? "");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const save = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const detected = await api.setBlenderPath(path.trim());
      setMessage(`Blender ${detected} est prêt.`);
      onChanged(path.trim());
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };
  const choose = async () => {
    try {
      const selected = await api.chooseBlenderPath();
      if (selected) setPath(selected);
    } catch (reason) {
      setError(errorMessage(reason));
    }
  };
  return (
    <Modal
      title="Réglages"
      subtitle="Votre environnement de création, en local."
      onClose={onClose}
    >
      <form className="form-stack" onSubmit={(event) => void save(event)}>
        <div className="settings-section">
          <h3>
            <Icon name="cube" />
            Blender
          </h3>
          <p className="muted small">
            Nécessaire pour les nouveaux fichiers .blend, l’analyse et l’export.
          </p>
          <label>
            Exécutable Blender
            <div className="input-action">
              <input
                value={path}
                required
                onChange={(event) => setPath(event.target.value)}
                placeholder="C:\\…\\blender.exe"
              />
              <button
                className="button secondary"
                type="button"
                onClick={() => void choose()}
              >
                <Icon name="folder" />
              </button>
            </div>
          </label>
          <button className="button secondary" disabled={busy || !path.trim()}>
            {busy ? "Vérification…" : "Vérifier et enregistrer"}
          </button>
          {message && (
            <div className="inline-success" role="status">
              <Icon name="check" />
              {message}
            </div>
          )}
          {error && (
            <div className="inline-error" role="alert">
              {error}
            </div>
          )}
        </div>
        <div className="settings-section">
          <h3>
            <Icon name="database" />
            Bibliothèque
          </h3>
          <p className="path-text">
            {libraryPath ?? "Aucune bibliothèque ouverte"}
          </p>
          <p className="muted small">
            Pour sauvegarder vos données, fermez Edge Lock et copiez le dossier
            complet de la bibliothèque sur un autre disque.
          </p>
        </div>
        <footer className="modal-actions">
          <span className="muted small">
            Edge Lock {version} · Stockage local
          </span>
          <button type="button" className="button primary" onClick={onClose}>
            Terminé
          </button>
        </footer>
      </form>
    </Modal>
  );
}
