use super::*;
use serde_json::json;

fn probe(width: u64, height: u64, duration: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"streams":[{"width":width,"height":height,"duration":duration}]}))
        .unwrap()
}

#[test]
fn probe_limits_duration_pixels_and_preserves_rotated_aspect_ratio() {
    for bytes in [
        probe(0, 10, "1"),
        probe(100_000, 100_000, "1"),
        probe(10, 10, "31"),
        probe(10, 10, "NaN"),
        probe(10, 10, "inf"),
        probe(10, 10, "0"),
        b"not json".to_vec(),
    ] {
        assert!(Plan::from_probe(&bytes).is_err());
    }
    let plan = Plan::from_probe(&probe(640, 360, "3.5")).unwrap();
    assert_eq!((plan.width, plan.height, plan.duration_ms), (160, 90, 3500));
    let rotated = serde_json::to_vec(&json!({"streams":[{"width":640,"height":360,"duration":"N/A","side_data_list":[{"rotation":-90}]}],"format":{"duration":"1.2"}})).unwrap();
    let plan = Plan::from_probe(&rotated).unwrap();
    assert_eq!((plan.width, plan.height, plan.duration_ms), (90, 160, 1200));
}

#[test]
fn raw_frames_are_bounded_and_timing_keeps_the_full_loop_duration() {
    let plan = Plan {
        width: 2,
        height: 1,
        duration_ms: 1251,
    };
    for bytes in [vec![], vec![0; 7], vec![0; 8 * 97]] {
        assert!(plan.frames(bytes).is_err());
    }
    let preview = plan.frames([vec![0; 8], vec![255; 8]].concat()).unwrap();
    assert_eq!(preview.frames.len(), 2);
    assert_eq!(
        preview.frames.iter().map(|f| f.duration_ms).sum::<u64>(),
        1251
    );
    assert_ne!(preview.frames[0].image, preview.frames[1].image);
    assert_eq!(preview.loops, None);
    let timeline = super::super::animation::Timeline::new(
        preview.frames.iter().map(|f| f.duration_ms),
        preview.loops,
    );
    assert_eq!(timeline.frame_at(700), 1);
    assert_eq!(timeline.frame_at(1251), 0);
}

#[tokio::test]
async fn missing_failed_and_oversized_helpers_return_readable_errors() {
    let (_stop, cancel) = watch::channel(false);
    let root = tempfile::tempdir().unwrap();
    let missing = run(
        Command::new(root.path().join("missing")),
        32,
        cancel.clone(),
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    assert!(missing.contains("ffmpeg and ffprobe"));
    for (script, expected) in [
        ("exit 7", "unavailable"),
        ("while :; do printf '0123456789'; done", "exceeds"),
    ] {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        assert!(
            run(command, 32, cancel.clone(), Duration::from_secs(1))
                .await
                .unwrap_err()
                .contains(expected)
        );
    }
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "printf 'bounded'"]);
    assert_eq!(
        run(command, 32, cancel, Duration::from_secs(1))
            .await
            .unwrap(),
        b"bounded"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn cancellation_and_timeout_kill_and_reap_the_decoder() {
    for cancel_early in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let pidfile = root.path().join("pid");
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "printf '%s' \"$$\" > \"$1\"; while :; do :; done",
                "gif-test",
            ])
            .arg(&pidfile);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run(command, 32, cancel, Duration::from_millis(400)));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        while !pidfile.exists() {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if cancel_early {
            stop.send(true).unwrap();
        }
        let error = task.await.unwrap().unwrap_err();
        assert!(error.contains(if cancel_early {
            "canceled"
        } else {
            "timed out"
        }));
        let pid = std::fs::read_to_string(pidfile).unwrap();
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "decoder was not reaped"
        );
    }
}
