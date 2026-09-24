use super::*;

use pretty_assertions::assert_eq;
use std::time::Duration;

#[tokio::test]
async fn pipe_process_has_no_console_and_preserves_stdio() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "pipe::tests::pipe_process_has_no_console_and_preserves_stdio",
                "--nocapture",
            ])
            .env("CODEX_TEST_BACKGROUND_CHILD", "1")
            .creation_flags(winapi::um::winbase::DETACHED_PROCESS)
            .output()
            .await?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "detached test did not pass exactly one test: {output:?}"
        );
        return Ok(());
    }
    let script = r#"
Add-Type -Name ConsoleProbe -Namespace CodexPipeTest -MemberDefinition '
    [System.Runtime.InteropServices.DllImport("kernel32.dll")]
    public static extern System.IntPtr GetConsoleWindow();
'
[Console]::Out.Write('console={0};stdin={1}',
    [CodexPipeTest.ConsoleProbe]::GetConsoleWindow().ToInt64(),
    [Console]::In.Read())
[Console]::Error.Write('captured stderr')
exit 23
"#;
    let args = [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ]
    .map(str::to_owned);
    let env = std::env::vars().collect();
    for stdin_mode in [PipeStdinMode::Null, PipeStdinMode::Piped] {
        let spawned = spawn_process_with_stdin_mode(
            OsStr::new("powershell.exe"),
            &args,
            Path::new("."),
            &env,
            /*arg0*/ &None,
            stdin_mode,
            &[],
        )
        .await?;
        let expected_stdin = match stdin_mode {
            PipeStdinMode::Null => -1,
            PipeStdinMode::Piped => {
                spawned.session.writer_sender().send(b"x".to_vec()).await?;
                120
            }
        };
        let collect = |mut rx: mpsc::Receiver<Vec<u8>>| async move {
            let mut bytes = Vec::new();
            while let Some(chunk) = rx.recv().await {
                bytes.extend(chunk);
            }
            bytes
        };
        let (code, stdout, stderr) = tokio::time::timeout(Duration::from_secs(30), async {
            tokio::join!(
                spawned.exit_rx,
                collect(spawned.stdout_rx),
                collect(spawned.stderr_rx),
            )
        })
        .await?;
        assert_eq!(
            (
                code?,
                String::from_utf8(stdout)?,
                String::from_utf8(stderr)?
            ),
            (
                23,
                format!("console=0;stdin={expected_stdin}"),
                "captured stderr".to_owned(),
            )
        );
    }
    Ok(())
}

#[test]
fn process_fallback_interrupt_terminates_root() -> anyhow::Result<()> {
    let mut child = std::process::Command::new("ping.exe")
        .args(["-n", "60", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut terminator = PipeChildTerminator {
        windows: WindowsChildTerminator::Process(child.id()),
    };

    terminator.signal(ProcessSignal::Interrupt)?;

    assert!(!child.wait()?.success());
    Ok(())
}
