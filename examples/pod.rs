//! **Spends money.** Rent a pod, wait for it, run a command in it, read its
//! log, and terminate it, terminating it even if a step fails.
//!
//! ```sh
//! RUNPOD_API_KEY=... cargo run --example pod -- "NVIDIA GeForce RTX 4090"
//! ```

use std::time::Duration;

use futures_util::StreamExt;
use runpod::{
    Client, CreateGpuConfig, CreatePodRequest, CreatePodResponse, LogStreamParams, PodExecRequest,
    PodStatus,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gpu = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "NVIDIA GeForce RTX 4090".into());
    let runpod = Client::new(std::env::var("RUNPOD_API_KEY")?)?;

    let created = runpod
        .create_pod(&CreatePodRequest {
            name: "rs-runpod-sdk-example".into(),
            image: Some("runpod/base:0.6.2-cuda12.4.1".into()),
            gpu: Some(CreateGpuConfig {
                id: gpu,
                count: Some(1),
                ..Default::default()
            }),
            disk: Some(10),
            ..Default::default()
        })
        .await?;
    let pod = match created {
        CreatePodResponse::Created(pod) => pod,
        CreatePodResponse::Accepted(queued) => {
            // No capacity: Runpod queued it. Cancel rather than wait here.
            println!("no capacity now; queued as {}, cancelling", queued.id);
            runpod.delete_queued_pod(&queued.id).await?;
            return Ok(());
        }
    };
    println!("created pod {}", pod.id);

    let outcome = use_pod(&runpod, &pod.id).await;
    runpod.delete_pod(&pod.id).await?;
    println!("terminated {}", pod.id);
    outcome
}

async fn use_pod(runpod: &Client, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..60 {
        let pod = runpod.get_pod(id).await?;
        println!("  {}", pod.status);
        match pod.status {
            PodStatus::Running => break,
            PodStatus::Error => return Err("the pod failed to start".into()),
            _ => tokio::time::sleep(Duration::from_secs(5)).await,
        }
    }

    let result = runpod
        .exec_pod(
            id,
            &PodExecRequest {
                command: vec!["nvidia-smi".into(), "-L".into()],
                timeout_seconds: Some(30),
                ..Default::default()
            },
        )
        .await?;
    println!("exit {:?}\n{}", result.exit_code, result.stdout);

    let params = LogStreamParams {
        tail: Some(20),
        ..Default::default()
    };
    let mut logs = runpod.stream_pod_logs(id, &params).await?;
    // The stream follows the log; stop after a short while.
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = logs.next().await {
            match event {
                Ok(e) => println!("[{}] {}", e.source, e.line),
                Err(e) => eprintln!("log stream: {e}"),
            }
        }
    })
    .await;
    Ok(())
}
