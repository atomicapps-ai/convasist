import { useEffect, useState } from "react";

import { Section, ViewShell } from "@/components/studio/ViewShell";
import { useBackend } from "@/lib/backend";
import type { RagDocument, SimConCategory, SimConSession } from "@/lib/ipc";
import { isDesktop } from "@/lib/platform";

const CATEGORIES: { value: SimConCategory; label: string; hint: string }[] = [
  { value: "interview", label: "Interview", hint: "Job or panel interview" },
  { value: "financial_review", label: "Financial review", hint: "Numbers, GAAP, forecasts" },
  { value: "performance_review", label: "Performance review", hint: "1:1, feedback, promotion" },
  { value: "sales_pitch", label: "Sales pitch", hint: "Demo, objection handling" },
  { value: "other", label: "Other", hint: "Anything high-stakes" },
];

const DOC_EXTENSIONS = ["pdf", "docx", "md", "txt", "html"];
const STEP_LABEL = ["the basics", "context & documents", "review"];

/**
 * Sim Con setup wizard (Step 1). Collects name, goal, type (and, for interviews,
 * the job description), plus context: Path A attaches library documents — you can
 * add new files directly, which land in a folder named after the Sim Con — and
 * Path B asks Ally to auto-generate context. Finishing saves a draft
 * SimConSession; the ingestion + research phase (C) consumes it.
 */
export function SimConSetup({
  initial,
  onDone,
  onCancel,
}: {
  initial?: SimConSession;
  onDone: () => void;
  onCancel: () => void;
}) {
  const backend = useBackend();
  const [step, setStep] = useState(1);
  const [title, setTitle] = useState(initial?.title ?? "");
  const [purpose, setPurpose] = useState(initial?.purpose ?? "");
  const [category, setCategory] = useState<SimConCategory>(
    initial?.category ?? "interview",
  );
  const [jobDescription, setJobDescription] = useState(
    initial?.job_description ?? "",
  );
  const [docs, setDocs] = useState<RagDocument[]>([]);
  const [selected, setSelected] = useState<string[]>(
    initial?.source_doc_ids ?? [],
  );
  const [autoGen, setAutoGen] = useState(initial?.auto_generate_context ?? true);
  const [adding, setAdding] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    backend.rag
      .list()
      .then(setDocs)
      .catch(() => setDocs([]));
  }, [backend]);

  const toggleDoc = (id: string) =>
    setSelected((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]));

  // Path A — add files directly: copy them into this Sim Con's folder, then
  // ingest into the RAG library so the counterparty is grounded in them.
  const addDocuments = async () => {
    setAdding(true);
    setError(null);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: true,
        filters: [{ name: "Documents", extensions: DOC_EXTENSIONS }],
      });
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      if (paths.length === 0) return;
      const stored = await backend.simcon.storeDocs(title.trim() || "untitled", paths);
      const reports = await backend.rag.ingest(stored);
      const newIds = reports.map((r) => r.document.id);
      setDocs(await backend.rag.list());
      setSelected((s) => Array.from(new Set([...s, ...newIds])));
    } catch {
      setError("Couldn't add documents.");
    } finally {
      setAdding(false);
    }
  };

  const canNext = step === 1 ? title.trim().length > 0 : true;

  const finish = async () => {
    setSaving(true);
    setError(null);
    try {
      const saved = await backend.simcon.save({
        id: initial?.id ?? "",
        title: title.trim(),
        purpose: purpose.trim(),
        job_description: jobDescription.trim() ? jobDescription.trim() : null,
        category,
        status: initial?.status ?? "draft",
        created_at_unix_ms: initial?.created_at_unix_ms ?? 0,
        updated_at_unix_ms: 0,
        source_doc_ids: selected,
        auto_generate_context: autoGen,
        knowledge_profile_id: initial?.knowledge_profile_id ?? null,
        personas: initial?.personas ?? [],
        chosen_persona_id: initial?.chosen_persona_id ?? null,
        conversation_id: initial?.conversation_id ?? null,
        dossier_doc_id: initial?.dossier_doc_id ?? null,
      });
      // Build the knowledge base (attached docs + research) and mark it ready.
      await backend.simcon.prepare(saved.id);
      onDone();
    } catch {
      setError("Couldn't save — Sim Con runs on the desktop app.");
      setSaving(false);
    }
  };

  return (
    <ViewShell
      icon="simicon"
      title={initial ? "Edit Sim Con" : "New Sim Con"}
      subtitle={`Step ${step} of 3 — ${STEP_LABEL[step - 1]}`}
      actions={
        <button type="button" className="btn" onClick={onCancel}>
          Cancel
        </button>
      }
    >
      {step === 1 && (
        <Section title="What are you rehearsing?">
          <div className="flex flex-col gap-3">
            <label className="field">
              Name
              <input
                className="input"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="Senior Accountant interview with the CFO"
              />
            </label>
            <label className="field">
              Goal
              <textarea
                className="input"
                rows={3}
                value={purpose}
                onChange={(e) => setPurpose(e.target.value)}
                placeholder="Prep for technical GAAP questions and leadership scenarios"
              />
            </label>
            <div className="field">
              Type
              <div className="flex flex-wrap gap-2">
                {CATEGORIES.map((c) => (
                  <button
                    key={c.value}
                    type="button"
                    title={c.hint}
                    onClick={() => setCategory(c.value)}
                    className={`btn ${category === c.value ? "btn-primary" : ""}`}
                  >
                    {c.label}
                  </button>
                ))}
              </div>
            </div>
            {category === "interview" && (
              <label className="field">
                Job description
                <textarea
                  className="input"
                  rows={4}
                  value={jobDescription}
                  onChange={(e) => setJobDescription(e.target.value)}
                  placeholder="Paste the role's job description — conva grounds the interviewer's questions in it."
                />
              </label>
            )}
          </div>
        </Section>
      )}

      {step === 2 && (
        <>
          <Section
            title="Documents"
            description="conva grounds the counterparty and its questions in these. Add files directly (they're kept in a folder named after this Sim Con) or pick from your library."
          >
            {isDesktop && (
              <div className="mb-3">
                <button
                  type="button"
                  className="btn"
                  disabled={adding}
                  onClick={() => void addDocuments()}
                >
                  {adding ? "Adding…" : "Add documents…"}
                </button>
              </div>
            )}
            {docs.length === 0 ? (
              <p className="text-sm text-fg-muted">
                No documents yet — add some above, or let Ally research context
                below.
              </p>
            ) : (
              <ul className="flex flex-col divide-y divide-border">
                {docs.map((d) => (
                  <li key={d.id} className="flex items-center gap-3 py-2">
                    <input
                      type="checkbox"
                      checked={selected.includes(d.id)}
                      onChange={() => toggleDoc(d.id)}
                      aria-label={`Attach ${d.file_name}`}
                    />
                    <span className="min-w-0 flex-1 truncate text-sm text-fg">
                      {d.file_name}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </Section>
          <Section
            title="Let Ally research"
            description="Ally searches the web for relevant background — standard questions, company profile, market rates — and indexes it alongside your docs."
          >
            <label className="flex items-center gap-2 text-sm text-fg">
              <input
                type="checkbox"
                checked={autoGen}
                onChange={(e) => setAutoGen(e.target.checked)}
              />
              Auto-generate context for this Sim Con
            </label>
          </Section>
        </>
      )}

      {step === 3 && (
        <Section title="Review">
          <dl className="grid grid-cols-[7rem_1fr] gap-x-4 gap-y-1.5 text-sm">
            <dt className="text-fg-faint">Name</dt>
            <dd className="text-fg">{title || "—"}</dd>
            <dt className="text-fg-faint">Goal</dt>
            <dd className="text-fg-muted">{purpose || "—"}</dd>
            <dt className="text-fg-faint">Type</dt>
            <dd className="text-fg">
              {CATEGORIES.find((c) => c.value === category)?.label}
            </dd>
            {category === "interview" && (
              <>
                <dt className="text-fg-faint">Job description</dt>
                <dd className="text-fg-muted">
                  {jobDescription.trim() ? "Provided" : "—"}
                </dd>
              </>
            )}
            <dt className="text-fg-faint">Documents</dt>
            <dd className="text-fg">{selected.length} attached</dd>
            <dt className="text-fg-faint">Ally research</dt>
            <dd className="text-fg">{autoGen ? "On" : "Off"}</dd>
          </dl>
          <p className="mt-3 text-[12px] leading-relaxed text-fg-faint">
            Finishing saves this Sim Con. Building the knowledge base, generating
            personas, and the live session come next.
          </p>
          {error && <p className="mt-2 text-sm text-rec">{error}</p>}
        </Section>
      )}

      <div className="mt-4 flex items-center gap-2">
        {step > 1 && (
          <button
            type="button"
            className="btn"
            onClick={() => setStep((s) => s - 1)}
          >
            Back
          </button>
        )}
        <span className="ml-auto" />
        {step < 3 ? (
          <button
            type="button"
            className="btn btn-primary"
            disabled={!canNext}
            onClick={() => setStep((s) => s + 1)}
          >
            Next
          </button>
        ) : (
          <button
            type="button"
            className="btn btn-primary"
            disabled={saving}
            onClick={() => void finish()}
          >
            {saving ? "Preparing…" : "Finish"}
          </button>
        )}
      </div>
    </ViewShell>
  );
}
