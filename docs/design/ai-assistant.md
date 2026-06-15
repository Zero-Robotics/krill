# Design: In-process AI assistant for krill

Status: **Proposed** · Owner: Tommaso · Last updated: 2026-06-15

## 1. Goal

When a service in a krill graph fails, give the user a fast, accurate **triage**:
what happened, the most likely cause, and concrete next checks — assembled from
the context only krill holds (the dependency graph, the cross-service timeline,
exit codes, health config). The assistant is **advisory only**: it never touches
the control loop (no auto-restart, no config edits, no startup gating).

Non-goals: replacing the operator's judgement, acting on the system, or becoming
a general chatbot. krill stays a DAG orchestrator; this is a side-car.

## 2. Why this belongs in krill (and not a copy-paste into a chat window)

The differentiator is **context assembly**, not the model. Most log-AI tooling
sees one stream in isolation. krill knows, at the instant of failure: the
resolved command + env, executor type (pixi/shell/docker), exit code + signal,
restart count vs policy, the health-check config, the service's `krill.yaml`
stanza, **its dependencies and dependents and their states**, and **what other
services logged in the window around the failure**. Robotics bring-up failures
are overwhelmingly cross-cutting and ordering-sensitive ("X aborted because its
dependency Y wasn't ready"), which a single-stream tool structurally cannot
diagnose. The value is in building that bundle; the model (or a rule engine) is
a thin consumer on top.

## 3. Principles

1. **Out of the control path.** Strictly advisory. A wrong suggestion must never
   be able to change system state. At most: "here is a command you could run."
2. **Optional and opt-in.** Compiled behind a feature flag; off by default. The
   daemon and TUI never depend on it for correctness.
3. **Degrades gracefully.** A no-LLM heuristic backend ships first and is always
   available (offline, deterministic). The LLM is an enhancement, not a
   dependency.
4. **Privacy first.** Nothing leaves the process unless explicitly enabled.
   Redact env/secrets. Support a local-model backend for air-gapped setups.
5. **Show the evidence.** Every suggestion renders alongside the context it used
   and a confidence level. No black-box verdicts.

## 4. Architecture

```
                 ┌─────────────────────────────────────────────┐
                 │ krill-daemon (orchestrator)                 │
                 │   owns: DAG, runners, LogStore, timeline    │
                 └───────────────┬─────────────────────────────┘
                                 │ on failure / on request
                                 ▼
                 ┌─────────────────────────────────────────────┐
                 │ IncidentBuilder  (the valuable part)        │
                 │   assembles a structured Incident from      │
                 │   graph + timeline + process + config       │
                 └───────────────┬─────────────────────────────┘
                                 ▼
                 ┌─────────────────────────────────────────────┐
                 │ Advisor trait                               │
                 │   fn diagnose(&self, &Incident) -> Advice   │
                 ├─────────────────────────────────────────────┤
                 │ Heuristic (default, offline, deterministic) │
                 │ Local LLM (Ollama / llama.cpp)              │
                 │ Cloud LLM (Anthropic / OpenAI)              │
                 └───────────────┬─────────────────────────────┘
                                 ▼
                 ┌─────────────────────────────────────────────┐
                 │ TUI panel  /  `krill explain <service>`     │
                 │   renders Advice + evidence + confidence    │
                 └─────────────────────────────────────────────┘
```

New crate: `krill-assist` (feature `assist`, off by default). It depends on
`krill-common` for the shared types; the daemon depends on `krill-assist` only
under the feature flag.

## 5. The `Incident` object (build this first)

This is the substrate. It is independently useful — it improves the dashboard
error display, `krill logs`, and post-mortems — whether or not any AI reads it.
It is serializable (so it can also be written to the session dir for offline
tooling).

```rust
pub struct Incident {
    pub service: String,
    pub command: String,            // resolved argv
    pub cwd: String,
    pub executor: String,           // pixi | shell | docker | ...
    pub env: Vec<(String, String)>, // REDACTED (see §8)

    pub exit_code: Option<i32>,
    pub signal: Option<i32>,        // e.g. 6 = SIGABRT, 9 = SIGKILL
    pub restart_count: u32,
    pub max_restarts: u32,
    pub crossed_max_restarts: bool,

    pub health: Option<HealthConfig>,
    pub config_stanza: String,      // this service's krill.yaml block

    pub own_log_tail: Vec<LogRecord>,    // last N lines, stderr prioritized
    pub neighbor_log_tail: Vec<LogRecord>, // ±window from deps/dependents

    pub deps: Vec<DepState>,        // name + last known status
    pub dependents: Vec<DepState>,
    pub failed_at: DateTime<Utc>,
}

pub struct LogRecord {
    pub ts: DateTime<Utc>,
    pub service: String,
    pub stream: Stream,   // Stdout | Stderr   <-- requires the capture-side fix
    pub level: Level,     // parsed/inferred
    pub text: String,
}
```

> Dependency: `LogRecord.stream`/`level` require the capture-side change noted in
> the TODO ("tag stream + level on capture"). Today `is_stderr` is dropped after
> picking a tracing level and the timeline hard-codes `Info`. That fix is a
> prerequisite for good triage and is worth doing regardless.

## 6. Backends

### 6.1 Heuristic (ships first, no LLM)

Deterministic rules over the `Incident`. Catches a large fraction of real cases,
offline and instantly:

- Exit/signal table: `134/SIGABRT → assertion/abort`, `137/SIGKILL → likely OOM`,
  `1` with `ImportError|ModuleNotFound` → env/build, etc.
- "Dependency never healthy": a dep is `Failed`/`Starting` at failure time and
  the service died shortly after start → probable ordering/readiness issue.
- Pattern matches in the log tail: `address already in use`, `connection refused`,
  `No such file or directory`, `version `GLIBC`/symbol`, `CUDA`/`device`.
- `crossed_max_restarts` → flapping; surface the *first* failure, not the last.

### 6.2 LLM (optional)

Same `Incident` in; structured `Advice` out. Behind `Advisor`. Two flavors:
local (Ollama/llama.cpp) for air-gapped, cloud (Anthropic/OpenAI) for quality.
Response is cached by **error signature** (hash of signal + normalized top stderr
line) so a flapping service doesn't re-spend tokens.

### 6.3 Output contract

```rust
pub struct Advice {
    pub summary: String,            // one line
    pub likely_cause: String,
    pub checks: Vec<String>,        // concrete things to inspect/run
    pub suggested_fix: Option<String>, // never auto-applied
    pub confidence: Confidence,     // Low | Medium | High
    pub evidence: Vec<String>,      // which incident fields drove this
    pub source: AdviceSource,       // Heuristic | LocalLlm | CloudLlm
}
```

## 7. Triggers & UX

- **On-demand first** (lower risk, cheaper): press `a` / `?` on a failed service
  in the TUI, or `krill explain <service>` on the CLI.
- **Auto-triage on failure**: off by default; opt-in via config. When on, the
  heuristic runs automatically (free) and the dashboard shows a one-line verdict;
  the LLM is only invoked on explicit request.
- Rendering: a dedicated panel (or detail-view section) showing `summary →
  likely_cause → checks`, with evidence and a clear "advisory" framing. A
  "copy command" affordance for `suggested_fix`; never an "apply" button in v1.

## 8. Controls

- Feature flag `assist`; config block `[assist]` with `enabled`, `backend`,
  `auto_triage`, `redact`, token/cost caps.
- Redaction before anything leaves the process: drop env keys matching
  `*KEY*|*TOKEN*|*SECRET*|*PASSWORD*`, and path-normalize `$HOME`.
- Caching keyed by error signature; cap calls per minute.

## 9. Sequencing

1. **Capture-side tagging** — store `(stream, level, ts)` per log line; fix the
   timeline severity bug. (Prereq; useful on its own.)
2. **`IncidentBuilder`** in the daemon + serialize incidents to the session dir.
   Improves dashboard/`krill logs` immediately.
3. **Heuristic backend** + `krill explain <service>` (offline, deterministic).
4. **TUI panel** + on-demand key.
5. **`Advisor` trait + LLM backends** (local first, then cloud), caching,
   redaction.
6. (Later) auto-triage, feedback loop (👍/👎 to tune heuristics).

The ordering is deliberate: each stage is useful even if the next never ships,
and the bulk of the work (1–2) strengthens krill for users who never enable AI.

## 10. Open questions

- In-process vs. separate consumer of `timeline.jsonl` — **decided: in-process**,
  behind a feature flag (better UX, easier graph access; flag keeps krill lean).
- Default heuristic ruleset scope — start minimal, grow from real incidents.
- Whether `suggested_fix` ever becomes "apply" (explicit, reversible only) —
  deferred out of v1.
