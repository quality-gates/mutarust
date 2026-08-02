fn changed_sources(mutator: &str, source: &str) -> Vec<String> {
    mutarust::Registry::builtins()
        .get(mutator)
        .expect("the built-in mutator must exist")
        .mutations(source)
        .into_iter()
        .map(|mutation| {
            mutation
                .apply(source)
                .expect("the mutation range must apply")
        })
        .collect()
}

#[test]
fn concurrency_runs_documented_standard_thread_spawns_immediately() {
    let source = "use std::thread; fn work() {} fn run() { std::thread::spawn(move || work()); ::std::thread::spawn(work); thread::spawn(|| work()); }";

    assert_eq!(
        changed_sources("concurrency/goroutine-remove", source),
        vec![
            source.replacen(
                "std::thread::spawn(move || work())",
                "(move || work())()",
                1,
            ),
            source.replacen("::std::thread::spawn(work)", "(work)()", 1),
            source.replacen("thread::spawn(|| work())", "(|| work())()", 1),
        ]
    );
}

#[test]
fn concurrency_awaits_documented_tokio_tasks_in_an_async_context() {
    let source = "async fn work() {} async fn run() { tokio::spawn(async move { work().await; }); ::tokio::task::spawn(work()); } fn outside() { tokio::spawn(work()); }";

    assert_eq!(
        changed_sources("concurrency/goroutine-remove", source),
        vec![
            source.replacen(
                "tokio::spawn(async move { work().await; })",
                "(async move { work().await; }).await",
                1,
            ),
            source.replacen("::tokio::task::spawn(work())", "(work()).await", 1,),
        ]
    );
}

#[test]
fn concurrency_rejects_local_modules_that_shadow_supported_crates() {
    let source = "mod std { pub mod thread { pub fn spawn<T>(_: T) {} } } mod tokio { pub fn spawn<T>(_: T) {} pub mod task { pub fn spawn<T>(_: T) {} } } async fn run() { std::thread::spawn(|| {}); tokio::spawn(async {}); tokio::task::spawn(async {}); }";

    assert!(changed_sources("concurrency/goroutine-remove", source).is_empty());
}

#[test]
fn concurrency_accepts_root_paths_but_rejects_ambiguous_thread_imports() {
    let source = "mod std { pub mod thread {} } mod tokio {} mod fake { pub fn spawn<T>(_: T) {} } fn work() {} async fn future() {} async fn run() { ::std::thread::spawn(work); ::tokio::spawn(future()); { use ::std::thread; thread::spawn(|| work()); } { use crate::fake as thread; thread::spawn(work); } }";

    assert_eq!(
        changed_sources("concurrency/goroutine-remove", source),
        vec![
            source.replacen("::std::thread::spawn(work)", "(work)()", 1),
            source.replacen("::tokio::spawn(future())", "(future()).await", 1),
            source.replacen("thread::spawn(|| work())", "(|| work())()", 1),
        ]
    );
}

#[test]
fn concurrency_rejects_root_paths_rebound_by_crate_root_aliases() {
    let source = "extern crate fake_std as std; extern crate fake_tokio as tokio; mod nested { fn work() {} async fn future() {} fn run() { ::std::thread::spawn(work); } async fn async_run() { ::tokio::spawn(future()); { ::tokio::task::spawn(future()); } } }";

    assert!(changed_sources("concurrency/goroutine-remove", source).is_empty());
}

#[test]
fn concurrency_rejects_spawn_results_methods_and_unsupported_runtimes() {
    let source = "fn work() {} async fn future() {} fn run() -> std::thread::JoinHandle<()> { let _handle = std::thread::spawn(work); std::thread::Builder::new().spawn(work).unwrap(); rayon::spawn(work); std::thread::spawn(work) } async fn async_run() { let _handle = tokio::spawn(future()); tokio::task::spawn_blocking(work); async_std::task::spawn(future()); }";

    assert!(changed_sources("concurrency/goroutine-remove", source).is_empty());
}

#[test]
fn concurrency_tracks_async_blocks_and_closures() {
    let source = "async fn first() {} async fn second() {} async fn third() {} async fn run() { (|| { tokio::spawn(first()); })(); (async || { tokio::spawn(second()); })().await; async { tokio::task::spawn(third()); }.await; }";

    assert_eq!(
        changed_sources("concurrency/goroutine-remove", source),
        vec![
            source.replacen("tokio::spawn(second())", "(second()).await", 1),
            source.replacen("tokio::task::spawn(third())", "(third()).await", 1),
        ]
    );
}

#[test]
fn concurrency_rejects_unrooted_standard_import_through_a_local_std_module() {
    let source = "mod std { pub mod thread { pub fn spawn<T>(_: T) {} } } use std::thread; fn run() { thread::spawn(|| {}); }";

    assert!(changed_sources("concurrency/goroutine-remove", source).is_empty());
}

#[test]
fn concurrency_returns_no_candidates_for_malformed_rust() {
    assert!(
        changed_sources(
            "concurrency/goroutine-remove",
            "fn run( { std::thread::spawn(|| {});",
        )
        .is_empty()
    );
}

#[test]
fn concurrency_and_selection_reject_generic_runtime_shadows() {
    let source = "struct Wrapper<T>(T); impl<tokio> Wrapper<tokio> { async fn run() { tokio::spawn(async {}); tokio::select! { _ = first() => one(), _ = second() => two(), } } } trait Worker<tokio> { async fn work() { tokio::spawn(async {}); tokio::select! { _ = first() => one(), _ = second() => two(), } } }";

    assert!(changed_sources("concurrency/goroutine-remove", source).is_empty());
    assert!(changed_sources("select/case-remove", source).is_empty());
}

#[test]
fn selection_removes_complete_tokio_branches() {
    let source = r#"async fn run() {
    tokio::select! {
        value = first(), if ready() => use_first(value),
        _ = second() => use_second(),
        else => fallback(),
    }
}"#;

    assert_eq!(
        changed_sources("select/case-remove", source),
        vec![
            source.replacen("value = first(), if ready() => use_first(value),", "", 1,),
            source.replacen("_ = second() => use_second(),", "", 1),
        ]
    );
}

#[test]
fn selection_removes_the_complete_tokio_fallback() {
    let source = r#"async fn run() {
    tokio::select! {
        _ = first() => use_first(),
        else => fallback(),
    }
}"#;

    assert_eq!(
        changed_sources("select/default-remove", source),
        vec![source.replacen("else => fallback(),", "", 1)]
    );
}

#[test]
fn selection_supports_biased_guards_and_nested_tokio_selections() {
    let source = r#"async fn run() {
    tokio::select! {
        biased;
        value = first(), if ready(value) => use_first(value),
        _ = second() => tokio::select! {
            _ = inner_one() => use_one(),
            _ = inner_two() => use_two(),
        },
    }
}"#;

    assert_eq!(
        changed_sources("select/case-remove", source),
        vec![
            source.replacen(
                "value = first(), if ready(value) => use_first(value),",
                "",
                1,
            ),
            source.replacen(
                "_ = second() => tokio::select! {\n            _ = inner_one() => use_one(),\n            _ = inner_two() => use_two(),\n        },",
                "",
                1,
            ),
            source.replacen("_ = inner_one() => use_one(),", "", 1),
            source.replacen("_ = inner_two() => use_two(),", "", 1),
        ]
    );
}

#[test]
fn selection_keeps_one_clause_and_accepts_a_fallback_as_the_other_clause() {
    let source = "async fn run() { tokio::select! { _ = work() => used(), else => fallback(), } }";

    assert_eq!(
        changed_sources("select/case-remove", source),
        vec![source.replacen("_ = work() => used(),", "", 1)]
    );
    assert_eq!(
        changed_sources("select/default-remove", source),
        vec![source.replacen("else => fallback(),", "", 1)]
    );
}

#[test]
fn selection_rejects_a_single_case_and_an_only_fallback() {
    let case = "async fn run() { tokio::select! { _ = work() => used(), } }";
    let fallback = "async fn run() { tokio::select! { else => fallback(), } }";

    assert!(changed_sources("select/case-remove", case).is_empty());
    assert!(changed_sources("select/default-remove", fallback).is_empty());
}

#[test]
fn selection_rejects_shadows_unsupported_macros_and_invalid_input() {
    let shadowed = "mod tokio { macro_rules! select { ($($token:tt)*) => {}; } } async fn run() { tokio::select! { _ = first() => one(), _ = second() => two(), } }";
    let unsupported = "async fn run() { futures::select! { first() => one(), second() => two(), } other!(tokio::select! { _ = first() => one(), _ = second() => two(), }); }";
    let invalid = "async fn run() { tokio::select! { _ = => one(), else => } }";

    for source in [shadowed, unsupported, invalid] {
        assert!(changed_sources("select/case-remove", source).is_empty());
        assert!(changed_sources("select/default-remove", source).is_empty());
    }
}

#[test]
fn selection_accepts_a_root_qualified_tokio_macro_through_a_shadow() {
    let source = "mod tokio {} async fn run() { ::tokio::select! { _ = first() => one(), _ = second() => two(), } }";

    assert_eq!(
        changed_sources("select/case-remove", source),
        vec![
            source.replacen("_ = first() => one(),", "", 1),
            source.replacen("_ = second() => two(),", "", 1),
        ]
    );
}

#[test]
fn selection_rejects_a_root_path_rebound_by_a_crate_root_alias() {
    let source = "extern crate fake_tokio as tokio; mod nested { async fn run() { { ::tokio::select! { _ = first() => one(), _ = second() => two(), } } } }";

    assert!(changed_sources("select/case-remove", source).is_empty());
    assert!(changed_sources("select/default-remove", source).is_empty());
}

#[test]
fn concurrency_selection_fixture_candidate_oracle_is_current() {
    let source = include_str!("fixtures/concurrency-selection/src/lib.rs");
    let expected = include_str!("fixtures/concurrency-selection/expected-mutants.txt");
    let states = [
        "Killed", "Killed", "Escaped", "Killed", "Killed", "Killed", "Killed", "Killed", "Killed",
        "Escaped", "Killed", "Killed", "Killed", "Killed", "Killed",
    ];
    let registry = mutarust::Registry::builtins();
    let mut state_index = 0;
    let mut actual = Vec::new();
    for name in [
        "concurrency/goroutine-remove",
        "select/case-remove",
        "select/default-remove",
    ] {
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
