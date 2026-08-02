# Built-in Mutators

Mutarust has the same stable mutator names as Mutago. The changes below apply
to Rust source syntax. Mutarust does not change type syntax, patterns, or macro
token input.

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
| `conditional/bool-literal` | Change `true` to `false`, or `false` to `true` |
| `conditional/negated` | `>` to `<=`, `<` to `>=`, `>=` to `<`, `<=` to `>`, `==` to `!=`, and `!=` to `==` |
| `conditional/not` | Remove direct unary `!` from a condition or logical operand |
| `expression/comparison` | `<` to `<=`, `<=` to `<`, `>` to `>=`, and `>=` to `>` |
| `expression/logical` | `&&` to `||`, and `||` to `&&` |
| `expression/string-literal` | Change a direct, nonempty string operand of `==` or `!=` to `""` |
| `loop/break` | Change `break` to `continue`, or `continue` to `break` |
| `loop/condition` | Replace a `while` or `while let` condition with `false` |
| `loop/range_break` | Insert `break;` at the start of a `for` body |
| `numbers/decrementer` | Subtract one from a decimal integer or float literal |
| `numbers/float-negate` | Change a nonzero float literal to `0.0` |
| `numbers/incrementer` | Add one to a decimal integer or float literal |
| `statement/remove` | Remove a semicolon assignment, compound assignment, call, method call, or macro statement |

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
control-flow expression. It does not change macro token input.

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
