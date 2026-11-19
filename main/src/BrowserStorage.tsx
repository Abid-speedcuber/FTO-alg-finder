import { formatBytes } from "./formatBytes";
import { useEffect, useState } from "react";
import { isTauri } from "./platform";

export default function BrowserStorage() {
  const [status, setStatus] = useState("");
  async function refresh(requestPersistence = false) {
    try {
      if (requestPersistence) await navigator.storage.persist();
      const [estimate, persistent] = await Promise.all([navigator.storage.estimate(), navigator.storage.persisted()]);
      setStatus(`${formatBytes(estimate.usage)} used of ${formatBytes(estimate.quota)} browser quota. ${persistent ? "Persistent storage granted." : "Storage may be evicted by the browser."}`);
    } catch { setStatus("Storage estimates are unavailable in this browser."); }
  }
  useEffect(() => { if (!isTauri()) void refresh(); }, []);
  if (isTauri()) return null;
  return <div className="hint">
    <p>{status}</p>
    <button className="secondary" onClick={() => void refresh(true)}>Keep saved tables on this device</button>
    <p>Tables save after each operation. Keep this tab open until saving finishes. This browser build uses CPU generation and has a 4 GiB solver address space shared by all threads. Saved tables stay in IndexedDB and load only when needed. Unloading releases their solver allocations.</p>
  </div>;
}
