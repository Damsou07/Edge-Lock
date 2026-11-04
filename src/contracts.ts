export interface ExportConfig {
  primaryFile: string | null;
  generatedGlbFile?: string | null;
  scene: string | null;
  collection: string | null;
  applyModifiers: boolean;
}
export const defaultExportConfig: ExportConfig = {
  primaryFile: null,
  scene: null,
  collection: null,
  applyModifiers: true,
};
export interface Asset {
  id: string;
  name: string;
  categoryId: string | null;
  tags: string[];
  gameIds: string[];
  archived: boolean;
  createdAt: string;
  updatedAt: string;
  workingBytes: number;
  historyBytes: number;
  triangles: number | null;
  variantCount: number;
}
export interface Variant {
  id: string;
  assetId: string;
  name: string;
  workspacePath: string;
  headSnapshotId: string;
  backupSnapshotId?: string | null;
  workingBytes: number;
  triangles: number | null;
  config: ExportConfig;
}
export interface ManifestEntry {
  path: string;
  kind: "file" | "directory";
  hash: string | null;
  size: number;
}
export interface Snapshot {
  storage?: {
    kind: "full" | "preview" | "deleted";
    previewPath: string | null;
    restoredFrom: string | null;
    protected: boolean;
    legacy: boolean;
    previewTrusted: boolean;
  };
  id: string;
  variantId: string;
  parentId: string | null;
  createdAt: string;
  message: string;
  logicalBytes: number;
  config: ExportConfig;
  entries: ManifestEntry[];
}
export interface AssetDetail {
  asset: Asset;
  variants: Variant[];
}
export interface VariantDetail {
  variant: Variant;
  snapshots: Snapshot[];
  files: ManifestEntry[];
}
export interface ChangeSet {
  added: string[];
  modified: string[];
  deleted: string[];
  configChanged: boolean;
  workingBytes: number;
}
export interface Category {
  id: string;
  name: string;
  parentId: string | null;
}
export interface Game {
  id: string;
  name: string;
}
export interface Taxonomy {
  categories: Category[];
  games: Game[];
  tags: string[];
}
export interface LibraryInfo {
  root: string;
  assetCount: number;
  physicalBytes: number;
  physicalBytesComplete?: boolean;
  recoveryNotices: string[];
}
export interface AssetFilter {
  search: string;
  categoryId: string | null;
  gameId: string | null;
  tag: string | null;
  sort: "name" | "updated" | "size" | "triangles";
  archived: boolean;
  offset: number;
  limit: number;
}
export interface AssetPage {
  items: Asset[];
  total: number;
}
export interface SnapshotPage {
  items: Snapshot[];
  total: number;
}
export interface Settings {
  libraryPath: string | null;
  blenderPath: string | null;
}
export interface Bootstrap {
  settings: Settings;
  library: LibraryInfo | null;
  startupError: string | null;
  version: string;
}
export interface Geometry {
  meshObjects: number;
  vertices: number;
  edges?: number | null;
  faces: number;
  triangles: number;
}
export interface Dependency {
  path: string;
  status: string;
  kind?: string;
  renderRelevant?: boolean;
}
export interface Inspection {
  geometry: Geometry;
  scenes: string[];
  collections: string[];
  dependencies: Dependency[];
  warnings: string[];
  blenderVersion: string;
}
export interface PreviewResult {
  url: string;
  artifactKey?: string;
  inspection: Inspection;
  sourceLabel: string;
  sourceFile: string;
  sourceKind: "assetGlb" | "generatedGlb";
}
export interface ThumbnailResource {
  url: string;
  resourceToken: string;
}
export type ThumbnailResult =
  | ({ state: "ready" } & ThumbnailResource)
  | { state: "render"; preview: PreviewResult; resourceToken: string };
export interface AppError {
  code: string;
  message: string;
  details?: { candidates?: string[] } | null;
}
export interface WorkspaceStatus {
  variantId: string;
  revision: number;
  state: "checking" | "syncing" | "ready" | "failed" | "nonconforming";
  sourceChanged: boolean;
  canCreateSnapshot: boolean;
  artifactKey: string | null;
  error: AppError | null;
}
export type JobRequest =
  | { kind: "archiveDeletionPlan"; assetId?: string }
  | { kind: "archiveDeletionApply"; assetId?: string; planToken: string }
  | { kind: "storageDiagnostic" }
  | {
      kind: "storagePlan";
      variantId?: string;
      action: StorageAction;
      snapshotId?: string;
    }
  | {
      kind: "storageApply";
      variantId?: string;
      action: StorageAction;
      snapshotId?: string;
      planToken: string;
    }
  | {
      kind: "restore";
      variantId: string;
      expectedHead: string;
      expectedBackup: string;
    }
  | { kind: "discardChanges"; variantId: string; expectedHead: string }
  | { kind: "thumbnail"; assetId: string }
  | { kind: "import"; source: string; name: string; config: ExportConfig }
  | { kind: "create"; name: string }
  | { kind: "snapshot"; variantId: string; message: string }
  | { kind: "variant"; snapshotId: string; name: string }
  | { kind: "changes"; variantId: string }
  | { kind: "inspect"; variantId: string; snapshotId?: string | null }
  | {
      kind: "preview";
      variantId: string;
      snapshotId?: string | null;
      previewFile?: string;
    }
  | {
      kind: "export";
      variantId: string;
      snapshotId?: string | null;
      sourceFile?: string;
      destination: string;
      overwrite: boolean;
    };
export interface Job {
  id: string;
  kind: JobRequest["kind"];
  status: "queued" | "running" | "completed" | "failed" | "cancelled";
  phase: string;
  completed: number;
  total: number | null;
  result: unknown;
  error: AppError | null;
}

export interface ArchiveDeletionPlan {
  planToken: string;
  assets: { id: string; name: string }[];
  assetIds: string[];
  variantIds: string[];
  assetCount: number;
  variantCount: number;
  snapshotCount: number;
  workspaceBytes: number;
  workspaceBytesComplete?: boolean;
  warnings: string[];
}
export interface WorkspaceDiscardReport {
  variantId: string;
  headSnapshotId: string;
  cleanupPending: boolean;
  warnings: string[];
}
export interface ArchiveDeletionReport {
  deletedAssetIds: string[];
  deletedVariantIds: string[];
  releasedBytes: number;
  cleanupPending: boolean;
  warnings: string[];
}

export type StorageAction =
  "compact" | "releaseBackup" | "deleteVersion" | "recompress" | "purge";
export interface StorageReport {
  planToken: string;
  logicalBytes: number;
  physicalBytes: number;
  reclaimableBytes: number;
  releasedBytes: number;
  versions: { id: string; message: string }[];
  warnings: string[];
}

export interface StorageMeasure {
  fileCount: number;
  bytes: number;
}
export interface StorageDiagnostic {
  startedAt: string;
  completedAt: string;
  complete: boolean;
  total: StorageMeasure;
  categories: (StorageMeasure & { key: string; label: string })[];
  details: {
    key: string;
    label: string;
    count: number;
    bytes: number;
    examples: { path: string; size: number }[];
  }[];
  logicalObjectBytes: number;
  purgeAvailable: boolean;
  purgeBlockedReason: string | null;
  warnings: string[];
}
