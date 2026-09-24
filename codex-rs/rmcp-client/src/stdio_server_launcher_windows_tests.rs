use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobFailure {
    Create,
    AssignAndResume,
}

thread_local! {
    static JOB_FAILURE: std::cell::Cell<Option<JobFailure>> = const { std::cell::Cell::new(None) };
}

pub(super) fn inject_job_creation_failure(
    job: io::Result<codex_utils_pty::JobObject>,
) -> io::Result<codex_utils_pty::JobObject> {
    if JOB_FAILURE.get() == Some(JobFailure::Create) {
        JOB_FAILURE.set(None);
        Err(io::Error::other("injected MCP job creation failure"))
    } else {
        job
    }
}

pub(super) fn assignment_process_id(process_id: u32) -> u32 {
    if JOB_FAILURE.take() == Some(JobFailure::AssignAndResume) {
        // Force the real OpenProcess failure with an invalid PID while the
        // transport still owns the suspended child. Retry must drop that child.
        u32::MAX
    } else {
        process_id
    }
}

#[tokio::test]
async fn local_stdio_fallback_roundtrip_has_no_console() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        let output = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "stdio_server_launcher::windows_tests::local_stdio_fallback_roundtrip_has_no_console",
                "--nocapture",
            ])
            .env("CODEX_TEST_BACKGROUND_CHILD", "1")
            .creation_flags(0x0000_0008)
            .output()
            .await?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
        return Ok(());
    }
    for failure in [JobFailure::Create, JobFailure::AssignAndResume] {
        JOB_FAILURE.set(Some(failure));
        assert_local_stdio_roundtrip_has_no_console().await?;
        assert_eq!(JOB_FAILURE.get(), None, "injection must be consumed");
    }
    Ok(())
}

#[tokio::test]
async fn local_stdio_roundtrip_has_no_console() -> anyhow::Result<()> {
    assert_local_stdio_roundtrip_has_no_console().await
}

async fn assert_local_stdio_roundtrip_has_no_console() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let output = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "stdio_server_launcher::windows_tests::local_stdio_roundtrip_has_no_console",
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
    let result_path = cwd.path().join("console.txt");
    let script = r#"
Add-Type -Name Probe -Namespace CodexMcpTest -MemberDefinition '
    [System.Runtime.InteropServices.DllImport("kernel32.dll")]
    public static extern System.IntPtr GetConsoleWindow();
'
$request = [Console]::In.ReadLine() | ConvertFrom-Json
[System.IO.File]::WriteAllText($env:PROBE_RESULT,
    ('console={0};request={1}' -f [CodexMcpTest.Probe]::GetConsoleWindow().ToInt64(), $request.method))
[Console]::Out.WriteLine((@{jsonrpc='2.0'; id=$request.id; result=@{}} | ConvertTo-Json -Compress))
[Console]::In.ReadLine() | Out-Null
"#;
    let command = StdioServerCommand::new(
        "powershell.exe".into(),
        ["-NoProfile", "-NonInteractive", "-Command", script]
            .map(OsString::from)
            .into(),
        Some(HashMap::from([(
            OsString::from("PROBE_RESULT"),
            result_path.clone().into_os_string(),
        )])),
        Vec::new(),
        Some(cwd.path().display().to_string()),
        McpProtocolMode::Legacy,
    );
    let mut transport = if JOB_FAILURE.get().is_some() {
        // Keep the thread-local injection on this thread, exercising the same
        // production entry point that launch() runs via spawn_blocking.
        LocalStdioServerLauncher::launch_server(command, cwd.path().to_path_buf())?
    } else {
        LocalStdioServerLauncher::new(cwd.path().to_path_buf())
            .launch(command)
            .await?
    };
    transport
        .send(serde_json::from_value(
            json!({"jsonrpc":"2.0", "id":1, "method":"ping"}),
        )?)
        .await?;
    let response = tokio::time::timeout(Duration::from_secs(30), transport.receive())
        .await?
        .ok_or_else(|| io::Error::other("MCP response stream closed"))?;
    assert_eq!(
        serde_json::to_value(response)?,
        json!({"jsonrpc":"2.0", "id":1, "result":{}})
    );
    assert_eq!(
        std::fs::read_to_string(result_path)?,
        "console=0;request=ping"
    );
    transport.close().await?;
    Ok(())
}
