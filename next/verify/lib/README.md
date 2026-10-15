# Verify Library

The `verify/lib` crate provides a lightweight, asynchronous HDD (Humanity Driven Development) execution engine for Rust. It parses Gherkin feature files and executes matching step definitions against a shared "World" state.

It is designed to be used in two main contexts:
1.  **End-to-End Runners**: Orchestrating complex test scenarios involving external devices (e.g., Android Emulators).
2.  **Integration Tests**: Verifying internal library behavior using a mock environment.

## Core API

### 1. `Features<W>`

The `Features` struct is the core runtime engine. It holds the registry of step definitions and executes Gherkin features. generic over `W` (the "World").

**Key Methods:**
-   `new()`: Creates a new engine instance.
-   `filter(tag: &str)`: Filters execution to scenarios matching the given tag (e.g., `@wip`).
-   `register(pattern: &str, step: S)`: Registers an async step function to a regex pattern.
-   `given`, `when`, `then`, `and`: Semantic aliases for `register`.
-   `before(step: S)`, `after(step: S)`: Registers hooks to run before/after each scenario.
-   `execute(path: P, world: &mut W)`: Parses and runs a feature file from disk.
-   `execute_from_memory(content: &str, world: &mut W)`: Runs a feature string directly.

### 2. `AsyncStep<W>`

The `AsyncStep` trait defines the interface for step functions. Any function matching the following signature automatically implements this trait:

```rust
fn my_step(world: &mut World, args: Vec<String>, ctx: StepContext) -> Pin<Box<dyn Future<Output = ()> + Send>>
```

*Note: The `codegen` tool automatically generates wrapper code to adapt simpler async functions to this signature.*

### 3. `StepContext` & `DataTable`

Steps receive a `StepContext` containing optional data, such as Gherkin Data Tables.

-   `ctx.table`: An `Option<Vec<Vec<String>>>` representing the data table.
-   **Helper Functions**: The library provides utilities in `utils` to parse these tables:
    -   `table_to_struct`: Converts a vertical table (Key | Value) to a struct.
    -   `horizontal_table_to_structs`: Converts a horizontal table (Header | Rows) to a list of structs.
    -   `assert_json_matches_table`: Validates that a struct matches a table's expected values.

## Usage Example

```rust
use verify::lib::{Features, StepContext};

struct MyWorld {
    counter: i32,
}

// 1. Define specific step functions (typically generated via codegen)
async fn given_counter_reset(w: &mut MyWorld, _args: Vec<String>, _ctx: StepContext) {
    w.counter = 0;
}

async fn when_increment(w: &mut MyWorld, args: Vec<String>, _ctx: StepContext) {
    let val: i32 = args[0].parse().unwrap();
    w.counter += val;
}

#[tokio::main]
async fn main() {
    let mut features = Features::<MyWorld>::new();
    let mut world = MyWorld { counter: 0 };

    // 2. Register steps
    features.register(r"Given I reset the counter", given_counter_reset);
    features.register(r"When I add (\d+)", when_increment);

    // 3. Execute
    let feature = r#"
        Feature: Calculator
            Scenario: Addition
                Given I reset the counter
                When I add 5
    "#;
    
    features.execute_from_memory(feature, &mut world).await;
}
```

## Module Structure

-   `features`: The engine logic (`Features<W>`).
-   `step`: Core traits (`AsyncStep`, `StepContext`).
-   `utils`: Utilities for handling Gherkin Data Tables and JSON assertions.
