use pretty_assertions::assert_eq;
use std::os::windows::process::CommandExt;

#[test]
fn powershell_discovery_from_detached_parent() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "powershell::windows_tests::powershell_discovery_from_detached_parent",
                "--nocapture",
            ])
            .env("CODEX_TEST_BACKGROUND_CHILD", "1")
            .creation_flags(0x0000_0008)
            .output()?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
        return Ok(());
    }
    assert!(super::try_find_powershell_executable_blocking().is_some());
    // Probe pwsh through cmd and validate it when installed; absence remains supported.
    let _ = super::try_find_pwsh_executable_blocking();
    let script = r#"Add-Type -Name Probe -Namespace ShellPresentation -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();'; [Console]::Out.Write([ShellPresentation.Probe]::GetConsoleWindow().ToInt64()); exit 23"#;
    let output = super::background_output(std::process::Command::new("powershell.exe").args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ]))?;
    assert_eq!(
        (
            output.status.code(),
            String::from_utf8(output.stdout)?,
            output.stderr
        ),
        (Some(23), "0".to_owned(), Vec::new())
    );
    Ok(())
}
