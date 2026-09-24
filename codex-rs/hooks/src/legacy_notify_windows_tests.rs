use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use pretty_assertions::assert_eq;

#[tokio::test]
async fn legacy_notify_runs_without_a_console() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        let output = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "legacy_notify::windows_tests::legacy_notify_runs_without_a_console",
                "--nocapture",
            ])
            .env("CODEX_TEST_BACKGROUND_CHILD", "1")
            .creation_flags(0x0000_0008)
            .kill_on_drop(true)
            .output()
            .await?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
        return Ok(());
    }
    let cwd = tempfile::tempdir()?;
    let script = cwd.path().join("notify.ps1");
    let result = cwd.path().join("result.txt");
    std::fs::write(
        &script,
        r#"
        param([string]$Payload)
        Add-Type -Name Probe -Namespace NotifyPresentation -MemberDefinition '
            [System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();';
        $value='console={0};stdin={1};payload={2}' -f [NotifyPresentation.Probe]::GetConsoleWindow().ToInt64(),[Console]::In.Read(),($Payload | ConvertFrom-Json).type
        [System.IO.File]::WriteAllText($env:CODEX_NOTIFY_RESULT,$value)
    "#,
    )?;
    let mut env: Vec<_> = std::env::vars_os().collect();
    env.push((
        OsString::from("CODEX_NOTIFY_RESULT"),
        result.as_os_str().to_owned(),
    ));
    let hook = super::notify_hook(
        vec![
            "powershell.exe".to_owned(),
            "-NoProfile".to_owned(),
            "-NonInteractive".to_owned(),
            "-ExecutionPolicy".to_owned(),
            "Bypass".to_owned(),
            "-File".to_owned(),
            script.display().to_string(),
        ],
        Arc::new(env),
    );
    let payload = crate::HookPayload {
        session_id: codex_protocol::ThreadId::new(),
        cwd: codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(cwd.path())?,
        client: None,
        triggered_at: chrono::Utc::now(),
        hook_event: crate::HookEvent::AfterAgent {
            event: crate::HookEventAfterAgent {
                thread_id: codex_protocol::ThreadId::new(),
                turn_id: "presentation".to_owned(),
                input_messages: Vec::new(),
                last_assistant_message: None,
            },
        },
    };
    assert!(matches!(
        (hook.func)(&payload).await,
        crate::HookResult::Success
    ));
    let actual = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(value) = std::fs::read_to_string(&result)
                && !value.is_empty()
            {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    assert_eq!(actual, "console=0;stdin=-1;payload=agent-turn-complete");
    Ok(())
}
