use desktop_weather::{Config, observe, parse};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn structured_observations_preserve_units_and_stale_age_without_markup() {
    let home = home();
    let config = Config::default();
    let cache = home.path().join("cache");
    let (endpoint, worker) = server(200, response("clear <sky>"), Duration::ZERO);
    let reading = observe(&config, home.path(), &cache, 1000, &endpoint, false).unwrap();
    worker.join().unwrap();
    assert_eq!(reading.observation.description, "clear <sky>");
    assert_eq!(reading.observation.temperature, 18.3);
    assert_eq!(reading.observation.wind_kmh, 9.0);
    assert_eq!(reading.stale_seconds, None);
    fs::remove_file(home.path().join(".owm-key")).unwrap();
    let stale = observe(&config, home.path(), &cache, 2000, &endpoint, false).unwrap();
    assert_eq!(stale.stale_seconds, Some(1000));
}

fn response(description: &str) -> String {
    serde_json::json!({
        "weather": [{"id": 800, "description": description}],
        "main": {"temp": 18.3}, "wind": {"speed": 2.5},
        "sys": {"sunrise": 10, "sunset": 2000}
    })
    .to_string()
}

fn server(status: u16, body: String, delay: Duration) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/weather", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") && request.len() < 16384 {
            if stream.read(&mut byte).unwrap() == 0 {
                break;
            }
            request.push(byte[0]);
        }
        thread::sleep(delay);
        let _ = write!(
            stream,
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8(request).unwrap()
    });
    (endpoint, worker)
}

fn home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::write(home.path().join(".owm-key"), "fake-test-secret\n").unwrap();
    home
}

#[test]
fn successful_fetch_encodes_location_and_caches_without_credentials() {
    let home = home();
    let cache = home.path().join("cache");
    let config = Config {
        city: "São Paulo".into(),
        country: "BR".into(),
        ..Config::default()
    };
    let (endpoint, worker) = server(200, response("clear <sky> & \"sun\""), Duration::ZERO);
    let reading = observe(&config, home.path(), &cache, 1000, &endpoint, false).unwrap();
    assert_eq!(reading.observation.temperature, 18.3);
    assert_eq!(reading.observation.wind_kmh, 9.0);
    assert_eq!(reading.observation.description, "clear <sky> & \"sun\"");
    let request = worker.join().unwrap();
    assert!(request.contains("q=S%C3%A3o+Paulo%2CBR"));
    assert!(request.contains("units=metric"));
    let bytes = fs::read_to_string(cache.join("weather.json")).unwrap();
    assert!(!bytes.contains("fake-test-secret"));
    assert_eq!(
        fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(cache.join("weather.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    fs::remove_file(home.path().join(".owm-key")).unwrap();
    let fresh = observe(
        &config,
        home.path(),
        &cache,
        1010,
        "invalid://no-request-expected",
        false,
    )
    .unwrap();
    assert_eq!(fresh.stale_seconds, None);
    let stale = observe(&config, home.path(), &cache, 2000, &endpoint, false).unwrap();
    assert_eq!(stale.stale_seconds, Some(1000));
    assert!(
        observe(
            &config,
            home.path(),
            &cache,
            1000 + config.max_stale_seconds + 1,
            &endpoint,
            false
        )
        .is_err()
    );
    let changed = Config {
        city: "Copenhagen".into(),
        ..config.clone()
    };
    assert!(observe(&changed, home.path(), &cache, 1010, &endpoint, false).is_err());
    assert!(observe(&config, home.path(), &cache, 999, &endpoint, false).is_err());
}

#[test]
fn forced_refresh_uses_cached_reading_on_http_failure() {
    let home = home();
    let cache = home.path().join("cache");
    let config = Config::default();
    let (endpoint, worker) = server(200, response("clear sky"), Duration::ZERO);
    assert!(
        observe(&config, home.path(), &cache, 1000, &endpoint, false)
            .unwrap()
            .stale_seconds
            .is_none()
    );
    worker.join().unwrap();
    let (endpoint, worker) = server(503, "unavailable".into(), Duration::ZERO);
    assert_eq!(
        observe(&config, home.path(), &cache, 1001, &endpoint, true)
            .unwrap()
            .stale_seconds,
        Some(1)
    );
    worker.join().unwrap();
}

#[test]
fn malformed_and_oversized_responses_never_become_cached_successes() {
    for body in [
        "not JSON".into(),
        "x".repeat(1024 * 1024 + 1),
        "{\"weather\":[]}".into(),
    ] {
        let home = home();
        let cache = home.path().join("cache");
        let (endpoint, worker) = server(200, body, Duration::ZERO);
        assert!(
            observe(
                &Config::default(),
                home.path(),
                &cache,
                1000,
                &endpoint,
                false
            )
            .is_err()
        );
        assert!(!cache.join("weather.json").exists());
        worker.join().unwrap();
    }
}

#[test]
fn http_and_connection_errors_do_not_expose_the_api_key() {
    let home = home();
    let cache = home.path().join("cache");
    let config = Config::default();
    let (endpoint, worker) = server(401, "fake-test-secret".into(), Duration::ZERO);
    let unauthorized = observe(&config, home.path(), &cache, 1000, &endpoint, false)
        .unwrap_err()
        .to_string();
    worker.join().unwrap();
    assert!(unauthorized.contains("HTTP 401"));
    let disconnected = observe(&config, home.path(), &cache, 1001, &endpoint, false)
        .unwrap_err()
        .to_string();
    for error in [unauthorized, disconnected] {
        assert!(!error.contains("fake-test-secret"));
        assert!(!error.contains("appid="));
    }
}

#[test]
fn slow_responses_have_a_total_deadline() {
    let home = home();
    let config = Config {
        request_timeout_seconds: 1,
        ..Config::default()
    };
    let (endpoint, worker) = server(200, response("clear sky"), Duration::from_millis(1400));
    let started = Instant::now();
    let reading = observe(
        &config,
        home.path(),
        &home.path().join("cache"),
        1000,
        &endpoint,
        false,
    );
    let elapsed = started.elapsed();
    worker.join().unwrap();
    assert!(reading.is_err());
    assert!(
        elapsed < Duration::from_secs(3),
        "request deadline was not enforced"
    );
}

#[test]
fn invalid_settings_and_measurements_are_rejected() {
    let home = home();
    let path = home.path().join("config.json");
    for value in [
        r#"{"request_timeout_seconds":0}"#,
        r#"{"request_timeout_seconds":31}"#,
        r#"{"city":""}"#,
        r#"{"cache_seconds":999999}"#,
        r#"{"unknown":true}"#,
    ] {
        fs::write(&path, value).unwrap();
        assert!(Config::read(&path).is_err());
    }
    let mut value: serde_json::Value = serde_json::from_str(&response("clear sky")).unwrap();
    value["wind"]["speed"] = (-1).into();
    assert!(parse(&serde_json::to_vec(&value).unwrap()).is_err());
    value["weather"] = serde_json::json!([]);
    assert!(parse(&serde_json::to_vec(&value).unwrap()).is_err());
}
