# 05 - Methodology & Engineering Workflow

## 1. Rationale & Philosophy

Developing on local-first, low-resource hardware (16 GB unified RAM, shared iGPU) requires zero architectural drift. Unconstrained AI code generation consistently leads to premature API hallucinations, memory bloat, and silent regressions.

To eliminate these failure modes, **Jeanne** applies **Spec-Driven Development (SDD)** coupled with strict **Test-Driven Development (TDD)** and hardware budget guardrails.

---

## 2. The SDD Tripartite Governance Model

The methodology separates concerns across three architectural levels:

```
┌──────────────────────────────────────────────────────────┐
│  AGENTS.md (Root)                                        │
│  - Strict Golden Rule: Mandatory spec before code        │
│  - Context-efficient, non-negotiable policy              │
└────────────────────────────┬─────────────────────────────┘
                             │ enforces
                             ▼
┌──────────────────────────────────────────────────────────┐
│  .agent/skills/sdd-workflow/SKILL.md                     │
│  - On-demand execution runbook for AI agents             │
│  - Step-by-step TDD instructions & anti-rationalization  │
└────────────────────────────┬─────────────────────────────┘
                             │ produces & validates
                             ▼
┌──────────────────────────────────────────────────────────┐
│  docs/specs/<milestone>_SPEC_<feature>.md                │
│  - Immutable contract: Structs, SQL DDL, IPC, RAM budget │
│  - Audit checklist & test assertion matrix               │
└──────────────────────────────────────────────────────────┘
```

---

## 3. Milestone Execution Lifecycle

Every milestone outlined in `docs/04_ROADMAP_AND_MILESTONES.md` follows a linear lifecycle:

1. **Contract Authoring**: The milestone specification is drafted in `docs/specs/` following `TEMPLATE_SPEC.md`.
2. **Red State Validation**: Unit and integration test suites are written and committed. Tests must compile and fail predictably.
3. **Green State Implementation**: Backend domain logic (`crates/core`) and desktop UI bindings (`apps/desktop`) are implemented until all test assertions pass.
4. **Hardware Footprint Audit**: Memory consumption is benchmarked to guarantee compliance with the assigned RAM budget (< 80 MB idle, < 4.5 GB active 3B inference).
5. **Milestone Sign-off**: Acceptance criteria are validated, and the release slice is committed.
