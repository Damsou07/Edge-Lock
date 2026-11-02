import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import type { GLTF } from "three/addons/loaders/GLTFLoader.js";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { Icon } from "./Icon";
import {
  createPreviewRenderer,
  createPreviewStage,
  disposePreviewRenderer,
  framePreviewModel,
  loadPreviewModel,
  releaseModel,
} from "./viewer-runtime";
import { errorMessage } from "../lib/format";

type ViewerRuntime = {
  frame: () => void;
  play: (index: number, playing: boolean) => void;
  seek: (time: number) => void;
};

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
    let loadedModel: GLTF | undefined;
    let stage: ReturnType<typeof createPreviewStage> | undefined;
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
    const render = () => {
      if (!disposed && renderer && stage)
        renderer.render(stage.scene, stage.camera);
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
      renderer = createPreviewRenderer();
      stage = createPreviewStage(renderer);
      const { scene, camera } = stage;
      renderer.domElement.setAttribute("aria-label", "Aperçu 3D interactif");
      renderer.domElement.addEventListener("webglcontextlost", contextLost);
      container.appendChild(renderer.domElement);
      controls = new OrbitControls(camera, renderer.domElement);
      controls.enableDamping = false;
      controls.addEventListener("change", render);
      const resizeCanvas = () => {
        const width = Math.max(1, container.clientWidth);
        const height = Math.max(1, container.clientHeight);
        renderer?.setSize(width, height, false);
        camera.aspect = width / height;
        camera.updateProjectionMatrix();
        render();
      };
      resizeCanvas();
      resize = new ResizeObserver(resizeCanvas);
      resize.observe(container);
      void (async () => {
        const model = await loadPreviewModel(url, abort.signal);
        if (disposed) {
          releaseModel(model.scene, ...model.scenes);
          return;
        }
        loadedModel = model;
        loaded = model.scene;
        scene.add(loaded);
        const frame = () => {
          if (!loaded || !controls) return false;
          let framed: ReturnType<typeof framePreviewModel>;
          try {
            framed = framePreviewModel(loaded, camera);
          } catch (reason) {
            fail(reason);
            return false;
          }
          const { target, radius } = framed;
          controls.target.copy(target);
          controls.minDistance = radius * 0.05;
          controls.maxDistance = radius * 30;
          controls.update();
          render();
          return true;
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
        if (!frame()) return;
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
        loaded.removeFromParent();
      }
      if (loadedModel) releaseModel(loadedModel.scene, ...loadedModel.scenes);
      stage?.dispose();
      if (renderer) {
        renderer.domElement.removeEventListener(
          "webglcontextlost",
          contextLost,
        );
        disposePreviewRenderer(renderer);
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
