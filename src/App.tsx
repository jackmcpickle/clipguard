import { useEffect, useState, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
  blocked: boolean;
}

interface TimestampedWarning extends PasteWarning {
  ts: number;
}

type RuleAction = "notify" | "block";

interface BlockRule {
  from_app_id: string | null;
  from_app_name: string | null;
  to_app_id: string | null;
  to_app_name: string | null;
  action: RuleAction;
}

interface AppBundleInfo {
  bundle_id: string;
  name: string;
}

function App() {
  const [guardEnabled, setGuardEnabled] = useState(true);
  const [autostartEnabled, setAutostartEnabled] = useState(false);
  const [lastSource, setLastSource] = useState<ClipboardEvent | null>(null);
  const [recentWarnings, setRecentWarnings] = useState<TimestampedWarning[]>(
    []
  );
  const [rules, setRules] = useState<BlockRule[]>([]);
  const [accessibilityGranted, setAccessibilityGranted] = useState(false);
  const [appList, setAppList] = useState<AppBundleInfo[]>([]);
  const [appPickerOpen, setAppPickerOpen] = useState(false);
  const [appPickerSearch, setAppPickerSearch] = useState("");
  const appPickerCallbackRef = useRef<((app: AppBundleInfo) => void) | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const cleanups: (() => void)[] = [];

    invoke<boolean>("get_enabled").then(setGuardEnabled);
    isEnabled().then(setAutostartEnabled);
    invoke<BlockRule[]>("get_rules").then((loaded) => {
      if (loaded.length === 0) {
        const defaultRule: BlockRule = {
          from_app_id: null,
          from_app_name: null,
          to_app_id: null,
          to_app_name: null,
          action: "notify",
        };
        setRules([defaultRule]);
      } else {
        setRules(loaded);
      }
    });
    invoke<boolean>("check_accessibility").then(setAccessibilityGranted);

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

    // Re-check accessibility on window focus (catches revocations)
    getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused) {
          invoke<boolean>("check_accessibility").then(setAccessibilityGranted);
        }
      })
      .then((f) => cleanups.push(f));

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

  const saveRules = async (updated: BlockRule[]) => {
    setRules(updated);
    await invoke("set_rules", { newRules: updated });
  };

  const openAppPicker = useCallback(async (callback: (app: AppBundleInfo) => void) => {
    appPickerCallbackRef.current = callback;
    setAppPickerSearch("");
    const apps = await invoke<AppBundleInfo[]>("list_apps");
    setAppList(apps);
    setAppPickerOpen(true);
    setTimeout(() => searchInputRef.current?.focus(), 0);
  }, []);

  const closeAppPicker = useCallback(() => {
    setAppPickerOpen(false);
    appPickerCallbackRef.current = null;
  }, []);

  const selectApp = useCallback((app: AppBundleInfo) => {
    appPickerCallbackRef.current?.(app);
    closeAppPicker();
  }, [closeAppPicker]);

  const updateRule = (index: number, patch: Partial<BlockRule>) => {
    const updated = rules.map((r, i) => (i === index ? { ...r, ...patch } : r));
    saveRules(updated);
  };

  const removeRule = (index: number) => {
    saveRules(rules.filter((_, i) => i !== index));
  };

  const addRule = () => {
    saveRules([
      ...rules,
      {
        from_app_id: null,
        from_app_name: null,
        to_app_id: null,
        to_app_name: null,
        action: "notify",
      },
    ]);
  };

  const browseFrom = (index: number) => {
    openAppPicker((app) => {
      updateRule(index, {
        from_app_id: app.bundle_id,
        from_app_name: app.name,
      });
    });
  };

  const browseTo = (index: number) => {
    openAppPicker((app) => {
      updateRule(index, {
        to_app_id: app.bundle_id,
        to_app_name: app.name,
      });
    });
  };

  const clearFrom = (index: number) => {
    updateRule(index, { from_app_id: null, from_app_name: null });
  };

  const clearTo = (index: number) => {
    updateRule(index, { to_app_id: null, to_app_name: null });
  };

  const toggleAction = (index: number) => {
    const current = rules[index].action;
    updateRule(index, { action: current === "notify" ? "block" : "notify" });
  };

  const isInvalidRule = (r: BlockRule) =>
    r.from_app_id === null && r.to_app_id === null;

  const hasBlockRules = rules.some((r) => r.action === "block");

  const refreshAccessibility = async () => {
    const granted = await invoke<boolean>("check_accessibility");
    setAccessibilityGranted(granted);
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

      {hasBlockRules && (
        <section
          className={`permission-banner ${accessibilityGranted ? "granted" : "warning"}`}
        >
          <div className="row space-between">
            <span>
              {accessibilityGranted
                ? "Accessibility permission granted"
                : "Block rules require Accessibility permission"}
            </span>
            {!accessibilityGranted && (
              <div className="permission-actions">
                <button
                  className="btn-permission"
                  onClick={() => invoke("open_accessibility_settings")}
                >
                  Grant
                </button>
                <button className="btn-refresh" onClick={refreshAccessibility}>
                  Check
                </button>
              </div>
            )}
          </div>
          {!accessibilityGranted && (
            <p className="muted">
              Without it, block rules will only notify.
            </p>
          )}
        </section>
      )}

      <section className="card">
        <h2>Rules</h2>
        {rules.map((rule, i) => (
          <div
            key={i}
            className={`rule-row ${isInvalidRule(rule) ? "rule-invalid" : ""}`}
          >
            <div className="rule-fields">
              <div className="rule-field">
                <span className="rule-label">From</span>
                <span className="rule-app">
                  {rule.from_app_name ?? "Any App"}
                </span>
                <div className="rule-btns">
                  <button className="btn-browse" onClick={() => browseFrom(i)}>
                    Browse
                  </button>
                  {rule.from_app_id && (
                    <button className="btn-clear" onClick={() => clearFrom(i)}>
                      x
                    </button>
                  )}
                </div>
              </div>

              <span className="rule-arrow">→</span>

              <div className="rule-field">
                <span className="rule-label">To</span>
                <span className="rule-app">
                  {rule.to_app_name ?? "All Apps"}
                </span>
                <div className="rule-btns">
                  <button className="btn-browse" onClick={() => browseTo(i)}>
                    Browse
                  </button>
                  {rule.to_app_id && (
                    <button className="btn-clear" onClick={() => clearTo(i)}>
                      x
                    </button>
                  )}
                </div>
              </div>
            </div>

            <div className="rule-actions">
              <button
                className={`action-toggle ${rule.action === "block" ? "action-block" : "action-notify"}`}
                onClick={() => toggleAction(i)}
              >
                {rule.action === "notify" ? "Notify" : "Block"}
              </button>

              <button className="btn-remove" onClick={() => removeRule(i)}>
                ×
              </button>
            </div>

            {isInvalidRule(rule) && (
              <span className="rule-error">
                At least one app must be specified
              </span>
            )}
          </div>
        ))}
        <button className="btn-add" onClick={addRule}>
          + Add Rule
        </button>
      </section>

      <section className="card">
        <h2>Last Clipboard Source</h2>
        {lastSource ? (
          <p className="source-info">
            {lastSource.source_app_name ??
              lastSource.source_app_id ??
              "Unknown"}
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
      {appPickerOpen && (
        <div className="app-picker-overlay" onClick={closeAppPicker} onKeyDown={(e) => e.key === "Escape" && closeAppPicker()}>
          <div className="app-picker-modal" onClick={(e) => e.stopPropagation()}>
            <input
              ref={searchInputRef}
              className="app-picker-search"
              type="text"
              placeholder="Search apps..."
              value={appPickerSearch}
              onChange={(e) => setAppPickerSearch(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && closeAppPicker()}
            />
            <div className="app-picker-list">
              {appList
                .filter((a) => {
                  const q = appPickerSearch.toLowerCase();
                  return a.name.toLowerCase().includes(q) || a.bundle_id.toLowerCase().includes(q);
                })
                .map((app) => (
                  <button
                    key={app.bundle_id}
                    className="app-picker-item"
                    onClick={() => selectApp(app)}
                  >
                    <span className="app-picker-name">{app.name}</span>
                    <span className="app-picker-id">{app.bundle_id}</span>
                  </button>
                ))}
            </div>
          </div>
        </div>
      )}
    </main>
  );
}

export default App;
