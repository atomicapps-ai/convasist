import { useCallback, useEffect, useState } from "react";

import { Section, ViewShell } from "@/components/studio/ViewShell";
import { Icon } from "@/components/ui/Icon";
import { useBackend } from "@/lib/backend";
import type { KnowledgeProfile, RagDocument, SimConSession } from "@/lib/ipc";
import { useNavStore } from "@/state/nav";
import { useRehearsalStore } from "@/state/rehearsal";

/**
 * Sim Con detail — the persona step (Step 3) and the launch point for the live
 * session (Step 4, next phase). Generate 3 counterparty personas from the
 * knowledge base, pick one, then Start. Edit reopens the setup wizard.
 */
export function SimConDetail({
  id,
  onEdit,
  onBack,
}: {
  id: string;
  onEdit: () => void;
  onBack: () => void;
}) {
  const backend = useBackend();
  const [session, setSession] = useState<SimConSession | null>(null);
  const [profile, setProfile] = useState<KnowledgeProfile | null>(null);
  const [docs, setDocs] = useState<RagDocument[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    backend.simcon
      .load(id)
      .then(setSession)
      .catch(() => setError("Couldn't load this Sim Con."));
  }, [backend, id]);

  useEffect(() => {
    load();
  }, [load]);

  // Load the knowledge base (attached docs + researched sources) so the user
  // can see exactly what grounds the rehearsal — including what Ally found.
  const profileId = session?.knowledge_profile_id ?? null;
  useEffect(() => {
    if (!profileId) {
      setProfile(null);
      return;
    }
    void backend.simcon.loadProfile(profileId).then(setProfile).catch(() => {});
    void backend.rag.list().then(setDocs).catch(() => {});
  }, [backend, profileId]);

  const docName = (docId: string) =>
    docs.find((d) => d.id === docId)?.file_name ?? docId;

  // ── Ally prep dossier ─────────────────────────────────────────────────────
  const dossierId = session?.dossier_doc_id ?? null;
  const [dossierBusy, setDossierBusy] = useState(false);
  const [dossierText, setDossierText] = useState<string | null>(null);
  const [showDossier, setShowDossier] = useState(false);

  const generateDossier = async () => {
    setDossierBusy(true);
    setError(null);
    try {
      const updated = await backend.simcon.generateDossier(id);
      setSession(updated);
      setShowDossier(true);
      // Load the freshly written document so it shows inline right away.
      if (updated.dossier_doc_id) {
        setDossierText(
          (await backend.rag.documentText(updated.dossier_doc_id)) ?? "",
        );
      }
      // Refresh the knowledge base doc list — non-fatal if it fails.
      if (updated.knowledge_profile_id) {
        try {
          setProfile(
            await backend.simcon.loadProfile(updated.knowledge_profile_id),
          );
          setDocs(await backend.rag.list());
        } catch {
          /* the dossier still generated; ignore a refresh hiccup */
        }
      }
    } catch (e) {
      setError(
        `Couldn't generate the prep document: ${String(e).replace(/^Error:\s*/, "")}`,
      );
    } finally {
      setDossierBusy(false);
    }
  };

  const toggleDossier = async () => {
    const next = !showDossier;
    setShowDossier(next);
    if (next && dossierText === null && dossierId) {
      setDossierText((await backend.rag.documentText(dossierId)) ?? "");
    }
  };

  const generate = async () => {
    setBusy(true);
    setError(null);
    try {
      setSession(await backend.simcon.generatePersonas(id));
    } catch {
      setError(
        "Couldn't generate personas — check your Ally provider key in Settings.",
      );
    } finally {
      setBusy(false);
    }
  };

  const choose = async (pid: string) => {
    try {
      setSession(await backend.simcon.choosePersona(id, pid));
    } catch {
      /* best-effort */
    }
  };

  const personas = session?.personas ?? [];
  const chosen = session?.chosen_persona_id ?? null;
  const chosenPersona = personas.find((p) => p.id === chosen) ?? null;

  // ── Live rehearsal (Step 4) ───────────────────────────────────────────────
  // Launches into the real cockpit (transcript · spine · Ally); the floating
  // RehearsalBar carries the live controls from there.
  const setView = useNavStore((s) => s.setView);
  const beginRehearsal = useRehearsalStore((s) => s.begin);
  const [starting, setStarting] = useState(false);
  const [rehearsalError, setRehearsalError] = useState<string | null>(null);

  const startRehearsal = async () => {
    setStarting(true);
    setRehearsalError(null);
    try {
      await backend.simcon.startRehearsal(id);
      beginRehearsal(chosenPersona?.title ?? "Counterparty");
      setView("live");
    } catch (e) {
      setRehearsalError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setStarting(false);
    }
  };

  return (
    <ViewShell
      icon="simicon"
      title={session?.title || "Sim Con"}
      subtitle={session?.purpose || "Rehearse a high-stakes call."}
      actions={
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={onEdit}
            title="Edit setup"
            aria-label="Edit setup"
            className="rounded-sm p-1.5 text-fg-faint transition hover:bg-panel-raised/60 hover:text-fg"
          >
            <Icon name="edit" size={15} />
          </button>
          <button type="button" className="btn" onClick={onBack}>
            Back
          </button>
        </div>
      }
    >
      {error && (
        <Section title="Sim Con">
          <p className="text-sm text-rec">{error}</p>
        </Section>
      )}

      <Section
        title="Counterparty"
        description="Choose who you'll rehearse against — the AI plays this persona, grounded in your knowledge base."
      >
        {personas.length === 0 ? (
          <div className="flex flex-col gap-2">
            <p className="text-sm text-fg-muted">
              Generate the personas conva thinks you should be ready to face.
            </p>
            <button
              type="button"
              className="btn btn-primary self-start"
              disabled={busy}
              onClick={() => void generate()}
            >
              {busy ? "Generating…" : "Generate personas"}
            </button>
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            <ul className="flex flex-col gap-2">
              {personas.map((p) => {
                const isChosen = chosen === p.id;
                return (
                  <li
                    key={p.id}
                    className={`rounded border p-3 transition ${
                      isChosen
                        ? "border-outbound/50 bg-outbound/[0.08]"
                        : "border-border"
                    }`}
                  >
                    <div className="flex items-start gap-2">
                      <h3 className="min-w-0 flex-1 text-sm font-bold tracking-tight text-fg">
                        {p.title}
                      </h3>
                      {p.recommended && (
                        <span className="shrink-0 rounded-full border border-ai/40 bg-ai/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-ai">
                          ★ Recommended
                        </span>
                      )}
                    </div>
                    <p className="mt-1 text-[12px] leading-relaxed text-fg-muted">
                      {p.summary}
                    </p>
                    {p.style_tags.length > 0 && (
                      <div className="mt-2 flex flex-wrap gap-1">
                        {p.style_tags.map((t) => (
                          <span
                            key={t}
                            className="rounded-sm border border-border px-1.5 py-0.5 text-[10px] text-fg-faint"
                          >
                            {t}
                          </span>
                        ))}
                      </div>
                    )}
                    <div className="mt-2">
                      <button
                        type="button"
                        className={`btn ${isChosen ? "btn-primary" : ""}`}
                        onClick={() => void choose(p.id)}
                      >
                        {isChosen ? "Chosen ✓" : "Choose"}
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
            <button
              type="button"
              className="btn self-start"
              disabled={busy}
              onClick={() => void generate()}
            >
              {busy ? "Regenerating…" : "Regenerate"}
            </button>
          </div>
        )}
      </Section>

      <Section
        title="Knowledge base"
        description="What grounds this rehearsal — your attached documents plus anything Ally researched. The AI persona draws on all of it."
      >
        {!profile ? (
          <p className="text-[12px] text-fg-faint">
            {profileId
              ? "Loading…"
              : "Not prepared yet — finish setup to build the knowledge base."}
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {/* Ally prep dossier — a document Ally writes from the material. */}
            <div className="rounded-lg border border-ai/30 bg-ai/[0.06] p-3">
              <div className="flex items-center gap-2">
                <Icon name="simicon" size={15} className="shrink-0 text-ai" />
                <span className="text-[12px] font-semibold text-fg">
                  Ally prep document
                </span>
                <div className="flex-1" />
                {dossierId && (
                  <button
                    type="button"
                    onClick={() => void toggleDossier()}
                    className="rounded-sm px-2 py-0.5 text-[11px] font-semibold text-ai hover:bg-ai/10"
                  >
                    {showDossier ? "Hide" : "View"}
                  </button>
                )}
                <button
                  type="button"
                  disabled={dossierBusy}
                  onClick={() => void generateDossier()}
                  className="rounded-sm border border-ai/40 px-2 py-0.5 text-[11px] font-semibold text-ai hover:bg-ai/10 disabled:opacity-40"
                >
                  {dossierBusy
                    ? "Writing…"
                    : dossierId
                      ? "Regenerate"
                      : "Generate"}
                </button>
              </div>
              <p className="mt-1 text-[11px] leading-relaxed text-fg-faint">
                Ally reads your documents + research and writes a briefing (likely
                questions, talking points, background). Saved to your Library and
                used to ground the rehearsal.
              </p>
              {dossierId && showDossier && (
                <pre className="mt-2 max-h-[40vh] overflow-y-auto whitespace-pre-wrap rounded border border-border bg-bg/50 p-2.5 text-[12px] leading-relaxed text-fg-muted">
                  {dossierText === null
                    ? "Loading…"
                    : dossierText.trim() === ""
                      ? "(No content returned — try Regenerate.)"
                      : dossierText}
                </pre>
              )}
            </div>

            {/* Attached documents (these live in your Library too). The dossier
                is shown above, so it's excluded here. */}
            {(() => {
              const attached = profile.doc_ids.filter((d) => d !== dossierId);
              return (
                <div>
                  <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-fg-faint">
                    Documents ({attached.length})
                  </h3>
                  {attached.length === 0 ? (
                    <p className="text-[12px] text-fg-faint">
                      No documents attached.
                    </p>
                  ) : (
                    <ul className="flex flex-col gap-0.5">
                      {attached.map((docId) => (
                        <li
                          key={docId}
                          className="flex items-center gap-1.5 text-[12px] text-fg-muted"
                        >
                          <Icon
                            name="book"
                            size={13}
                            className="shrink-0 text-fg-faint"
                          />
                          <span className="truncate">{docName(docId)}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              );
            })()}

            {/* Web research Ally collected — links out to each source. */}
            <div>
              <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-fg-faint">
                Ally research ({profile.research.length})
              </h3>
              {profile.research.length === 0 ? (
                <p className="text-[12px] text-fg-faint">
                  No web research — grounded on your documents only.
                </p>
              ) : (
                <ul className="flex flex-col gap-2">
                  {profile.research.map((src, i) => (
                    <li key={`${src.url}-${i}`} className="text-[12px]">
                      <button
                        type="button"
                        onClick={() => void backend.auth.openUrl(src.url)}
                        title={src.url}
                        className="text-left font-medium text-ai hover:underline"
                      >
                        {src.title || src.url}
                      </button>
                      {src.snippet && (
                        <p className="mt-0.5 line-clamp-2 text-fg-faint">
                          {src.snippet}
                        </p>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        )}
      </Section>

      <Section
        title="Rehearse"
        description="Opens the live cockpit — transcript, spine, and Ally. Speak your side out loud; pause and the persona replies in character and speaks back. Use a headset so it doesn't hear its own voice."
      >
        <div className="flex flex-col gap-2">
          <button
            type="button"
            className="btn btn-primary self-start"
            disabled={!chosen || starting}
            onClick={() => void startRehearsal()}
          >
            {starting ? "Starting…" : "Start rehearsal"}
          </button>
          {!chosen && (
            <p className="text-[11px] text-fg-faint">
              Choose a persona above to rehearse against.
            </p>
          )}
          {rehearsalError && (
            <p className="text-[12px] text-rec" role="alert">
              {rehearsalError}
            </p>
          )}
        </div>
      </Section>
    </ViewShell>
  );
}
