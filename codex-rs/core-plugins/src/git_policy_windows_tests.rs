use super::PluginGitMode;
use pretty_assertions::assert_eq;
use std::os::windows::process::CommandExt;
use std::path::Path;

#[test]
fn automatic_git_children_do_not_inherit_a_visible_console() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let output = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", "git_policy::windows_tests::automatic_git_children_do_not_inherit_a_visible_console", "--nocapture"])
            .env("CODEX_TEST_BACKGROUND_CHILD", "1")
            .creation_flags(DETACHED_PROCESS)
            .output()?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "detached test did not pass exactly one test: {output:?}"
        );
        return Ok(());
    }
    let script = r#"Add-Type -Name Probe -Namespace CodexPluginGitTest -MemberDefinition '
            [System.Runtime.InteropServices.DllImport("kernel32.dll")]
            public static extern System.IntPtr GetConsoleWindow();
        ';
        [Console]::Out.Write('console={0}', [CodexPluginGitTest.Probe]::GetConsoleWindow().ToInt64());
        [Console]::Error.Write('captured stderr'); exit 23"#;
    // A Git alias lets the child report the console inherited from the real Git process.
    let output = PluginGitMode::Automatic
        .command(Path::new("git.exe"))
        .env("CODEX_GIT_CONSOLE_PROBE", script)
        .args([
            "-c",
            r#"alias.console-probe=!powershell.exe -NoProfile -NonInteractive -Command "$CODEX_GIT_CONSOLE_PROBE""#,
            "console-probe",
        ])
        .output()?;
    assert_eq!(
        (
            output.status.code(),
            String::from_utf8(output.stdout)?,
            String::from_utf8(output.stderr)?,
        ),
        (
            Some(23),
            "console=0".to_owned(),
            "captured stderr".to_owned()
        )
    );
    Ok(())
}
