use chrono::{TimeZone, Utc};
use kube_bug_agent::backpressure::domain::*;
use std::time::Duration;

fn limit(value: u32) -> ConcurrencyLimit {
    ConcurrencyLimit::new(value).unwrap()
}
fn policy() -> BackpressurePolicy {
    BackpressurePolicy {
        minimum: limit(1),
        maximum: limit(8),
        latency_budget: Latency::milliseconds(200.0).unwrap(),
        error_budget: Ratio::new(0.1).unwrap(),
        minimum_samples: RequestCount::new(10),
        healthy_windows: WindowCount::new(2).unwrap(),
        cooldown: Duration::from_secs(3),
    }
}
fn controller() -> BackpressureController {
    BackpressureController::new(BackendName::new("orders").unwrap(), policy()).unwrap()
}
fn sample(time: i64, completed: u64, failed: u64, latency: Option<f64>) -> TrafficSnapshot {
    TrafficSnapshot {
        observed_at: Utc.timestamp_opt(1_700_000_000 + time, 0).unwrap(),
        uptime: Duration::from_secs(time as u64),
        completed: RequestCount::new(completed),
        failed: RequestCount::new(failed),
        timed_out: RequestCount::new(0),
        rejected: RequestCount::new(0),
        active: RequestCount::new(2),
        pending: RequestCount::new(0),
        p95: latency.map(|value| Latency::milliseconds(value).unwrap()),
    }
}

#[test]
fn validated_values_reject_invalid_configuration_and_deserialization() {
    for name in ["", "a.b", "orders/../../", "a b", &"x".repeat(65)] {
        assert!(BackendName::new(name).is_err());
    }
    assert_eq!(
        BackendName::new("orders-v2_1").unwrap().to_string(),
        "orders-v2_1"
    );
    assert!(ConcurrencyLimit::new(0).is_err());
    assert!(ConcurrencyLimit::new(10001).is_err());
    assert_eq!(u32::from(limit(8)), 8);
    assert_eq!(
        serde_json::from_str::<ConcurrencyLimit>("8").unwrap(),
        limit(8)
    );
    assert!(serde_json::from_str::<ConcurrencyLimit>("0").is_err());
    for value in [-1.0, f64::NAN, f64::INFINITY, 3_600_001.0] {
        assert!(Latency::milliseconds(value).is_err());
    }
    assert_eq!(f64::from(Latency::try_from(4.5).unwrap()), 4.5);
    assert!(serde_json::from_str::<Latency>("-1").is_err());
    for value in [-1.0, 1.1, f64::NAN, f64::INFINITY] {
        assert!(Ratio::new(value).is_err());
    }
    assert_eq!(f64::from(Ratio::try_from(0.5).unwrap()), 0.5);
    assert!(serde_json::from_str::<Ratio>("2").is_err());
    assert!(WindowCount::new(0).is_err());
    assert!(WindowCount::new(101).is_err());
    assert_eq!(WindowCount::new(3).unwrap().value(), 3);
    for invalid in [
        BackpressurePolicy {
            minimum: limit(9),
            ..policy()
        },
        BackpressurePolicy {
            latency_budget: Latency::milliseconds(0.0).unwrap(),
            ..policy()
        },
        BackpressurePolicy {
            error_budget: Ratio::new(0.0).unwrap(),
            ..policy()
        },
        BackpressurePolicy {
            minimum_samples: RequestCount::new(0),
            ..policy()
        },
        BackpressurePolicy {
            cooldown: Duration::ZERO,
            ..policy()
        },
    ] {
        assert!(BackpressureController::new(BackendName::new("orders").unwrap(), invalid).is_err());
    }
    assert!(
        controller()
            .observe(sample(1, 0, 0, None), limit(9))
            .is_err()
    );
}

#[test]
fn overload_reduces_limits_and_cooldown_prevents_oscillation() {
    let mut controller = controller();
    assert_eq!(controller.backend().as_str(), "orders");
    assert_eq!(
        controller
            .observe(sample(1, 0, 0, None), limit(8))
            .unwrap()
            .reason,
        DecisionReason::Baseline
    );
    let reduction = controller
        .observe(sample(2, 20, 5, Some(50.0)), limit(8))
        .unwrap();
    assert_eq!(reduction.reason, DecisionReason::Reduce);
    assert_eq!(reduction.after, limit(4));
    controller.confirm(&reduction);
    let cooling = controller
        .observe(sample(3, 40, 10, Some(500.0)), limit(4))
        .unwrap();
    assert_eq!(cooling.reason, DecisionReason::Cooldown);
    assert_eq!(cooling.after, limit(4));
    let reduction = controller
        .observe(sample(5, 60, 10, Some(500.0)), limit(4))
        .unwrap();
    assert_eq!(reduction.after, limit(2));
    assert_eq!(reduction.reason, DecisionReason::Reduce);
    assert_eq!(
        controller
            .observe(sample(9, 80, 20, None), limit(1))
            .unwrap()
            .reason,
        DecisionReason::AtMinimum
    );
}

#[test]
fn recovery_is_gradual_and_requires_healthy_samples_with_latency() {
    let mut controller = controller();
    controller.observe(sample(1, 0, 0, None), limit(2)).unwrap();
    assert_eq!(
        controller
            .observe(sample(2, 20, 0, None), limit(2))
            .unwrap()
            .reason,
        DecisionReason::Hold
    );
    assert_eq!(
        controller
            .observe(sample(3, 21, 0, Some(50.0)), limit(2))
            .unwrap()
            .reason,
        DecisionReason::InsufficientTraffic
    );
    assert_eq!(
        controller
            .observe(sample(4, 41, 0, Some(50.0)), limit(2))
            .unwrap()
            .reason,
        DecisionReason::Hold
    );
    let recovery = controller
        .observe(sample(5, 61, 0, Some(50.0)), limit(2))
        .unwrap();
    assert_eq!(recovery.reason, DecisionReason::Recover);
    assert_eq!(recovery.after, limit(3));
    controller.confirm(&recovery);
    assert_eq!(
        controller
            .observe(sample(6, 81, 0, Some(50.0)), limit(3))
            .unwrap()
            .reason,
        DecisionReason::Cooldown
    );
    assert_eq!(
        controller
            .observe(sample(9, 101, 0, Some(50.0)), limit(8))
            .unwrap()
            .reason,
        DecisionReason::AtMaximum
    );
    let held = controller
        .observe(sample(10, 121, 1, Some(180.0)), limit(8))
        .unwrap();
    controller.confirm(&held);
}

#[test]
fn local_rejections_are_not_backend_failures_and_timeouts_are_not_double_counted() {
    let previous = sample(1, 0, 0, None);
    let mut current = sample(2, 100, 90, Some(50.0));
    current.rejected = RequestCount::new(90);
    let window = PressureWindow::between(&previous, &current)
        .unwrap()
        .unwrap();
    assert_eq!(window.completed.value(), 10);
    assert_eq!(window.failed.value(), 0);
    assert_eq!(window.error_ratio.value(), 0.0);
    current.timed_out = RequestCount::new(4);
    assert_eq!(
        PressureWindow::between(&previous, &current)
            .unwrap()
            .unwrap()
            .failed
            .value(),
        4
    );
    current.rejected = RequestCount::new(200);
    assert_eq!(
        PressureWindow::between(&previous, &current)
            .unwrap()
            .unwrap()
            .error_ratio
            .value(),
        0.0
    );
}

#[test]
fn stale_samples_and_counter_resets_never_trigger_an_increase() {
    let previous = sample(10, 100, 20, Some(50.0));
    assert!(PressureWindow::between(&previous, &previous).is_err());
    let mut current = sample(11, 110, 21, Some(50.0));
    current.uptime = Duration::ZERO;
    assert!(
        PressureWindow::between(&previous, &current)
            .unwrap()
            .is_none()
    );
    for kind in 0..4 {
        let mut previous = previous.clone();
        previous.timed_out = RequestCount::new(20);
        previous.rejected = RequestCount::new(20);
        let mut current = sample(11, 110, 21, Some(50.0));
        current.timed_out = RequestCount::new(21);
        current.rejected = RequestCount::new(21);
        match kind {
            0 => current.completed = RequestCount::new(0),
            1 => current.failed = RequestCount::new(0),
            2 => current.timed_out = RequestCount::new(0),
            _ => current.rejected = RequestCount::new(0),
        }
        assert!(
            PressureWindow::between(&previous, &current)
                .unwrap()
                .is_none()
        );
    }
    let mut controller = controller();
    controller.observe(previous, limit(4)).unwrap();
    assert_eq!(
        controller
            .observe(sample(11, 0, 0, None), limit(4))
            .unwrap()
            .reason,
        DecisionReason::CounterReset
    );
}
