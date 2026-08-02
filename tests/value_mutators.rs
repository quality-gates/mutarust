use mutarust::Registry;

const VALUE_MUTATOR_NAMES: [&str; 4] = [
    "composite/field-clear",
    "expression/context-nil",
    "statement/remove-self-assign",
    "statement/return",
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

fn replace_nth(source: &str, old: &str, new: &str, occurrence: usize) -> String {
    let start = source
        .match_indices(old)
        .nth(occurrence)
        .map(|(start, _)| start)
        .unwrap_or_else(|| panic!("missing occurrence {occurrence} of {old}"));
    let mut changed = source.to_owned();
    changed.replace_range(start..start + old.len(), new);
    changed
}

#[test]
fn builtin_value_mutator_names_match_mutago() {
    let registry = Registry::builtins();

    for expected in VALUE_MUTATOR_NAMES {
        assert!(
            registry.names().any(|name| name == expected),
            "missing built-in {expected}"
        );
    }
}

#[test]
fn composite_field_clear_uses_default_rest_or_known_field_defaults() {
    let source = "#[derive(Default)] struct Config { timeout: u64, enabled: bool, name: &'static str, context: Option<u8> } fn build(timeout: u64) -> Config { Config { timeout, enabled: true, name: \"worker\", context: Some(1), ..Default::default() } } fn direct() -> Config { Config { timeout: 9, enabled: true, name: \"worker\", context: Some(1) } }";

    assert_eq!(
        changed_sources("composite/field-clear", source),
        vec![
            source.replacen("timeout, ", "", 1),
            source.replacen("enabled: true, ", "", 1),
            source.replacen("name: \"worker\", ", "", 1),
            source.replacen("context: Some(1), ", "", 1),
            source.replacen("timeout: 9", "timeout: 0", 1),
            replace_nth(source, "enabled: true", "enabled: false", 1),
            replace_nth(source, "name: \"worker\"", "name: \"\"", 1),
            replace_nth(
                source,
                "context: Some(1)",
                "context: ::core::option::Option::None",
                1,
            ),
        ]
    );
}

#[test]
fn composite_field_clear_skips_defaults_and_unsupported_fields() {
    let source = "#[derive(Default)] struct Config { count: i32, enabled: bool, name: &'static str, context: Option<i32> } fn unchanged(count: i32, base: Config) { let _ = Config { count: 0, enabled: false, name: \"\", context: None }; let _ = Config { count, ..base }; let _ = (count, false); let _ = [count]; }";

    assert!(changed_sources("composite/field-clear", source).is_empty());
}

#[test]
fn composite_field_clear_skips_default_collection_constructors() {
    let source = "#[derive(Default)] struct Values { names: Vec<String>, title: String } fn values() -> Values { Values { names: Vec::new(), title: String::new(), ..Default::default() } }";

    assert!(changed_sources("composite/field-clear", source).is_empty());
}

#[test]
fn composite_field_clear_accepts_matching_type_default_rest() {
    let source = "#[derive(Default)] struct Values { count: i32 } fn values() -> Values { Values { count: 7, ..Default::default() } }";

    assert_eq!(
        changed_sources("composite/field-clear", source),
        vec![source.replacen("count: 7, ", "", 1)]
    );
}

#[test]
fn composite_field_clear_rejects_qualified_type_name_collisions() {
    let source = "#[derive(Default)] struct Config { enabled: bool } mod external { pub struct Config { pub enabled: bool } impl Default for Config { fn default() -> Self { Self { enabled: true } } } } fn config() -> external::Config { external::Config { enabled: true, ..Default::default() } }";

    assert!(
        changed_sources("composite/field-clear", source)
            .iter()
            .all(|changed| !changed.contains("external::Config { ..Default::default() }"))
    );
}

#[test]
fn composite_field_clear_rejects_imported_type_name_collisions() {
    let source = "#[derive(Default)] struct Config { enabled: bool } mod external { pub struct Config { pub enabled: bool } impl Default for Config { fn default() -> Self { Self { enabled: true } } } } fn config() -> external::Config { use external::Config; Config { enabled: true, ..Default::default() } }";

    assert!(
        changed_sources("composite/field-clear", source)
            .iter()
            .all(|changed| !changed.contains("Config { ..Default::default() }"))
    );
}

#[test]
fn composite_field_clear_rejects_inherent_and_shadowed_default_calls() {
    let source = "#[derive(Default)] struct Values { enabled: bool } impl Values { fn default() -> Self { Self { enabled: true } } } fn inherent() -> Values { Values { enabled: true, ..Values::default() } } mod shadowed { trait Default { fn default() -> Values; } impl Default for Values { fn default() -> Values { Values { enabled: true } } } fn value() -> Values { Values { enabled: true, ..Default::default() } } }";

    let changed = changed_sources("composite/field-clear", source);

    assert!(
        changed
            .iter()
            .all(|changed| !changed.contains("Values { ..Values::default() }"))
    );
    assert!(
        changed
            .iter()
            .all(|changed| !changed.contains("Values { ..Default::default() }"))
    );
}

#[test]
fn composite_field_clear_rejects_generic_default_and_custom_derive_paths() {
    let generic = "#[derive(Default)] struct Values { enabled: bool } trait Factory { fn default() -> Values; } fn value<Default: Factory>() -> Values { Values { enabled: true, ..Default::default() } }";
    let custom_derive = "mod custom { pub use core::default::Default; } #[derive(custom::Default)] struct Values { enabled: bool } fn value() -> Values { Values { enabled: true, ..::core::default::Default::default() } }";
    let shadowed_core = "mod core { pub mod default { pub use ::core::clone::Clone as Default; } } #[derive(core::default::Default)] struct Values { enabled: bool } impl ::core::default::Default for Values { fn default() -> Self { Self { enabled: true } } } fn value() -> Values { Values { enabled: true, ..::core::default::Default::default() } }";

    for source in [generic, custom_derive, shadowed_core] {
        assert!(
            changed_sources("composite/field-clear", source)
                .iter()
                .all(|changed| !changed.contains("Values { ..")),
            "an unproved Default source must not approve field removal: {source}"
        );
    }
}

#[test]
fn composite_field_clear_distinguishes_generic_and_standard_constructors() {
    let generic = "#[derive(Default)] struct Holder<T: Default> { value: T } trait Factory { fn new() -> Self; } fn value<String: Default + Factory>() -> Holder<String> { Holder { value: String::new(), ..Default::default() } }";
    let standard = "#[derive(Default)] struct Holder { value: String } fn value() -> Holder { Holder { value: ::std::string::String::new(), ..Default::default() } }";

    assert!(
        changed_sources("composite/field-clear", generic)
            .iter()
            .any(|changed| changed.contains("Holder { ..Default::default() }"))
    );
    assert!(changed_sources("composite/field-clear", standard).is_empty());
}

#[test]
fn composite_field_clear_keeps_generic_shadows_in_their_function() {
    let source = "#[derive(Default)] struct Holder { name: String, values: Vec<u8> } fn unrelated<Vec>() {} #[allow(non_snake_case)] fn String() {} fn value() -> Holder { Holder { name: String::new(), values: Vec::new(), ..Default::default() } }";

    assert!(changed_sources("composite/field-clear", source).is_empty());
}

#[test]
fn composite_field_clear_accepts_unrooted_standard_paths() {
    let source = "#[derive(Default)] struct Holder { name: String, values: Vec<u8> } fn value() -> Holder { Holder { name: std::string::String::new(), values: std::vec::Vec::new(), ..core::default::Default::default() } }";

    assert!(changed_sources("composite/field-clear", source).is_empty());
}

#[test]
fn composite_field_clear_accepts_the_standard_alloc_crate() {
    let source = "extern crate alloc; #[derive(Default)] struct Holder { name: alloc::string::String } fn value() -> Holder { Holder { name: alloc::string::String::new(), ..Default::default() } }";

    assert!(changed_sources("composite/field-clear", source).is_empty());
}

#[test]
fn composite_field_clear_rejects_a_manual_struct_default() {
    let source = "struct Values { enabled: bool } impl Default for Values { fn default() -> Self { Self { enabled: true } } } fn values() -> Values { Values { enabled: true, ..Default::default() } }";

    assert_eq!(
        changed_sources("composite/field-clear", source),
        vec![source.replacen("enabled: true", "enabled: false", 1)]
    );
}

#[test]
fn composite_field_clear_skips_empty_array_defaults() {
    let source = "#[derive(Default)] struct Values { empty: [u8; 0], borrowed: &'static [u8] } fn values() -> Values { Values { empty: [], borrowed: &[], ..Default::default() } }";

    assert!(changed_sources("composite/field-clear", source).is_empty());
}

#[test]
fn composite_field_clear_handles_negative_numeric_literals() {
    let source = "#[derive(Default)] struct Values { number: i32, zero: i32, float: f64, zero_float: f64 } fn direct() -> Values { Values { number: -2, zero: -0, float: -1.5, zero_float: -0.0 } } fn rest() -> Values { Values { number: -0, ..Default::default() } }";

    assert_eq!(
        changed_sources("composite/field-clear", source),
        vec![
            source.replacen("number: -2", "number: 0", 1),
            source.replacen("float: -1.5", "float: 0.0", 1),
            source.replacen("zero_float: -0.0", "zero_float: 0.0", 1),
        ]
    );
}

#[test]
fn remove_self_assign_removes_only_the_same_safe_place() {
    let source = "fn update(mut value: i32, mut pair: (i32, i32), other: i32, values: &mut [i32], index: usize, pointer: &mut i32) { value = value; pair.0 = pair.0; (value) = (value); value = other; value += value; values[index] = values[index]; *pointer = *pointer; }";

    assert_eq!(
        changed_sources("statement/remove-self-assign", source),
        vec![
            source.replacen("value = value;", "", 1),
            source.replacen("pair.0 = pair.0;", "", 1),
            source.replacen("(value) = (value);", "", 1),
        ]
    );
    assert_eq!(
        changed_sources("statement/remove", source),
        vec![
            source.replacen("value = other;", "", 1),
            source.replacen("value += value;", "", 1),
            source.replacen("values[index] = values[index];", "", 1),
            source.replacen("*pointer = *pointer;", "", 1),
        ]
    );
}

#[test]
fn context_nil_changes_direct_option_arguments() {
    let source = "struct Context; struct Sink; fn consume(_: Option<&Context>) {} impl Sink { fn consume(&self, _: Option<&Context>) {} } fn pass(context: &Context, sink: &Sink) { consume(Some(context)); consume(Option::Some(context)); sink.consume(::core::option::Option::Some(context)); let held = Some(context); consume(held); consume(None); }";

    assert_eq!(
        changed_sources("expression/context-nil", source),
        vec![
            source.replacen(
                "consume(Some(context))",
                "consume(::core::option::Option::None)",
                1
            ),
            source.replacen(
                "consume(Option::Some(context))",
                "consume(::core::option::Option::None)",
                1,
            ),
            source.replacen(
                "sink.consume(::core::option::Option::Some(context))",
                "sink.consume(::core::option::Option::None)",
                1,
            ),
        ]
    );
}

#[test]
fn context_nil_skips_shadowed_or_indirect_option_values() {
    let source = "struct Context; fn Some<T>(value: T) -> T { value } fn consume(_: Option<&Context>) {} fn pass(context: &Context) { consume(Some(context)); let held = ::core::option::Option::Some(context); consume(held); consume(::core::option::Option::None); }";

    assert!(changed_sources("expression/context-nil", source).is_empty());
}

#[test]
fn context_nil_skips_imported_some_and_option_names() {
    let source = "struct Context; mod custom { pub enum Choice<T> { Some(T) } pub struct Option; impl Option { pub fn Some<T>(value: T) -> ::core::option::Option<T> { ::core::option::Option::Some(value) } } } mod core { pub mod option { pub struct Option; impl Option { pub fn Some<T>(value: T) -> ::core::option::Option<T> { ::core::option::Option::Some(value) } } } } fn choice(_: custom::Choice<&Context>) {} fn option(_: ::core::option::Option<&Context>) {} fn pass_choice(context: &Context) { use custom::Choice::*; choice(Some(context)); } fn pass_option(context: &Context) { use custom::Option; option(Option::Some(context)); option(core::option::Option::Some(context)); }";

    assert!(changed_sources("expression/context-nil", source).is_empty());
}

#[test]
fn context_nil_skips_an_extern_crate_option_alias() {
    let source = "extern crate custom_option as Option; struct Context; fn option(_: ::core::option::Option<&Context>) {} fn pass(context: &Context) { option(Option::Some(context)); }";

    assert!(changed_sources("expression/context-nil", source).is_empty());
}

#[test]
fn context_nil_keeps_shadowing_inside_its_lexical_scope() {
    let source = "struct Context; fn consume(_: Option<&Context>) {} mod unrelated { pub fn Some<T>(value: T) -> T { value } } fn pass(context: &Context) { consume(Some(context)); }";

    assert_eq!(
        changed_sources("expression/context-nil", source),
        vec![source.replacen(
            "consume(Some(context))",
            "consume(::core::option::Option::None)",
            1,
        )]
    );
}

#[test]
fn context_nil_keeps_generic_option_shadows_in_their_function() {
    let source =
        "fn unrelated<Option>() {} fn consume(_: Option<i32>) {} fn pass() { consume(Some(1)); }";

    assert_eq!(
        changed_sources("expression/context-nil", source),
        vec![source.replacen(
            "consume(Some(1))",
            "consume(::core::option::Option::None)",
            1,
        )]
    );
}

#[test]
fn context_nil_does_not_use_a_function_signature_for_a_shadowing_value() {
    let source = "fn consume(_: Option<i32>) {} fn pass() { let consume = |value| value.is_some(); consume(Some(1)); }";

    assert_eq!(
        changed_sources("expression/context-nil", source),
        vec![source.replacen(
            "consume(Some(1))",
            "consume(::core::option::Option::None)",
            1,
        )]
    );
}

#[test]
fn composite_field_clear_does_not_treat_custom_none_as_option_none() {
    let source = "#[derive(Default)] struct Config { state: State } enum State { None } fn config() -> Config { Config { state: State::None, ..Default::default() } }";

    assert_eq!(
        changed_sources("composite/field-clear", source),
        vec![source.replacen("state: State::None, ", "", 1)]
    );
}

#[test]
fn return_mutator_uses_safe_primitive_defaults() {
    let source = "fn flag() -> bool { return true; } fn count() -> u32 { return 7; } fn ratio() -> f64 { return 1.5; } fn letter() -> char { return 'x'; } fn text() -> &'static str { return \"value\"; }";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![
            source.replacen("return true", "return false", 1),
            source.replacen("return 7", "return 0", 1),
            source.replacen("return 1.5", "return 0.0", 1),
            source.replacen("return 'x'", "return '\\0'", 1),
            source.replacen("return \"value\"", "return \"\"", 1),
        ]
    );
}

#[test]
fn return_mutator_keeps_generic_string_shadows_in_their_function() {
    let source = "fn unrelated<String>() {} fn value() -> String { return String::new(); }";

    assert!(changed_sources("statement/return", source).is_empty());
}

#[test]
fn return_mutator_supports_borrowed_optional_generic_and_default_types() {
    let source = "#[derive(Default)] struct Record { value: i32 } fn borrowed<'a>(borrowed_value: &'a [i32]) -> &'a [i32] { return borrowed_value; } fn optional<T>(value: T) -> Option<T> { return Some(value); } fn generic<T: Default>(generic_value: T) -> T { return generic_value; } fn where_generic<T>(where_value: T) -> T where T: Default { return where_value; } fn record(value: i32) -> Record { return Record { value }; }";
    let qualified_default = "::core::default::Default::default()";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![
            source.replacen("return borrowed_value", "return &[]", 1),
            source.replacen(
                "return Some(value)",
                "return ::core::option::Option::None",
                1,
            ),
            source.replacen(
                "return generic_value",
                &format!("return {qualified_default}"),
                1,
            ),
            source.replacen(
                "return where_value",
                &format!("return {qualified_default}"),
                1,
            ),
            source.replacen(
                "return Record { value }",
                &format!("return {qualified_default}"),
                1,
            ),
        ]
    );
}

#[test]
fn return_mutator_accepts_a_qualified_default_derive() {
    let source = "#[derive(core::default::Default)] struct Record { value: i32 } fn record(value: i32) -> Record { return Record { value }; }";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![source.replacen(
            "return Record { value }",
            "return ::core::default::Default::default()",
            1,
        )]
    );
}

#[test]
fn return_mutator_changes_a_custom_none_value() {
    let source = "#[allow(non_upper_case_globals)] const None: Option<i32> = Some(1); fn value() -> Option<i32> { return None; }";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![source.replacen("return None", "return ::core::option::Option::None", 1,)]
    );
}

#[test]
fn return_mutator_uses_default_for_self_in_an_inherent_impl() {
    let source = "#[derive(Default)] struct Record { value: i32 } impl Record { fn reset(value: i32) -> Self { return Self { value }; } }";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![source.replacen(
            "return Self { value }",
            "return ::core::default::Default::default()",
            1,
        )]
    );
}

#[test]
fn return_mutator_changes_each_supported_tuple_value() {
    let source = "fn pair(value: i32) -> (bool, i32) { return (true, value); }";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![
            source.replacen("(true, value)", "(false, value)", 1),
            source.replacen("(true, value)", "(true, 0)", 1),
        ]
    );
}

#[test]
fn return_mutator_uses_default_for_owned_standard_collections() {
    let source = "fn text(value: String) -> String { return value; } fn values(value: Vec<i32>) -> Vec<i32> { return value; }";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![
            source.replacen(
                "return value",
                "return ::core::default::Default::default()",
                1,
            ),
            replace_nth(
                source,
                "return value",
                "return ::core::default::Default::default()",
                1,
            ),
        ]
    );
}

#[test]
fn return_mutator_skips_known_default_equivalents() {
    let source = "fn flag() -> bool { return Default::default(); } fn text() -> String { return String::new(); } fn values() -> Vec<i32> { return Vec::new(); } fn integer() -> i32 { return -0; } fn float() -> f64 { return -0.0; }";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![source.replacen("return -0.0", "return 0.0", 1)]
    );
}

#[test]
fn return_mutator_does_not_hide_inherent_nonstandard_defaults() {
    let source = "#[derive(Default)] struct Values { enabled: bool } impl Values { fn default() -> Self { Self { enabled: true } } } fn values() -> Values { return Values::default(); }";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![source.replacen(
            "return Values::default()",
            "return ::core::default::Default::default()",
            1,
        )]
    );
}

#[test]
fn return_mutator_keeps_method_and_trait_return_scopes() {
    let source = "trait Enabled { fn enabled(&self) -> bool { return true; } } struct State; impl State { fn count(&self) -> i32 { return 3; } } impl Enabled for State {}";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![
            source.replacen("return true", "return false", 1),
            source.replacen("return 3", "return 0", 1),
        ]
    );
}

#[test]
fn return_mutator_keeps_return_scopes_and_skips_unsupported_values() {
    let source = "fn defaults() -> bool { return false; } fn tail() -> bool { true } fn result() -> Result<i32, ()> { return Ok(1); } fn borrowed<T>(value: &T) -> &T { return value; } fn unconstrained<T>(value: T) -> T { return value; } fn scoped() -> i32 { let closure = || -> bool { return true; }; let _future = async { return true; }; let _ = closure; return 4; }";

    assert_eq!(
        changed_sources("statement/return", source),
        vec![
            source.replacen("return true", "return false", 1),
            source.replacen("return 4", "return 0", 1),
        ]
    );
}

#[test]
fn value_mutators_reject_malformed_rust() {
    let registry = Registry::builtins();

    for source in [
        "fn missing(",
        "fn broken() { return",
        "struct Config { value: i32 } fn broken() { Config { value:",
    ] {
        for name in VALUE_MUTATOR_NAMES {
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
fn parser_corpus_keeps_every_value_mutation_valid_rust() {
    let corpus = [
        "#[derive(Default)] struct Café<'a> { name: &'a str, active: bool } fn make<'a>(name: &'a str) -> Café<'a> { return Café { name, active: true, ..Default::default() }; }",
        "fn generic<T: Default>(value: T) -> T { return value; } fn optional<T>(value: T) { consume(Some(&value)); }",
        "fn assign(mut pair: (i32, i32)) { pair.0 = pair.0; }",
        "fn nested() -> bool { let closure = || -> bool { return true; }; let _future = async { return false; }; return closure(); }",
        "fn raw() -> &'static str { return r#\"value\"#; }",
    ];
    let registry = Registry::builtins();

    for source in corpus {
        for name in VALUE_MUTATOR_NAMES {
            for mutation in registry
                .get(name)
                .unwrap_or_else(|| panic!("missing built-in {name}"))
                .mutations(source)
            {
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
fn value_fixture_exposes_every_supported_return_candidate() {
    let source = include_str!("fixtures/value/src/lib.rs");

    assert_eq!(changed_sources("statement/return", source).len(), 4);
}
