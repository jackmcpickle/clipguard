import { type ReactElement, useCallback, useRef, useState } from 'react';

interface AppBundleInfo {
    bundle_id: string;
    name: string;
}

interface AppPickerModalProps {
    appList: AppBundleInfo[];
    onSelect: (app: AppBundleInfo) => void;
    onClose: () => void;
    isWindowsPlatform: boolean;
}

function AppPickerModal({
    appList,
    onSelect,
    onClose,
    isWindowsPlatform,
}: AppPickerModalProps): ReactElement {
    const [search, setSearch] = useState('');
    const [error, setError] = useState<string | null>(null);
    const searchInputRef = useRef<HTMLInputElement>(null);

    const parseManualAppId = useCallback(
        (raw: string): { value: string | null; error: string | null } => {
            const trimmed = raw.trim().replace(/^['"]+|['"]+$/g, '');
            if (!trimmed) {
                return { value: null, error: 'Enter an app id' };
            }

            if (!isWindowsPlatform) {
                return { value: trimmed, error: null };
            }

            if (trimmed.includes('\\') || trimmed.includes('/')) {
                return {
                    value: null,
                    error: 'Use exe name only (example: msedge.exe)',
                };
            }

            const normalized = trimmed.toLowerCase();
            if (!normalized.endsWith('.exe')) {
                return {
                    value: null,
                    error: 'Windows app id must end with .exe',
                };
            }

            return { value: normalized, error: null };
        },
        [isWindowsPlatform],
    );

    const submitPickerInput = useCallback((): void => {
        const query = search.trim();
        if (!query) {
            return;
        }

        const exact = appList.find(
            (a) =>
                a.bundle_id.toLowerCase() === query.toLowerCase() ||
                a.name.toLowerCase() === query.toLowerCase(),
        );
        if (exact) {
            setError(null);
            onSelect(exact);
            return;
        }

        const manual = parseManualAppId(query);
        if (manual.error || !manual.value) {
            setError(manual.error);
            return;
        }

        setError(null);
        onSelect({ bundle_id: manual.value, name: manual.value });
    }, [appList, search, parseManualAppId, onSelect]);

    const filteredApps = appList.filter((a) => {
        const q = search.toLowerCase();
        return (
            a.name.toLowerCase().includes(q) ||
            a.bundle_id.toLowerCase().includes(q)
        );
    });

    const manualPreview = parseManualAppId(search);

    return (
        <div
            role="presentation"
            className="app-picker-overlay"
            onClick={onClose}
        >
            <div
                role="dialog"
                className="app-picker-modal"
                onClick={(e): void => {
                    e.stopPropagation();
                }}
                onKeyDown={(e): void => {
                    e.stopPropagation();
                }}
            >
                <input
                    ref={searchInputRef}
                    className="app-picker-search"
                    type="text"
                    placeholder={
                        appList.length > 0
                            ? 'Search apps or type app id...'
                            : 'Enter app id...'
                    }
                    value={search}
                    onChange={(e): void => {
                        setSearch(e.target.value);
                        if (error) {
                            setError(null);
                        }
                    }}
                    onKeyDown={(e): void => {
                        if (e.key === 'Escape') onClose();
                        if (e.key === 'Enter') {
                            submitPickerInput();
                        }
                    }}
                />
                <div className="app-picker-list">
                    {search.trim() && (
                        <button
                            type="button"
                            className="app-picker-item app-picker-manual"
                            onClick={submitPickerInput}
                        >
                            <span className="app-picker-name">
                                Use manual entry
                            </span>
                            <span className="app-picker-id">
                                {manualPreview.value ?? search.trim()}
                            </span>
                        </button>
                    )}

                    {filteredApps.map((app) => (
                        <button
                            type="button"
                            key={app.bundle_id}
                            className="app-picker-item"
                            onClick={(): void => {
                                onSelect(app);
                            }}
                        >
                            <span className="app-picker-name">{app.name}</span>
                            <span className="app-picker-id">
                                {app.bundle_id}
                            </span>
                        </button>
                    ))}

                    {filteredApps.length === 0 && (
                        <div className="app-picker-hint">
                            {appList.length === 0
                                ? 'No detected apps. Type an app id and press Enter'
                                : 'No matches. Press Enter for manual entry'}
                        </div>
                    )}
                </div>
                {error && <div className="app-picker-error">{error}</div>}
            </div>
        </div>
    );
}

export { AppPickerModal };
export type { AppBundleInfo };
