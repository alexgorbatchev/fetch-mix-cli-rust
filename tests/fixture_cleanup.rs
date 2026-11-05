mod common;

#[test]
#[ignore = "subprocess fixture driven by cleanup tests"]
fn fixture_process() {
    #[cfg(unix)]
    if std::env::var("FIXTURE_EXIT").as_deref() == Ok("separate-group") {
        use std::os::unix::process::CommandExt;
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "fixture_process", "--ignored"])
            .env("FIXTURE_EXIT", "stubborn")
            .process_group(0)
            .status()
            .unwrap();
        std::process::exit(status.code().unwrap_or(1));
    }
    let directory = common::scratch();
    let fixture = common::track_fixture(directory.as_ref());
    let output = std::process::Command::new(&fixture)
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    let record = std::path::PathBuf::from(std::env::var_os("FIXTURE_RECORD").unwrap());
    std::fs::write(record.with_extension("pid"), std::process::id().to_string()).unwrap();
    #[cfg(unix)]
    if std::env::var("FIXTURE_EXIT").as_deref() == Ok("stubborn") {
        // This subprocess deliberately requires the runner's bounded escalation.
        unsafe { libc::signal(libc::SIGTERM, libc::SIG_IGN) };
    }
    std::fs::write(&record, fixture.as_os_str().as_encoded_bytes()).unwrap();
    if std::env::var_os("FIXTURE_PANIC").is_some() {
        panic!("intentional fixture failure");
    }
    match std::env::var("FIXTURE_EXIT").as_deref() {
        Ok("failure") => std::process::exit(7),
        Ok("timeout") => std::process::exit(124),
        Ok("hold" | "stubborn") => loop {
            std::thread::park();
        },
        _ => {}
    }
    directory.close().unwrap();
}

fn check_cleanup(panic: bool) {
    let directory = common::scratch();
    let record = directory.path().join("fixture-path");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "fixture_process", "--ignored"])
        .env("FIXTURE_RECORD", &record);
    if panic {
        command.env("FIXTURE_PANIC", "1");
    }
    let output = command.output().unwrap();
    assert_eq!(output.status.success(), !panic);
    let fixture = std::fs::read_to_string(record).unwrap();
    assert!(
        !std::path::Path::new(&fixture).exists(),
        "fixture survived test exit: {fixture}"
    );
}

#[test]
fn fixture_executable_is_removed_on_success() {
    check_cleanup(false);
}

#[test]
fn fixture_executable_is_removed_on_panic() {
    check_cleanup(true);
}

#[cfg(unix)]
mod runner {
    use super::common;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    struct Running(Child);

    impl Drop for Running {
        fn drop(&mut self) {
            if self.0.try_wait().unwrap().is_none() {
                // The child is our live runner, which cleans its command group on TERM.
                unsafe { libc::kill(self.0.id() as libc::pid_t, libc::SIGTERM) };
                self.0.wait().unwrap();
            }
        }
    }

    fn start(scratch: &Path, record: &Path, mode: &str) -> Running {
        Running(
            Command::new("bash")
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/run-tests.sh"))
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", "fixture_process", "--ignored"])
                .env("TMPDIR", scratch)
                .env("FIXTURE_RECORD", record)
                .env("FIXTURE_EXIT", mode)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    fn wait_ready(record: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !record.exists() {
            assert!(Instant::now() < deadline, "fixture did not become ready");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_clean(scratch: &Path, record: &Path) {
        let fixture = std::fs::read_to_string(record).unwrap();
        assert!(!Path::new(&fixture).exists(), "fixture survived: {fixture}");
        let entries: Vec<_> = std::fs::read_dir(scratch)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, ["keep"]);
        assert_eq!(std::fs::read(scratch.join("keep")).unwrap(), b"other task");
        let pid: libc::pid_t = std::fs::read_to_string(record.with_extension("pid"))
            .unwrap()
            .parse()
            .unwrap();
        // Signal zero checks existence without changing the former fixture process.
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "fixture process survived"
        );
    }

    #[test]
    fn command_status_and_sibling_files_survive_cleanup() {
        let directory = common::scratch();
        let scratch = directory.path().join("scratch");
        std::fs::create_dir(&scratch).unwrap();
        std::fs::write(scratch.join("keep"), "other task").unwrap();
        for (mode, expected) in [("success", 0), ("failure", 7), ("timeout", 124)] {
            let record = directory.path().join(mode);
            let mut child = start(&scratch, &record, mode);
            assert_eq!(child.0.wait().unwrap().code(), Some(expected));
            assert_clean(&scratch, &record);
        }
    }

    #[test]
    fn signals_stop_the_command_before_removing_scratch() {
        let directory = common::scratch();
        let scratch = directory.path().join("scratch");
        std::fs::create_dir(&scratch).unwrap();
        std::fs::write(scratch.join("keep"), "other task").unwrap();
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            let record = directory.path().join(signal.to_string());
            let mut child = start(&scratch, &record, "hold");
            wait_ready(&record);
            // Signal only this runner; it owns and terminates the test process group.
            assert_eq!(
                unsafe { libc::kill(child.0.id() as libc::pid_t, signal) },
                0
            );
            assert_eq!(child.0.wait().unwrap().code(), Some(128 + signal));
            assert_clean(&scratch, &record);
        }
    }

    #[test]
    fn termination_escalates_for_a_stubborn_command() {
        check_stubborn_command("stubborn");
    }

    #[test]
    fn cancellation_stops_descendants_in_separate_process_groups() {
        check_stubborn_command("separate-group");
    }

    fn check_stubborn_command(mode: &str) {
        let directory = common::scratch();
        let scratch = directory.path().join("scratch");
        std::fs::create_dir(&scratch).unwrap();
        std::fs::write(scratch.join("keep"), "other task").unwrap();
        let record = directory.path().join("ready");
        let mut child = start(&scratch, &record, mode);
        wait_ready(&record);
        // The runner receives TERM; the fixture ignores it until escalation to KILL.
        assert_eq!(
            unsafe { libc::kill(child.0.id() as libc::pid_t, libc::SIGTERM) },
            0
        );
        assert_eq!(child.0.wait().unwrap().code(), Some(143));
        assert_clean(&scratch, &record);
    }

    #[test]
    fn concurrent_commands_keep_separate_scratch() {
        let directory = common::scratch();
        let scratch = directory.path().join("scratch");
        std::fs::create_dir(&scratch).unwrap();
        std::fs::write(scratch.join("keep"), "other task").unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        let mut one = start(&scratch, &first, "hold");
        let mut two = start(&scratch, &second, "hold");
        wait_ready(&first);
        wait_ready(&second);
        let first_path = std::fs::read_to_string(&first).unwrap();
        let second_path = std::fs::read_to_string(&second).unwrap();
        assert_ne!(first_path, second_path);
        // Stopping the first owner must preserve the second owner's fixture.
        assert_eq!(
            unsafe { libc::kill(one.0.id() as libc::pid_t, libc::SIGTERM) },
            0
        );
        assert_eq!(one.0.wait().unwrap().code(), Some(143));
        assert!(!Path::new(&first_path).exists());
        assert!(Path::new(&second_path).exists());
        // The second runner remains independently responsible for its own group.
        assert_eq!(
            unsafe { libc::kill(two.0.id() as libc::pid_t, libc::SIGTERM) },
            0
        );
        assert_eq!(two.0.wait().unwrap().code(), Some(143));
        assert_clean(&scratch, &second);
    }
}
