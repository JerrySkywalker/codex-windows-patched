use std::ffi::OsString;
use std::os::windows::process::CommandExt;

use pretty_assertions::assert_eq;

fn detached(test: &str) -> anyhow::Result<bool> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_some() {
        return Ok(false);
    }
    let output = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", test, "--nocapture"])
        .env("CODEX_TEST_BACKGROUND_CHILD", "1")
        .creation_flags(0x0000_0008)
        .output()?;
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "{output:?}"
    );
    Ok(true)
}

#[test]
fn synchronous_operations_preserve_captured_io_without_a_console() -> anyhow::Result<()> {
    if detached(
        "windows_presentation_tests::synchronous_operations_preserve_captured_io_without_a_console",
    )? {
        return Ok(());
    }
    let cwd = tempfile::tempdir()?;
    let script = r#"Add-Type -Name Probe -Namespace GitPresentation -MemberDefinition '
        [System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();';
        [Console]::Out.Write('console={0}',[GitPresentation.Probe]::GetConsoleWindow().ToInt64());
        [Console]::Error.Write('captured stderr'); exit 23"#;
    let env = [(
        OsString::from("CODEX_GIT_CONSOLE_PROBE"),
        OsString::from(script),
    )];
    let error = crate::operations::run_git_for_stdout(cwd.path(), ["-c",
        r#"alias.console-probe=!powershell.exe -NoProfile -NonInteractive -Command "$CODEX_GIT_CONSOLE_PROBE""#,
        "console-probe"], Some(&env)).expect_err("probe must preserve the failure status");
    match error {
        crate::GitToolingError::GitCommand { status, stderr, .. } => {
            assert_eq!(
                (status.code(), stderr),
                (Some(23), "captured stderr".to_owned())
            );
        }
        other => panic!("unexpected error: {other}"),
    }
    let script = script.replace("exit 23", "exit 0");
    let env = [(
        OsString::from("CODEX_GIT_CONSOLE_PROBE"),
        OsString::from(script),
    )];
    let stdout = crate::operations::run_git_for_stdout(
        cwd.path(),
        [
            "-c",
            r#"alias.console-probe=!powershell.exe -NoProfile -NonInteractive -Command "$CODEX_GIT_CONSOLE_PROBE""#,
            "console-probe",
        ],
        Some(&env),
    )?;
    assert_eq!(stdout, "console=0");
    Ok(())
}

#[test]
fn synchronous_apply_root_and_stage_from_detached_parent() -> anyhow::Result<()> {
    if detached(
        "windows_presentation_tests::synchronous_apply_root_and_stage_from_detached_parent",
    )? {
        return Ok(());
    }
    let cwd = tempfile::tempdir()?;
    crate::operations::run_git_for_status(cwd.path(), ["init", "--quiet"], /*env*/ None)?;
    let diff = "diff --git a/file.txt b/file.txt\nnew file mode 100644\n--- /dev/null\n+++ b/file.txt\n@@ -0,0 +1 @@\n+content\n";
    let request = crate::ApplyGitRequest {
        cwd: cwd.path().to_path_buf(),
        diff: diff.to_owned(),
        revert: false,
        preflight: true,
    };
    let result = crate::apply_git_patch(&request)?;
    assert_eq!(
        (result.exit_code, result.stdout, result.stderr),
        (0, String::new(), String::new())
    );
    assert!(!cwd.path().join("file.txt").exists());
    std::fs::write(cwd.path().join("file.txt"), "content\n")?;
    crate::stage_paths(cwd.path(), diff)?;
    assert_eq!(
        crate::operations::run_git_for_stdout(
            cwd.path(),
            ["diff", "--cached", "--name-only"],
            /*env*/ None
        )?,
        "file.txt"
    );
    let outside = tempfile::tempdir()?;
    let error = crate::apply_git_patch(&crate::ApplyGitRequest {
        cwd: outside.path().to_path_buf(),
        ..request
    })
    .expect_err("probe must preserve the failure status");
    assert!(
        error
            .to_string()
            .contains("not a git repository (exit 128)")
    );
    Ok(())
}
