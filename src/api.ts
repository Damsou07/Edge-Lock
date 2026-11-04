import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type {
  AssetDetail,
  AssetFilter,
  AssetPage,
  Bootstrap,
  ExportConfig,
  FileSelections,
  Job,
  JobRequest,
  LibraryInfo,
  Snapshot,
  Taxonomy,
  ThumbnailResource,
  VariantDetail,
  WorkspaceStatus,
} from "./contracts";

export const api = {
  bootstrap: () => invoke<Bootstrap>("get_bootstrap"),
  chooseFolder: () => invoke<string | null>("choose_folder"),
  chooseExportPath: (suggestedName: string) =>
    invoke<string | null>("choose_export_path", { suggestedName }),
  chooseBlenderPath: () => invoke<string | null>("choose_blender_path"),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  openLibrary: (path: string) => invoke<LibraryInfo>("open_library", { path }),
  listAssets: (filter: AssetFilter) =>
    invoke<AssetPage>("list_assets", { filter }),
  getAsset: (id: string) => invoke<AssetDetail>("get_asset", { id }),
  getAssetThumbnails: (assetIds: string[]) =>
    invoke<Record<string, ThumbnailResource>>("get_asset_thumbnails", {
      assetIds,
    }),
  storeThumbnail: (previewToken: string, pngBytes: number[]) =>
    invoke<ThumbnailResource>("store_thumbnail", { previewToken, pngBytes }),
  releasePreviewResources: (tokens: string[]) =>
    invoke<void>("release_preview_resources", { tokens }),
  invalidateThumbnail: (resourceToken: string) =>
    invoke<void>("invalidate_thumbnail", { resourceToken }),
  getVariant: (id: string) => invoke<VariantDetail>("get_variant", { id }),
  getWorkspaceStatus: (variantId: string) =>
    invoke<WorkspaceStatus>("get_workspace_status", { variantId }),
  requestWorkspaceSync: (variantId: string) =>
    invoke<WorkspaceStatus>("request_workspace_sync", { variantId }),
  getSnapshot: (id: string) => invoke<Snapshot>("get_snapshot", { id }),
  listSnapshots: (variantId: string, offset: number, limit = 50) =>
    invoke<{ items: Snapshot[]; total: number }>("list_snapshots", {
      variantId,
      offset,
      limit,
    }),
  getTaxonomy: () => invoke<Taxonomy>("get_taxonomy"),
  updateAsset: (data: {
    id: string;
    name: string;
    categoryId: string | null;
    tags: string[];
    gameIds: string[];
    archived: boolean;
  }) => invoke<void>("update_asset", data),
  saveCategory: (
    name: string,
    parentId: string | null,
    id: string | null = null,
  ) => invoke<void>("save_category", { id, name, parentId }),
  saveGame: (name: string, id: string | null = null) =>
    invoke<void>("save_game", { id, name }),
  deleteCategory: (id: string) => invoke<number>("delete_category", { id }),
  deleteGame: (id: string) => invoke<number>("delete_game", { id }),
  updateConfig: (variantId: string, config: ExportConfig) =>
    invoke<void>("update_config", { variantId, config }),
  renameSnapshot: (id: string, message: string) =>
    invoke<void>("rename_snapshot", { id, message }),
  renameVariant: (id: string, name: string) =>
    invoke<void>("rename_variant", { id, name }),
  getFileSelections: (variantId: string, snapshotId: string | null = null) =>
    invoke<FileSelections>("get_file_selections", { variantId, snapshotId }),
  setFileSelection: (
    variantId: string,
    snapshotId: string | null,
    kind: "preview" | "export",
    file: string | null,
  ) =>
    invoke<void>("set_file_selection", { variantId, snapshotId, kind, file }),
  setBlenderPath: (path: string) =>
    invoke<string>("set_blender_path", { path }),
  openWorkspace: (variantId: string) =>
    invoke<void>("open_workspace", { variantId }),
  openBlender: (variantId: string) =>
    invoke<void>("open_blender", { variantId }),
  startJob: (request: JobRequest) => invoke<string>("start_job", { request }),
  getJob: (id: string) => invoke<Job>("get_job", { id }),
  cancelJob: (id: string) => invoke<void>("cancel_job", { id }),
  onJob: (callback: (job: Job) => void) =>
    listen<Job>("edge-lock://job", (event) => callback(event.payload)),
  onWorkspaceChanged: (callback: (variantId: string) => void) =>
    listen<{ variantId: string }>("edge-lock://workspace-changed", (event) =>
      callback(event.payload.variantId),
    ),
  onWorkspaceStatus: (callback: (status: WorkspaceStatus) => void) =>
    listen<WorkspaceStatus>("edge-lock://workspace-status", (event) =>
      callback(event.payload),
    ),
  onFolderDrop: async (callback: (paths: string[]) => void) =>
    getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "drop") callback(event.payload.paths);
    }),
};
