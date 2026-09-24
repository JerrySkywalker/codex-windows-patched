use codex_utils_pty::JobObject;
use pretty_assertions::assert_eq;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use windows_sys::Win32::System::Console::GetConsoleWindow;
use windows_sys::Win32::System::Threading::DETACHED_PROCESS;

#[tokio::test]
async fn missing_junction_from_detached_parent_preserves_capture_and_fallback() -> anyhow::Result<()>
{
    if std::env::var_os("CODEX_TEST_CWD_JUNCTION_CHILD").is_none() {
        let profile = tempfile::tempdir()?;
        let cleanup = JobObject::create_without_breakaway()?;
        let mut command = tokio::process::Command::new(std::env::current_exe()?);
        command
            .args([
                "--exact",
                "win::cwd_junction::tests::missing_junction_from_detached_parent_preserves_capture_and_fallback",
                "--nocapture",
            ])
            .env("CODEX_TEST_CWD_JUNCTION_CHILD", "1")
            .env("USERPROFILE", profile.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = cleanup.spawn_contained_with_flags(&mut command, DETACHED_PROCESS)?;
        let output =
            tokio::time::timeout(Duration::from_secs(20), child.wait_with_output()).await??;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
        print!("{}", String::from_utf8_lossy(&output.stdout));
        return Ok(());
    }
    assert_eq!(unsafe { GetConsoleWindow() }, 0);
    let profile = PathBuf::from(std::env::var_os("USERPROFILE").expect("disposable profile"));
    let target = profile.join("target with spaces");
    let logs = profile.join("logs");
    std::fs::create_dir_all(&target)?;
    std::fs::create_dir_all(&logs)?;
    std::fs::write(target.join("sentinel"), b"junction target")?;
    let root = super::junction_root_for_userprofile(&profile.to_string_lossy());
    let expected = root.join(super::junction_name_for_path(&target));
    assert!(!expected.exists(), "must invoke the missing-junction spawn");
    let junction = super::create_cwd_junction(&target, Some(&logs)).expect("junction creation");
    assert_eq!(junction, expected);
    assert_eq!(
        dunce::canonicalize(&junction)?,
        dunce::canonicalize(&target)?
    );
    assert_eq!(
        std::fs::read(junction.join("sentinel"))?,
        b"junction target"
    );
    assert_eq!(
        super::create_cwd_junction(&target, Some(&logs)),
        Some(junction.clone())
    );

    // mklink can create a dangling junction successfully. The product still falls
    // back when exists() is false, retaining the captured success stdout in its log.
    let missing = profile.join("missing target");
    assert_eq!(super::create_cwd_junction(&missing, Some(&logs)), None);
    // A dangling junction fails exists(), so trying the same link again reaches
    // mklink and its already-existing-link error, retaining captured stderr.
    assert_eq!(super::create_cwd_junction(&missing, Some(&logs)), None);
    let log = std::fs::read_dir(&logs)?
        .map(|entry| std::fs::read_to_string(entry?.path()))
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    assert!(
        log.contains("junction: creating via cmd /c mklink /J"),
        "{log}"
    );
    assert!(
        log.contains("junction: created ") && log.contains("junction: reusing existing "),
        "{log}"
    );
    let failures: Vec<_> = log
        .lines()
        .filter(|line| line.contains("junction: mklink failed status="))
        .collect();
    assert_eq!(failures.len(), 2, "{log}");
    assert!(
        failures.iter().any(|line| line
            .split_once("stdout=")
            .is_some_and(|(_, output)| !output.starts_with(" stderr="))),
        "{log}"
    );
    assert!(
        failures.iter().any(|line| line
            .split_once("stderr=")
            .is_some_and(|(_, error)| !error.is_empty())),
        "{log}"
    );
    // Remove the disposable links themselves before the parent's TempDir cleanup.
    std::fs::remove_dir(junction)?;
    std::fs::remove_dir(root.join(super::junction_name_for_path(&missing)))?;
    println!("RUNTIME_CWD_HELPER_FUNCTIONAL=PASS\n{log}");
    Ok(())
}
