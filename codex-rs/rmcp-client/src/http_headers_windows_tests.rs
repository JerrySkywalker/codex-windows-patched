use pretty_assertions::assert_eq;

#[tokio::test]
async fn header_helper_is_contained_and_captured_without_a_console() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_TEST_BACKGROUND_CHILD").is_none() {
        let output = tokio::process::Command::new(std::env::current_exe()?)
            .args(["--exact", "http_headers::windows_tests::header_helper_is_contained_and_captured_without_a_console", "--nocapture"])
            .env("CODEX_TEST_BACKGROUND_CHILD", "1")
            .env("CODEX_HEADER_UNRELATED", "must-not-be-inherited")
            .creation_flags(0x0000_0008)
            .kill_on_drop(true)
            .output().await?;
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
        return Ok(());
    }
    let cwd = tempfile::tempdir()?;
    std::fs::write(
        cwd.path().join("helper with spaces.ps1"),
        r#"
        Add-Type -Name Probe -Namespace HeaderPresentation -MemberDefinition '
            [System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();
            [System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetCurrentProcess();
            [System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern bool IsProcessInJob(System.IntPtr p, System.IntPtr j, out bool inside);';
        $inside=$false
        if (![HeaderPresentation.Probe]::IsProcessInJob([HeaderPresentation.Probe]::GetCurrentProcess(),[IntPtr]::Zero,[ref]$inside)) { exit 7 }
        @{ 'X-Console'=[HeaderPresentation.Probe]::GetConsoleWindow().ToInt64().ToString();
           'X-Stdin'=[Console]::In.Read().ToString(); 'X-Contained'=$inside.ToString();
           'X-Ambient'=[string]$env:CODEX_HEADER_UNRELATED; 'X-Cwd'=[Environment]::CurrentDirectory } | ConvertTo-Json -Compress
    "#,
    )?;
    let headers = super::run_helper(r#"powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "helper with spaces.ps1""#, cwd.path()).await?;
    let mut expected = http::HeaderMap::new();
    for (key, value) in [
        ("x-console", "0"),
        ("x-stdin", "-1"),
        ("x-contained", "True"),
        ("x-ambient", ""),
    ] {
        expected.insert(
            http::HeaderName::from_static(key),
            http::HeaderValue::from_str(value)?,
        );
    }
    expected.insert(
        "x-cwd",
        http::HeaderValue::from_str(&cwd.path().to_string_lossy())?,
    );
    assert_eq!(headers, expected);
    Ok(())
}
