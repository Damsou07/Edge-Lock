import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { FormEvent } from "react";
import { api } from "../api";
import type {
  AppError,
  AssetDetail,
  ChangeSet,
  PreviewResult,
  Snapshot,
  StorageAction,
  Taxonomy,
  VariantDetail,
} from "../contracts";
import { defaultExportConfig } from "../contracts";
import { useWorkspaceStatus } from "../lib/useWorkspaceStatus";
import {
  errorMessage,
  extension,
  formatBytes,
  formatCount,
  formatDate,
} from "../lib/format";
import type { RunJob } from "../lib/useJobs";
import { Icon } from "./Icon";
import { MetadataModal } from "./AssetForms";
import { Modal } from "./Modal";
import { HistoryStorageModal, RestoreBackupModal } from "./HistoryStorageModal";

const ModelViewer = lazy(() => import("./ModelViewer"));

function releasePreview(url: string) {
  const token = new URL(url).pathname.slice(1);
  void api.releasePreviewResources([token]).catch(() => {
    /* Resource cleanup must not interrupt navigation. */
  });
}

function VersionModal({
  mode,
  snapshotId,
  variantId,
  runJob,
  onClose,
  allowed = true,
}: {
  mode: "snapshot" | "variant";
  snapshotId: string;
  variantId: string;
  runJob: RunJob;
  onClose: () => void;
  allowed?: boolean;
}) {
  const [value, setValue] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!allowed) return;
    setBusy(true);
    setError("");
    try {
      if (mode === "snapshot")
        await runJob({ kind: "snapshot", variantId, message: value.trim() });
      else await runJob({ kind: "variant", snapshotId, name: value.trim() });
      onClose();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title={mode === "snapshot" ? "Créer une version" : "Créer une variante"}
      subtitle={
        mode === "snapshot"
          ? "Un point de repère dans l’histoire de votre asset."
          : "Un dossier indépendant, à partir de cette version."
      }
      onClose={onClose}
    >
      <form className="form-stack" onSubmit={(event) => void submit(event)}>
        <label>
          {mode === "snapshot" ? "Qu’avez-vous changé ?" : "Nom de la variante"}
          <input
            autoFocus
            required
            maxLength={mode === "snapshot" ? 500 : 120}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder={
              mode === "snapshot"
                ? "Ex. Nouvelles textures et ajustement du rig"
                : "Ex. Texture désert"
            }
          />
        </label>
        <div className="callout">
          <Icon name={mode === "snapshot" ? "save" : "branch"} />
          <p>
            {mode === "snapshot"
              ? "Enregistrez d’abord vos changements dans Blender. Edge Lock capture les fichiers présents sur le disque."
              : "Les fichiers de travail sont copiés dans un nouveau dossier. L’historique partage les fichiers identiques."}
          </p>
        </div>
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        <footer className="modal-actions">
          <button type="button" className="button secondary" onClick={onClose}>
            {busy ? "Fermer" : "Annuler"}
          </button>
          <button
            className="button primary"
            disabled={busy || !value.trim() || !allowed}
          >
            <Icon
              name={busy ? "refresh" : mode === "snapshot" ? "save" : "branch"}
              className={busy ? "spinning" : ""}
            />
            {busy
              ? "En cours…"
              : mode === "snapshot"
                ? "Créer la version"
                : "Créer la variante"}
          </button>
        </footer>
      </form>
    </Modal>
  );
}

export function AssetDetailView({
  id,
  taxonomy,
  generation,
  onBack,
  onChanged,
  onError,
  runJob,
}: {
  id: string;
  taxonomy: Taxonomy;
  generation: number;
  onBack: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
  runJob: RunJob;
}) {
  const [detail, setDetail] = useState<AssetDetail | null>(null);
  const [variantId, setVariantId] = useState("");
  const [variantDetail, setVariantDetail] = useState<VariantDetail | null>(
    null,
  );
  const [snapshotId, setSnapshotId] = useState<string | null>(null);
  const [snapshotDetail, setSnapshotDetail] = useState<Snapshot | null>(null);
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [historyTotal, setHistoryTotal] = useState(0);
  const [historyBusy, setHistoryBusy] = useState(false);
  const [changes, setChanges] = useState<ChangeSet | null>(null);
  const [comparedKey, setComparedKey] = useState("");
  const [changesBusy, setChangesBusy] = useState(false);
  const [preview, setPreview] = useState<PreviewResult | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [previewError, setPreviewError] = useState("");
  const [previewCandidates, setPreviewCandidates] = useState<string[]>([]);
  const [previewFile, setPreviewFile] = useState("");
  const [localRefresh, setLocalRefresh] = useState(0);
  const [tab, setTab] = useState<"files" | "history" | "changes">("files");
  const [fileLimit, setFileLimit] = useState(100);
  const [modal, setModal] = useState<
    "metadata" | "snapshot" | "variant" | null
  >(null);
  const [copyDone, setCopyDone] = useState(false);
  const [replacePath, setReplacePath] = useState<string | null>(null);
  const [exportBusy, setExportBusy] = useState(false);
  const [storageAction, setStorageAction] = useState<{
    action: StorageAction;
    snapshotId?: string;
  } | null>(null);
  const [restoreOpen, setRestoreOpen] = useState(false);
  const selection = [id, variantId, snapshotId ?? "current"].join(":");
  const currentSelection = useRef(selection);
  currentSelection.current = selection;
  const alive = useRef(true);
  const previewRequest = useRef(0);
  const changesRequest = useRef(0);
  const autoPreviewAttempt = useRef("");
  const workspace = useWorkspaceStatus(variantId);
  const currentWorkspaceStatus = workspace.current;
  const status = workspace.status;
  const settledRevision = workspace.settledRevision;
  const comparisonKey = [
    variantId,
    generation,
    localRefresh,
    settledRevision,
  ].join(":");
  const syncBusy =
    (!status && !workspace.error) ||
    status?.state === "checking" ||
    status?.state === "syncing";

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      previewRequest.current += 1;
      changesRequest.current += 1;
    };
  }, []);
  useEffect(() => {
    const url = preview?.url;
    return () => {
      if (url) releasePreview(url);
    };
  }, [preview?.url]);
  useEffect(() => {
    let active = true;
    void api
      .getAsset(id)
      .then((value) => {
        if (active) {
          setDetail(value);
          setVariantId((previous) =>
            value.variants.some((variant) => variant.id === previous)
              ? previous
              : (value.variants[0]?.id ?? ""),
          );
        }
      })
      .catch((reason) => {
        if (active) onError(errorMessage(reason));
      });
    return () => {
      active = false;
    };
  }, [id, generation, localRefresh, onError]);
  useEffect(() => {
    if (!variantId) return;
    let active = true;
    void api
      .getVariant(variantId)
      .then((value) => {
        if (active) setVariantDetail(value);
      })
      .catch((reason) => {
        if (active) onError(errorMessage(reason));
      });
    return () => {
      active = false;
    };
  }, [variantId, generation, localRefresh, settledRevision, onError]);
  useEffect(() => {
    if (!variantId) return;
    let active = true;
    setHistoryBusy(true);
    void api
      .listSnapshots(variantId, 0)
      .then((value) => {
        if (active) {
          setSnapshots(value.items);
          setHistoryTotal(value.total);
        }
      })
      .catch((reason) => {
        if (active) onError(errorMessage(reason));
      })
      .finally(() => {
        if (active) setHistoryBusy(false);
      });
    return () => {
      active = false;
    };
  }, [variantId, generation, localRefresh, onError]);
  useEffect(() => {
    setSnapshotDetail(null);
    if (!snapshotId) return;
    let active = true;
    void api
      .getSnapshot(snapshotId)
      .then((value) => {
        if (active) setSnapshotDetail(value);
      })
      .catch((reason) => {
        if (active) onError(errorMessage(reason));
      });
    return () => {
      active = false;
    };
  }, [snapshotId, onError]);
  useEffect(() => {
    previewRequest.current += 1;
    autoPreviewAttempt.current = "";
    setPreview(null);
    setPreviewBusy(false);
    setPreviewError("");
    setPreviewCandidates([]);
    setPreviewFile("");
    setFileLimit(100);
  }, [selection]);
  useEffect(() => {
    if (!variantId) return;
    let active = true;
    const request = ++changesRequest.current;
    setChangesBusy(true);
    setChanges(null);
    void runJob<ChangeSet>({ kind: "changes", variantId })
      .then((value) => {
        if (active && changesRequest.current === request) {
          setChanges(value);
          setComparedKey(comparisonKey);
        }
      })
      .catch((reason) => {
        if (active && changesRequest.current === request)
          onError(errorMessage(reason));
      })
      .finally(() => {
        if (active && changesRequest.current === request) setChangesBusy(false);
      });
    return () => {
      active = false;
    };
  }, [
    variantId,
    generation,
    localRefresh,
    settledRevision,
    comparisonKey,
    runJob,
    onError,
  ]);
  const selectedSnapshot =
    snapshotDetail?.id === snapshotId
      ? snapshotDetail
      : snapshots.find((snapshot) => snapshot.id === snapshotId);
  const files = useMemo(
    () =>
      snapshotId
        ? (selectedSnapshot?.entries ?? [])
        : (variantDetail?.files ?? []),
    [snapshotId, selectedSnapshot?.entries, variantDetail?.files],
  );
  const activeConfig =
    selectedSnapshot?.config ??
    variantDetail?.variant.config ??
    defaultExportConfig;
  const pendingCount = changes
    ? changes.added.length +
      changes.modified.length +
      changes.deleted.length +
      Number(changes.configChanged)
    : null;
  const dirty = status?.sourceChanged ?? false;
  const canCreateSnapshot =
    !!variantDetail &&
    !snapshotId &&
    !changesBusy &&
    comparedKey === comparisonKey &&
    pendingCount != null &&
    pendingCount > 0 &&
    !workspace.error &&
    status?.state === "ready" &&
    status.canCreateSnapshot;
  const displayInspection = preview?.inspection;
  const warnings = preview?.inspection.warnings ?? [];
  const dependencies = (preview?.inspection.dependencies ?? []).filter(
    (dependency) =>
      dependency.renderRelevant !== false &&
      !["internal", "packed", "embedded", "ok"].includes(dependency.status),
  );
  const previewDegraded = dependencies.some(
    (dependency) => dependency.status === "missing",
  );
  const syncError = workspace.error || status?.error?.message || "";
  const showFailure = (reason: unknown) => onError(errorMessage(reason));
  const generatePreview = useCallback(
    async (selectedFile = "") => {
      const target = selection;
      const artifact = snapshotId
        ? null
        : currentWorkspaceStatus.current?.artifactKey;
      const request = ++previewRequest.current;
      const isCurrent = () =>
        alive.current &&
        currentSelection.current === target &&
        previewRequest.current === request &&
        (snapshotId ||
          currentWorkspaceStatus.current?.artifactKey === artifact);
      setPreviewBusy(true);
      setPreviewError("");
      setPreviewFile(selectedFile);
      try {
        const result = await runJob<PreviewResult>({
          kind: "preview",
          variantId,
          snapshotId,
          ...(snapshotId && selectedFile ? { previewFile: selectedFile } : {}),
        });
        if (isCurrent()) {
          setPreview(result);
          setPreviewCandidates([]);
        } else releasePreview(result.url);
      } catch (reason) {
        if (!isCurrent()) return;
        const error = reason as AppError | null;
        const candidates = error?.details?.candidates;
        if (
          snapshotId &&
          error?.code === "preview_ambiguous" &&
          Array.isArray(candidates) &&
          candidates.length > 0 &&
          candidates.every((candidate) => typeof candidate === "string")
        ) {
          setPreviewCandidates(candidates);
          setPreviewFile("");
        } else setPreviewError(errorMessage(reason));
      } finally {
        if (
          alive.current &&
          currentSelection.current === target &&
          previewRequest.current === request
        )
          setPreviewBusy(false);
      }
    },
    [selection, snapshotId, variantId, runJob, currentWorkspaceStatus],
  );
  const autoPreviewKey = snapshotId
    ? selection
    : status?.state === "ready" && status.artifactKey
      ? [selection, status.artifactKey].join(":")
      : "";
  useEffect(() => {
    if (
      !variantDetail ||
      !autoPreviewKey ||
      autoPreviewAttempt.current === autoPreviewKey
    )
      return;
    autoPreviewAttempt.current = autoPreviewKey;
    void generatePreview();
  }, [variantDetail, autoPreviewKey, generatePreview]);
  const refreshChanges = async () => {
    setLocalRefresh((value) => value + 1);
    if (!snapshotId) await workspace.retry();
  };
  const exportTo = async (destination: string, overwrite: boolean) => {
    setExportBusy(true);
    try {
      await runJob({
        kind: "export",
        variantId,
        snapshotId,
        destination,
        overwrite,
      });
      setReplacePath(null);
    } catch (reason) {
      if ((reason as AppError)?.code === "destination_exists")
        setReplacePath(destination);
      else showFailure(reason);
    } finally {
      setExportBusy(false);
    }
  };
  const exportGlb = async () => {
    try {
      const destination = await api.chooseExportPath(
        `${detail?.asset.name ?? "asset"}-${variantDetail?.variant.name ?? "principal"}.glb`,
      );
      if (destination) await exportTo(destination, false);
    } catch (reason) {
      showFailure(reason);
    }
  };
  const copyPath = async () => {
    if (!variantDetail) return;
    try {
      await api.copyText(variantDetail.variant.workspacePath);
      setCopyDone(true);
      setTimeout(() => setCopyDone(false), 1800);
    } catch (reason) {
      showFailure(reason);
    }
  };
  const loadMoreHistory = async () => {
    const target = variantId;
    setHistoryBusy(true);
    try {
      const next = await api.listSnapshots(target, snapshots.length);
      if (currentSelection.current.startsWith(`${id}:${target}:`)) {
        setSnapshots((previous) => [
          ...previous,
          ...next.items.filter(
            (item) => !previous.some((existing) => existing.id === item.id),
          ),
        ]);
        setHistoryTotal(next.total);
      }
    } catch (reason) {
      showFailure(reason);
    } finally {
      setHistoryBusy(false);
    }
  };

  if (!detail)
    return (
      <div className="page-loader">
        <Icon name="refresh" className="spinning" size={26} />
        <p>Ouverture de l’asset…</p>
      </div>
    );
  const asset = detail.asset;
  const variant = variantDetail?.variant;
  const category = taxonomy.categories.find(
    (item) => item.id === asset.categoryId,
  );
  const openSnapshot = (snapshot: Snapshot) => {
    setSnapshotId(snapshot.id);
    setTab("history");
  };

  return (
    <section className="detail-content">
      <button className="back-link" onClick={onBack}>
        <Icon name="back" size={16} />
        Retour à la bibliothèque
      </button>
      <div className="page-heading detail-heading">
        <div>
          <div className="asset-kicker">
            <span className="eyebrow">
              {category?.name ?? "SANS CATÉGORIE"}
            </span>
            {asset.archived && <span className="tag-chip">Archivé</span>}
          </div>
          <h1>{asset.name}</h1>
          <div className="detail-tags">
            {asset.tags.map((tag) => (
              <span className="tag-chip" key={tag}>
                <Icon name="tag" size={11} />
                {tag}
              </span>
            ))}
            {asset.gameIds.map((gameId) => (
              <span className="game-chip" key={gameId}>
                <Icon name="game" size={13} />
                {taxonomy.games.find((game) => game.id === gameId)?.name ??
                  "Projet"}
              </span>
            ))}
          </div>
        </div>
        <div className="heading-actions">
          <button
            className="button secondary"
            onClick={() => setModal("metadata")}
          >
            <Icon name="organize" />
            Organiser
          </button>
          <button
            className="button primary"
            disabled={
              !variant ||
              exportBusy ||
              (!snapshotId && status?.state !== "ready")
            }
            onClick={() => void exportGlb()}
          >
            <Icon
              name={exportBusy ? "refresh" : "export"}
              className={exportBusy ? "spinning" : ""}
            />
            Exporter .glb
          </button>
        </div>
      </div>
      <div className="variant-bar">
        <div className="variant-selector">
          <Icon name="branch" size={17} />
          <label htmlFor="variant-select">Variante</label>
          <select
            id="variant-select"
            value={variantId}
            onChange={(event) => {
              setVariantId(event.target.value);
              setSnapshotId(null);
              setVariantDetail(null);
              setSnapshots([]);
            }}
          >
            {detail.variants.map((item) => (
              <option value={item.id} key={item.id}>
                {item.name}
              </option>
            ))}
          </select>
        </div>
        <button
          className="text-button"
          disabled={!variant || selectedSnapshot?.storage?.kind === "preview"}
          onClick={() => setModal("variant")}
        >
          <Icon name="plus" size={15} />
          Nouvelle variante
        </button>
        <span className="variant-bar-spacer" />
        <span className={`workspace-state ${dirty ? "dirty" : ""}`}>
          <span className="live-dot" />
          {snapshotId
            ? "Version enregistrée"
            : syncBusy || changesBusy
              ? status?.state === "syncing"
                ? "Synchronisation…"
                : "Vérification…"
              : status?.state === "nonconforming"
                ? "Projet non conforme"
                : status?.state === "failed"
                  ? "Synchronisation interrompue"
                  : dirty
                    ? "Modifications non versionnées"
                    : "À jour"}
        </span>
      </div>
      {snapshotId && (
        <div className="history-banner">
          <Icon name="history" />
          <div>
            <strong>
              Version du {formatDate(selectedSnapshot?.createdAt)}
            </strong>
            <span>{selectedSnapshot?.message}</span>
          </div>
          <button
            className="button secondary small-button"
            onClick={() => setSnapshotId(null)}
          >
            Revenir au travail en cours
          </button>
        </div>
      )}
      <div className="asset-workbench">
        <div className="preview-column">
          <div className="preview-panel">
            {preview ? (
              <Suspense
                fallback={
                  <div className="preview-loading">
                    <Icon name="refresh" className="spinning" />
                    Préparation de la visionneuse…
                  </div>
                }
              >
                <ModelViewer
                  url={preview.url}
                  label={
                    snapshotId ? "VERSION ENREGISTRÉE" : "DOSSIER DE TRAVAIL"
                  }
                />
              </Suspense>
            ) : (
              <div className="preview-placeholder">
                <div className="preview-grid" />
                <div className="preview-object">
                  <Icon name="cube" size={82} />
                </div>
                <h2>Votre asset, sous tous les angles.</h2>
                <p>
                  {previewBusy
                    ? "Préparation de l’aperçu local…"
                    : "Explorez le modèle et ses animations sans quitter votre bibliothèque."}
                </p>
                <button
                  className="button secondary"
                  disabled={
                    !variant ||
                    previewBusy ||
                    (!snapshotId && status?.state !== "ready") ||
                    previewCandidates.length > 0
                  }
                  onClick={() => void generatePreview(previewFile)}
                >
                  <Icon
                    name={previewBusy ? "refresh" : "play"}
                    className={previewBusy ? "spinning" : ""}
                  />
                  {previewBusy
                    ? "Génération en cours…"
                    : "Afficher l’aperçu 3D"}
                </button>
                <span className="preview-format">
                  GLB · ROTATION · ZOOM · ANIMATIONS
                </span>
              </div>
            )}
            {preview && (
              <div className="preview-caption">
                <span title={preview.sourceFile}>
                  {syncBusy && !snapshotId
                    ? "Synchronisation du modèle…"
                    : `${preview.sourceLabel} · ${preview.sourceFile}`}
                </span>
                <button
                  className="text-button"
                  disabled={
                    previewBusy || (!snapshotId && status?.state !== "ready")
                  }
                  onClick={() => void generatePreview(previewFile)}
                >
                  <Icon
                    name="refresh"
                    size={13}
                    className={previewBusy ? "spinning" : ""}
                  />
                  Actualiser
                </button>
              </div>
            )}
          </div>
          {previewCandidates.length > 0 && (
            <form
              className="preview-source-choice"
              onSubmit={(event) => {
                event.preventDefault();
                void generatePreview(previewFile);
              }}
            >
              <label>
                GLB de prévisualisation
                <select
                  value={previewFile}
                  disabled={previewBusy}
                  onChange={(event) => setPreviewFile(event.target.value)}
                >
                  <option value="">Choisir un fichier GLB</option>
                  {previewCandidates.map((candidate) => (
                    <option key={candidate} value={candidate}>
                      {candidate}
                    </option>
                  ))}
                </select>
              </label>
              <button
                className="button secondary small-button"
                disabled={
                  !previewFile ||
                  previewBusy ||
                  (!snapshotId && status?.state !== "ready")
                }
              >
                Utiliser ce GLB
              </button>
            </form>
          )}
          {previewError && (
            <div className="inline-error preview-notice" role="alert">
              <strong>Aperçu indisponible</strong>
              <p>{previewError}</p>
              <p>
                Le fichier source reste accessible depuis l’espace de travail.
              </p>
            </div>
          )}
          {previewDegraded && (
            <div className="preview-notice preview-degraded" role="status">
              <Icon name="warning" size={16} />
              <span>
                Aperçu dégradé : certaines ressources de rendu sont absentes.
                Consultez la section Dépendances.
              </span>
            </div>
          )}
          <div className="asset-metrics">
            <div>
              <Icon name="database" />
              <span>{snapshotId ? "Cette version" : "Dossier de travail"}</span>
              <strong>
                {formatBytes(
                  snapshotId
                    ? selectedSnapshot?.logicalBytes
                    : (changes?.workingBytes ?? variant?.workingBytes),
                )}
              </strong>
            </div>
            <div>
              <Icon name="history" />
              <span title="Contenu unique de l’historique de toutes les variantes, avant compression Zstd">
                Historique
              </span>
              <strong>{formatBytes(asset.historyBytes)}</strong>
            </div>
            <div>
              <Icon name="mesh" />
              <span>Triangles</span>
              <strong>
                {formatCount(
                  displayInspection?.geometry.triangles ??
                    (snapshotId ? null : variant?.triangles),
                )}
              </strong>
            </div>
            <div>
              <Icon name="cube" />
              <span>Objets maillés</span>
              <strong>
                {formatCount(displayInspection?.geometry.meshObjects)}
              </strong>
            </div>
          </div>
          {displayInspection && (
            <div className="inspection-note">
              <span>
                {formatCount(displayInspection.geometry.vertices)} sommets
              </span>
              <span
                title={
                  displayInspection.geometry.edges == null
                    ? "Le nombre d’arêtes n’est pas disponible pour cette analyse."
                    : undefined
                }
              >
                {formatCount(displayInspection.geometry.edges)} arêtes
              </span>
              <span>{formatCount(displayInspection.geometry.faces)} faces</span>
              <span>
                {displayInspection.blenderVersion.startsWith("GLB")
                  ? displayInspection.blenderVersion
                  : `Blender ${displayInspection.blenderVersion}`}
              </span>
              <span className="inspection-provenance">
                Géométrie du modèle exporté · {preview?.sourceFile}
              </span>
            </div>
          )}
        </div>
        <aside className="workspace-panel">
          <div className="panel-heading">
            <h2>Espace de travail</h2>
            <Icon name="folder" size={17} />
          </div>
          <p className="muted small">
            Un dossier réel pour Blender et vos agents IA.
          </p>
          <div className="workspace-path">
            <code>{variant?.workspacePath ?? "Chargement…"}</code>
            <button
              className="icon-button"
              aria-label="Copier le chemin du dossier"
              title="Copier le chemin"
              disabled={!variant}
              onClick={() => void copyPath()}
            >
              <Icon name={copyDone ? "check" : "copy"} size={16} />
            </button>
          </div>
          <div className="workspace-actions">
            <button
              className="button secondary"
              disabled={!variant}
              onClick={() =>
                void api.openWorkspace(variantId).catch(showFailure)
              }
            >
              <Icon name="folder" />
              Ouvrir le dossier
            </button>
            <button
              className="button blender-button"
              disabled={!variant || !!snapshotId}
              title={
                snapshotId
                  ? "Créez une variante pour ouvrir cette version dans Blender."
                  : "Ouvrir le fichier principal"
              }
              onClick={() => void api.openBlender(variantId).catch(showFailure)}
            >
              <Icon name="open" />
              Ouvrir dans Blender
            </button>
          </div>
          <div className="panel-divider" />
          {!snapshotId && syncBusy && (
            <p className="field-help" role="status">
              {status?.state === "syncing"
                ? "Mise à jour du GLB et de l’aperçu…"
                : "Vérification des fichiers…"}
            </p>
          )}
          {!snapshotId && syncError && (
            <div className="inline-error" role="alert">
              <p>{syncError}</p>
              {status?.state !== "nonconforming" && (
                <button
                  type="button"
                  className="text-button"
                  onClick={() => void workspace.retry()}
                >
                  Réessayer la synchronisation
                </button>
              )}
            </div>
          )}
          <div className="panel-divider" />
          <div className="version-callout">
            <Icon name="history" size={21} />
            <div>
              <strong>Gardez une trace de vos avancées.</strong>
              <p>
                Enregistrez dans Blender : le GLB et l’aperçu se mettent à jour
                automatiquement.
              </p>
            </div>
          </div>
          <button
            className="button primary full-width"
            disabled={!canCreateSnapshot}
            onClick={() => setModal("snapshot")}
          >
            <Icon name="save" />
            Créer une version
          </button>
        </aside>
      </div>
      {warnings.length > 0 && (
        <section
          className="inspection-warnings"
          aria-label="Informations d’analyse"
        >
          <Icon name="warning" />
          <div>
            <h2>Informations d’analyse</h2>
            {warnings.map((warning) => (
              <p key={warning}>{warning}</p>
            ))}
          </div>
        </section>
      )}
      {dependencies.length > 0 && (
        <section className="inspection-warnings" aria-label="Dépendances">
          <Icon name="warning" />
          <div>
            <h2>Dépendances</h2>
            {dependencies.map((dependency) => (
              <p key={dependency.path}>
                <code>{dependency.path}</code> —{" "}
                {dependency.status === "missing"
                  ? "manquante"
                  : dependency.status === "external"
                    ? "externe"
                    : dependency.status}
              </p>
            ))}
          </div>
        </section>
      )}
      <div className="asset-bottom-panel">
        <div
          className="asset-tabs"
          role="tablist"
          aria-label="Contenu de l’asset"
        >
          <button
            role="tab"
            aria-selected={tab === "files"}
            className={tab === "files" ? "active" : ""}
            onClick={() => setTab("files")}
          >
            <Icon name="folder" size={16} />
            Fichiers
            <span>{files.filter((file) => file.kind === "file").length}</span>
          </button>
          <button
            role="tab"
            aria-selected={tab === "history"}
            className={tab === "history" ? "active" : ""}
            onClick={() => setTab("history")}
          >
            <Icon name="history" size={16} />
            Historique<span>{historyTotal}</span>
          </button>
          <button
            role="tab"
            aria-selected={tab === "changes"}
            className={tab === "changes" ? "active" : ""}
            onClick={() => setTab("changes")}
          >
            <Icon name="diff" size={16} />
            Modifications
            {pendingCount != null && (
              <span className={pendingCount > 0 ? "changes-count" : ""}>
                {pendingCount}
              </span>
            )}
          </button>
          <span className="tab-spacer" />
          <button
            className="text-button refresh-files"
            disabled={changesBusy}
            onClick={() => void refreshChanges()}
          >
            <Icon
              name="refresh"
              size={14}
              className={changesBusy ? "spinning" : ""}
            />
            Actualiser
          </button>
        </div>
        {tab === "files" && (
          <div role="tabpanel" className="file-panel">
            {files.length ? (
              <>
                <div className="file-table-header">
                  <span>NOM</span>
                  <span>TYPE</span>
                  <span>TAILLE</span>
                </div>
                {files.slice(0, fileLimit).map((file) => (
                  <div className="file-row" key={file.path}>
                    <span>
                      <Icon
                        name={file.kind === "directory" ? "folder" : "file"}
                        size={17}
                      />
                      <code>{file.path}</code>
                      {file.path === activeConfig.primaryFile && (
                        <span className="primary-file-badge">Principal</span>
                      )}
                    </span>
                    <span>
                      {file.kind === "directory"
                        ? "Dossier"
                        : extension(file.path)}
                    </span>
                    <span>
                      {file.kind === "directory" ? "—" : formatBytes(file.size)}
                    </span>
                  </div>
                ))}
                {files.length > fileLimit && (
                  <button
                    className="button secondary load-more"
                    onClick={() => setFileLimit((value) => value + 100)}
                  >
                    Afficher les 100 suivants
                  </button>
                )}
              </>
            ) : (
              <div className="small-empty">
                <Icon name="folder" size={24} />
                <p>Ce dossier est prêt à accueillir vos fichiers.</p>
              </div>
            )}
          </div>
        )}
        {tab === "history" && (
          <div role="tabpanel" className="history-panel">
            <div className="history-storage-actions">
              <button
                className="button secondary small-button"
                onClick={() => setStorageAction({ action: "recompress" })}
              >
                Compresser avec Zstd
              </button>
              <button
                className="button secondary small-button"
                onClick={() => setStorageAction({ action: "compact" })}
              >
                Alléger l’ancien historique
              </button>
              <button
                className="button secondary small-button"
                onClick={() => setStorageAction({ action: "purge" })}
              >
                Libérer les objets inutilisés
              </button>
              {variant?.backupSnapshotId && (
                <>
                  <button
                    className="button secondary small-button"
                    onClick={() => setRestoreOpen(true)}
                  >
                    Restaurer le backup
                  </button>
                  <button
                    className="button secondary small-button"
                    onClick={() =>
                      setStorageAction({ action: "releaseBackup" })
                    }
                  >
                    Libérer le backup complet
                  </button>
                </>
              )}
            </div>
            {snapshots.length ? (
              snapshots.map((snapshot) => (
                <div
                  className={`history-row ${snapshotId === snapshot.id ? "selected" : ""}`}
                  key={snapshot.id}
                >
                  <div className="timeline-point">
                    <span />
                  </div>
                  <div className="history-description">
                    <div>
                      <strong>{snapshot.message}</strong>
                      {snapshot.id === variant?.headSnapshotId && (
                        <span className="head-badge">Dernière version</span>
                      )}
                      {snapshot.id === variant?.backupSnapshotId && (
                        <span className="head-badge">Backup complet</span>
                      )}
                      {snapshot.storage?.kind === "preview" && (
                        <span className="head-badge">Consultation seule</span>
                      )}
                      {snapshot.storage?.kind === "full" &&
                        !snapshot.storage.protected && (
                          <span className="head-badge">
                            {snapshot.storage.legacy
                              ? "Historique complet conservé"
                              : "Complet · conversion en attente"}
                          </span>
                        )}
                      {snapshot.storage?.previewTrusted === false && (
                        <span className="head-badge">Aperçu non certifié</span>
                      )}
                    </div>
                    <span>
                      {formatDate(snapshot.createdAt)}
                      <i>·</i>
                      {formatBytes(snapshot.logicalBytes)}
                      <i>·</i>
                      <code>{snapshot.id.slice(0, 8)}</code>
                    </span>
                  </div>
                  <button
                    className="button secondary small-button"
                    disabled={snapshot.storage?.previewTrusted === false}
                    onClick={() => openSnapshot(snapshot)}
                  >
                    Consulter
                    <Icon name="chevron" size={13} />
                  </button>
                  {snapshot.storage?.kind === "preview" &&
                    !snapshot.storage.protected && (
                      <button
                        className="button secondary small-button"
                        onClick={() =>
                          setStorageAction({
                            action: "deleteVersion",
                            snapshotId: snapshot.id,
                          })
                        }
                      >
                        Supprimer
                      </button>
                    )}
                </div>
              ))
            ) : (
              <div className="small-empty">
                <Icon name="history" size={24} />
                <p>
                  {historyBusy
                    ? "Chargement de l’historique…"
                    : "Aucune version enregistrée."}
                </p>
              </div>
            )}
            {snapshots.length < historyTotal && (
              <button
                className="button secondary load-more"
                disabled={historyBusy}
                onClick={() => void loadMoreHistory()}
              >
                {historyBusy
                  ? "Chargement…"
                  : "Charger les versions précédentes"}
              </button>
            )}
          </div>
        )}
        {tab === "changes" && (
          <div role="tabpanel" className="changes-panel">
            <p className="changes-description">
              Comparaison du dossier de travail avec sa dernière version
              enregistrée.
            </p>
            {changes && pendingCount === 0 ? (
              <div className="small-empty">
                <Icon name="check" size={25} />
                <p>
                  Tout est à jour. Votre dernière version correspond aux
                  fichiers sur le disque.
                </p>
              </div>
            ) : changes ? (
              <>
                {changes.configChanged && (
                  <div className="change-row modified">
                    <span>Modifié</span>
                    <code>Configuration de l’asset</code>
                  </div>
                )}
                {(["added", "modified", "deleted"] as const).flatMap((kind) =>
                  changes[kind].map((path) => (
                    <div
                      className={`change-row ${kind}`}
                      key={`${kind}-${path}`}
                    >
                      <span>
                        {kind === "added"
                          ? "Ajouté"
                          : kind === "deleted"
                            ? "Supprimé"
                            : "Modifié"}
                      </span>
                      <code>{path}</code>
                    </div>
                  )),
                )}
              </>
            ) : (
              <div className="small-empty">
                <Icon
                  name="refresh"
                  size={24}
                  className={changesBusy ? "spinning" : ""}
                />
                <p>
                  {changesBusy
                    ? "Analyse des fichiers…"
                    : "Actualisez pour comparer les fichiers."}
                </p>
              </div>
            )}
          </div>
        )}
      </div>
      {modal === "metadata" && (
        <MetadataModal
          asset={asset}
          taxonomy={taxonomy}
          onClose={() => setModal(null)}
          onSaved={onChanged}
        />
      )}
      {storageAction && variant && (
        <HistoryStorageModal
          key={`${variant.id}:${storageAction.action}:${storageAction.snapshotId ?? ""}`}
          variantId={variant.id}
          {...storageAction}
          runJob={runJob}
          onSaved={() => {
            setSnapshotId(null);
            setLocalRefresh((value) => value + 1);
            onChanged();
          }}
          onClose={() => setStorageAction(null)}
        />
      )}
      {restoreOpen && variant && (
        <RestoreBackupModal
          key={variant.id}
          variant={variant}
          runJob={runJob}
          onRestored={() => {
            setSnapshotId(null);
            setLocalRefresh((value) => value + 1);
            onChanged();
          }}
          onClose={() => setRestoreOpen(false)}
        />
      )}
      {(modal === "snapshot" || modal === "variant") && variant && (
        <VersionModal
          mode={modal}
          variantId={variantId}
          snapshotId={snapshotId ?? variant.headSnapshotId}
          runJob={runJob}
          allowed={modal !== "snapshot" || canCreateSnapshot}
          onClose={() => setModal(null)}
        />
      )}
      {replacePath && (
        <Modal
          title="Remplacer le fichier existant ?"
          onClose={() => setReplacePath(null)}
        >
          <div className="form-stack">
            <p>
              Un fichier existe déjà à cet emplacement. Le remplacement sera
              effectué uniquement après un export réussi.
            </p>
            <code className="path-text">{replacePath}</code>
            <footer className="modal-actions">
              <button
                className="button secondary"
                onClick={() => setReplacePath(null)}
              >
                Annuler
              </button>
              <button
                className="button primary"
                disabled={exportBusy}
                onClick={() => void exportTo(replacePath, true)}
              >
                Remplacer et exporter
              </button>
            </footer>
          </div>
        </Modal>
      )}
    </section>
  );
}
