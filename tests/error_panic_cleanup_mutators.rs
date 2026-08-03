use mutarust::Registry;

const MUTATOR_NAMES: [&str; 3] = [
    "expression/errorf-wrap",
    "expression/recover-clear",
    "statement/defer-remove",
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
fn builtin_error_panic_and_cleanup_names_match_mutago() {
    let registry = Registry::builtins();

    for expected in MUTATOR_NAMES {
        assert!(
            registry.names().any(|name| name == expected),
            "missing built-in {expected}"
        );
    }
}

#[test]
fn error_wrap_removes_standard_error_source_links() {
    let source = "#[derive(Debug)] struct Cause; impl ::core::fmt::Display for Cause { fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { f.write_str(\"cause\") } } impl ::std::error::Error for Cause {} #[derive(Debug)] struct Wrapped { cause: Cause } impl ::std::error::Error for Wrapped { fn source(&self) -> ::core::option::Option<&(dyn ::std::error::Error + 'static)> { ::core::option::Option::Some(&self.cause) } } impl ::core::fmt::Display for Wrapped { fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { f.write_str(\"wrapped\") } }";

    assert_eq!(
        changed_sources("expression/errorf-wrap", source),
        vec![source.replacen(
            "::core::option::Option::Some(&self.cause)",
            "::core::option::Option::None",
            1,
        )]
    );
}

#[test]
fn error_wrap_rejects_source_methods_on_other_traits() {
    let source = "trait LocalError { fn source(&self) -> Option<&Self>; } struct Failure; impl LocalError for Failure { fn source(&self) -> Option<&Self> { Some(self) } }";

    assert!(changed_sources("expression/errorf-wrap", source).is_empty());
}

#[test]
fn error_wrap_accepts_a_standard_error_impl_in_a_module() {
    let source = "mod nested { #[derive(Debug)] struct Failure; impl ::core::fmt::Display for Failure { fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { formatter.write_str(\"failure\") } } impl ::std::error::Error for Failure { fn source(&self) -> ::core::option::Option<&(dyn ::std::error::Error + 'static)> { ::std::option::Option::Some(self) } } }";

    assert_eq!(
        changed_sources("expression/errorf-wrap", source),
        vec![source.replacen(
            "::std::option::Option::Some(self)",
            "::core::option::Option::None",
            1,
        )]
    );
}

#[test]
fn recovery_propagates_panics_from_standard_catch_unwind_calls() {
    let source =
        "fn caught() -> bool { ::std::panic::catch_unwind(|| panic!(\"failure\")).is_err() }";
    let call = "::std::panic::catch_unwind(|| panic!(\"failure\"))";
    let replacement = format!(
        "match {call} {{ ::core::result::Result::Ok(value) => ::core::result::Result::<_, ::std::boxed::Box<dyn ::core::any::Any + ::core::marker::Send>>::Ok(value), ::core::result::Result::Err(payload) => ::std::panic::resume_unwind(payload), }}"
    );

    assert_eq!(
        changed_sources("expression/recover-clear", source),
        vec![source.replacen(call, &replacement, 1)]
    );
}

#[test]
fn recovery_rejects_shadowed_and_nonstandard_catch_functions() {
    let source = "mod std { pub mod panic { pub fn catch_unwind<T>(value: T) -> T { value } } } fn catch_unwind<T>(value: T) -> T { value } fn run() { let _ = std::panic::catch_unwind(|| 1); let _ = catch_unwind(|| 2); }";

    assert!(changed_sources("expression/recover-clear", source).is_empty());
}

#[test]
fn cleanup_removes_documented_explicit_drop_timing() {
    let source = "struct Guard; impl ::core::ops::Drop for Guard { fn drop(&mut self) { release(); } } fn cleanup(first: Guard, second: Guard, third: Guard) { drop(first); work(); ::std::mem::drop(second); work(); ::core::mem::drop(third); work(); }";

    assert_eq!(
        changed_sources("statement/defer-remove", source),
        vec![
            source.replacen("drop(first);", "", 1),
            source.replacen("::std::mem::drop(second);", "", 1),
            source.replacen("::core::mem::drop(third);", "", 1),
        ]
    );
}

#[test]
fn cleanup_rejects_values_without_proved_cleanup_and_a_final_drop() {
    let source = "struct Guard; impl ::core::ops::Drop for Guard { fn drop(&mut self) {} } fn cleanup(value: u8, guard: Guard) { drop(value); work(); drop(&value); work(); drop(guard); }";

    assert!(changed_sources("statement/defer-remove", source).is_empty());
}

#[test]
fn cleanup_keeps_bindings_that_shadow_a_proved_guard() {
    let source = "struct Guard; impl ::core::ops::Drop for Guard { fn drop(&mut self) { release(); } } fn cleanup(guard: Guard) { let closure = |guard: u8| { drop(guard); work(); }; for guard in [1_u8] { drop(guard); work(); } if let Some(guard) = Some(1_u8) { drop(guard); work(); } while let Some(guard) = None::<u8> { drop(guard); work(); } closure(1); drop(guard); }";

    assert!(changed_sources("statement/defer-remove", source).is_empty());
}

#[test]
fn cleanup_rejects_empty_drop_work_and_a_later_item() {
    let source = "struct Empty; impl ::core::ops::Drop for Empty { fn drop(&mut self) {} } struct Guard; impl ::core::ops::Drop for Guard { fn drop(&mut self) { release(); } } fn cleanup(empty: Empty, guard: Guard) { drop(empty); work(); drop(guard); struct Marker; }";

    assert!(changed_sources("statement/defer-remove", source).is_empty());
}

#[test]
fn cleanup_does_not_share_drop_proof_between_modules() {
    let source = "mod proved { struct Guard; impl ::core::ops::Drop for Guard { fn drop(&mut self) {} } } mod unproved { struct Guard; fn cleanup(guard: Guard) { drop(guard); work(); } }";

    assert!(changed_sources("statement/defer-remove", source).is_empty());
}

#[test]
fn cleanup_rejects_shadowed_drop_functions() {
    let source = "fn drop(_: String) {} fn cleanup(value: String) { drop(value); }";

    assert!(changed_sources("statement/defer-remove", source).is_empty());
}

#[test]
fn standard_root_aliases_are_not_candidates() {
    let std_alias = "extern crate fake_std as std; fn run(value: String) { ::std::mem::drop(value); let _ = ::std::panic::catch_unwind(|| 1); }";
    let source_alias = "extern crate fake_core as core; #[derive(Debug)] struct Failure; impl ::std::error::Error for Failure { fn source(&self) -> ::core::option::Option<&(dyn ::std::error::Error + 'static)> { ::core::option::Option::Some(self) } }";

    assert!(changed_sources("expression/errorf-wrap", source_alias).is_empty());
    assert!(changed_sources("expression/recover-clear", std_alias).is_empty());
    assert!(changed_sources("statement/defer-remove", std_alias).is_empty());
}

#[test]
fn malformed_rust_has_no_error_panic_or_cleanup_candidates() {
    let source = "fn broken( { ::std::panic::catch_unwind(|| panic!());";

    for name in MUTATOR_NAMES {
        assert!(changed_sources(name, source).is_empty());
    }
}

#[test]
fn error_panic_cleanup_fixture_candidate_oracle_is_current() {
    let source = include_str!("fixtures/error-panic-cleanup/src/lib.rs");
    let expected = include_str!("fixtures/error-panic-cleanup/expected-mutants.txt");
    let states = [
        "Killed", "Escaped", "Killed", "Escaped", "Killed", "Escaped", "Skipped",
    ];
    let registry = Registry::builtins();
    let mut state_index = 0;
    let mut actual = Vec::new();
    for name in MUTATOR_NAMES {
        for mutation in registry.get(name).unwrap().mutations(source) {
            let (range, replacement) = mutation.identity();
            let original = source.get(range).expect("fixture range must be valid");
            actual.push(format!(
                "{name} :: {} :: {} :: {}",
                original.replace('\n', "\\n"),
                replacement.replace('\n', "\\n"),
                states[state_index]
            ));
            state_index += 1;
        }
    }

    assert_eq!(state_index, states.len());
    assert_eq!(actual.join("\n") + "\n", expected);
}
