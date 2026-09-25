use ao_process::{
    is_process_alive, managed_processes, ManagedProcess, ProcessEvent, SpawnSpec, Stream,
};
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

#[tokio::test(flavor = "multi_thread")]
async fn a_killed_process_is_always_reported_as_killed() {
    // The exit can be noticed before the kill request is: the report must
    // still say that Agent Office ended it.
    for _ in 0..25 {
        let (process, rx) =
            ManagedProcess::spawn(SpawnSpec::new(fake_child()).arg("sleep")).unwrap();
        process.kill_tree();
        let events = collect_until_exit(rx).await;
        let Some(ProcessEvent::Exited(info)) = events.last() else {
            panic!("no exit")
        };
        assert!(info.killed, "{info:?}");
        assert!(!info.success);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn processes_left_running_by_a_finished_child_are_stopped_and_counted() {
    let dir = std::env::temp_dir().join(format!("ao-process-orphan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pidfile = dir.join("orphan.pids");
    let (_process, rx) = ManagedProcess::spawn(
        SpawnSpec::new(fake_child())
            .arg("orphan")
            .arg(pidfile.as_os_str()),
    )
    .unwrap();
    let events = collect_until_exit(rx).await;
    let Some(ProcessEvent::Exited(info)) = events.last() else {
        panic!("no exit")
    };
    assert!(info.success, "{info:?}");
    assert!(!info.killed);
    assert!(info.leftovers >= 1, "{info:?}");
    assert!(info.describe().contains("leftover"), "{}", info.describe());
    let grandchild: u32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        wait_until(|| !is_process_alive(grandchild), Duration::from_secs(10)).await,
        "leftover grandchild {grandchild} survived"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn registry_output_time_and_tree_size_are_reported() {
    let dir = std::env::temp_dir().join(format!("ao-process-registry-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pidfile = dir.join("tree.pids");
    let (process, mut rx) = ManagedProcess::spawn(
        SpawnSpec::new(fake_child())
            .arg("tree")
            .arg(pidfile.as_os_str()),
    )
    .unwrap();
    process.set_label("test session");
    assert_eq!(process.last_output_ms(), None);
    // "ready" is printed once the grandchild is up.
    let first = tokio::time::timeout(Duration::from_secs(20), rx.recv())
        .await
        .unwrap();
    assert!(matches!(first, Some(ProcessEvent::Line { .. })));
    assert!(process.last_output_ms().is_some());
    let snapshot = managed_processes()
        .into_iter()
        .find(|p| p.pid == process.pid())
        .expect("listed");
    assert_eq!(snapshot.label, "test session");
    assert!(snapshot.exit.is_none());
    if cfg!(any(windows, target_os = "linux")) {
        assert!(snapshot.tree_processes.unwrap_or(0) >= 2, "{snapshot:?}");
    }
    process.kill_tree();
    let events = collect_until_exit(rx).await;
    assert!(matches!(events.last(), Some(ProcessEvent::Exited(i)) if i.killed));
    drop(process);
    assert!(managed_processes()
        .iter()
        .all(|p| p.label != "test session"));
    let _ = std::fs::remove_dir_all(dir);
}

/// npm installs CLIs as `.cmd` shims; Windows runs them through cmd.exe.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn cmd_shims_pass_arguments_and_exit_codes_and_die_with_their_tree() {
    let dir = std::env::temp_dir().join(format!("ao-process-shim-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let shim = dir.join("fake agent.cmd");
    std::fs::write(
        &shim,
        format!("@echo off\r\n\"{}\" %*\r\n", fake_child().display()),
    )
    .unwrap();

    let (_p, rx) =
        ManagedProcess::spawn(SpawnSpec::new(&shim).args(["args", "two words", "C:\\a b\\c"]))
            .unwrap();
    let events = collect_until_exit(rx).await;
    let lines: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            ProcessEvent::Line {
                stream: Stream::Stdout,
                line,
            } => Some(line.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(lines, vec!["arg: two words", "arg: C:\\a b\\c"]);

    let (_p, rx) = ManagedProcess::spawn(SpawnSpec::new(&shim).args(["print", "1", "3"])).unwrap();
    let events = collect_until_exit(rx).await;
    assert!(matches!(events.last(), Some(ProcessEvent::Exited(i)) if i.code == Some(3)));

    let pidfile = dir.join("tree.pids");
    let (process, rx) =
        ManagedProcess::spawn(SpawnSpec::new(&shim).arg("tree").arg(pidfile.as_os_str())).unwrap();
    assert!(
        wait_until(
            || std::fs::read_to_string(&pidfile).is_ok_and(|s| s.contains(' ')),
            Duration::from_secs(20)
        )
        .await
    );
    let grandchild: u32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    process.kill_tree();
    collect_until_exit(rx).await;
    assert!(
        wait_until(|| !is_process_alive(grandchild), Duration::from_secs(10)).await,
        "grandchild {grandchild} of the shim survived"
    );
    let _ = std::fs::remove_dir_all(dir);
}
