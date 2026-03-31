//! Cross-framework benchmark — same tasks, same mock MCP, different agent runtimes.
//!
//! Prerequisites:
//! 1. Local LLM via ollama (fixed model version)
//! 2. Frameworks running and connected to mock MCP + same LLM.
//!    Mock MCP is started in-process automatically.
//!    Unreachable frameworks are skipped with a warning.
//!
//! Ports are configurable via env vars:
//!   MOCK_MCP_PORT (default: 0 = random), CRABTALK_PORT (6688),
//!   OPENCLAW_PORT (18789), OPENCODE_PORT (4096), HERMES_PORT (8080)

use crabtalk_bench::{
    gateway::{
        Gateway, check_reachable, crabtalk::CrabtalkGateway, hermes::HermesGateway,
        openclaw::OpenClawGateway, opencode::OpenCodeGateway,
    },
    mock_mcp,
    task::tasks,
};
use criterion::{Criterion, criterion_group, criterion_main};

fn env_port(var: &str, default: u16) -> u16 {
    std::env::var(var)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn bench_framework(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let task_defs = tasks();
    let mcp_port = env_port("MOCK_MCP_PORT", 0);
    let mcp_handle = rt.block_on(mock_mcp::start(mcp_port, &task_defs));
    eprintln!("mock MCP listening at http://{}/mcp", mcp_handle.addr());

    let all_gateways: Vec<(&str, u16, Box<dyn Gateway>)> = vec![
        {
            let port = env_port("CRABTALK_PORT", 6688);
            ("crabtalk", port, Box::new(CrabtalkGateway::new(port)))
        },
        {
            let port = env_port("OPENCLAW_PORT", 18789);
            let token = std::env::var("OPENCLAW_TOKEN").unwrap_or_default();
            (
                "openclaw",
                port,
                Box::new(OpenClawGateway::new(port, token)),
            )
        },
        {
            let port = env_port("OPENCODE_PORT", 4096);
            ("opencode", port, Box::new(OpenCodeGateway::new(port)))
        },
        {
            let port = env_port("HERMES_PORT", 8080);
            ("hermes", port, Box::new(HermesGateway::new(port)))
        },
    ];

    // Skip frameworks that aren't running.
    let gateways: Vec<_> = all_gateways
        .into_iter()
        .filter(|(name, port, _)| {
            if check_reachable(*port) {
                true
            } else {
                eprintln!("SKIP {name}: not reachable on port {port}");
                false
            }
        })
        .collect();

    if gateways.is_empty() {
        eprintln!("no frameworks available — nothing to benchmark");
        return;
    }

    for task in &task_defs {
        let mut group = c.benchmark_group(task.name);
        // These are real LLM calls — use fewer samples and longer measurement.
        group.sample_size(10);
        group.measurement_time(std::time::Duration::from_secs(30));

        for (name, _, gw) in &gateways {
            group.bench_function(*name, |b| {
                // Mock MCP state isn't reset between iterations — after the first
                // iteration exhausts scripted responses, subsequent ones always get
                // the last response. This is fine for latency measurement; the
                // validation pass (Phase 3) tests correctness separately.
                b.iter(|| gw.run_task(&rt, task));
            });
        }
        group.finish();
    }

    rt.block_on(mcp_handle.shutdown());
}

criterion_group!(benches, bench_framework);
criterion_main!(benches);
