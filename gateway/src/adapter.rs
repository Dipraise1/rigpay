use crate::config::Service;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

/// Run a service adapter for a *paid* job. Adapters never see payment data or
/// keys — they get an input file, an output path, and a job id.
pub async fn run(service: &Service, job_id: &str, data_dir: &Path, payload: &[u8]) -> Result<Vec<u8>> {
    let jobs_dir = data_dir.join("jobs");
    tokio::fs::create_dir_all(&jobs_dir).await?;
    let input = jobs_dir.join(format!("{job_id}.in"));
    let output = jobs_dir.join(format!("{job_id}.out"));
    tokio::fs::write(&input, payload).await?;

    let cmd = substitute(
        &service.command,
        &input.to_string_lossy(),
        &output.to_string_lossy(),
        job_id,
    );

    let fut = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(Duration::from_secs(service.timeout_secs), fut)
        .await
        .with_context(|| format!("adapter for {} timed out after {}s", service.id, service.timeout_secs))?
        .context("adapter failed to spawn")?;

    if !out.status.success() {
        bail!(
            "adapter for {} exited {}: {}",
            service.id,
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    // Adapters that write {output} win; otherwise stdout is the result.
    match tokio::fs::read(&output).await {
        Ok(bytes) => Ok(bytes),
        Err(_) => Ok(out.stdout),
    }
}

fn substitute(command: &str, input: &str, output: &str, job_id: &str) -> String {
    command
        .replace("{input}", input)
        .replace("{output}", output)
        .replace("{job_id}", job_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_all_placeholders() {
        let cmd = substitute("run.sh {input} {output} --id {job_id}", "/a.in", "/a.out", "j1");
        assert_eq!(cmd, "run.sh /a.in /a.out --id j1");
    }

    #[tokio::test]
    async fn stdout_is_result_when_no_output_file() {
        let svc = Service {
            id: "echo".into(),
            summary: String::new(),
            price: 0.0,
            unit: "per_request".into(),
            adapter: "command".into(),
            command: "cat {input}".into(),
            timeout_secs: 5,
            max_concurrent: 1,
            enabled: true,
        };
        let dir = std::env::temp_dir().join("rende-test");
        let res = run(&svc, "t1", &dir, b"hello").await.unwrap();
        assert_eq!(res, b"hello");
    }

    #[tokio::test]
    async fn failing_adapter_reports_stderr() {
        let svc = Service {
            id: "boom".into(),
            summary: String::new(),
            price: 0.0,
            unit: "per_request".into(),
            adapter: "command".into(),
            command: "echo nope >&2; exit 3".into(),
            timeout_secs: 5,
            max_concurrent: 1,
            enabled: true,
        };
        let dir = std::env::temp_dir().join("rende-test");
        let err = run(&svc, "t2", &dir, b"").await.unwrap_err().to_string();
        assert!(err.contains("nope"), "got: {err}");
    }
}
