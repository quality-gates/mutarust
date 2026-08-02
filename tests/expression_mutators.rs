use mutarust::Registry;

fn changed_sources(mutator: &str, source: &str) -> Vec<String> {
    Registry::builtins()
        .get(mutator)
        .expect("the built-in mutator must exist")
        .mutations(source)
        .into_iter()
        .map(|mutation| {
            mutation
                .apply(source)
                .expect("a built-in mutation must apply")
        })
        .collect()
}

#[test]
fn builtin_mutator_names_match_completed_mutago_groups() {
    assert_eq!(
        Registry::builtins().names().collect::<Vec<_>>(),
        vec![
            "arithmetic/assign_invert",
            "arithmetic/assignment",
            "arithmetic/base",
            "arithmetic/bitwise",
            "arithmetic/negate",
            "branch/case",
            "branch/else",
            "branch/if",
            "conditional/bool-literal",
            "conditional/negated",
            "conditional/not",
            "expression/comparison",
            "expression/logical",
            "expression/string-literal",
            "loop/break",
            "loop/condition",
            "loop/range_break",
            "numbers/decrementer",
            "numbers/float-negate",
            "numbers/incrementer",
            "statement/remove",
        ]
    );
}

#[test]
fn arithmetic_base_changes_binary_arithmetic_operators() {
    let source = "fn calculate(a: i32, b: i32) { let _ = (a + b, a - b, a * b, a / b, a % b); }";

    assert_eq!(
        changed_sources("arithmetic/base", source),
        vec![
            source.replacen("a + b", "a - b", 1),
            source.replacen("a - b", "a + b", 1),
            source.replacen("a * b", "a / b", 1),
            source.replacen("a / b", "a * b", 1),
            source.replacen("a % b", "a * b", 1),
        ]
    );
}

#[test]
fn arithmetic_bitwise_changes_rust_bitwise_operators_only() {
    let source = "fn bits(a: u8, b: u8) { let _ = (a & b, a | b, a ^ b, a << b, a >> b); }";

    assert_eq!(
        changed_sources("arithmetic/bitwise", source),
        vec![
            source.replacen("a & b", "a | b", 1),
            source.replacen("a | b", "a & b", 1),
            source.replacen("a ^ b", "a & b", 1),
            source.replacen("a << b", "a >> b", 1),
            source.replacen("a >> b", "a << b", 1),
        ]
    );
    assert!(
        changed_sources("arithmetic/bitwise", "fn accepts<T: Send + Sync>() {}").is_empty(),
        "type bounds are not bitwise expressions"
    );
}

#[test]
fn arithmetic_assign_invert_changes_rust_compound_assignments() {
    let source = "fn update(mut a: i32, b: i32) { a += b; a -= b; a *= b; a /= b; a %= b; a &= b; a |= b; a ^= b; a <<= b; a >>= b; }";

    assert_eq!(
        changed_sources("arithmetic/assign_invert", source),
        vec![
            source.replacen("a += b", "a -= b", 1),
            source.replacen("a -= b", "a += b", 1),
            source.replacen("a *= b", "a /= b", 1),
            source.replacen("a /= b", "a *= b", 1),
            source.replacen("a %= b", "a *= b", 1),
            source.replacen("a &= b", "a |= b", 1),
            source.replacen("a |= b", "a &= b", 1),
            source.replacen("a ^= b", "a &= b", 1),
            source.replacen("a <<= b", "a >>= b", 1),
            source.replacen("a >>= b", "a <<= b", 1),
        ]
    );
}

#[test]
fn arithmetic_assignment_replaces_compound_assignments_with_plain_assignment() {
    let source = "fn update(mut a: i32, b: i32) { a += b; a -= b; a *= b; a /= b; a %= b; a &= b; a |= b; a ^= b; a <<= b; a >>= b; }";
    let operators = ["+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="];
    let expected = operators
        .into_iter()
        .map(|operator| source.replacen(operator, "=", 1))
        .collect::<Vec<_>>();

    assert_eq!(changed_sources("arithmetic/assignment", source), expected);
}

#[test]
fn arithmetic_negate_removes_rust_unary_minus() {
    let source = "fn negate(value: i32) -> i32 { -value }";

    assert_eq!(
        changed_sources("arithmetic/negate", source),
        vec!["fn negate(value: i32) -> i32 { value }"]
    );
    assert!(
        changed_sources(
            "arithmetic/negate",
            "fn subtract(a: i32, b: i32) -> i32 { a - b }"
        )
        .is_empty()
    );
}

#[test]
fn numbers_incrementer_increments_rust_number_literals_outside_lengths() {
    let source = "fn values() { let _ = 1; let _ = 1.5; let _ = 2u8; let _: [u8; 4] = [0; 4]; }";

    assert_eq!(
        changed_sources("numbers/incrementer", source),
        vec![
            source.replacen("= 1;", "= 2;", 1),
            source.replacen("= 1.5;", "= 2.5;", 1),
            source.replacen("= 2u8;", "= 3u8;", 1),
            source.replacen("[0; 4]", "[1; 4]", 1),
        ]
    );
}

#[test]
fn numbers_decrementer_decrements_rust_number_literals_outside_lengths() {
    let source = "fn values() { let _ = 3; let _ = 2.5; let _ = 4i64; let _: [u8; 4] = [1; 4]; }";

    assert_eq!(
        changed_sources("numbers/decrementer", source),
        vec![
            source.replacen("= 3;", "= 2;", 1),
            source.replacen("= 2.5;", "= 1.5;", 1),
            source.replacen("= 4i64;", "= 3i64;", 1),
            source.replacen("[1; 4]", "[0; 4]", 1),
        ]
    );
}

#[test]
fn numbers_float_negate_replaces_nonzero_floats_with_zero() {
    let source = "fn floats() { let _ = 1.5; let _ = 2.0f32; let _ = 0.0; let _ = 3; }";

    assert_eq!(
        changed_sources("numbers/float-negate", source),
        vec![
            source.replacen("1.5", "0.0", 1),
            source.replacen("2.0f32", "0.0f32", 1),
        ]
    );
}

#[test]
fn number_mutators_reject_suffix_overflow_and_precision_no_ops() {
    let increment =
        "fn limits() { let _ = 255u8; let _ = 127i8; let _ = 1e20; let _ = 3.4028235e38f32; }";
    let decrement = "fn limits() { let _ = 0u8; let _ = 1e20; }";

    assert!(changed_sources("numbers/incrementer", increment).is_empty());
    assert!(changed_sources("numbers/decrementer", decrement).is_empty());
}

#[test]
fn conditional_negated_inverts_each_rust_comparison() {
    let source =
        "fn compare(a: i32, b: i32) { let _ = (a > b, a < b, a >= b, a <= b, a == b, a != b); }";

    assert_eq!(
        changed_sources("conditional/negated", source),
        vec![
            source.replacen("a > b", "a <= b", 1),
            source.replacen("a < b", "a >= b", 1),
            source.replacen("a >= b", "a < b", 1),
            source.replacen("a <= b", "a > b", 1),
            source.replacen("a == b", "a != b", 1),
            source.replacen("a != b", "a == b", 1),
        ]
    );
}

#[test]
fn expression_comparison_shifts_strict_and_inclusive_bounds() {
    let source = "fn compare(a: i32, b: i32) { let _ = (a < b, a <= b, a > b, a >= b); }";

    assert_eq!(
        changed_sources("expression/comparison", source),
        vec![
            source.replacen("a < b", "a <= b", 1),
            source.replacen("a <= b", "a < b", 1),
            source.replacen("a > b", "a >= b", 1),
            source.replacen("a >= b", "a > b", 1),
        ]
    );
}

#[test]
fn expression_logical_swaps_and_and_or() {
    let source = "fn logical(a: bool, b: bool) { let _ = (a && b, a || b); }";

    assert_eq!(
        changed_sources("expression/logical", source),
        vec![
            source.replacen("a && b", "a || b", 1),
            source.replacen("a || b", "a && b", 1),
        ]
    );
}

#[test]
fn expression_string_literal_empties_direct_comparison_operands() {
    let source = "fn strings(name: &str) { let café = name == \"expected\"; let _ = \"other\" != name; let _ = name == \"\"; let _ = name == format!(\"expected\"); }";

    assert_eq!(
        changed_sources("expression/string-literal", source),
        vec![
            source.replacen("\"expected\"", "\"\"", 1),
            source.replacen("\"other\"", "\"\"", 1),
        ]
    );
}

#[test]
fn conditional_bool_literal_changes_direct_assignments_and_call_arguments() {
    let source = "fn booleans(mut enabled: bool) -> bool { let local = true; enabled = false; use_flag(true); object().use_flag(false); if true { assert!(true); } enabled }";

    assert_eq!(
        changed_sources("conditional/bool-literal", source),
        vec![
            source.replacen("local = true", "local = false", 1),
            source.replacen("enabled = false", "enabled = true", 1),
            source.replacen("use_flag(true)", "use_flag(false)", 1),
            source.replacen("use_flag(false)", "use_flag(true)", 1),
        ]
    );
}

#[test]
fn conditional_not_removes_direct_condition_negations() {
    let source = "fn conditions(a: bool, b: bool) { if !a {} while !b {} let _ = !a; let _ = a && !b; let _ = !a || b; }";

    assert_eq!(
        changed_sources("conditional/not", source),
        vec![
            source.replacen("if !a", "if a", 1),
            source.replacen("while !b", "while b", 1),
            source.replacen("a && !b", "a && b", 1),
            source.replacen("!a || b", "a || b", 1),
        ]
    );
}

#[test]
fn parser_fuzz_corpus_keeps_every_expression_mutation_valid() {
    let mut corpus = vec![
        r#"fn all(mut a: i32, b: i32, flag: bool) {
            a += b;
            let local = true;
            call(false);
            if !flag && a < b { a = -a + 1; }
            let _ = (a & b) | (a << 2);
            let _ = "café" == name();
        }"#
        .to_owned(),
        r#"fn numbers() {
            let _ = (1_000u64, 2.5f32, 4.0e2f64);
            let _: [u8; 8] = [3; 8];
        }"#
        .to_owned(),
        r####"fn strings(value: &str) {
            let _ = value != r###"raw \" text"###;
            let _ = value == "";
        }"####
            .to_owned(),
        r#"fn nested(a: bool, b: bool, c: bool) {
            while !(a || b) {}
            let _ = (a && !b) || c;
        }"#
        .to_owned(),
    ];
    for operator in [
        "+", "-", "*", "/", "%", "&", "|", "^", "<<", ">>", "<", "<=", ">", ">=", "==", "!=", "&&",
        "||",
    ] {
        corpus.push(format!(
            "fn fuzz(a: i32, b: i32) {{ let _ = a {operator} b; }}"
        ));
    }
    for operator in ["+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="] {
        corpus.push(format!("fn fuzz(mut a: i32, b: i32) {{ a {operator} b; }}"));
    }
    for literal in ["0", "1_000u64", "127i8", "1.5", "2.0f32", "4.0e2f64"] {
        corpus.push(format!("fn fuzz() {{ let _ = {literal}; }}"));
    }
    let registry = Registry::builtins();

    for source in &corpus {
        for name in registry.names() {
            let mutator = registry.get(name).expect("the built-in mutator must exist");
            for mutation in mutator.mutations(source) {
                let changed = mutation
                    .apply(source)
                    .expect("a built-in mutation must apply");
                assert!(
                    syn::parse_file(&changed).is_ok(),
                    "{name} produced invalid Rust:\n{changed}"
                );
            }
        }
    }
}

#[test]
fn expression_mutators_reject_malformed_rust_without_candidates() {
    let corpus = [
        "fn missing_body(",
        "fn unclosed() { let value = true;",
        "fn bad_operator(a: i32) { let _ = a +; }",
        "fn bad_string() { let _ = \"unterminated; }",
    ];
    let registry = Registry::builtins();

    for source in corpus {
        for name in registry.names() {
            let mutator = registry.get(name).expect("the built-in mutator must exist");
            assert!(
                mutator.mutations(source).is_empty(),
                "{name} accepted malformed Rust: {source}"
            );
        }
    }
}

#[test]
fn expression_mutators_do_not_enter_patterns_types_or_const_arguments() {
    let source = r#"
        struct Flag<const VALUE: bool>;
        struct Count<const VALUE: usize>;
        fn excluded(value: i32) {
            match value { 1 => (), _ => () }
            let _: [u8; 1 + 2];
            let _: Count<{ 1 + 2 }>;
            let _ = Count::<{ 1 + 2 }>;
            let _ = Flag::<{ 1 < 2 }>;
        }
    "#;
    let registry = Registry::builtins();

    for name in registry.names() {
        assert!(
            registry
                .get(name)
                .expect("the built-in mutator must exist")
                .mutations(source)
                .is_empty(),
            "{name} entered excluded syntax"
        );
    }
}
