use super::SpawnChildRequest;
use super::StdioPolicy;
use super::spawn_child_async;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::time::Duration;

#[tokio::test]
async fn redirected_shell_has_no_console_and_preserves_stdio() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let output = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "spawn::tests::redirected_shell_has_no_console_and_preserves_stdio",
                "--nocapture",
            ])
            .env("CODEX_TEST_BACKGROUND_CHILD", "1")
            .creation_flags(DETACHED_PROCESS)
            .output()
            .await?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "detached test did not pass exactly one test: {output:?}"
        );
        return Ok(());
    }
    let cwd = tempfile::tempdir()?;
    let script = r#"
$ErrorActionPreference = 'Stop'
Add-Type -Name ConsoleProbe -Namespace CodexSpawnTest -MemberDefinition '
    [System.Runtime.InteropServices.DllImport("kernel32.dll")]
    public static extern System.IntPtr GetConsoleWindow();
'
[Console]::Out.Write('console={0};stdin={1}',
    [CodexSpawnTest.ConsoleProbe]::GetConsoleWindow().ToInt64(),
    [Console]::In.Read())
[Console]::Error.Write('captured stderr')
exit 23
"#;
    let child = spawn_child_async(SpawnChildRequest {
        program: which::which("pwsh.exe").or_else(|_| which::which("powershell.exe"))?,
        args: [
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        arg0: None,
        cwd: AbsolutePathBuf::try_from(cwd.path())?,
        network_sandbox_policy: NetworkSandboxPolicy::Enabled,
        network: None,
        stdio_policy: StdioPolicy::RedirectForShellTool,
        env: ["SystemRoot", "PATH", "TEMP", "TMP"]
            .into_iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| (name.to_owned(), value))
            })
            .collect(),
    })
    .await?;
    assert!(child.stdin.is_none());

    let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await??;
    assert_eq!(
        (
            output.status.code(),
            String::from_utf8(output.stdout)?,
            String::from_utf8(output.stderr)?,
        ),
        (
            Some(23),
            "console=0;stdin=-1".to_owned(),
            "captured stderr".to_owned(),
        )
    );
    Ok(())
}
