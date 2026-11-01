"""Edge Lock's isolated Blender worker. Only JSON files cross the process boundary."""
import glob
import json
from pathlib import Path
import sys
import traceback

import bpy

PROTOCOL_VERSION = 1


def selected_scene(config):
    name = config.get("scene")
    if name:
        scene = bpy.data.scenes.get(name)
        if scene is None:
            raise ValueError(f"Scène introuvable : {name}")
        bpy.context.window.scene = scene
    scene = bpy.context.scene
    collection_name = config.get("collection")
    collection = None
    if collection_name:
        candidates = [item for item in scene.collection.children if item.name == collection_name]
        if len(candidates) != 1:
            raise ValueError(f"Collection racine introuvable ou ambiguë : {collection_name}")
        collection = candidates[0]
        layer = next((item for item in bpy.context.view_layer.layer_collection.children
                      if item.collection == collection), None)
        if layer is None or layer.exclude:
            raise ValueError("La collection choisie est exclue du View Layer actif dans Blender.")
        bpy.context.view_layer.active_layer_collection = layer
    objects = set(collection.all_objects if collection else scene.objects)
    return scene, collection, objects


def dependency_report(workspace):
    root = Path(workspace).resolve()
    dependencies = {}
    # packed=False excludes incorporated resources. local=False includes linked libraries.
    for raw in bpy.utils.blend_paths(absolute=True, packed=False, local=False):
        if not raw:
            continue
        # Blender permits UDIM tiles and sequences. Resolve explicit UDIM patterns without
        # searching any unrelated directory for a replacement texture.
        paths = glob.glob(raw.replace("<UDIM>", "[1-9][0-9][0-9][0-9]")) if "<UDIM>" in raw else [raw]
        if not paths:
            dependencies[raw] = {"path": raw, "status": "missing"}
        for name in paths:
            path = Path(name).resolve()
            is_internal = path.is_relative_to(root)
            if not path.exists():
                status = "missing"
            elif not is_internal:
                status = "external"
            else:
                status = "internal"
            display_path = str(path.relative_to(root)) if is_internal else str(path)
            dependencies[str(path)] = {"path": display_path, "status": status}
    for group in (bpy.data.images, bpy.data.libraries, bpy.data.fonts, bpy.data.sounds):
        for item in group:
            if getattr(item, "packed_file", None) or getattr(item, "packed_files", None):
                name = getattr(item, "filepath", "") or item.name
                dependencies["packed:" + name] = {"path": name, "status": "packed"}
    return sorted(dependencies.values(), key=lambda item: (item["status"], item["path"]))


def geometry_stats(objects):
    counts = {"meshObjects": 0, "vertices": 0, "faces": 0, "triangles": 0}
    graph = bpy.context.evaluated_depsgraph_get()
    cache = {}
    for instance in graph.object_instances:
        evaluated = instance.object
        if evaluated.type != "MESH":
            continue
        if instance.is_instance:
            if instance.parent is None or instance.parent.original not in objects:
                continue
        elif evaluated.original not in objects:
            continue
        key = evaluated.as_pointer()
        if key not in cache:
            mesh = evaluated.to_mesh()
            try:
                if mesh is None:
                    continue
                mesh.calc_loop_triangles()
                cache[key] = (len(mesh.vertices), len(mesh.polygons), len(mesh.loop_triangles))
            finally:
                evaluated.to_mesh_clear()
        vertices, faces, triangles = cache[key]
        counts["meshObjects"] += 1
        counts["vertices"] += vertices
        counts["faces"] += faces
        counts["triangles"] += triangles
    return counts


def inspect_project(workspace, config, include_geometry=True):
    scene, collection, objects = selected_scene(config)
    dependencies = dependency_report(workspace)
    warnings = []
    if any(item["status"] in ("external", "missing") for item in dependencies):
        warnings.append("Des dépendances sont externes ou manquantes. Rapatriez-les dans le dossier de l'asset et utilisez des chemins relatifs dans Blender.")
    if bpy.app.autoexec_fail:
        warnings.append("Des scripts Python embarqués ont été désactivés ; certains pilotes peuvent ne pas être évalués.")
    result = {
        "geometry": geometry_stats(objects) if include_geometry else {"meshObjects": 0, "vertices": 0, "faces": 0, "triangles": 0},
        "scenes": sorted(scene.name for scene in bpy.data.scenes),
        "collections": sorted(item.name for item in scene.collection.children),
        "dependencies": dependencies,
        "warnings": warnings,
        "blenderVersion": bpy.app.version_string,
    }
    return result, scene, collection, objects


def run(request):
    if request.get("protocolVersion") != PROTOCOL_VERSION:
        raise ValueError("Version de protocole Edge Lock incompatible.")
    operation = request["operation"]
    if operation == "create":
        bpy.ops.wm.read_factory_settings(use_empty=True)
        bpy.context.preferences.filepaths.save_version = 0
        bpy.context.preferences.filepaths.file_preview_type = "NONE"
        bpy.ops.wm.save_as_mainfile(filepath=request["blendFile"], check_existing=False)
        return None

    bpy.ops.wm.open_mainfile(filepath=request["blendFile"], load_ui=False, use_scripts=False)
    config = request.get("config", {})
    result, scene, collection, objects = inspect_project(request["workspace"], config, include_geometry=operation == "inspect")
    if operation == "inspect":
        return result
    if operation != "export":
        raise ValueError("Opération Blender inconnue.")
    unresolved = [item for item in result["dependencies"] if item["status"] in ("external", "missing")]
    if unresolved:
        raise ValueError("Export impossible : dépendances non figées dans cette version : " + "; ".join(item["path"] for item in unresolved[:10]))
    if bpy.app.autoexec_fail:
        raise ValueError("Export impossible : la scène dépend de scripts embarqués désactivés.")
    apply_modifiers = bool(config.get("applyModifiers", False))
    for obj in objects:
        if obj.type != "MESH":
            continue
        if apply_modifiers and obj.data.shape_keys and len(obj.data.shape_keys.key_blocks) > 1:
            raise ValueError("Désactivez « Appliquer les modificateurs » pour préserver les shape keys.")
        for modifier in obj.modifiers:
            if modifier.type == "ARMATURE" and modifier.object is not None and modifier.object not in objects:
                raise ValueError(f"Le rig de {obj.name} se trouve hors de la collection. Exportez la scène ou regroupez le rig dans cette collection.")
    result["geometry"] = geometry_stats(objects)
    result_code = bpy.ops.export_scene.gltf(
        filepath=request["destination"], check_existing=False, export_format="GLB",
        use_active_scene=True, use_active_collection=collection is not None,
        use_active_collection_with_nested=True, use_selection=False,
        use_visible=False, use_renderable=False,
        export_animations=True, export_animation_mode="ACTIONS",
        export_skins=True, export_morph=True, export_apply=apply_modifiers,
        export_materials="EXPORT", export_image_format="AUTO",
        export_cameras=False, export_lights=False,
        export_draco_mesh_compression_enable=False, export_use_gltfpack=False,
        export_hierarchy_flatten_bones=False, export_hierarchy_flatten_objs=False,
        will_save_settings=False,
    )
    if "FINISHED" not in result_code or not Path(request["destination"]).is_file():
        raise RuntimeError("Blender n'a pas produit le fichier GLB attendu.")
    return result


def main():
    arguments = sys.argv[sys.argv.index("--") + 1:]
    if len(arguments) != 2:
        raise ValueError("Arguments Edge Lock invalides.")
    request_path, response_path = map(Path, arguments)
    request = json.loads(request_path.read_text(encoding="utf-8"))
    try:
        response = {"protocolVersion": PROTOCOL_VERSION, "result": run(request), "error": None}
    except Exception as error:
        response = {"protocolVersion": PROTOCOL_VERSION, "result": None, "error": str(error)}
        response_path.write_text(json.dumps(response, ensure_ascii=False), encoding="utf-8")
        traceback.print_exc()
        raise
    response_path.write_text(json.dumps(response, ensure_ascii=False), encoding="utf-8")


if __name__ == "__main__":
    main()
