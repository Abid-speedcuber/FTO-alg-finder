import { useEffect, useState } from "react";
import { invoke, listen } from "./platform";

export const GPU_KEY = "fto.gpuAcceleration.v1";
export type GpuProfile = { eligible: boolean; adapter: string; cpuNodesPerSecond: number; gpuNodesPerSecond: number; cpuWorkers: number; message: string };
export default function useGpuAcceleration() {
  const [preference, setPreference] = useState<boolean | null>(() => {
    try { const value = localStorage.getItem(GPU_KEY); return value === "true" ? true : value === "false" ? false : null; } catch { return null; }
  });
  const [profile, setProfile] = useState<GpuProfile | null>(null);
  const [profileRevision, setProfileRevision] = useState(0);
  const [generationStatus, setGenerationStatus] = useState({ active: false, gpuEnabled: false, backend: "cpu", message: "" });
  const [switchPending, setSwitchPending] = useState(false);
  const [settingsMessage, setSettingsMessage] = useState("");
  const useGpu = profile?.eligible === true && (preference ?? true);
  useEffect(() => {
    if (preference === null) return;
    try { localStorage.setItem(GPU_KEY, String(preference)); } catch { /* Optional persistence. */ }
  }, [preference]);
  useEffect(() => {
    let disposed = false;
    void invoke<GpuProfile>("get_gpu_profile").then(result => { if (!disposed) setProfile(result); }).catch(() => {
      if (!disposed) setProfile({ eligible: false, adapter: "", cpuNodesPerSecond: 0, gpuNodesPerSecond: 0, cpuWorkers: 0, message: "Hardware comparison unavailable." });
    });
    return () => { disposed = true; };
  }, [profileRevision]);
  useEffect(() => {
    let disposed = false; let remove: (() => void) | undefined;
    void listen<typeof generationStatus>("generation-status", ({ payload }) => { if (!disposed) setGenerationStatus(payload); }).then(unlisten => {
      if (disposed) unlisten(); else remove = unlisten;
    }).catch(() => {});
    return () => { disposed = true; remove?.(); };
  }, []);
  async function changeGpu(enabled: boolean) {
    if (!profile?.eligible || switchPending) return;
    setPreference(enabled); setSettingsMessage(""); setSwitchPending(true);
    try { await invoke("set_generation_gpu", { enabled }); }
    catch { setSettingsMessage("Preference saved. It will apply to the next build."); }
    finally { setSwitchPending(false); }
  }
  function resetGpu() { setPreference(null); setProfile(null); setSettingsMessage(""); setProfileRevision(r => r+1); }
  return { useGpu, profile, generationStatus, setGenerationStatus, switchPending, settingsMessage, changeGpu, resetGpu };
}
