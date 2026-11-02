"""Edge Lock's isolated Blender worker. Only JSON files cross the process boundary."""
import glob
import json
from pathlib import Path
import re
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


class SourcePaths:
    """Resolve Blender references without changing any file on disk."""

    def __init__(self, workspace, logical_workspace):
        self.workspace = Path(workspace).resolve()
        self.logical_workspace = Path(logical_workspace).resolve()
        self.logical_blend = self.logical_workspace / Path(bpy.data.filepath).relative_to(self.workspace)

    def logical(self, path):
        path = Path(path).resolve()
        if path.is_relative_to(self.workspace):
            return self.logical_workspace / path.relative_to(self.workspace)
        return path

    def actual(self, path):
        if path.is_relative_to(self.logical_workspace):
            return self.workspace / path.relative_to(self.logical_workspace)
        return path

    def library(self, library):
        # Blender rebases nested Library.filepath values against the current main
        # file when loading them. Prefer that live resolved location if present;
        # only unresolved original references need the logical main-file base.
        actual = Path(bpy.path.abspath(library.filepath)).resolve()
        if actual.is_file() or not library.filepath.startswith('//'):
            return self.logical(actual)
        return self.resolve(library.filepath)

    def resolve(self, raw, owner=None):
        if raw.startswith('//'):
            return (Path(owner or self.logical_blend).parent / raw[2:]).resolve()
        return self.logical(Path(bpy.path.abspath(raw)))

    def resource(self, raw, library=None):
        return self.resolve(raw, self.library(library) if library else None)


def rebase_preview_sources(paths):
    # Reloading a library can replace IDs and discover nested libraries. Re-enumerate
    # after each reload rather than keeping references to invalidated datablocks.
    processed = set()
    for _ in range(64):
        pending = None
        for library in list(bpy.data.libraries):
            if library.packed_file:
                continue
            target = paths.actual(paths.library(library))
            identity = (library.name, str(target))
            if identity not in processed:
                processed.add(identity)
                if target.is_file() and library.filepath != str(target):
                    pending = (library, target)
                    break
        if pending is None:
            break
        library, target = pending
        library.filepath = str(target)
        library.reload()
    else:
        raise ValueError("Trop de bibliothèques imbriquées pour préparer cet aperçu (64).")
    for group in (bpy.data.images, bpy.data.fonts, bpy.data.sounds,
                  bpy.data.movieclips, bpy.data.volumes, bpy.data.cache_files):
        for item in list(group):
            raw = getattr(item, 'filepath', '')
            if not raw or raw == '<builtin>' or getattr(item, 'packed_file', None) or getattr(item, 'packed_files', None):
                continue
            target = str(paths.actual(paths.resource(raw, item.library)))
            if raw != target:
                item.filepath = target


def render_ids(objects, collection):
    # user_map maps a referenced ID to the IDs using it. Reverse once, then follow
    # actual scene/collection users; fake users and unused brush libraries are excluded.
    references = {}
    for dependency, users in bpy.data.user_map().items():
        for user in users:
            references.setdefault(user, set()).add(dependency)
    used = set(objects)
    collections = [collection or bpy.context.scene.collection]
    while collections:
        current = collections.pop()
        if current not in used:
            used.add(current)
            collections.extend(current.children)
    pending = list(used)
    while pending:
        for dependency in references.get(pending.pop(), ()):
            if dependency not in used:
                used.add(dependency)
                pending.append(dependency)
    libraries = {item.library for item in used if item.library}
    for library in list(libraries):
        while library.parent and library.parent not in libraries:
            library = library.parent
            libraries.add(library)
    return used, libraries


def source_members(path):
    literal = glob.escape(str(path))
    pattern = literal.replace('<UDIM>', '[1-9][0-9][0-9][0-9]')
    pattern = re.sub(r'#+', lambda match: '[0-9]' * len(match.group()), pattern)
    if pattern != literal:
        return glob.glob(pattern)
    return [str(path)] if path.is_file() else []


def dependency_report(workspace, objects, collection, paths=None):
    paths = paths or SourcePaths(workspace, workspace)
    root = paths.logical_workspace
    used, libraries = render_ids(objects, collection)
    dependencies = {}

    def add(raw, kind, relevant, library=None):
        if not raw or raw == '<builtin>':
            return
        source = paths.resource(raw, library)
        actual = paths.actual(source)
        members = source_members(actual)
        internal = source.is_relative_to(root)
        if not members:
            members = [str(actual)]
        for name in members:
            member = Path(name)
            logical = paths.logical(member)
            if not member.is_file():
                status = "missing"
            elif not internal:
                status = "external"
            else:
                status = "internal"
            display = str(logical.relative_to(root)) if internal else str(logical)
            key = (str(source), display, kind)
            existing = dependencies.get(key)
            dependencies[key] = {"path": display, "status": status, "kind": kind,
                                 "renderRelevant": relevant or bool(existing and existing['renderRelevant']),
                                 "sourcePath": str(source)}

    for library in bpy.data.libraries:
        if not library.packed_file:
            add(str(paths.library(library)), 'library', library in libraries)
    # file_path_map covers caches, volumes and other render data in addition to images.
    for item, references in bpy.data.file_path_map(include_libraries=False).items():
        if isinstance(item, bpy.types.Library):
            continue
        if isinstance(item, bpy.types.VectorFont) and item.filepath == '<builtin>':
            continue
        kind = ('image' if isinstance(item, bpy.types.Image) else
                'font' if isinstance(item, bpy.types.VectorFont) else
                'sound' if isinstance(item, bpy.types.Sound) else 'other')
        for raw in references:
            if isinstance(item, bpy.types.Image) and item.source == 'SEQUENCE':
                raw = re.sub(r'\d+(?=\.[^.]+$)', lambda match: '#' * len(match.group()), raw)
            add(raw, kind, item in used, item.library)
    # Weak asset-library references (for example Essentials brushes) are deliberately
    # absent from file_path_map. Retain them as secondary diagnostics, never as inputs
    # to preview conversion/cache invalidation.
    known = {str(paths.actual(Path(item['sourcePath']))) for item in dependencies.values()}
    for raw in bpy.utils.blend_paths(absolute=True, packed=False, local=False):
        if raw and str(Path(raw).resolve()) not in known:
            add(raw, 'library' if Path(raw).suffix.lower() == '.blend' else 'other', False)
    for group in (bpy.data.images, bpy.data.libraries, bpy.data.fonts, bpy.data.sounds):
        for item in group:
            if getattr(item, "packed_file", None) or getattr(item, "packed_files", None):
                name = getattr(item, "filepath", "") or item.name
                dependencies[("packed:" + name, '', '')] = {"path": name, "status": "packed",
                    "kind": 'image' if isinstance(item, bpy.types.Image) else 'other',
                    "renderRelevant": item in used, "sourcePath": None}
    return sorted(dependencies.values(), key=lambda item: (item["status"], item["path"]))


def geometry_stats(objects):
    counts = {"meshObjects": 0, "vertices": 0, "edges": 0, "faces": 0, "triangles": 0}
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
                cache[key] = (len(mesh.vertices), len(mesh.edges), len(mesh.polygons), len(mesh.loop_triangles))
            finally:
                evaluated.to_mesh_clear()
        vertices, edges, faces, triangles = cache[key]
        counts["meshObjects"] += 1
        counts["vertices"] += vertices
        counts["edges"] += edges
        counts["faces"] += faces
        counts["triangles"] += triangles
    return counts


def inspect_project(workspace, config, include_geometry=True, paths=None, preview=False):
    scene, collection, objects = selected_scene(config)
    dependencies = dependency_report(workspace, objects, collection, paths)
    warnings = []
    if preview and any(item['status'] == 'missing' and item['renderRelevant'] for item in dependencies):
        warnings.append("Aperçu visuellement dégradé : des ressources utilisées par le modèle sont manquantes. Consultez les dépendances.")
    elif not preview and any(item["renderRelevant"] and item["status"] in ("external", "missing") for item in dependencies):
        warnings.append("Des dépendances sont externes ou manquantes. Rapatriez-les dans le dossier de l'asset et utilisez des chemins relatifs dans Blender.")
    if bpy.app.autoexec_fail:
        warnings.append("Des scripts Python embarqués ont été désactivés ; certains pilotes peuvent ne pas être évalués.")
    result = {
        "geometry": geometry_stats(objects) if include_geometry else {"meshObjects": 0, "vertices": 0, "edges": 0, "faces": 0, "triangles": 0},
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
    canonical = operation == "generate"
    # A canonical GLB always represents the complete saved active scene. Old
    # per-export selections must never silently narrow the generated asset.
    config = {} if canonical else request.get("config", {})
    preview = operation in ('preview', 'inspectPreview')
    paths = SourcePaths(request['workspace'], request.get('logicalWorkspace') or request['workspace'])
    if preview or canonical:
        rebase_preview_sources(paths)
    result, scene, collection, objects = inspect_project(request["workspace"], config, include_geometry=operation == "inspect", paths=paths, preview=preview or canonical)
    if operation in ("inspect", "inspectPreview"):
        return result
    if operation not in ("export", "preview", "generate"):
        raise ValueError("Opération Blender inconnue.")
    unresolved = [item for item in result["dependencies"] if item["status"] in ("external", "missing")]
    if canonical:
        missing = [item for item in unresolved if item["status"] == "missing" and item["renderRelevant"]]
        if missing:
            raise ValueError("Génération du GLB impossible : ressources utilisées manquantes : " + "; ".join(item["path"] for item in missing[:10]))
    elif unresolved and not preview:
        raise ValueError("Export impossible : dépendances non figées dans cette version : " + "; ".join(item["path"] for item in unresolved[:10]))
    if bpy.app.autoexec_fail and not preview:
        raise ValueError("Export impossible : la scène dépend de scripts embarqués désactivés.")
    for obj in objects:
        if obj.type != "MESH":
            continue
        # Blender's evaluated export drops morph targets when any modifier is
        # present, including Armature. Fail before exporting instead of silently
        # discarding shape keys or their animation. Modifier-free meshes retain
        # their original data and morph targets with export_apply enabled.
        if obj.modifiers and obj.data.shape_keys and len(obj.data.shape_keys.key_blocks) > 1:
            raise ValueError(f"GLB impossible pour {obj.name} : les shape keys sont incompatibles avec l’application des modificateurs. Le fichier .blend reste intact.")
        for modifier in obj.modifiers:
            if modifier.type == "ARMATURE" and modifier.object is not None and modifier.object not in objects:
                raise ValueError(f"Le rig de {obj.name} se trouve hors de la collection. Exportez la scène ou regroupez le rig dans cette collection.")
    result["geometry"] = geometry_stats(objects)
    if preview:
        used, _ = render_ids(objects, collection)
        for image in list(bpy.data.images):
            if image not in used or image.packed_file or image.packed_files or image.source in ('GENERATED', 'VIEWER'):
                continue
            source = paths.resource(image.filepath, image.library)
            if not source_members(paths.actual(source)):
                replacement = bpy.data.images.new(image.name + ' (texture manquante)', width=2, height=2)
                replacement.generated_color = (1, 0, 1, 1)
                image.user_remap(replacement)
    result_code = bpy.ops.export_scene.gltf(
        filepath=request["destination"], check_existing=False, export_format="GLB",
        use_active_scene=True, use_active_collection=collection is not None,
        use_active_collection_with_nested=True, use_selection=False,
        use_visible=False, use_renderable=False,
        export_animations=True, export_animation_mode="ACTIONS",
        export_skins=True, export_morph=True, export_apply=True,
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
