# Custom Mutators

Mutarust exposes a public mutator interface. You can add a mutation operator
without a fork of the library core. You still build a custom command that
registers your mutator with the built-in set.

## The Mutator trait

```rust
use mutarust::{Mutation, Mutator};

pub trait Mutator: Send + Sync {
    fn name(&self) -> &str;
    fn mutations(&self, source: &str) -> Vec<Mutation>;
}
```

- `name` must use lower-case slash-separated words, for example `custom/flip`.
- `mutations` receives the full Rust source text for one file.
- Return an empty list when the source has no matching site.
- Each `Mutation` holds a byte range and a replacement string.

## Create one mutation

```rust
use mutarust::Mutation;

let mutation = Mutation::new(10..11, "-");
let changed = mutation.apply("let n = 1 + 2;").expect("range must be valid");
assert_eq!(changed, "let n = 1 - 2;");
```

## Register and run a custom mutator

```rust
use mutarust::{Mutation, Mutator, RegistryBuilder, run_mutation_tests};

struct FlipPlus;

impl Mutator for FlipPlus {
    fn name(&self) -> &str {
        "custom/flip-plus"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        source
            .match_indices('+')
            .map(|(index, _)| Mutation::new(index..index + 1, "-"))
            .collect()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = RegistryBuilder::with_builtins()
        .register(FlipPlus)
        .expect("custom mutator must register")
        .build();

    for name in registry.names() {
        println!("{name}");
    }

    let sources = vec![String::from("src/lib.rs")];
    let run = run_mutation_tests(&sources, &registry)?;
    println!("score: {:.1}%", run.mutation_score() * 100.0);
    Ok(())
}
```

A duplicate name or an invalid name returns `RegistryError`.

Add `mutarust` as a Cargo dependency. Build your command with `cargo build
--release`. Use that binary in place of the upstream `mutarust` command.

## Guidelines

- Return quickly when the source has no matching site.
- Keep the mutator name unique across the registry.
- Prefer small, single-purpose replacements.
- Do not mutate test source. The default discovery rules already exclude it.
- Prove the operator with a fixture crate before you use it in CI.
