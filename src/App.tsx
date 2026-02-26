import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import "./App.css";

interface ClipboardEvent {
  source_app_id: string | null;
  source_app_name: string | null;
}

interface PasteWarning {
  source_app_id: string | null;
  source_app_name: string | null;
  dest_app_id: string | null;
  dest_app_name: string | null;
}

interface TimestampedWarning extends PasteWarning {
  ts: number;
}

function App() {
  const [guardEnabled, setGuardEnabled] = useState(true);
  const [autostartEnabled, setAutostartEnabled] = useState(false);
  const [lastSource, setLastSource] = useState<ClipboardEvent | null>(null);
  const [recentWarnings, setRecentWarnings] = useState<TimestampedWarning[]>([]);

  useEffect(() => {
    const cleanups: (() => void)[] = [];

    invoke<boolean>("get_enabled").then(setGuardEnabled);
    isEnabled().then(setAutostartEnabled);

    listen<ClipboardEvent>("clipboard-changed", (e) => {
      setLastSource(e.payload);
    }).then((f) => cleanups.push(f));

    listen<PasteWarning>("paste-warning", (e) => {
      setRecentWarnings((prev) =>
        [{ ...e.payload, ts: Date.now() }, ...prev].slice(0, 20)
      );
    }).then((f) => cleanups.push(f));

    listen<boolean>("guard-toggled", (e) => {
      setGuardEnabled(e.payload);
    }).then((f) => cleanups.push(f));

    return () => cleanups.forEach((f) => f());
  }, []);

  const toggleGuard = async () => {
    const next = !guardEnabled;
    await invoke("set_enabled", { enabled: next });
    setGuardEnabled(next);
  };

  const toggleAutostart = async () => {
    if (autostartEnabled) {
      await disable();
    } else {
      await enable();
    }
    setAutostartEnabled(!autostartEnabled);
  };

  return (
    <main className="container">
      <h1>Clipboard Guard</h1>

      <section className="card">
        <div className="row space-between">
          <span>Protection</span>
          <button
            className={guardEnabled ? "btn-on" : "btn-off"}
            onClick={toggleGuard}
          >
            {guardEnabled ? "Enabled" : "Disabled"}
          </button>
        </div>
        <div className="row space-between">
          <span>Launch at login</span>
          <button
            className={autostartEnabled ? "btn-on" : "btn-off"}
            onClick={toggleAutostart}
          >
            {autostartEnabled ? "On" : "Off"}
          </button>
        </div>
      </section>

      <section className="card">
        <h2>Last Clipboard Source</h2>
        {lastSource ? (
          <p className="source-info">
            {lastSource.source_app_name ?? lastSource.source_app_id ?? "Unknown"}
          </p>
        ) : (
          <p className="muted">No clipboard activity yet</p>
        )}
      </section>

      <section className="card">
        <h2>Recent Warnings</h2>
        {recentWarnings.length === 0 ? (
          <p className="muted">No warnings yet</p>
        ) : (
          <ul className="warning-list">
            {recentWarnings.map((w) => (
              <li key={w.ts}>
                <strong>{w.source_app_name ?? "Unknown"}</strong>
                {" → "}
                <strong>{w.dest_app_name ?? "Terminal"}</strong>
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  );
}

export default App;
