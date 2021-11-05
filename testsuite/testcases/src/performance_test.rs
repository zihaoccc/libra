// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::generate_traffic;
use forge::{NetworkContext, NetworkTest, Result, Test, util_cmd, create_k8s_client, list_pods};
use tokio::time::Duration;
use std::env;
use rand::seq::IteratorRandom;
use tokio::runtime::Runtime;
use anyhow::{anyhow, format_err};
use futures::future::{FutureExt, join};

pub struct PerformanceBenchmark;

impl Test for PerformanceBenchmark {
    fn name(&self) -> &'static str {
        "performance::performance-benchmark"
    }
}

impl NetworkTest for PerformanceBenchmark {
    fn run<'t>(&self, ctx: &mut NetworkContext<'t>) -> Result<()> {
        let buffer = Duration::from_secs(60);
        let duration = Duration::from_secs(240);
        let all_validators = ctx
            .swarm()
            .validators()
            .map(|v| v.peer_id())
            .collect::<Vec<_>>();



        let rt = Runtime::new().unwrap();
        let (client, pods) = rt.block_on(async {
            let client = create_k8s_client().await;
            let pods = list_pods(client.clone()).await;
            (client, pods)
        });
        let node_name = pods.unwrap()
            .into_iter()
            .filter(|s| s.name.contains("diem-validator-validator")).next().unwrap();

        let run_id = "ct-0-cluster-test-circleci-1636057";
        let filename = "diem-node-perf.svg";
        let command = generate_perf_flamegraph_command(filename, &run_id, 60);
        rt.block_on(async {
            let flame_graph = util_cmd(&client, &node_name.node_name, command, "generate-flamegraph");
            tokio::time::sleep(buffer).await;
            let flame_graph_result = flame_graph.await;
            flame_graph_result.map_err(|e| format_err!("Failed to generate flamegraph: {:?}", e)).unwrap();
        });
        // Generate some traffic
        let txn_stat = generate_traffic(ctx, &all_validators, duration, 0, None)?;
        ctx.report
            .report_txn_stats(self.name().to_string(), txn_stat, duration);
        println!(
            "perf flamegraph : https://toro-cluster-test-flamegraphs.s3-us-west-2.amazonaws.com/flamegraphs/{}/{}",
            run_id,
            filename
        );

        Ok(())
    }
}

fn generate_perf_flamegraph_command(filename: &str, run_id: &str, duration_secs: usize) -> String {
    format!(
        r#"
        set -xe;
        rm -rf /tmp/perf-data;
        mkdir /tmp/perf-data;
        cd /tmp/perf-data;
        perf record -F 99 -p $(ps aux | grep diem-node | grep -v grep | awk '{{print $2}}') --output=perf.data --call-graph dwarf -- sleep {duration_secs};
        perf script --input=perf.data | /usr/local/etc/FlameGraph/stackcollapse-perf.pl > out.perf-folded;
        /usr/local/etc/FlameGraph/flamegraph.pl out.perf-folded > {filename};
        aws s3 cp {filename} s3://toro-cluster-test-flamegraphs/flamegraphs/{run_id}/{filename};"#,
        duration_secs = duration_secs,
        filename = filename,
        run_id = run_id,
    )
}
