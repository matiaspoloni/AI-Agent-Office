use ao_process::{is_process_alive, ManagedProcess, ProcessEvent, SpawnSpec, Stream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fake_child() -> PathBuf {
    ao_testkit::bins::cargo_bin("ao-testkit", "ao-fake-child")
}

async fn collect_until_exit(
    mut rx: tokio::sync::mpsc::Receiver<ProcessEvent>,
) -> Vec<ProcessEvent> {
    let mut events = Vec::new();
    while let Some(event) = tokio::time::timeout(Duration::from_secs(20), rx.recv())
        .await
        .expect("event")
    {
        let done = matches!(event, ProcessEvent::Exited(_));
        events.push(event);
        if done {
            break;
        }
    }
    events
}

async fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    condition()
}

#[tokio::test(flavor = "multi_thread")]
async fn captures_stdout_stderr_and_exit_code() {
    let (process, rx) =
        ManagedProcess::spawn(SpawnSpec::new(fake_child()).args(["print", "3", "7"])).unwrap();
    assert!(process.pid() > 0);
    let events = collect_until_exit(rx).await;
    let stdout: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ProcessEvent::Line {
                stream: Stream::Stdout,
                line,
            } => Some(line.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(stdout, vec!["line 0", "line 1", "line 2"]);
    assert!(events.iter().any(
        |e| matches!(e, ProcessEvent::Line { stream: Stream::Stderr, line } if line == "err 0")
    ));
    let Some(ProcessEvent::Exited(info)) = events.last() else {
        panic!("no exit event")
    };
    assert_eq!(info.code, Some(7));
    assert!(!info.success);
    assert!(!info.killed);
    assert_eq!(process.exit_info().unwrap().code, Some(7));
}

#[tokio::test(flavor = "multi_thread")]
async fn writes_stdin_and_stops_gracefully_on_eof() {
    let (process, mut rx) =
        ManagedProcess::spawn(SpawnSpec::new(fake_child()).arg("echo")).unwrap();
    process.write_line("hello").await.unwrap();
    let first = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        first,
        ProcessEvent::Line {
            stream: Stream::Stdout,
            line: "echo: hello".into()
        }
    );
    let info = process.stop(Duration::from_secs(10)).await;
    assert!(info.success, "{info:?}");
    assert!(!info.killed);
    assert!(process.write_line("late").await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn kill_tree_ends_grandchildren_but_not_foreign_processes() {
    let dir = std::env::temp_dir().join(format!("ao-process-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pidfile = dir.join("tree.pids");

    // A process that is NOT part of the tree we kill.
    let (bystander, _bystander_rx) =
        ManagedProcess::spawn(SpawnSpec::new(fake_child()).arg("sleep")).unwrap();

    let (process, rx) = ManagedProcess::spawn(
        SpawnSpec::new(fake_child())
            .arg("tree")
            .arg(pidfile.as_os_str()),
    )
    .unwrap();
    assert!(
        wait_until(
            || std::fs::read_to_string(&pidfile).is_ok_and(|s| s.contains(' ')),
            Duration::from_secs(20)
        )
        .await
    );
    let pids: Vec<u32> = std::fs::read_to_string(&pidfile)
        .unwrap()
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    let grandchild = pids[1];
    assert!(is_process_alive(grandchild));

    process.kill_tree();
    let events = collect_until_exit(rx).await;
    let Some(ProcessEvent::Exited(info)) = events.last() else {
        panic!("no exit")
    };
    assert!(info.killed);
    assert!(
        wait_until(|| !is_process_alive(grandchild), Duration::from_secs(10)).await,
        "grandchild {grandchild} survived"
    );
    assert!(
        bystander.is_running(),
        "a process outside the tree was affected"
    );
    assert!(is_process_alive(bystander.pid()));

    bystander.kill_tree();
    bystander.wait(Duration::from_secs(10)).await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_handle_does_not_leave_orphans() {
    let (process, _rx) = ManagedProcess::spawn(SpawnSpec::new(fake_child()).arg("sleep")).unwrap();
    let pid = process.pid();
    assert!(is_process_alive(pid));
    drop(process);
    assert!(wait_until(|| !is_process_alive(pid), Duration::from_secs(10)).await);
}
