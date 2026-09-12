# Milestone Specification Template (TEMPLATE_SPEC.md)

This document establishes the mandatory 5-section structure for all technical milestone specifications in the **Jeanne** project. Every specification must adhere strictly to this template to enable deterministic implementation under Spec-Driven Development (SDD).

---

## 1. Overview & Objective
* **Milestone Identifier & Title**: Milestone $N$ — Formal Title.
* **Core Problem Statement**: Specific architectural problem resolved by this milestone.
* **Hardware Ceiling**: Maximum resident and peak RAM allocations, CPU/iGPU utilization boundaries under the strict 16 GB constraint.
* **Dependencies & Tooling**: Comprehensive list of Rust crates, Node.js packages, and external system binaries with explicit versions.

## 2. Data Models & Interface Contracts
* **Rust Domain Structures**: Complete, production-ready Rust structs, enums, and trait definitions with explicit `serde` attributes.
* **Database Schemas (DDL)**: Complete SQLite table definitions, virtual tables (`vec0`, `fts5`), indices, triggers, and initialization `PRAGMA` settings.
* **IPC Command Signatures**: Exact Tauri v2 IPC commands and corresponding TypeScript interface definitions.
* **Network & Wire Protocols**: Complete request/response JSON schemas, SSE formats, or JSON-RPC 2.0 payloads.

## 3. Scenarios & Edge Cases
* **Nominal Execution Flow**: Step-by-step end-to-end lifecycle walkthrough.
* **Failure Modes & Error Handling**: Graceful degradation, connection drops, hardware disconnection, rate limits, lock contention.
* **Boundary Conditions**: Zero-state behaviors, file overflow limits, maximum token boundaries.

## 4. Acceptance Test Matrix (TDD Assertions)
* **Test Case Definitions**: Granular, test-first specifications numbered `TEST-XX-YY`.
* **Arrange-Act-Assert (AAA)**: Concrete inputs, execution path, and exact expected assertions for unit and integration tests.

## 5. Verification & Sign-off Checklist
* **Automated Quality Gates**: Cargo check, test suite execution, clippy linter status.
* **Performance & Memory Audit**: Measured memory benchmarks verifying zero leak and adherence to the hardware ceiling.
