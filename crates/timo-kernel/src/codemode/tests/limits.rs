//! codemode 资源上限、超时与取消（设计 phase-3.5/11 §4.4）。

use super::*;

#[tokio::test]
async fn output_limits_abort_the_script() {
    let host = host();
    let small = Limits {
        max_output_chars: 10,
        ..Limits::default()
    };
    let output = run(&host, r#"text("x".repeat(20))"#, small).await;
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).contains("Script output limit exceeded (10 characters)"),
        "{}",
        text(&output)
    );

    let few = Limits {
        max_output_calls: 1,
        ..Limits::default()
    };
    let output = run(&host, "text(1); text(2);", few).await;
    assert!(
        text(&output).contains("Script output limit exceeded (1 output calls)"),
        "{}",
        text(&output)
    );
}

#[tokio::test]
async fn timeout_and_cancel_interrupt_a_busy_script() {
    let host = host();
    let output = run(
        &host,
        "// @options: {\"timeout_ms\": 100}\nwhile (true) {}",
        Limits::default(),
    )
    .await;
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).contains("Script timed out: execution timed out after 100 ms"),
        "{}",
        text(&output)
    );

    let cancel = CancellationToken::new();
    let (ctx, rx) = ctx(
        Arc::clone(&host) as Arc<dyn NestedToolHost>,
        "while (true) {}",
        cancel.clone(),
    );
    let catalog = host.catalog();
    let task = tokio::spawn(sandbox::execute(
        ctx,
        Arc::new(catalog),
        Arc::new(Limits::default()),
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    let output = tokio::time::timeout(BOUND, task)
        .await
        .expect("cancel must be bounded")
        .unwrap()
        .unwrap();
    drop(rx);
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).contains("Script aborted: execution aborted"),
        "{}",
        text(&output)
    );
}

#[tokio::test]
async fn timeout_lets_nested_calls_clean_up_before_returning() {
    let host = host();
    let output = run(
        &host,
        "// @options: {\"timeout_ms\": 200}\nawait tools.cancellable({});",
        Limits::default(),
    )
    .await;
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).contains("Script timed out: execution timed out after 200 ms"),
        "{}",
        text(&output)
    );
    assert!(
        host.cancelled.load(Ordering::SeqCst),
        "取消必须先传给嵌套调用，让它清理（杀进程树），不能直接 abort"
    );
}

#[tokio::test]
async fn memory_limit_surfaces_as_a_script_error() {
    let host = host();
    let tiny = Limits {
        memory_bytes: 4 * 1024 * 1024,
        ..Limits::default()
    };
    let output = run(
        &host,
        "const a = []; while (true) { a.push(\"x\".repeat(4096)); }",
        tiny,
    )
    .await;
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).to_lowercase().contains("memory"),
        "{}",
        text(&output)
    );
}

#[tokio::test]
async fn a_busy_worker_pool_fails_fast_instead_of_queueing_forever() {
    // 占满进程级 worker（默认 2 个），让新脚本在 worker 等待上超时。
    let permits = sandbox::workers(2).acquire_many_owned(2).await.unwrap();
    let host = host();
    let output = run(
        &host,
        "// @options: {\"timeout_ms\": 100}\ntext(1)",
        Limits::default(),
    )
    .await;
    drop(permits);
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).contains("while waiting for a free codemode worker"),
        "{}",
        text(&output)
    );
}
