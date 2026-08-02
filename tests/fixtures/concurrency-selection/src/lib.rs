use std::sync::mpsc;
use std::thread;

tokio::task_local! {
    static TASK_MARKER: ();
}

pub fn standard_spawns_use_new_threads() -> [bool; 3] {
    let (direct_send, direct_receive) = mpsc::channel();
    let direct_caller = thread::current().id();
    std::thread::spawn(move || {
        direct_send
            .send(thread::current().id() == direct_caller)
            .unwrap();
    });

    let (root_send, root_receive) = mpsc::channel();
    let root_caller = thread::current().id();
    ::std::thread::spawn(move || {
        root_send
            .send(thread::current().id() == root_caller)
            .unwrap();
    });

    let (imported_send, imported_receive) = mpsc::channel();
    let imported_caller = thread::current().id();
    thread::spawn(move || {
        imported_send
            .send(thread::current().id() == imported_caller)
            .unwrap();
    });

    [
        direct_receive.recv().unwrap(),
        root_receive.recv().unwrap(),
        imported_receive.recv().unwrap(),
    ]
}

pub async fn task_spawns_use_new_tasks() -> [bool; 6] {
    TASK_MARKER
        .scope((), async {
            let (direct_send, direct_receive) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                direct_send.send(TASK_MARKER.try_with(|_| true).unwrap_or(false)).unwrap();
            });

            let (root_send, root_receive) = tokio::sync::oneshot::channel();
            ::tokio::spawn(async move {
                root_send.send(TASK_MARKER.try_with(|_| true).unwrap_or(false)).unwrap();
            });

            let (task_send, task_receive) = tokio::sync::oneshot::channel();
            tokio::task::spawn(async move {
                task_send.send(TASK_MARKER.try_with(|_| true).unwrap_or(false)).unwrap();
            });

            let (root_task_send, root_task_receive) = tokio::sync::oneshot::channel();
            ::tokio::task::spawn(async move {
                root_task_send.send(TASK_MARKER.try_with(|_| true).unwrap_or(false)).unwrap();
            });

            let from_closure = (async || {
                let (send, receive) = tokio::sync::oneshot::channel();
                tokio::spawn(async move {
                    send.send(TASK_MARKER.try_with(|_| true).unwrap_or(false)).unwrap();
                });
                receive.await.unwrap()
            })()
            .await;

            let from_block = async {
                let (send, receive) = tokio::sync::oneshot::channel();
                tokio::task::spawn(async move {
                    send.send(TASK_MARKER.try_with(|_| true).unwrap_or(false)).unwrap();
                });
                receive.await.unwrap()
            }
            .await;

            [
                direct_receive.await.unwrap(),
                root_receive.await.unwrap(),
                task_receive.await.unwrap(),
                root_task_receive.await.unwrap(),
                from_closure,
                from_block,
            ]
        })
        .await
}

pub async fn select_value(mode: u8) -> &'static str {
    tokio::select! {
        biased;
        value = async { "outer-first" }, if mode == 1 => value,
        _ = async {}, if mode >= 2 => nested_select_value(mode).await,
        else => "outer-fallback",
    }
}

async fn nested_select_value(mode: u8) -> &'static str {
    ::tokio::select! {
        value = async { "inner-first" }, if mode == 2 => value,
        value = async { "inner-second" }, if mode == 3 => value,
        else => "inner-fallback",
    }
}

#[cfg(any())]
async fn invalid_select_input_is_not_a_candidate() {
    tokio::select! { _ = => invalid(), else => }
}
