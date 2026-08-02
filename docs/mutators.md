# Built-in Mutators

Mutarust has the same stable mutator names as Mutago. The changes below apply
to Rust expression syntax. Mutarust does not change type syntax, patterns, or
macro token input.

| Stable name | Rust source change |
| --- | --- |
| `arithmetic/assign_invert` | `+=` to `-=`, `-=` to `+=`, `*=` to `/=`, `/=` to `*=`, `%=` to `*=`, `&=` to `|=`, `|=` to `&=`, `^=` to `&=`, `<<=` to `>>=`, and `>>=` to `<<=` |
| `arithmetic/assignment` | Each compound assignment above to `=` |
| `arithmetic/base` | `+` to `-`, `-` to `+`, `*` to `/`, `/` to `*`, and `%` to `*` |
| `arithmetic/bitwise` | `&` to `|`, `|` to `&`, `^` to `&`, `<<` to `>>`, and `>>` to `<<` |
| `arithmetic/negate` | Remove unary `-` |
| `conditional/bool-literal` | Change `true` to `false`, or `false` to `true` |
| `conditional/negated` | `>` to `<=`, `<` to `>=`, `>=` to `<`, `<=` to `>`, `==` to `!=`, and `!=` to `==` |
| `conditional/not` | Remove direct unary `!` from a condition or logical operand |
| `expression/comparison` | `<` to `<=`, `<=` to `<`, `>` to `>=`, and `>=` to `>` |
| `expression/logical` | `&&` to `||`, and `||` to `&&` |
| `expression/string-literal` | Change a direct, nonempty string operand of `==` or `!=` to `""` |
| `numbers/decrementer` | Subtract one from a decimal integer or float literal |
| `numbers/float-negate` | Change a nonzero float literal to `0.0` |
| `numbers/incrementer` | Add one to a decimal integer or float literal |

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
