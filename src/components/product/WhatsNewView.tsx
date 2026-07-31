import { Section, ViewShell } from "@/components/studio/ViewShell";
import { BUILD } from "@/lib/debug";
import { RELEASES } from "@/lib/releases";

/**
 * "What's New" — the release notes for the version the user is running (and the
 * ones before it). Mirrors {@link WhatsComingView} (roadmap) but points the
 * other way: WhatsComing is what's ahead, WhatsNew is what shipped. Data comes
 * from {@link RELEASES}, regenerated per release from git-cliff (SDLC §3.2).
 */
export function WhatsNewView() {
  return (
    <ViewShell
      icon="sparkle"
      title="What's New"
      subtitle="What changed in each release of conva."
      badge={
        <span className="inline-flex items-center rounded-full border border-border-strong bg-panel-raised/60 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-fg-faint">
          You're on v{BUILD.version}
        </span>
      }
    >
      {RELEASES.map((rel) => {
        const current = rel.version === BUILD.version;
        return (
          <Section
            key={rel.version}
            title={`v${rel.version}`}
            description={
              rel.summary ? `${rel.date} — ${rel.summary}` : rel.date
            }
          >
            {current && (
              <p className="mb-3 inline-flex items-center gap-1.5 rounded-full border border-ok/40 bg-ok/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-ok">
                You're running this
              </p>
            )}
            <div className="flex flex-col gap-4">
              {rel.sections.map((sec) => (
                <div key={sec.title} className="flex flex-col gap-1.5">
                  <h3 className="text-[11px] font-semibold uppercase tracking-wide text-fg-faint">
                    {sec.title}
                  </h3>
                  <ul className="flex flex-col gap-1.5">
                    {sec.items.map((item, i) => (
                      <li
                        key={i}
                        className="flex gap-2 text-[12px] leading-relaxed text-fg-muted"
                      >
                        <span aria-hidden className="mt-1.5 h-1 w-1 shrink-0 rounded-full bg-border-strong" />
                        <span className="min-w-0 flex-1">{item}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </div>
          </Section>
        );
      })}

      <p className="px-1 text-[11px] leading-relaxed text-fg-faint">
        The full technical changelog for every version lives at{" "}
        <span className="font-mono text-fg-muted">getconva.com/docs/latest</span>{" "}
        and on each version's GitHub release.
      </p>
    </ViewShell>
  );
}
