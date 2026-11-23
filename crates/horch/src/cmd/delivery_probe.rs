use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use horch_core::messaging::delivery::{self, TypedLine};
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::{model::Direction, paneshell::PaneShell, PaneHost};

const MARKER: &str = "HORCH_DELIVERY_73";

fn budget(ctx: &RuntimeContext) -> Duration {
    fault_ms(ctx, "delivery-probe-ms").unwrap_or(Duration::from_secs(5))
}

fn call_timeout(ctx: &RuntimeContext, default: Duration) -> Duration {
    fault_ms(ctx, "delivery-probe-call-ms").unwrap_or(default)
}

fn fault_ms(ctx: &RuntimeContext, point: &str) -> Option<Duration> {
    ctx.settings
        .faults
        .indexed(point)
        .and_then(|s| s.parse::<u64>().ok())
        .map(|ms| Duration::from_millis(ms.clamp(1, 30_000)))
}

pub fn doctor(ctx: &RuntimeContext) {
    match doctor_check(ctx) {
        Ok(()) => println!("delivery: confirmed (typed echo executed)."),
        Err(e) => eprintln!("warning: delivery test failed: {e:#}"),
    }
}

fn doctor_check(ctx: &RuntimeContext) -> Result<()> {
    let deadline = Instant::now() + budget(ctx);
    let herdr = PaneHost::of(ctx)
        .with_timeout(call_timeout(ctx, Duration::from_secs(2)))
        .until(Some(deadline));
    let ws = herdr.workspace_create("horch delivery test", None, false)?;
    let checked: Result<()> = (|| {
        let pane = herdr.pane_split(&ws.root_pane_id, Direction::Right)?;
        let command = TypedLine::delivery_echo(PaneShell::host());
        let _guard =
            delivery::start_push_budget(deadline.saturating_duration_since(Instant::now()));
        let timing = delivery::Timing {
            tail_timeout: Duration::from_millis(250),
            poll_start: Duration::from_millis(20),
            poll_max: Duration::from_millis(50),
            settle_short: Duration::from_millis(100),
            settle_long: Duration::from_millis(100),
            second_enter: Duration::from_millis(100),
        };
        delivery::send_line_with(&herdr, &pane, command, &timing)?;
        loop {
            if herdr.pane_read(&pane, "recent-unwrapped")?.contains(MARKER) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("typed echo was not observed before the deadline");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })();
    let cleanup = PaneHost::of(ctx)
        .with_timeout(call_timeout(ctx, Duration::from_secs(2)))
        .workspace_close(&ws.workspace_id);
    match (checked, cleanup) {
        (Err(e), Err(cleanup)) => Err(e.context(format!(
            "scratch workspace {} could not be closed: {cleanup:#}",
            ws.workspace_id
        ))),
        (Err(e), _) => Err(e),
        (Ok(()), Err(e)) => Err(e.context("closing delivery-test workspace")),
        (Ok(()), Ok(())) => Ok(()),
    }
}
