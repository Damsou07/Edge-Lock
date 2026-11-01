export interface ExportConfig {
  primaryFile: string | null;
  scene: string | null;
  collection: string | null;
  applyModifiers: boolean;
}
export const defaultExportConfig: ExportConfig = {
  primaryFile: null,
  scene: null,
  collection: null,
  applyModifiers: false,
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
  faces: number;
  triangles: number;
}
export interface Inspection {
  geometry: Geometry;
  scenes: string[];
  collections: string[];
  dependencies: { path: string; status: string }[];
  warnings: string[];
  blenderVersion: string;
}
export interface PreviewResult {
  url: string;
  inspection: Inspection;
  sourceLabel: string;
}
export interface AppError {
  code: string;
  message: string;
}
export type JobRequest =
  | { kind: "import"; source: string; name: string; config: ExportConfig }
  | { kind: "create"; name: string }
  | { kind: "snapshot"; variantId: string; message: string }
  | { kind: "variant"; snapshotId: string; name: string }
  | { kind: "changes"; variantId: string }
  | { kind: "inspect"; variantId: string; snapshotId?: string | null }
  | { kind: "preview"; variantId: string; snapshotId?: string | null }
  | {
      kind: "export";
      variantId: string;
      snapshotId?: string | null;
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
