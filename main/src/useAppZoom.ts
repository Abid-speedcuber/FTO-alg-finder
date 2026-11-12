import { useEffect } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";

const ZOOM_KEY = "fto.appZoom.v1";

export default function useAppZoom() {
  useEffect(() => {
    let zoom = 1;
    try {
      const saved = Number(localStorage.getItem(ZOOM_KEY));
      if (Number.isFinite(saved) && saved >= 0.5 && saved <= 3) zoom = saved;
    } catch { /* Zoom also works when local storage is unavailable. */ }
    let disposed = false;
    let pending = Promise.resolve();
    function apply(value: number) {
      // Keep held/repeated keys ordered, including asynchronous native commands.
      pending = pending.then(async () => {
        if (disposed) return;
        if (isTauri()) await getCurrentWebview().setZoom(value);
        else document.documentElement.style.setProperty("zoom",String(value));
        if (disposed) return;
        try { localStorage.setItem(ZOOM_KEY,String(value)); } catch { /* Optional persistence. */ }
        requestAnimationFrame(() => { if (!disposed) window.dispatchEvent(new Event("resize")); });
      }).catch(error => console.error("Could not change app zoom",error));
    }
    apply(zoom);
    function keydown(event: KeyboardEvent) {
      if ((!event.ctrlKey && !event.metaKey) || event.altKey || event.isComposing) return;
      let next: number;
      if (event.key === "=" || event.key === "+" || event.code === "NumpadAdd") next = Math.min(3,Math.round((zoom+0.1)*100)/100);
      else if (event.key === "-" || event.code === "NumpadSubtract") next = Math.max(0.5,Math.round((zoom-0.1)*100)/100);
      else if (event.key === "0" || event.code === "Numpad0") next = 1;
      else return;
      event.preventDefault();
      if (next !== zoom) { zoom = next; apply(zoom); }
    }
    window.addEventListener("keydown",keydown,true);
    return () => { disposed = true; window.removeEventListener("keydown",keydown,true); };
  }, []);
}
