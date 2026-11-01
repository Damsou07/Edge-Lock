import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type {
  AssetDetail,
  AssetFilter,
  AssetPage,
  Bootstrap,
  ExportConfig,
  Job,
  JobRequest,
  LibraryInfo,
  Snapshot,
  Taxonomy,
  VariantDetail,
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
  getVariant: (id: string) => invoke<VariantDetail>("get_variant", { id }),
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
  updateConfig: (variantId: string, config: ExportConfig) =>
    invoke<void>("update_config", { variantId, config }),
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
  onFolderDrop: async (callback: (paths: string[]) => void) =>
    getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "drop") callback(event.payload.paths);
    }),
};
