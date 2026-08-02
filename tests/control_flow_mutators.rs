use mutarust::Registry;

const CONTROL_FLOW_NAMES: [&str; 7] = [
    "branch/case",
    "branch/else",
    "branch/if",
    "loop/break",
    "loop/condition",
    "loop/range_break",
    "statement/remove",
];

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
fn builtin_control_flow_mutator_names_match_mutago() {
    let registry = Registry::builtins();
    let names = registry.names().collect::<Vec<_>>();

    for expected in CONTROL_FLOW_NAMES {
        assert!(names.contains(&expected), "missing built-in {expected}");
    }
}

#[test]
fn loop_break_swaps_break_and_continue_for_rust_loops() {
    let source = "fn visit(values: &[i32]) { 'outer: for value in values { if *value == 0 { continue 'outer; } break 'outer; } loop { break 1; } }";

    assert_eq!(
        changed_sources("loop/break", source),
        vec![
            source.replacen("continue 'outer", "break 'outer", 1),
            source.replacen("break 'outer", "continue 'outer", 1),
        ]
    );
}

#[test]
fn loop_condition_stops_while_and_while_let_loops() {
    let source = "fn wait(mut values: Vec<i32>) { while ready() { work(); } while let Some(value) = values.pop() { use_value(value); } while false {} }";

    assert_eq!(
        changed_sources("loop/condition", source),
        vec![
            source.replacen("ready()", "false", 1),
            source.replacen("let Some(value) = values.pop()", "false", 1),
        ]
    );
}

#[test]
fn loop_range_break_exits_each_for_loop_before_its_body() {
    let source =
        "fn visit(values: &[i32]) { for value in values { use_value(value); } for _ in 0..0 {} }";

    assert_eq!(
        changed_sources("loop/range_break", source),
        vec![
            source.replacen("{ use_value(value);", "{ break; use_value(value);", 1),
            source.replacen("{}", "{ break;}", 1),
        ]
    );
}

#[test]
fn branch_if_clears_if_and_else_if_bodies() {
    let source = "fn choose(first: bool, second: bool) { if first { one(); } else if second { two(); } else { three(); } if false {} }";

    assert_eq!(
        changed_sources("branch/if", source),
        vec![
            source.replacen("one();", "", 1),
            source.replacen("two();", "", 1),
        ]
    );
}

#[test]
fn branch_else_clears_else_blocks_but_not_else_if_branches() {
    let source = "fn choose(first: bool, second: bool) { if first { one(); } else if second { two(); } else { three(); } if first {} else {} }";

    assert_eq!(
        changed_sources("branch/else", source),
        vec![source.replacen("three();", "", 1)]
    );
}

#[test]
fn branch_case_clears_match_arm_bodies() {
    let source =
        "fn choose(value: i32) { match value { 0 => { zero(); }, 1 => one(), _ => { other(); } } }";

    assert_eq!(
        changed_sources("branch/case", source),
        vec![
            source.replacen("zero();", "", 1),
            source.replacen("one()", "{}", 1),
            source.replacen("other();", "", 1),
        ]
    );
}

#[test]
fn overlapping_branch_and_statement_mutations_have_one_changed_source() {
    let source = "fn act() { if ready() { call(); } }";

    assert_eq!(
        changed_sources("branch/if", source),
        changed_sources("statement/remove", source)
    );
}

#[test]
fn loop_range_break_keeps_inner_attributes_before_the_inserted_statement() {
    let source = "fn visit(values: &[i32]) { for value in values { #![allow(unused_variables)] use_value(value); } }";

    assert_eq!(
        changed_sources("loop/range_break", source),
        vec![source.replacen(
            "#![allow(unused_variables)]",
            "#![allow(unused_variables)] break;",
            1,
        )]
    );
}

#[test]
fn loop_range_break_skips_a_first_break_to_the_current_loop_label() {
    let source = "fn visit(values: &[i32]) { 'items: for value in values { break 'items; } }";

    assert!(changed_sources("loop/range_break", source).is_empty());
}

#[test]
fn loop_range_break_keeps_a_first_break_that_targets_another_loop() {
    let source =
        "fn visit(values: &[i32]) { 'outer: loop { for value in values { break 'outer; } } }";

    assert_eq!(
        changed_sources("loop/range_break", source),
        vec![source.replacen("{ break 'outer;", "{ break; break 'outer;", 1)]
    );
}

#[test]
fn control_flow_mutators_skip_equivalent_or_unsafe_changes() {
    let cases = [
        ("loop/break", "fn safe() { 'block: { break 'block; } }"),
        ("loop/condition", "fn safe() { while false {} }"),
        ("loop/range_break", "fn safe() { for _ in 0..1 { break; } }"),
        ("branch/if", "fn safe() { if true {} }"),
        ("branch/else", "fn safe() { if true {} else {} }"),
        (
            "branch/case",
            "fn safe(value: i32) { match value { 0 => {}, _ => () } }",
        ),
        ("statement/remove", "fn safe() { let kept = 1; kept }"),
    ];

    for (name, source) in cases {
        assert!(changed_sources(name, source).is_empty(), "unsafe {name}");
    }
}

#[test]
fn statement_remove_removes_only_supported_rust_expression_statements() {
    let source = "fn act(mut value: i32) { value = next(); call(); object().method(); macro_call!(); let kept = value; return_value(kept); if kept > 0 { branch(); } }";

    assert_eq!(
        changed_sources("statement/remove", source),
        vec![
            source.replacen("value = next();", "", 1),
            source.replacen("call();", "", 1),
            source.replacen("object().method();", "", 1),
            source.replacen("macro_call!();", "", 1),
            source.replacen("return_value(kept);", "", 1),
            source.replacen("branch();", "", 1),
        ]
    );
}

#[test]
fn control_flow_mutators_reject_malformed_rust() {
    let registry = Registry::builtins();

    for source in [
        "fn missing_body(",
        "fn open_loop() { while ready() {",
        "fn bad_match(value: i32) { match value { 0 => } }",
    ] {
        for name in CONTROL_FLOW_NAMES {
            assert!(
                registry
                    .get(name)
                    .unwrap_or_else(|| panic!("missing built-in {name}"))
                    .mutations(source)
                    .is_empty(),
                "{name} accepted malformed Rust: {source}"
            );
        }
    }
}

#[test]
fn parser_fuzz_corpus_keeps_every_control_flow_mutation_valid() {
    let corpus = [
        "fn loops(values: &[i32]) { 'outer: for value in values { if *value == 0 { continue 'outer; } break 'outer; } while ready() { call(); } }",
        "fn patterns(mut values: Vec<i32>) { while let Some(value) = values.pop() { use_value(value); } }",
        "fn branches(value: i32) { if first() { one(); } else if second() { two(); } else { three(); } match value { 0 => { zero(); }, _ => { other(); } } }",
        "fn statements(mut value: i32) { value = next(); call(); object().method(); macro_call!(); }",
        "fn unusual() { for café in [1, 2] { use_value(café); } match 1 { 1 if guard() => { raw(); }, _ => {} } }",
    ];
    let registry = Registry::builtins();

    for source in corpus {
        for name in CONTROL_FLOW_NAMES {
            let mutator = registry
                .get(name)
                .unwrap_or_else(|| panic!("missing built-in {name}"));
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
