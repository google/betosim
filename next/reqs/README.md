# Requirements and BDD Directory Layout

This directory contains the requirements, specifications, and BDD scenarios for `netsim`.

## Directory Structure

- **`epics/`**: High-level Epic specifications (Markdown files).
- **`features/`**: Detailed Feature specifications (Markdown files). Used for complex features requiring explanation beyond scenarios.
- **`bdd/`**: Executable BDD/Gherkin scenarios (`.feature` files), organized by epic in subdirectories.

## Guidelines: When to use what

To maintain lean and useful documentation, follow these rules of thumb:

### 1. Reference BDD files directly from Epics
Use this approach when the feature is simple and the BDD scenarios speak for themselves.
- **Hierarchy**: `Epic (.md)` ➔ `BDD (.feature)`
- **Example**: Simple features where the `Given/When/Then` steps provide enough context for anyone reading them.

### 2. Create a Feature Spec in `features/`
Use this approach when the feature is complex and needs explanation of business rules, diagrams, or context that doesn't fit well in Gherkin syntax.
- **Hierarchy**: `Epic (.md)` ➔ `Feature (.md)` ➔ `BDD (.feature)`
- **Example**: Features with complex logic, state machines, or extensive non-functional requirements.

## Summary of Document Types

| Document Type | Folder | Purpose | Content |
| :--- | :--- | :--- | :--- |
| **Epic Spec** | `epics/` | The Big Picture | High-level goals, EARS requirements, links to Features or BDD. |
| **Feature Spec** | `features/` | The Business Rules | Explanations, diagrams, context, links to BDD scenarios. |
| **BDD Scenario** | `bdd/` | The Executable Examples | `Given/When/Then` test cases that verify the rules. |

## Tagging Strategy for Missing Features/Tests

To track progress and allow test runners to skip unimplemented or untestable scenarios, use the following tags:

- **`@nyi` (Not Yet Implemented)**: Use this for scenarios where the feature itself is not yet implemented in `netsim` (e.g., RTT ranging).
- **`@nyt` (Not Yet Testable)**: Use this for scenarios where the feature exists in `netsim`, but the BDD framework lacks the step definitions (glue code) to run the test.
- **`@skip`**: Use this for scenarios that should be skipped for execution.
- **`@epic:<epic-name>`**: Categorizes scenarios by Epic (e.g., `@epic:wifi`).
- **`@feature:<feature-name>`**: Categorizes scenarios by Feature (e.g., `@feature:wifi_ap`).


Example:
```gherkin
  @nyi
  Scenario: WiFi RTT Ranging
    Given two devices supporting WiFi RTT
    When one device requests ranging to the other using RTT
    Then the system should return the distance based on RTT
```
