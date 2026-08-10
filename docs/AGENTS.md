# AGENTS.md

This directory contains project documentation for requirements, technical specifications, and architectural decisions.

The goal is to preserve important context so that contributors and agents can understand not only what the system does, but also what it is intended to do and why key decisions were made.

## Documentation Types

Use three primary document types:

```text
docs/
├── AGENTS.md
├── prd/
├── specs/
└── adr/
```

Their responsibilities are different:

| Type | Purpose                                             |
| ---- | --------------------------------------------------- |
| PRD  | Describe what should be built and why               |
| Spec | Describe how a feature or system should behave      |
| ADR  | Record why an important technical decision was made |

Keep these concerns separate where practical.

---

## PRD

Location:

```text
docs/prd/
```

PRD stands for **Product Requirements Document**.

Use a PRD to describe:

* the problem
* the goal
* user or business requirements
* scope
* non-goals
* constraints
* acceptance criteria

A PRD should focus on desired outcomes rather than implementation details.

### Suggested Structure

```markdown
# <Title>

## Status

Draft | Accepted | Implemented | Deprecated

## Context

Describe the problem and relevant background.

## Goals

- ...

## Requirements

- ...

## Non-goals

- ...

## Constraints

- ...

## Acceptance Criteria

- ...
```

Do not put detailed algorithms, internal APIs, class structures, or implementation-specific decisions in a PRD unless they are themselves explicit requirements.

---

## Spec

Location:

```text
docs/specs/
```

A Spec describes the intended technical behavior of a feature, subsystem, protocol, or other implementation area.

Use a Spec when understanding the implementation requires more context than can reasonably be expressed through code alone.

A Spec may describe:

* architecture
* components and responsibilities
* data flow
* control flow
* state transitions
* data models
* interfaces
* protocols
* algorithms
* invariants
* edge cases
* failure behavior
* compatibility requirements
* performance constraints

### Suggested Structure

```markdown
# <Title>

## Status

Draft | Accepted | Implemented | Deprecated

## Overview

Describe what this feature or subsystem does.

## Goals

- ...

## Non-goals

- ...

## Design

Describe the intended design and behavior.

## Data Model

Describe important data structures if applicable.

## Invariants

Describe assumptions or properties that must remain true.

## Edge Cases

Describe important exceptional or ambiguous cases.

## Failure Handling

Describe expected failure behavior where relevant.

## Related Decisions

- ADR-XXX
```

A Spec should describe stable behavior and design concepts.

Avoid merely translating source code into prose.

---

## ADR

Location:

```text
docs/adr/
```

ADR stands for **Architecture Decision Record**.

Use an ADR to record a significant technical decision when:

* multiple reasonable alternatives exist
* the decision has meaningful tradeoffs
* the decision affects architecture or system behavior
* the decision is difficult or expensive to reverse
* the chosen approach may be non-obvious to future contributors

Do not create ADRs for routine implementation details.

### Naming

Use sequential numbering:

```text
001-use-example-approach.md
002-change-storage-model.md
003-adopt-new-protocol.md
```

Do not renumber existing ADRs.

### Suggested Structure

```markdown
# ADR-XXX: <Decision>

## Status

Proposed | Accepted | Superseded | Deprecated

## Context

Describe the problem and relevant constraints.

## Options Considered

### Option A

Pros:

- ...

Cons:

- ...

### Option B

Pros:

- ...

Cons:

- ...

## Decision

Describe the selected approach.

## Rationale

Explain why it was selected.

## Consequences

Describe important benefits, costs, limitations, and follow-up implications.

## References

Link related Specs, PRDs, issues, pull requests, experiments, or other evidence.
```

When an ADR is replaced, keep the original document and mark it as superseded rather than deleting it.

---

## Choosing the Right Document

Use this rule:

```text
What are we building, and why?
→ PRD

How should it work?
→ Spec

Why did we choose this approach?
→ ADR
```

A change may require more than one document.

For example:

```text
PRD
A new capability is required.

Spec
Defines its technical behavior.

ADR
Records an important implementation choice made while designing it.
```

Do not create documents merely to satisfy process. Create them when they preserve useful context.

---

## Agent Instructions

When working on this repository:

1. Check relevant documentation before making substantial changes.
2. Read the relevant PRD to understand intended requirements.
3. Read relevant Specs to understand intended technical behavior.
4. Read referenced ADRs before changing non-obvious architectural decisions.
5. Preserve documented requirements and invariants unless the task explicitly changes them.
6. Update documentation when implementation changes documented behavior.
7. Create an ADR when introducing a significant architectural decision that should be preserved.
8. Do not invent historical rationale without evidence.
9. Mark assumptions or uncertain conclusions explicitly.
10. Keep documentation concise and focused on information that is difficult to recover from code alone.

---

## Documentation and Code

Documentation and implementation should evolve together.

Update documentation when a change affects:

* documented requirements
* public or internal behavior described by a Spec
* important interfaces or data models
* architectural constraints
* significant technical decisions

Routine refactoring that does not change documented behavior usually does not require documentation updates.

If code and documentation disagree, do not silently assume which one is correct.

Determine whether:

* the implementation is incorrect
* the documentation is outdated
* the intended design has changed

Then update the appropriate source.

---

## Writing Guidelines

Prefer documentation that explains intent, constraints, and reasoning.

Avoid unnecessary implementation narration.

Bad:

```text
Function A calls function B, then function B calls function C.
```

Better:

```text
The processing stages are separated so that each stage can evolve independently while preserving a stable interface between them.
```

Use concrete examples when they make behavior easier to understand.

Keep documents focused. Split a document when unrelated concerns begin to accumulate.

Do not duplicate the same information across PRDs, Specs, and ADRs. Link related documents instead.
