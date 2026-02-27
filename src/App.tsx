import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
import {
    useEffect,
    useState,
    useRef,
    useCallback,
    type ReactElement,
} from 'react';
import { AppPickerModal } from './AppPickerModal';
import type { AppBundleInfo } from './AppPickerModal';
import './App.css';

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

type RuleAction = 'notify' | 'block';

interface BlockRule {
    from_app_id: string | null;
    from_app_name: string | null;
    to_app_id: string | null;
    to_app_name: string | null;
    action: RuleAction;
}

interface BlockRuleWithId extends BlockRule {
    id: string;
}

let ruleIdCounter = 0;
function nextRuleId(): string {
    ruleIdCounter += 1;
    return String(ruleIdCounter);
}

function withId(rule: BlockRule): BlockRuleWithId {
    return { ...rule, id: nextRuleId() };
}

function App(): ReactElement {
    const [isWindowsPlatform, setIsWindowsPlatform] = useState(false);
    const [guardEnabled, setGuardEnabled] = useState(true);
    const [autostartEnabled, setAutostartEnabled] = useState(false);
    const [lastSource, setLastSource] = useState<ClipboardEvent | null>(null);
    const [recentWarnings, setRecentWarnings] = useState<TimestampedWarning[]>(
        [],
    );
    const [rules, setRules] = useState<BlockRuleWithId[]>([]);
    const [accessibilityGranted, setAccessibilityGranted] = useState(false);
    const [appList, setAppList] = useState<AppBundleInfo[]>([]);
    const [appPickerOpen, setAppPickerOpen] = useState(false);
    const appPickerCallbackRef = useRef<((app: AppBundleInfo) => void) | null>(
        null,
    );

    useEffect((): (() => void) => {
        const cleanups: (() => void)[] = [];

        void invoke<boolean>('get_enabled').then(setGuardEnabled);
        void invoke<boolean>('is_windows_platform').then(setIsWindowsPlatform);
        void isEnabled().then(setAutostartEnabled);
        void invoke<BlockRule[]>('get_rules').then((loaded) => {
            if (loaded.length === 0) {
                setRules([
                    withId({
                        from_app_id: null,
                        from_app_name: null,
                        to_app_id: null,
                        to_app_name: null,
                        action: 'notify',
                    }),
                ]);
            } else {
                setRules(loaded.map(withId));
            }
        });
        void invoke<boolean>('check_accessibility').then(
            setAccessibilityGranted,
        );

        void listen<ClipboardEvent>('clipboard-changed', (e) => {
            setLastSource(e.payload);
        }).then((f) => cleanups.push(f));

        void listen<PasteWarning>('paste-warning', (e) => {
            setRecentWarnings((prev) =>
                [{ ...e.payload, ts: Date.now() }, ...prev].slice(0, 20),
            );
        }).then((f) => cleanups.push(f));

        void listen<boolean>('guard-toggled', (e) => {
            setGuardEnabled(e.payload);
        }).then((f) => cleanups.push(f));

        void getCurrentWindow()
            .onFocusChanged(({ payload: focused }) => {
                if (focused) {
                    void invoke<boolean>('check_accessibility').then(
                        setAccessibilityGranted,
                    );
                }
            })
            .then((f) => cleanups.push(f));

        return (): void => {
            cleanups.forEach((f) => {
                f();
            });
        };
    }, []);

    async function toggleGuard(): Promise<void> {
        const next = !guardEnabled;
        await invoke('set_enabled', { enabled: next });
        setGuardEnabled(next);
    }

    async function toggleAutostart(): Promise<void> {
        if (autostartEnabled) {
            await disable();
        } else {
            await enable();
        }
        setAutostartEnabled(!autostartEnabled);
    }

    async function saveRules(updated: BlockRuleWithId[]): Promise<void> {
        setRules(updated);
        await invoke('set_rules', { newRules: updated });
    }

    const openAppPicker = useCallback(
        async (callback: (app: AppBundleInfo) => void): Promise<void> => {
            appPickerCallbackRef.current = callback;
            const apps = await invoke<AppBundleInfo[]>('list_apps');
            setAppList(apps);
            setAppPickerOpen(true);
        },
        [],
    );

    const closeAppPicker = useCallback((): void => {
        setAppPickerOpen(false);
        appPickerCallbackRef.current = null;
    }, []);

    const selectApp = useCallback(
        (app: AppBundleInfo): void => {
            appPickerCallbackRef.current?.(app);
            closeAppPicker();
        },
        [closeAppPicker],
    );

    function updateRule(index: number, patch: Partial<BlockRuleWithId>): void {
        const updated = rules.map((r, i) =>
            i === index ? { ...r, ...patch } : r,
        );
        void saveRules(updated);
    }

    function removeRule(index: number): void {
        void saveRules(rules.filter((_, i) => i !== index));
    }

    function addRule(): void {
        void saveRules([
            ...rules,
            withId({
                from_app_id: null,
                from_app_name: null,
                to_app_id: null,
                to_app_name: null,
                action: 'notify',
            }),
        ]);
    }

    function browseFrom(index: number): void {
        void openAppPicker((app) => {
            updateRule(index, {
                from_app_id: app.bundle_id,
                from_app_name: app.name,
            });
        });
    }

    function browseTo(index: number): void {
        void openAppPicker((app) => {
            updateRule(index, {
                to_app_id: app.bundle_id,
                to_app_name: app.name,
            });
        });
    }

    function clearFrom(index: number): void {
        updateRule(index, { from_app_id: null, from_app_name: null });
    }

    function clearTo(index: number): void {
        updateRule(index, { to_app_id: null, to_app_name: null });
    }

    function toggleAction(index: number): void {
        const current = rules[index].action;
        updateRule(index, {
            action: current === 'notify' ? 'block' : 'notify',
        });
    }

    function isInvalidRule(r: BlockRuleWithId): boolean {
        return r.from_app_id === null && r.to_app_id === null;
    }

    const hasBlockRules = rules.some((r) => r.action === 'block');

    async function refreshAccessibility(): Promise<void> {
        const granted = await invoke<boolean>('check_accessibility');
        setAccessibilityGranted(granted);
    }

    return (
        <main className="container">
            <h1>Clipboard Guard</h1>

            <section className="card">
                <div className="row space-between">
                    <span>Protection</span>
                    <button
                        type="button"
                        className={guardEnabled ? 'btn-on' : 'btn-off'}
                        onClick={(): void => {
                            void toggleGuard();
                        }}
                    >
                        {guardEnabled ? 'Enabled' : 'Disabled'}
                    </button>
                </div>
                <div className="row space-between">
                    <span>Launch at login</span>
                    <button
                        type="button"
                        className={autostartEnabled ? 'btn-on' : 'btn-off'}
                        onClick={(): void => {
                            void toggleAutostart();
                        }}
                    >
                        {autostartEnabled ? 'On' : 'Off'}
                    </button>
                </div>
            </section>

            {hasBlockRules && (
                <section
                    className={`permission-banner ${accessibilityGranted ? 'granted' : 'warning'}`}
                >
                    <div className="row space-between">
                        <span>
                            {accessibilityGranted
                                ? 'Accessibility permission granted'
                                : 'Block rules require Accessibility permission'}
                        </span>
                        {!accessibilityGranted && (
                            <div className="permission-actions">
                                <button
                                    type="button"
                                    className="btn-permission"
                                    onClick={(): void => {
                                        void invoke(
                                            'open_accessibility_settings',
                                        );
                                    }}
                                >
                                    Grant
                                </button>
                                <button
                                    type="button"
                                    className="btn-refresh"
                                    onClick={(): void => {
                                        void refreshAccessibility();
                                    }}
                                >
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
                        key={rule.id}
                        className={`rule-row ${isInvalidRule(rule) ? 'rule-invalid' : ''}`}
                    >
                        <div className="rule-fields">
                            <div className="rule-field">
                                <span className="rule-label">From</span>
                                <span className="rule-app">
                                    {rule.from_app_name ?? 'Any App'}
                                </span>
                                <div className="rule-btns">
                                    <button
                                        type="button"
                                        className="btn-browse"
                                        onClick={(): void => {
                                            browseFrom(i);
                                        }}
                                    >
                                        Browse
                                    </button>
                                    {rule.from_app_id && (
                                        <button
                                            type="button"
                                            className="btn-clear"
                                            onClick={(): void => {
                                                clearFrom(i);
                                            }}
                                        >
                                            x
                                        </button>
                                    )}
                                </div>
                            </div>

                            <span className="rule-arrow">→</span>

                            <div className="rule-field">
                                <span className="rule-label">To</span>
                                <span className="rule-app">
                                    {rule.to_app_name ?? 'All Apps'}
                                </span>
                                <div className="rule-btns">
                                    <button
                                        type="button"
                                        className="btn-browse"
                                        onClick={(): void => {
                                            browseTo(i);
                                        }}
                                    >
                                        Browse
                                    </button>
                                    {rule.to_app_id && (
                                        <button
                                            type="button"
                                            className="btn-clear"
                                            onClick={(): void => {
                                                clearTo(i);
                                            }}
                                        >
                                            x
                                        </button>
                                    )}
                                </div>
                            </div>
                        </div>

                        <div className="rule-actions">
                            <button
                                type="button"
                                className={`action-toggle ${rule.action === 'block' ? 'action-block' : 'action-notify'}`}
                                onClick={(): void => {
                                    toggleAction(i);
                                }}
                            >
                                {rule.action === 'notify' ? 'Notify' : 'Block'}
                            </button>

                            <button
                                type="button"
                                className="btn-remove"
                                onClick={(): void => {
                                    removeRule(i);
                                }}
                            >
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
                <button
                    type="button"
                    className="btn-add"
                    onClick={addRule}
                >
                    + Add Rule
                </button>
            </section>

            <section className="card">
                <h2>Last Clipboard Source</h2>
                {lastSource ? (
                    <p className="source-info">
                        {lastSource.source_app_name ??
                            lastSource.source_app_id ??
                            'Unknown'}
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
                                <strong>
                                    {w.source_app_name ?? 'Unknown'}
                                </strong>
                                {' → '}
                                <strong>{w.dest_app_name ?? 'Terminal'}</strong>
                            </li>
                        ))}
                    </ul>
                )}
            </section>
            {appPickerOpen && (
                <AppPickerModal
                    appList={appList}
                    onSelect={selectApp}
                    onClose={closeAppPicker}
                    isWindowsPlatform={isWindowsPlatform}
                />
            )}
        </main>
    );
}

export { App };
