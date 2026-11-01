import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { Icon } from "./Icon";
import {
  inspectGlb,
  isEmbeddedImageUri,
  MAX_PREVIEW_BYTES,
} from "./viewer-utils";
import { errorMessage } from "../lib/format";

type ViewerRuntime = {
  frame: () => void;
  play: (index: number, playing: boolean) => void;
  seek: (time: number) => void;
};

function releaseModel(root: THREE.Object3D) {
  const geometries = new Set<THREE.BufferGeometry>();
  const materials = new Set<THREE.Material>();
  const textures = new Set<THREE.Texture>();
  const skeletons = new Set<THREE.Skeleton>();
  root.traverse((object) => {
    if (object instanceof THREE.Mesh) {
      geometries.add(object.geometry);
      for (const material of Array.isArray(object.material)
        ? object.material
        : [object.material])
        materials.add(material);
    }
    if (object instanceof THREE.SkinnedMesh) skeletons.add(object.skeleton);
  });
  for (const material of materials)
    for (const value of Object.values(material))
      if (value instanceof THREE.Texture) textures.add(value);
  for (const texture of textures) {
    const bitmap: unknown = texture.source.data;
    if (typeof ImageBitmap !== "undefined" && bitmap instanceof ImageBitmap)
      bitmap.close();
    texture.dispose();
  }
  geometries.forEach((resource) => resource.dispose());
  materials.forEach((resource) => resource.dispose());
  skeletons.forEach((resource) => resource.dispose());
}

export default function ModelViewer({
  url,
  label,
}: {
  url: string;
  label: string;
}) {
  const host = useRef<HTMLDivElement>(null);
  const runtime = useRef<ViewerRuntime | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "error">(
    "loading",
  );
  const [error, setError] = useState("");
  const [clips, setClips] = useState<{ name: string; duration: number }[]>([]);
  const [clip, setClip] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);

  useEffect(() => {
    const container = host.current;
    if (!container) return;
    let disposed = false;
    let renderer: THREE.WebGLRenderer | undefined;
    let controls: OrbitControls | undefined;
    let loaded: THREE.Object3D | undefined;
    let mixer: THREE.AnimationMixer | undefined;
    let action: THREE.AnimationAction | undefined;
    let resize: ResizeObserver | undefined;
    let frameId = 0;
    let last = 0;
    let currentPlaying = false;
    const abort = new AbortController();
    setStatus("loading");
    setError("");
    setClips([]);
    setPlaying(false);
    setClip(0);
    setTime(0);
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(42, 1, 0.01, 10_000);
    scene.background = new THREE.Color("#141a23");
    scene.add(new THREE.HemisphereLight(0xdceaff, 0x555e73, 2.7));
    const key = new THREE.DirectionalLight(0xffffff, 3.4);
    key.position.set(4, 6, 5);
    scene.add(key);
    const fill = new THREE.DirectionalLight(0x8bbcff, 1.6);
    fill.position.set(-4, 2, -3);
    scene.add(fill);
    const render = () => {
      if (!disposed && renderer) renderer.render(scene, camera);
    };
    const fail = (reason: unknown) => {
      if (!disposed) {
        currentPlaying = false;
        cancelAnimationFrame(frameId);
        setError(errorMessage(reason));
        setStatus("error");
        setPlaying(false);
      }
    };
    const contextLost = (event: Event) => {
      event.preventDefault();
      fail(
        new Error(
          "Le contexte graphique a été perdu. Fermez puis rouvrez l’aperçu.",
        ),
      );
    };
    const animate = (now: number) => {
      if (disposed || !currentPlaying) return;
      mixer?.update(last ? Math.min((now - last) / 1000, 0.1) : 0);
      last = now;
      if (action) setTime(action.time);
      render();
      frameId = requestAnimationFrame(animate);
    };
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
      renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5));
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      renderer.toneMappingExposure = 1.05;
      renderer.domElement.setAttribute("aria-label", "Aperçu 3D interactif");
      renderer.domElement.addEventListener("webglcontextlost", contextLost);
      container.appendChild(renderer.domElement);
      controls = new OrbitControls(camera, renderer.domElement);
      controls.enableDamping = false;
      controls.addEventListener("change", render);
      resize = new ResizeObserver(() => {
        const width = Math.max(1, container.clientWidth);
        const height = Math.max(1, container.clientHeight);
        renderer?.setSize(width, height, false);
        camera.aspect = width / height;
        camera.updateProjectionMatrix();
        render();
      });
      resize.observe(container);
      void (async () => {
        const response = await fetch(url, { signal: abort.signal });
        if (!response.ok)
          throw new Error("Impossible de charger le fichier d’aperçu.");
        if (Number(response.headers.get("content-length")) > MAX_PREVIEW_BYTES)
          throw new Error(
            "Cet aperçu dépasse 256 Mo. Ouvrez l’asset dans Blender.",
          );
        const bytes = await response.arrayBuffer();
        inspectGlb(bytes);
        if (disposed) return;
        const manager = new THREE.LoadingManager();
        manager.setURLModifier((resource) => {
          if (!resource.startsWith("blob:") && !isEmbeddedImageUri(resource))
            throw new Error("Ressource externe interdite dans l’aperçu.");
          return resource;
        });
        const model = await new GLTFLoader(manager).parseAsync(bytes, "");
        if (disposed) {
          releaseModel(model.scene);
          return;
        }
        loaded = model.scene;
        scene.add(loaded);
        const frame = () => {
          if (!loaded || !controls) return;
          const box = new THREE.Box3().setFromObject(loaded);
          if (box.isEmpty()) {
            fail(
              new Error("Cette scène ne contient aucune géométrie visible."),
            );
            return;
          }
          const center = box.getCenter(new THREE.Vector3());
          const radius = Math.max(
            box.getSize(new THREE.Vector3()).length() / 2,
            0.01,
          );
          const verticalFov = THREE.MathUtils.degToRad(camera.fov / 2);
          const limitingFov = Math.min(
            verticalFov,
            Math.atan(Math.tan(verticalFov) * camera.aspect),
          );
          const distance = (radius / Math.sin(limitingFov)) * 1.2;
          camera.position
            .copy(center)
            .add(
              new THREE.Vector3(1, 0.7, 1.2)
                .normalize()
                .multiplyScalar(distance),
            );
          camera.near = Math.max(radius / 1000, 0.0001);
          camera.far = Math.max(radius * 100, 10);
          camera.updateProjectionMatrix();
          controls.target.copy(center);
          controls.minDistance = radius * 0.05;
          controls.maxDistance = radius * 30;
          controls.update();
          render();
        };
        mixer = new THREE.AnimationMixer(loaded);
        const play = (index: number, active: boolean) => {
          currentPlaying = false;
          cancelAnimationFrame(frameId);
          last = 0;
          if (model.animations[index]) {
            const next = mixer!.clipAction(model.animations[index]);
            if (action !== next) {
              mixer!.stopAllAction();
              action = next;
              action.reset().play();
              setTime(0);
            }
            action.paused = !active;
            currentPlaying = active;
            if (active) frameId = requestAnimationFrame(animate);
            else {
              mixer!.update(0);
              render();
            }
          }
        };
        runtime.current = {
          frame,
          play,
          seek: (value) => {
            if (action && mixer) {
              action.time = value;
              mixer.update(0);
              setTime(value);
              render();
            }
          },
        };
        setClips(
          model.animations.map((item, index) => ({
            name: item.name || `Animation ${index + 1}`,
            duration: item.duration,
          })),
        );
        frame();
        if (model.animations.length) play(0, false);
        setStatus("ready");
        render();
      })().catch((reason) => {
        if (!abort.signal.aborted) fail(reason);
      });
    } catch (reason) {
      fail(reason);
    }
    return () => {
      disposed = true;
      abort.abort();
      cancelAnimationFrame(frameId);
      runtime.current = null;
      resize?.disconnect();
      controls?.dispose();
      mixer?.stopAllAction();
      if (loaded) {
        mixer?.uncacheRoot(loaded);
        scene.remove(loaded);
        releaseModel(loaded);
      }
      if (renderer) {
        renderer.domElement.removeEventListener(
          "webglcontextlost",
          contextLost,
        );
        renderer.dispose();
        renderer.domElement.remove();
      }
    };
  }, [url]);

  return (
    <div className="model-viewer">
      <div className="viewer-canvas" ref={host} />
      <div className="viewer-top">
        <span className="viewer-badge">
          <span className="live-dot" />
          {label}
        </span>
        <button
          type="button"
          className="icon-button viewer-control"
          title="Recentrer"
          aria-label="Recentrer le modèle"
          disabled={status !== "ready"}
          onClick={() => runtime.current?.frame()}
        >
          <Icon name="focus" />
        </button>
      </div>
      {status !== "ready" && (
        <div className="viewer-overlay">
          <Icon
            name={status === "loading" ? "refresh" : "warning"}
            size={26}
            className={status === "loading" ? "spinning" : ""}
          />
          <strong>
            {status === "loading"
              ? "Chargement du modèle…"
              : "Aperçu indisponible"}
          </strong>
          {error && <p>{error}</p>}
        </div>
      )}
      {status === "ready" && (
        <div className="viewer-help">
          Glisser pour tourner<span>·</span>Molette pour zoomer<span>·</span>
          Clic droit pour déplacer
        </div>
      )}
      {status === "ready" && clips.length > 0 && (
        <div className="animation-bar">
          <button
            className="icon-button"
            aria-label={playing ? "Mettre en pause" : "Lire l’animation"}
            onClick={() => {
              runtime.current?.play(clip, !playing);
              setPlaying(!playing);
            }}
          >
            <Icon name={playing ? "pause" : "play"} size={16} />
          </button>
          <select
            aria-label="Animation"
            value={clip}
            onChange={(event) => {
              const index = Number(event.target.value);
              setClip(index);
              runtime.current?.play(index, playing);
            }}
          >
            {clips.map((item, index) => (
              <option key={index} value={index}>
                {item.name}
              </option>
            ))}
          </select>
          <input
            aria-label="Position dans l’animation"
            type="range"
            min={0}
            max={clips[clip]?.duration || 1}
            step={0.01}
            value={time}
            onChange={(event) =>
              runtime.current?.seek(Number(event.target.value))
            }
          />
          <span>{time.toFixed(1)} s</span>
        </div>
      )}
    </div>
  );
}
