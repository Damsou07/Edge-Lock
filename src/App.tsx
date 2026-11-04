import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type {
  Asset,
  AssetFilter,
  AssetPage,
  Bootstrap,
  Category,
  JobRequest,
  Taxonomy,
} from "./contracts";
import { Icon } from "./components/Icon";
import { Jobs } from "./components/Jobs";
import {
  CreateAssetModal,
  SettingsModal,
  TaxonomyModal,
} from "./components/AssetForms";
import { AssetDetailView } from "./components/AssetDetailView";
import { StorageOptimizationView } from "./components/StorageOptimizationView";
import { HelpModal } from "./components/HelpModal";
import { ArchiveDeletionModal } from "./components/ArchiveDeletionModal";
import { TaxonomyDeletionModal } from "./components/TaxonomyDeletionModal";
import type { TaxonomyDeletionTarget } from "./components/TaxonomyDeletionModal";
import {
  errorMessage,
  formatBytes,
  formatCount,
  formatDate,
} from "./lib/format";
import { useJobs } from "./lib/useJobs";
import type { RunJobOptions } from "./lib/useJobs";
import { useAssetThumbnails } from "./lib/useAssetThumbnails";
import type { ThumbnailState } from "./lib/useAssetThumbnails";

const initialFilter: AssetFilter = {
  search: "",
  categoryId: null,
  gameId: null,
  tag: null,
  sort: "updated",
  archived: false,
  offset: 0,
  limit: 36,
};
const emptyTaxonomy: Taxonomy = { categories: [], games: [], tags: [] };

function CategoryTree({
  categories,
  parentId = null,
  selected,
  onSelect,
  onDelete,
  depth = 0,
}: {
  categories: Category[];
  parentId?: string | null;
  selected: string | null;
  onSelect: (id: string) => void;
  onDelete: (target: TaxonomyDeletionTarget) => void;
  depth?: number;
}) {
  if (depth > 12) return null;
  return categories
    .filter((category) => category.parentId === parentId)
    .map((category) => (
      <div key={category.id}>
        <div className="nav-taxonomy-row">
          <button
            className={`nav-item category-item ${selected === category.id ? "active" : ""}`}
            style={{ paddingLeft: 15 + depth * 16 }}
            onClick={() => onSelect(category.id)}
          >
            <Icon name="folder" size={16} />
            <span>{category.name}</span>
          </button>
          <button
            className="icon-button taxonomy-delete"
            aria-label={`Supprimer la catégorie ${category.name}`}
            onClick={() =>
              onDelete({
                kind: "category",
                id: category.id,
                name: category.name,
              })
            }
          >
            <Icon name="trash" size={14} />
          </button>
        </div>
        <CategoryTree
          categories={categories}
          parentId={category.id}
          selected={selected}
          onSelect={onSelect}
          onDelete={onDelete}
          depth={depth + 1}
        />
      </div>
    ));
}

function AssetCard({
  asset,
  onSelect,
  list,
  thumbnail,
  setVisible,
  onImageError,
}: {
  asset: Asset;
  onSelect: (id: string) => void;
  list: boolean;
  thumbnail?: ThumbnailState;
  setVisible: (id: string, visible: boolean) => void;
  onImageError: (id: string) => Promise<void>;
}) {
  const card = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const element = card.current;
    if (!element) return;
    if (typeof IntersectionObserver === "undefined") {
      setVisible(asset.id, true);
      return () => setVisible(asset.id, false);
    }
    const observer = new IntersectionObserver(([entry]) => {
      setVisible(asset.id, !!entry?.isIntersecting);
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
      setVisible(asset.id, false);
    };
  }, [asset.id, setVisible]);
  return (
    <button
      ref={card}
      className={`asset-card ${list ? "asset-row" : ""}`}
      onClick={() => onSelect(asset.id)}
    >
      <div className="asset-thumbnail">
        {thumbnail?.url ? (
          <img
            src={thumbnail.url}
            alt={`Aperçu texturé de ${asset.name}`}
            loading="lazy"
            decoding="async"
            onError={() => void onImageError(asset.id)}
          />
        ) : (
          <div className="asset-wireframe">
            <Icon name="cube" size={list ? 26 : 64} />
          </div>
        )}
        {thumbnail?.status !== "ready" && (
          <span
            className={`thumbnail-status ${thumbnail?.status === "error" ? "thumbnail-error" : ""}`}
            title={thumbnail?.error}
          >
            {thumbnail?.status === "error"
              ? "Aperçu indisponible · ouvrir la fiche"
              : thumbnail?.status === "loading"
                ? "Préparation de l’aperçu…"
                : "Aperçu en attente"}
          </span>
        )}
        <span className="asset-type">3D ASSET</span>
        {asset.archived && <span className="archived-badge">Archivé</span>}
      </div>
      <div className="asset-card-content">
        <div className="asset-title">
          <h3>{asset.name}</h3>
          <Icon name="chevron" size={15} />
        </div>
        <div className="asset-tags">
          {asset.tags.length ? (
            asset.tags.slice(0, 3).map((tag) => <span key={tag}>{tag}</span>)
          ) : (
            <span className="untagged">Sans tag</span>
          )}
        </div>
        <div className="asset-card-stats">
          <span>
            <Icon name="database" size={13} />
            {formatBytes(asset.workingBytes)}
          </span>
          <span title="Triangles">
            <Icon name="mesh" size={13} />
            {formatCount(asset.triangles)}
          </span>
        </div>
      </div>
      <div className="asset-card-footer">
        <span>
          <Icon name="branch" size={13} />
          {asset.variantCount}{" "}
          {asset.variantCount > 1 ? "variantes" : "variante"}
        </span>
        <span>{formatDate(asset.updatedAt)}</span>
      </div>
    </button>
  );
}

export default function App() {
  const [bootstrap, setBootstrap] = useState<Bootstrap | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [opening, setOpening] = useState(false);
  const [taxonomy, setTaxonomy] = useState<Taxonomy>(emptyTaxonomy);
  const [filter, setFilter] = useState<AssetFilter>(initialFilter);
  const [search, setSearch] = useState("");
  const [page, setPage] = useState<AssetPage>({ items: [], total: 0 });
  const [assetsLoading, setAssetsLoading] = useState(false);
  const [view, setView] = useState<"grid" | "list">("grid");
  const [selected, setSelected] = useState<string | null>(null);
  const [section, setSection] = useState<"assets" | "storage">("assets");
  const navigation = useRef(0);
  const [modal, setModal] = useState<
    "import" | "create" | "settings" | "category" | "game" | "help" | null
  >(null);
  const [importSource, setImportSource] = useState("");
  const [archiveDeletion, setArchiveDeletion] = useState<{
    assetId?: string;
  } | null>(null);
  const [taxonomyDeletion, setTaxonomyDeletion] =
    useState<TaxonomyDeletionTarget | null>(null);
  const [generation, setGeneration] = useState(0);
  const { jobs, runJob: runBaseJob, dismiss } = useJobs();
  const library = bootstrap?.library;
  const {
    states: thumbnails,
    setVisible,
    imageFailed,
    pause,
    resume,
  } = useAssetThumbnails(
    page.items,
    library?.root,
    !!library &&
      section === "assets" &&
      !selected &&
      !modal &&
      !opening &&
      !archiveDeletion &&
      !taxonomyDeletion,
    runBaseJob,
  );
  const refresh = useCallback(() => setGeneration((value) => value + 1), []);
  useEffect(() => {
    if (!library?.root) return;
    let active = true;
    let unsubscribe: (() => void) | undefined;
    const settled = new Map<string, string>();
    const revisions = new Map<string, number>();
    const pending = new Set<string>();
    void api
      .onWorkspaceStatus((status) => {
        if (
          !active ||
          status.revision <= (revisions.get(status.variantId) ?? -1)
        )
          return;
        revisions.set(status.variantId, status.revision);
        if (status.state === "checking" || status.state === "syncing") {
          pending.add(status.variantId);
          return;
        }
        if (status.state !== "ready") return;
        const key = JSON.stringify([status.artifactKey, status.sourceChanged]);
        const checked = pending.delete(status.variantId);
        if (settled.get(status.variantId) === key && !checked) return;
        settled.set(status.variantId, key);
        refresh();
        void api
          .bootstrap()
          .then((value) => {
            if (active) setBootstrap(value);
          })
          .catch(() => {
            /* Catalog data remains available on metadata read failure. */
          });
      })
      .then((fn) => {
        if (active) unsubscribe = fn;
        else fn();
      })
      .catch(() => {
        /* Detail reads recover state if events are unavailable. */
      });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [library?.root, refresh]);
  const runJob = useCallback(
    async <T = unknown,>(
      request: JobRequest,
      options?: RunJobOptions,
    ): Promise<T> => {
      await pause();
      try {
        const result = await runBaseJob<T>(request, options);
        if (
          [
            "import",
            "create",
            "variant",
            "snapshot",
            "restore",
            "storageApply",
            "variantDeletionApply",
          ].includes(request.kind)
        ) {
          try {
            setBootstrap(await api.bootstrap());
          } catch (reason) {
            setError(errorMessage(reason));
          }
          refresh();
        }
        return result;
      } finally {
        resume();
      }
    },
    [runBaseJob, refresh, pause, resume],
  );
  const selectAsset = async (id: string) => {
    const intent = ++navigation.current;
    await pause();
    if (navigation.current === intent) {
      setSection("assets");
      setSelected(id);
    }
    resume();
  };

  useEffect(() => {
    let active = true;
    void api
      .bootstrap()
      .then((value) => {
        if (active) {
          setBootstrap(value);
          if (value.startupError) setError(value.startupError);
        }
      })
      .catch((reason) => {
        if (active) setError(errorMessage(reason));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);
  useEffect(() => {
    const timer = setTimeout(
      () =>
        setFilter((previous) =>
          previous.search === search
            ? previous
            : { ...previous, search, offset: 0 },
        ),
      220,
    );
    return () => clearTimeout(timer);
  }, [search]);
  useEffect(() => {
    if (!library) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    void api
      .onFolderDrop((paths) => {
        const path = paths[0];
        if (!active || !path) return;
        if (paths.length !== 1) {
          setError("Déposez un seul dossier à la fois.");
          return;
        }
        if (modal || archiveDeletion || taxonomyDeletion) {
          setError("Fermez le formulaire ouvert avant de déposer un dossier.");
          return;
        }
        setError("");
        setImportSource(path);
        setModal("import");
      })
      .then((cleanup) => {
        if (active) unlisten = cleanup;
        else cleanup();
      })
      .catch((reason) => {
        if (active) setError(errorMessage(reason));
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [library, modal, archiveDeletion, taxonomyDeletion]);
  useEffect(() => {
    if (!library) return;
    let active = true;
    void api
      .getTaxonomy()
      .then((value) => {
        if (active) setTaxonomy(value);
      })
      .catch((reason) => {
        if (active) setError(errorMessage(reason));
      });
    return () => {
      active = false;
    };
  }, [library, generation]);
  useEffect(() => {
    if (!library) return;
    let active = true;
    setAssetsLoading(true);
    void api
      .listAssets(filter)
      .then((value) => {
        if (active) setPage(value);
      })
      .catch((reason) => {
        if (active) setError(errorMessage(reason));
      })
      .finally(() => {
        if (active) setAssetsLoading(false);
      });
    return () => {
      active = false;
    };
  }, [library, filter, generation]);

  const openLibrary = async () => {
    ++navigation.current;
    setError("");
    setOpening(true);
    try {
      await pause();
      const path = await api.chooseFolder();
      if (path) {
        const value = await api.openLibrary(path);
        setBootstrap((previous) => ({
          settings: {
            libraryPath: path,
            blenderPath: previous?.settings.blenderPath ?? null,
          },
          library: value,
          startupError: null,
          version: previous?.version ?? "0.3.0",
        }));
        setSelected(null);
        setSection("assets");
        setFilter(initialFilter);
        setSearch("");
        refresh();
      }
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setOpening(false);
      resume();
    }
  };
  const changeFilter = (patch: Partial<AssetFilter>) => {
    ++navigation.current;
    setSection("assets");
    setFilter((previous) => ({ ...previous, ...patch, offset: 0 }));
    setSelected(null);
  };
  const resetFilters = () => {
    ++navigation.current;
    setSection("assets");
    setSearch("");
    setFilter(initialFilter);
    setSelected(null);
  };
  const selectedCategory = taxonomy.categories.find(
    (item) => item.id === filter.categoryId,
  );
  const selectedGame = taxonomy.games.find((item) => item.id === filter.gameId);
  const heading =
    selectedCategory?.name ??
    selectedGame?.name ??
    (filter.archived ? "Archives" : "Tous les assets");
  const hasFilters = !!(
    filter.search ||
    filter.categoryId ||
    filter.gameId ||
    filter.tag ||
    filter.archived
  );
  const hasRunningJobs = jobs.some(
    (job) =>
      job.kind !== "thumbnail" &&
      (job.status === "running" || job.status === "queued"),
  );

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <a
          className="brand"
          href="#"
          onClick={(event) => {
            event.preventDefault();
            resetFilters();
          }}
        >
          <img src="/edge-lock-icon.png" alt="" />
          <div>
            <strong>EDGE LOCK</strong>
            <span>YOUR CREATIVE VAULT</span>
          </div>
        </a>
        <div className="library-switch">
          <Icon name="database" size={17} />
          <div>
            <strong>Bibliothèque locale</strong>
            <span>
              {library
                ? library.root.split(/[\\/]/).filter(Boolean).at(-1)
                : "Aucun dossier ouvert"}
            </span>
          </div>
          <span className={`connection-dot ${library ? "connected" : ""}`} />
        </div>
        <nav className="sidebar-nav" aria-label="Navigation principale">
          <div className="nav-section-label">BIBLIOTHÈQUE</div>
          <button
            className={`nav-item ${section === "assets" && !filter.categoryId && !filter.gameId && !filter.archived && !selected ? "active" : ""}`}
            onClick={resetFilters}
          >
            <Icon name="grid" />
            <span>Tous les assets</span>
            {library && (
              <span className="nav-count">
                {!hasFilters ? page.total : library.assetCount}
              </span>
            )}
          </button>
          <div className="nav-section-heading">
            <span>CATÉGORIES</span>
            <button
              className="icon-button"
              aria-label="Créer une catégorie"
              disabled={!library}
              onClick={() => setModal("category")}
            >
              <Icon name="plus" size={15} />
            </button>
          </div>
          <CategoryTree
            categories={taxonomy.categories}
            selected={section === "assets" ? filter.categoryId : null}
            onDelete={setTaxonomyDeletion}
            onSelect={(id) =>
              changeFilter({ categoryId: id, gameId: null, archived: false })
            }
          />
          {!taxonomy.categories.length && (
            <p className="sidebar-hint">Donnez une place à chaque création.</p>
          )}
          <div className="nav-section-heading">
            <span>PROJETS DE JEU</span>
            <button
              className="icon-button"
              aria-label="Créer un projet de jeu"
              disabled={!library}
              onClick={() => setModal("game")}
            >
              <Icon name="plus" size={15} />
            </button>
          </div>
          {taxonomy.games.map((game) => (
            <div key={game.id} className="nav-taxonomy-row">
              <button
                className={`nav-item ${section === "assets" && filter.gameId === game.id ? "active" : ""}`}
                onClick={() =>
                  changeFilter({
                    gameId: game.id,
                    categoryId: null,
                    archived: false,
                  })
                }
              >
                <Icon name="game" size={17} />
                <span>{game.name}</span>
              </button>
              <button
                className="icon-button taxonomy-delete"
                aria-label={`Supprimer le projet de jeu ${game.name}`}
                onClick={() =>
                  setTaxonomyDeletion({
                    kind: "game",
                    id: game.id,
                    name: game.name,
                  })
                }
              >
                <Icon name="trash" size={14} />
              </button>
            </div>
          ))}
          {!taxonomy.games.length && (
            <p className="sidebar-hint">Retrouvez les assets de vos jeux.</p>
          )}
          <div className="nav-divider" />
          <button
            className={`nav-item ${section === "assets" && filter.archived ? "active" : ""}`}
            onClick={() =>
              changeFilter({ archived: true, gameId: null, categoryId: null })
            }
          >
            <Icon name="archive" size={17} />
            <span>Archives</span>
          </button>
          <button
            className={`nav-item storage-nav-item ${section === "storage" ? "active" : ""}`}
            disabled={!library || opening}
            onClick={() => {
              ++navigation.current;
              setSelected(null);
              setSection("storage");
            }}
          >
            <Icon name="database" size={17} />
            <span>Optimisation du stockage</span>
          </button>
        </nav>
        <div className="sidebar-bottom">
          <div className="storage-card">
            <Icon name="database" size={17} />
            <div>
              <strong>
                {library
                  ? `${library.physicalBytesComplete === false ? "Au moins " : ""}${formatBytes(library.physicalBytes)} sur disque`
                  : "Votre espace, vos fichiers."}
              </strong>
              <span>
                {library
                  ? "Total de la bibliothèque"
                  : "Stockage local · Aucun cloud"}
              </span>
            </div>
          </div>
          <button
            className="nav-item help-nav-item"
            onClick={() => setModal("help")}
          >
            <Icon name="help" size={17} />
            <span>Aide et mode d’emploi</span>
          </button>
          <button className="nav-item" onClick={() => setModal("settings")}>
            <Icon name="settings" size={17} />
            <span>Réglages</span>
            <span className="version">v{bootstrap?.version ?? "0.3.0"}</span>
          </button>
        </div>
      </aside>
      <main className="main-area">
        <header className="topbar">
          <div className="breadcrumb">
            <Icon name="folder" size={16} />
            <span>Bibliothèque</span>
            <Icon name="chevron" size={13} />
            <strong>
              {section === "storage"
                ? "Optimisation du stockage"
                : selected
                  ? "Fiche de l’asset"
                  : heading}
            </strong>
          </div>
          <div className="local-status">
            <span className="live-dot" />
            Tout reste sur votre ordinateur
          </div>
        </header>
        {error && (
          <div className="error-banner" role="alert">
            <Icon name="warning" />
            <span>{error}</span>
            <button
              className="icon-button"
              aria-label="Masquer l’erreur"
              onClick={() => setError("")}
            >
              <Icon name="close" size={16} />
            </button>
          </div>
        )}
        {library?.recoveryNotices.map((notice) => (
          <div className="notice-banner" key={notice}>
            <Icon name="history" size={16} />
            {notice}
          </div>
        ))}
        {loading ? (
          <div className="page-loader">
            <Icon name="refresh" size={28} className="spinning" />
            <p>Ouverture de votre espace…</p>
          </div>
        ) : !library ? (
          <section className="onboarding">
            <div className="onboarding-art">
              <img src="/edge-lock-icon.png" alt="Cube Edge Lock" />
              <div className="art-caption">
                <span className="live-dot" />
                UN ESPACE POUR CRÉER
              </div>
            </div>
            <div className="onboarding-content">
              <span className="eyebrow">BIENVENUE DANS EDGE LOCK</span>
              <h1>
                Vos créations,
                <br />
                au même endroit.
              </h1>
              <p>
                Centralisez vos assets 3D, gardez chaque version et explorez vos
                variantes. Vos fichiers restent de vrais dossiers, prêts pour
                Blender.
              </p>
              <div className="onboarding-points">
                <span>
                  <Icon name="branch" />
                  Des variantes indépendantes
                </span>
                <span>
                  <Icon name="history" />
                  Un historique qui préserve votre travail
                </span>
                <span>
                  <Icon name="database" />
                  Une bibliothèque entièrement locale
                </span>
              </div>
              <button
                className="button primary large"
                disabled={opening}
                onClick={() => void openLibrary()}
              >
                <Icon
                  name={opening ? "refresh" : "folder"}
                  className={opening ? "spinning" : ""}
                />
                {opening ? "Ouverture…" : "Choisir un dossier"}
                <Icon name="arrow" size={16} />
              </button>
              <p className="onboarding-note">
                Choisissez un dossier vide pour commencer, ou une bibliothèque
                Edge Lock existante.
              </p>
            </div>
          </section>
        ) : section === "storage" ? (
          <StorageOptimizationView
            key={library.root}
            libraryRoot={library.root}
            jobs={jobs}
            runJob={runJob}
          />
        ) : selected ? (
          <AssetDetailView
            id={selected}
            taxonomy={taxonomy}
            generation={generation}
            onBack={() => setSelected(null)}
            onChanged={refresh}
            onDeleteRequested={() => setArchiveDeletion({ assetId: selected })}
            onError={setError}
            runJob={runJob}
          />
        ) : (
          <section className="library-content">
            <div className="page-heading">
              <div>
                <span className="eyebrow">VOTRE ESPACE DE CRÉATION</span>
                <h1>
                  {heading}
                  <span className="heading-count">{page.total}</span>
                </h1>
                <p>
                  {selectedGame
                    ? "Les créations qui donnent vie à votre univers."
                    : "Chaque asset. Chaque version. Toujours à portée de main."}
                </p>
              </div>
              <div className="heading-actions">
                {filter.archived && (
                  <button
                    className="button danger"
                    onClick={() => setArchiveDeletion({})}
                  >
                    <Icon name="trash" />
                    Vider les archives
                  </button>
                )}
                <button
                  className="button secondary"
                  onClick={() => setModal("import")}
                >
                  <Icon name="import" />
                  Importer
                </button>
                <button
                  className="button primary"
                  onClick={() => setModal("create")}
                >
                  <Icon name="plus" />
                  Nouvel asset
                </button>
              </div>
            </div>
            <div className="library-toolbar">
              <div className="search-field">
                <Icon name="search" size={18} />
                <input
                  type="search"
                  aria-label="Rechercher des assets"
                  value={search}
                  placeholder="Rechercher un asset…"
                  onChange={(event) => setSearch(event.target.value)}
                />
                {search && (
                  <button
                    className="icon-button"
                    aria-label="Effacer la recherche"
                    onClick={() => setSearch("")}
                  >
                    <Icon name="close" size={14} />
                  </button>
                )}
              </div>
              <select
                aria-label="Filtrer par tag"
                value={filter.tag ?? ""}
                onChange={(event) =>
                  changeFilter({ tag: event.target.value || null })
                }
              >
                <option value="">Tous les tags</option>
                {taxonomy.tags.map((tag) => (
                  <option value={tag} key={tag}>
                    {tag}
                  </option>
                ))}
              </select>
              <select
                aria-label="Trier les assets"
                value={filter.sort}
                onChange={(event) =>
                  changeFilter({
                    sort: event.target.value as AssetFilter["sort"],
                  })
                }
              >
                <option value="updated">Récemment modifiés</option>
                <option value="name">Nom A → Z</option>
                <option value="size">Taille du dossier</option>
                <option value="triangles">Nombre de triangles</option>
              </select>
              <div
                className="view-switch"
                role="group"
                aria-label="Mode d’affichage"
              >
                <button
                  className={view === "grid" ? "selected" : ""}
                  aria-label="Vue grille"
                  aria-pressed={view === "grid"}
                  onClick={() => setView("grid")}
                >
                  <Icon name="grid" size={17} />
                </button>
                <button
                  className={view === "list" ? "selected" : ""}
                  aria-label="Vue liste"
                  aria-pressed={view === "list"}
                  onClick={() => setView("list")}
                >
                  <Icon name="list" size={18} />
                </button>
              </div>
            </div>
            {hasFilters && (
              <div className="active-filters">
                <span>
                  {page.total} {page.total === 1 ? "résultat" : "résultats"}
                </span>
                {filter.tag && (
                  <span className="tag-chip">
                    <Icon name="tag" size={12} />
                    {filter.tag}
                  </span>
                )}
                <button className="text-button" onClick={resetFilters}>
                  Réinitialiser les filtres
                </button>
              </div>
            )}
            <div
              className={
                assetsLoading ? "asset-results is-loading" : "asset-results"
              }
              aria-busy={assetsLoading}
            >
              {page.items.length ? (
                <div className={view === "grid" ? "asset-grid" : "asset-list"}>
                  {page.items.map((asset) => (
                    <AssetCard
                      asset={asset}
                      key={asset.id}
                      list={view === "list"}
                      onSelect={(id) => void selectAsset(id)}
                      thumbnail={thumbnails[asset.id]}
                      setVisible={setVisible}
                      onImageError={imageFailed}
                    />
                  ))}
                </div>
              ) : (
                <div className="empty-state">
                  <div className="empty-icon">
                    <Icon name={hasFilters ? "search" : "cube"} size={34} />
                  </div>
                  <span className="eyebrow">
                    {hasFilters ? "UN PEU PLUS LOIN ?" : "TOUT COMMENCE ICI"}
                  </span>
                  <h2>
                    {hasFilters
                      ? "Aucun asset trouvé"
                      : "La place de votre prochaine création."}
                  </h2>
                  <p>
                    {hasFilters
                      ? "Essayez un autre nom, un autre tag ou retirez les filtres."
                      : "Importez un dossier existant ou créez votre premier asset. Ses fichiers et son historique seront réunis ici."}
                  </p>
                  <button
                    className="button secondary"
                    onClick={
                      hasFilters ? resetFilters : () => setModal("import")
                    }
                  >
                    <Icon name={hasFilters ? "refresh" : "import"} />
                    {hasFilters
                      ? "Réinitialiser les filtres"
                      : "Importer mon premier asset"}
                  </button>
                </div>
              )}
            </div>
            {page.total > filter.limit && (
              <div className="pagination">
                <span>
                  {filter.offset + 1}–
                  {Math.min(filter.offset + filter.limit, page.total)} sur{" "}
                  {page.total}
                </span>
                <button
                  className="button secondary small-button"
                  disabled={filter.offset === 0}
                  onClick={() =>
                    setFilter((previous) => ({
                      ...previous,
                      offset: Math.max(0, previous.offset - previous.limit),
                    }))
                  }
                >
                  Précédent
                </button>
                <button
                  className="button secondary small-button"
                  disabled={filter.offset + filter.limit >= page.total}
                  onClick={() =>
                    setFilter((previous) => ({
                      ...previous,
                      offset: previous.offset + previous.limit,
                    }))
                  }
                >
                  Suivant
                </button>
              </div>
            )}
            <div className="library-footer">
              <span>
                <Icon name="check" size={13} />
                Dossiers accessibles dans l’Explorateur et Blender
              </span>
              <button
                className="text-button"
                disabled={opening || hasRunningJobs}
                onClick={() => void openLibrary()}
              >
                Ouvrir une autre bibliothèque
              </button>
            </div>
          </section>
        )}
      </main>
      <Jobs jobs={jobs} dismiss={dismiss} onError={setError} />
      {archiveDeletion && (
        <ArchiveDeletionModal
          key={library?.root}
          {...archiveDeletion}
          runJob={runJob}
          onClose={() => setArchiveDeletion(null)}
          onDeleted={() => {
            setSelected(null);
            setFilter((previous) => ({ ...previous, offset: 0 }));
            refresh();
            void api
              .bootstrap()
              .then(setBootstrap)
              .catch((reason: unknown) => setError(errorMessage(reason)));
          }}
        />
      )}
      {taxonomyDeletion && (
        <TaxonomyDeletionModal
          target={taxonomyDeletion}
          onClose={() => setTaxonomyDeletion(null)}
          onDeleted={() => {
            resetFilters();
            refresh();
            void api
              .bootstrap()
              .then(setBootstrap)
              .catch((reason: unknown) => setError(errorMessage(reason)));
          }}
        />
      )}
      {modal === "help" && <HelpModal onClose={() => setModal(null)} />}
      {(modal === "import" || modal === "create") && (
        <CreateAssetModal
          mode={modal}
          initialSource={modal === "import" ? importSource : ""}
          runJob={runJob}
          onClose={() => {
            setModal(null);
            setImportSource("");
          }}
        />
      )}
      {(modal === "category" || modal === "game") && (
        <TaxonomyModal
          kind={modal}
          taxonomy={taxonomy}
          onClose={() => setModal(null)}
          onSaved={refresh}
        />
      )}
      {modal === "settings" && (
        <SettingsModal
          settings={
            bootstrap?.settings ?? { libraryPath: null, blenderPath: null }
          }
          libraryPath={library?.root ?? null}
          version={bootstrap?.version ?? "0.3.0"}
          onClose={() => setModal(null)}
          onChanged={(path) =>
            setBootstrap((previous) =>
              previous
                ? {
                    ...previous,
                    settings: { ...previous.settings, blenderPath: path },
                  }
                : previous,
            )
          }
        />
      )}
    </div>
  );
}
