# Roadmap

Where `gcp-racoon` is heading. This is a directional plan, not a promise of
dates: racoon is maintained by a solo developer, releases ship roughly
monthly, and priorities move toward what users actually report. If something
here matters to you, open an issue — real-world reports reorder this list
faster than anything else.

## Where racoon is today (v0.2.0)

- Real-time GCP log streaming in a keyboard-driven TUI, across projects and
  centralized Log Views
- Error Fingerprinting & Spike Radar: repetitive errors cluster into
  canonical patterns with live velocity sparklines
- Incident workflow: surrounding-instance context, distributed trace
  isolation, one-key Markdown Incident Snapshot, post-deployment fingerprint
  `diff`
- AI that is safe to point at production logs: multi-provider analysis
  (Gemini / Vertex AI via ADC, OpenAI, Anthropic, Agnes), natural-language
  filters, a client-side token optimizer, and a PII & secret guard that
  scrubs locally before anything leaves the machine
- Offline mode: export to JSON / NDJSON, replay local files or `stdin`
  (`racoon view -`) with the same TUI
- Distribution: release binaries for macOS / Linux / Windows,
  `cargo install gcp-racoon`, and a Docker image on GHCR

The product surface is broad already. The next stretch of work is therefore
less about new headline features and more about **being easy to try, easy to
trust, and easy to join**.

## Q4 2026 — Packaging & maintenance window

A short, quieter stretch: patch releases only, plus the packaging pieces
that make racoon trivial to pick up.

- [ ] Homebrew tap (`brew tap staninbui/tap && brew install gcp-racoon`),
      replacing the "coming soon" note in the README
- [ ] A 30-second demo recording and screenshots in the README
- [ ] A bundled sample log dataset so anyone can try the TUI offline in
      under a minute — no GCP account required (`racoon view` on the sample)
- [ ] v0.2.x patch releases as issues come in

## Q1 2027 — v0.3: trust & onboarding

- [ ] First-run experience: clearer auth diagnostics and a guided check
      when credentials, project access, or `logging.viewer` are missing
- [ ] Contributor infrastructure: `CONTRIBUTING.md`, issue templates, and a
      labelled set of good first issues
- [ ] Wider automated test coverage around the fragile parts: log parsers,
      the local filter engine, and fingerprinting
- [ ] A performance pass on high-volume streams (large `max_logs`, busy
      multi-project tails), with before/after numbers in the release notes

## Q2 2027 — Workflow depth

- [ ] Custom actions: user-defined commands bound to keys that act on the
      selected log entry (open a trace, copy a `curl`, file a ticket) —
      racoon's take on the k9s plugin idea, sized for a log tool
- [ ] Incident bundle export: one shareable archive containing the raw logs,
      the active filter, the radar summary, and the AI RCA report
- [ ] Shareable saved views: export/import of bookmarks and profiles so a
      team can standardise its triage starting points

## H2 2027 — Community & ecosystem

- [ ] Ingestion presets for common non-GCP sources fed through `stdin`
      (Docker JSON logs, journald, kubectl output)
- [ ] Japanese documentation alongside the English README
- [ ] Write-ups of the design trade-offs (fingerprinting, token budgeting,
      pre-AI redaction) so the reasoning is reviewable, not just the code

## Non-goals

Racoon is a log tool, and it intends to stay one:

- **No GCP resource management.** No scaling, restarting, or deleting
  resources; racoon is read-only against your projects.
- **No hosted SaaS.** Your logs and credentials stay on your machine.
- **No multi-cloud console clone.** Other clouds' logs are welcome through
  `stdin` and files, not through new provider integrations.

## How to influence this roadmap

- Open an issue describing the on-call moment that hurt — those reports
  carry the most weight.
- PRs are welcome; check the good-first-issue label for a bounded start.
- If racoon helped during an incident, a star and a short note about the
  setup (Cloud Run? GKE? multi-project?) genuinely help prioritise.
