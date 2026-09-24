use std::os::windows::process::CommandExt;

#[test]
fn marketplace_git_is_captured_without_a_console() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", "marketplace_add::install::windows_tests::marketplace_git_is_captured_without_a_console", "--nocapture"])
            .env("CODEX_TEST_BACKGROUND_CHILD", "1")
            .creation_flags(0x0000_0008)
            .output()?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
        return Ok(());
    }
    let cwd = tempfile::tempdir()?;
    std::fs::write(
        cwd.path().join("probe.ps1"),
        r#"
        Add-Type -Name Probe -Namespace MarketplacePresentation -MemberDefinition '
            [System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();';
        [Console]::Out.Write('console={0}',[MarketplacePresentation.Probe]::GetConsoleWindow().ToInt64());
        [Console]::Error.Write('captured stderr'); exit 23
    "#,
    )?;
    let error = super::run_git(&["-c", "alias.console-probe=!powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File ./probe.ps1", "console-probe"], Some(cwd.path())).err().ok_or_else(|| anyhow::anyhow!("probe must preserve the failure status"))?.to_string();
    assert!(
        error.contains("23") && error.contains("stdout:\nconsole=0\nstderr:\ncaptured stderr"),
        "{error}"
    );
    Ok(())
}
