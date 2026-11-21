use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use horch_core::messaging::{channel, delivery, mail::Mail};
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::{model::Direction, paneshell::PaneShell, PaneHost};

const PROBE: &str = "[horch] NOTE: run horch inbox to confirm delivery";
const RELAY_PROBE: &str = "[horch] NOTE: the channel relay delivers your fleet's reports";
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
        let command = match PaneShell::host() {
            PaneShell::Posix => "echo HORCH_DELIVERY_$((70+3))",
            PaneShell::PowerShell => "Write-Output \"HORCH_DELIVERY_$(70+3)\"",
        };
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

pub fn fleet(ctx: &RuntimeContext, workspace: &str, pane: &str) {
    match fleet_check(ctx, workspace, pane) {
        Ok(by) => println!("fleet delivery: confirmed by {by}."),
        Err(e) => eprintln!("warning: fleet delivery {e:#}; mail remains pending for the courier."),
    }
}

fn fleet_check(ctx: &RuntimeContext, workspace: &str, pane: &str) -> Result<&'static str> {
    let mail = Mail::under(&ctx.paths.state_root, workspace);
    let id = mail.enqueue("horch", PROBE)?;
    if let Err(error) = super::mail::start_courier(ctx, workspace) {
        eprintln!("warning: delivery-probe courier could not start: {error:#}");
    }
    let deadline = Instant::now() + budget(ctx);
    let herdr = PaneHost::for_workspace(ctx, workspace)
        .with_timeout(call_timeout(ctx, Duration::from_secs(1)))
        .until(Some(deadline));
    let socket = channel::socket_path(&ctx.paths.state_root, workspace);
    let mut attempted = false;
    let mut reason = "not acknowledged";
    loop {
        if !mail.pending()?.iter().any(|m| m.id == id) {
            return Ok("horch inbox");
        }
        if Instant::now() >= deadline {
            bail!("{reason}");
        }
        if !attempted && channel::is_live(&socket) {
            let left = deadline.saturating_duration_since(Instant::now());
            return match channel::send(&socket, "horch", RELAY_PROBE, left) {
                Ok(()) => {
                    mail.ack(&id)?;
                    Ok("the channel relay")
                }
                Err(error) => bail!("relay did not take the probe ({error}); not acknowledged"),
            };
        }
        if !attempted {
            match herdr.pane_get(pane) {
                Ok(state) if state.at_prompt() => {
                    attempted = true;
                    if herdr.agent_prompt(pane, PROBE).is_ok() {
                        reason = "not acknowledged";
                    } else {
                        reason = "push failed; not acknowledged";
                    }
                }
                Ok(_) => reason = "orchestrator not idle; not acknowledged",
                Err(_) => reason = "no relay or orchestrator pane; not acknowledged",
            }
        }
        std::thread::sleep(
            Duration::from_millis(25).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}
