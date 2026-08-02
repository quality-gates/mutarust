# Built-in Mutators

Mutarust has the same stable mutator names as Mutago. The changes below apply
to Rust source syntax. Mutarust does not change type syntax or patterns. It
changes macro token input only for the documented Tokio selection forms.

| Stable name | Rust source change |
| --- | --- |
| `arithmetic/assign_invert` | `+=` to `-=`, `-=` to `+=`, `*=` to `/=`, `/=` to `*=`, `%=` to `*=`, `&=` to `|=`, `|=` to `&=`, `^=` to `&=`, `<<=` to `>>=`, and `>>=` to `<<=` |
| `arithmetic/assignment` | Each compound assignment above to `=` |
| `arithmetic/base` | `+` to `-`, `-` to `+`, `*` to `/`, `/` to `*`, and `%` to `*` |
| `arithmetic/bitwise` | `&` to `|`, `|` to `&`, `^` to `&`, `<<` to `>>`, and `>>` to `<<` |
| `arithmetic/negate` | Remove unary `-` |
| `branch/case` | Remove all statements from a nonempty block arm, or replace another nonunit arm expression with `{}` |
| `branch/else` | Remove all statements from a nonempty direct `else` block |
| `branch/if` | Remove all statements from a nonempty `if` or `else if` body |
| `composite/field-clear` | Remove one nondefault named struct field before a local derived `Default` rest, or replace one explicit field value with a known Rust default |
| `concurrency/goroutine-remove` | Run one supported discarded thread closure immediately, or await one supported discarded Tokio task immediately |
| `conditional/bool-literal` | Change `true` to `false`, or `false` to `true` |
| `conditional/negated` | `>` to `<=`, `<` to `>=`, `>=` to `<`, `<=` to `>`, `==` to `!=`, and `!=` to `==` |
| `conditional/not` | Remove direct unary `!` from a condition or logical operand |
| `expression/comparison` | `<` to `<=`, `<=` to `<`, `>` to `>=`, and `>=` to `>` |
| `expression/context-nil` | Replace a direct `Some(value)` argument with `::core::option::Option::None` |
| `expression/logical` | `&&` to `||`, and `||` to `&&` |
| `expression/string-literal` | Change a direct, nonempty string operand of `==` or `!=` to `""` |
| `loop/break` | Change `break` to `continue`, or `continue` to `break` |
| `loop/condition` | Replace a `while` or `while let` condition with `false` |
| `loop/range_break` | Insert `break;` at the start of a `for` body |
| `numbers/decrementer` | Subtract one from a decimal integer or float literal |
| `numbers/float-negate` | Change a nonzero float literal to `0.0` |
| `numbers/incrementer` | Add one to a decimal integer or float literal |
| `select/case-remove` | Remove one complete nonfallback branch from a supported Tokio selection that has another clause |
| `select/default-remove` | Remove one complete Tokio `else` branch when a normal branch remains |
| `statement/remove` | Remove a semicolon assignment, compound assignment, call, method call, or macro statement |
| `statement/remove-self-assign` | Remove a semicolon assignment when both sides are the same safe place |
| `statement/return` | Replace an explicit return value with a valid default for its declared return type |

`branch/case` keeps the match pattern, guard, arrow, and comma. It does not
change an arm that is already `{}` or `()`. `branch/else` does not treat an
`else if` as an `else` block. The separate `branch/if` mutator changes that
`else if` body.

`loop/break` changes only control flow that targets a loop. It does not change
a `break` value or a break that targets a labeled block. It keeps a loop label.
`loop/condition` does not change `while false`. `loop/range_break` does not add
a second direct `break;` when the `for` body starts with one.

`statement/remove` changes only expressions that end with a semicolon. It does
not remove a `let`, item, tail expression, `return`, `break`, `continue`, or
control-flow expression. It does not change macro token input. The separate
`statement/remove-self-assign` mutator owns a plain self-assignment. It accepts
a local path, a field or tuple field that starts at a local path, or a tuple of
these places. It does not accept an index, dereference, call, macro, or compound
assignment.

`composite/field-clear` accepts named struct expressions. For a local struct
with a `Default` derive, an unshadowed `..Default::default()` rest lets it
remove one nondefault field and its comma. It does not use a type method or a
manual `Default` implementation because either one can set a nonstandard field
value. Without a rest, it changes only an explicit field value. The supported
direct values are `true`, nonzero integers
and floats, a non-NUL character, nonempty string and byte-string literals, and
an unshadowed `Some(value)`. It keeps literal suffixes and uses a fully
qualified `None`. It does not change shorthand fields without a default rest,
tuple values, array values, arbitrary rest expressions, or values that are
already known defaults.

`expression/context-nil` accepts `Some(value)`, `Option::Some(value)`, and the
fully qualified `::core` or `::std` form when one of these expressions is a
direct function or method argument. It uses `::core::option::Option::None`. It does not
change an indirect option value, an existing `None`, or an unqualified name
that a local item shadows. A replacement that Cargo cannot type-check is
`Skipped` before tests run. For a custom test command, Mutarust runs a candidate
only when a concrete local function parameter proves the option type. It marks
other candidates `Skipped` because the custom command selects its own compiler.

`statement/return` accepts an explicit `return value;` in a function, method,
trait default method, or closure with an explicit return type. It supports
Boolean, integer, float, character, string-slice, slice, option, and recursive
tuple return types. It also uses `Default::default()` for `String`, `Vec`, a
local type with a `Default` implementation or derive, and a type parameter with
a direct `Default` bound. For a tuple expression, it changes one supported
element at a time. It does not change a bare return, a tail expression, a
return in an inferred closure or async block, `Result`, `impl Trait`, a mutable
reference, an unconstrained type parameter, or a general borrowed value.
Cargo checks replacements that need type proof before it runs tests. Mutarust
skips these replacements before a custom test command because that command
selects its own compiler.

`concurrency/goroutine-remove` supports these standard thread statements:

```rust
std::thread::spawn(work);
::std::thread::spawn(work);
thread::spawn(work); // After an exact `use std::thread;` in this scope.
```

The replacement is `(work)();`. It supports these Tokio task statements in
an async function, async closure, or async block:

```rust
tokio::spawn(future);
::tokio::spawn(future);
tokio::task::spawn(future);
::tokio::task::spawn(future);
```

The replacement is `(future).await;`. Each spawn call must be a discarded
semicolon statement with one argument. The mutator keeps a `move` closure or
an `async move` block and evaluates it one time. It does not change a used
join handle, a builder or method spawn, `spawn_blocking`, `spawn_local`,
scoped threads, Rayon, async-std, Smol, an alias, or a local path that shadows
`std`, `thread`, or `tokio`.

The two `select` mutators support `tokio::select!` and
`::tokio::select!`. They support normal branches, branch guards, an optional
`biased;` prefix, an optional `else` branch, and nested supported selections.
`select/case-remove` removes the pattern, future, guard, arrow, handler, and
comma of one normal branch. It does this only when another normal or fallback
clause remains. `select/default-remove` removes the full `else` clause only
when a normal branch remains.

These mutators do not change an imported or renamed selection macro, a local
macro, `futures::select!`, Crossbeam selection, or invalid Tokio input. They
do not search inside unrelated macro input. Cargo checks each selection
replacement before tests run. A custom test command cannot give this Cargo
proof, so Mutarust marks these candidates `Skipped`.

Mutago v2.7.7 prose says that the selection mutators empty a clause body. Its
source code and tests remove the full clause. Mutarust follows the source code
and tests, and removes the full clause.

`conditional/bool-literal` changes direct local initializers, assignment
right-hand sides, function arguments, and method arguments. It does not change
return expressions, conditions, or macro input.

`conditional/not` changes a direct `!` expression in an `if` or `while`
condition. It also changes a direct left or right operand of `&&` or `||`.

The number mutators keep a Rust type suffix. They do not change non-decimal
integer forms, array lengths, or repeat counts. They reject overflow and
non-finite results. `numbers/float-negate` does not change zero.

Rust has no `&^`, `&^=`, or unary `+` expression operators. Mutarust does not
add replacements for these Go operators. The Rust parser limits all operator
mutators to expressions, so similar tokens in type bounds and patterns are not
candidates.

Mutarust parses each changed file before it records or runs a mutant. An edit
that does not produce valid Rust is not a candidate. Mutarust also removes
equal mutations before it runs tests.
