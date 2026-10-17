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
2. **Branch & Worktree Isolation**: A dedicated branch is created in an isolated Git worktree (`just start-milestone <id> <name>` or `just worktree-add`). **Direct commits or pushes to `main` are strictly forbidden** to preserve project stability and avoid breaking `main`.
3. **Red State Validation**: Unit and integration test suites are written and committed. Tests must compile and fail predictably.
4. **Green State Implementation**: Backend domain logic (`crates/core`) and desktop UI bindings (`apps/desktop`) are implemented until all test assertions pass.
5. **Hardware Footprint Audit**: Memory consumption is benchmarked to guarantee compliance with the assigned RAM budget (< 80 MB idle, < 4.5 GB active 3B inference).
6. **Milestone Sign-off & Functional Gate**: All acceptance criteria are validated, deterministic quality checks pass (`just pre-review`), and mandatory review reports (`M{ID}_CODE_REVIEW.md` and `M{ID}_QA_REPORT.md`) are approved.
7. **Merge to Main**: The Architect merges the feature branch into `main` (`just merge-milestone <id> <name>`) **only when the project on the branch is verified to be 100% functional**.
