import { useEffect, useState } from "react";

import { listProviderModels, setApiKey, testProvider } from "@/lib/commands";
import type { ModelSelection, ProviderId } from "@/lib/ipc";
import { useAppStore } from "@/state/app";

/**
 * AI provider configuration (design §4.6): provider + model dropdown pairs
 * for the quality and fast slots (fast collapses to "same as quality"),
 * per-provider API key stored in the OS vault, and a Test button that
 * reports measured first-token latency.
 */

function SlotEditor({
  label,
  value,
  onChange,
}: {
  label: string;
  value: ModelSelection;
  onChange: (next: ModelSelection) => void;
}) {
  const registry = useAppStore((s) => s.registry);
  const keyStatus = useAppStore((s) => s.keyStatus);
  const [liveModels, setLiveModels] = useState<string[]>([]);
  const provider = registry.find((p) => p.id === value.provider);

  // Curated defaults always present; live list merges in when fetchable.
  const curated = provider
    ? [...new Set([provider.default_quality_model, provider.default_fast_model])]
    : [];
  const models = [...new Set([...curated, ...liveModels, value.model])];

  useEffect(() => {
    setLiveModels([]);
    if (!keyStatus[value.provider]) return;
    let cancelled = false;
    listProviderModels(value.provider)
      .then((list) => {
        if (!cancelled) setLiveModels(list.map((m) => m.id));
      })
      .catch(() => {
        /* curated defaults remain */
      });
    return () => {
      cancelled = true;
    };
  }, [value.provider, keyStatus]);

  const onProviderChange = (id: ProviderId) => {
    const next = registry.find((p) => p.id === id);
    if (!next) return;
    onChange({ provider: id, model: next.default_quality_model });
  };

  return (
    <div className="flex min-w-0 flex-1 items-end gap-2">
      <label className="flex min-w-0 flex-1 flex-col gap-1 text-xs text-fg-muted">
        {label} — provider
        <select
          className="rounded-md border border-border bg-bg px-2 py-1.5 text-xs text-fg"
          value={value.provider}
          onChange={(e) => onProviderChange(e.target.value as ProviderId)}
        >
          {registry.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
              {p.is_local ? " (local)" : ""}
            </option>
          ))}
        </select>
      </label>
      <label className="flex min-w-0 flex-1 flex-col gap-1 text-xs text-fg-muted">
        model
        <select
          className="rounded-md border border-border bg-bg px-2 py-1.5 text-xs text-fg"
          value={value.model}
          onChange={(e) => onChange({ ...value, model: e.target.value })}
        >
          {models.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}

export function AiSettings() {
  const config = useAppStore((s) => s.config);
  const registry = useAppStore((s) => s.registry);
  const keyStatus = useAppStore((s) => s.keyStatus);
  const updateConfig = useAppStore((s) => s.updateConfig);
  const refreshKeyStatus = useAppStore((s) => s.refreshKeyStatus);

  const [keyInput, setKeyInput] = useState("");
  const [keyBusy, setKeyBusy] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);

  if (!config) return null;
  const quality = config.llm_quality;
  const fastSameAsQuality = config.llm_fast === null;
  const provider = registry.find((p) => p.id === quality.provider);
  const hasKey = keyStatus[quality.provider] ?? false;

  const saveKey = async () => {
    setKeyBusy(true);
    setTestResult(null);
    try {
      await setApiKey(quality.provider, keyInput.trim());
      setKeyInput("");
      await refreshKeyStatus();
    } catch (e) {
      setTestResult(String(e));
    } finally {
      setKeyBusy(false);
    }
  };

  const runTest = async () => {
    setKeyBusy(true);
    setTestResult("testing…");
    try {
      const ms = await testProvider(quality.provider, quality.model);
      setTestResult(`✓ first token in ${ms} ms`);
    } catch (e) {
      setTestResult(String(e));
    } finally {
      setKeyBusy(false);
    }
  };

  return (
    <div className="mt-3 border-t border-border pt-3">
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-fg-muted">
        AI — answers &amp; suggestions
      </h3>
      <div className="flex flex-col gap-2">
        <SlotEditor
          label="Quality slot (on-demand assists)"
          value={quality}
          onChange={(next) => {
            void updateConfig({
              llm_quality: next,
              // Keep a mirrored fast slot in sync with a provider change.
              llm_fast: fastSameAsQuality ? null : config.llm_fast,
            });
          }}
        />
        <label className="flex items-center gap-2 text-xs text-fg-muted">
          <input
            type="checkbox"
            checked={fastSameAsQuality}
            onChange={(e) =>
              void updateConfig({
                llm_fast: e.target.checked
                  ? null
                  : {
                      provider: quality.provider,
                      model:
                        provider?.default_fast_model ?? quality.model,
                    },
              })
            }
          />
          Fast slot (proactive suggestions) — same as quality
        </label>
        {!fastSameAsQuality && config.llm_fast && (
          <SlotEditor
            label="Fast slot"
            value={config.llm_fast}
            onChange={(next) => void updateConfig({ llm_fast: next })}
          />
        )}

        <div className="flex items-end gap-2">
          <label className="flex min-w-0 flex-1 flex-col gap-1 text-xs text-fg-muted">
            {provider?.name ?? "Provider"} API key{" "}
            {provider?.requires_api_key === false
              ? "(not required)"
              : hasKey
                ? "· saved ✓"
                : "· not set"}
            <input
              type="password"
              value={keyInput}
              onChange={(e) => setKeyInput(e.target.value)}
              placeholder={hasKey ? "Replace stored key…" : "Paste API key…"}
              className="rounded-md border border-border bg-bg px-2 py-1.5 text-xs text-fg placeholder:text-fg-faint"
            />
          </label>
          <button
            type="button"
            disabled={keyBusy || keyInput.trim() === ""}
            onClick={() => void saveKey()}
            className="rounded-md border border-border px-3 py-1.5 text-xs text-fg-muted hover:text-fg disabled:opacity-50"
          >
            Save key
          </button>
          <button
            type="button"
            disabled={keyBusy || (!hasKey && provider?.requires_api_key !== false)}
            onClick={() => void runTest()}
            className="rounded-md border border-ok/40 px-3 py-1.5 text-xs text-ok hover:bg-ok/10 disabled:opacity-50"
          >
            Test
          </button>
        </div>
        {testResult && (
          <p
            className={`text-xs ${testResult.startsWith("✓") ? "text-ok" : "text-fg-muted"}`}
            role="status"
          >
            {testResult}
          </p>
        )}
        <p className="text-[11px] text-fg-faint">
          Keys are stored in the Windows Credential Manager, never in files.
          Transcript text is sent to the selected provider only when you ask
          for an assist.
        </p>
      </div>
    </div>
  );
}
