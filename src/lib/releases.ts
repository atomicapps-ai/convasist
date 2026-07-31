/**
 * Release notes — the data behind the in-app "What's New" view.
 *
 * REGENERATED per release from the Conventional-Commit log (SDLC §3.2/§4.3):
 * the release pipeline runs git-cliff between the previous tag and `vX.Y.Z` to
 * produce the canonical CHANGELOG.md / GitHub Release body, and seeds a new
 * entry here; the owner edits the `summary` line for human framing on the
 * release branch. This module is the curated in-app slice of those notes — the
 * generated CHANGELOG stays the machine-of-record.
 *
 * Newest first. The entry whose `version` matches the running build
 * (`BUILD.version`, from vite `define`) is highlighted as "You're running this"
 * in {@link WhatsNewView}.
 */

export interface ReleaseSection {
  /** e.g. "Highlights" | "Features" | "Fixes" | "Performance". */
  title: string;
  items: string[];
}

export interface Release {
  /** Semver without a leading `v`, e.g. "0.1.1" — matches `BUILD.version`. */
  version: string;
  /** ISO date the version was tagged, e.g. "2026-07-31". */
  date: string;
  /** One-line human framing (owner-edited on the release branch). */
  summary?: string;
  sections: ReleaseSection[];
}

export const RELEASES: Release[] = [
  {
    version: "0.1.1",
    date: "2026-07-31",
    summary:
      "The current desktop baseline — both-sides capture, on-device transcription, and Ally grounded in your own library.",
    sections: [
      {
        title: "Highlights",
        items: [
          "Live dual-column transcription of both sides of a call, captured from your system audio — no bot joins the meeting.",
          "Ally answers grounded in your document vault, with a citation on every reply.",
          "On-device by default: audio, transcripts, and your library stay on your machine.",
          "Every build now shows its version and commit in the status bar and Settings → About, so you always know exactly what you're running.",
        ],
      },
    ],
  },
];
