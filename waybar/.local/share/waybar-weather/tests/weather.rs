use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::thread;
use std::time::{Duration, Instant};
use waybar_weather::{Config, parse, run};

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
    let module = run(&config, home.path(), &cache, 1000, &endpoint, false, true);
    assert_eq!(module.class, ["weather"]);
    assert!(module.text.contains("18°"));
    assert!(module.text.contains("9.0"));
    assert!(module.text.contains("#4c4f69"));
    assert!(module.tooltip.contains("&lt;sky&gt; &amp; &quot;sun&quot;"));
    serde_json::from_str::<serde_json::Value>(&serde_json::to_string(&module).unwrap()).unwrap();
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
    let fresh = run(
        &config,
        home.path(),
        &cache,
        1010,
        "invalid://no-request-expected",
        false,
        false,
    );
    assert_eq!(fresh.class, ["weather"]);
    assert!(fresh.text.contains("#cdd6f4"));
    let stale = run(&config, home.path(), &cache, 2000, &endpoint, false, false);
    assert_eq!(stale.class, ["weather", "stale"]);
    assert!(stale.tooltip.contains("Cached reading"));
    let expired = run(
        &config,
        home.path(),
        &cache,
        1000 + config.max_stale_seconds + 1,
        &endpoint,
        false,
        false,
    );
    assert!(expired.class.contains(&"unavailable"));
    let changed = Config {
        city: "Copenhagen".into(),
        ..config.clone()
    };
    assert!(
        run(&changed, home.path(), &cache, 1010, &endpoint, false, false)
            .class
            .contains(&"unavailable")
    );
    assert!(
        run(&config, home.path(), &cache, 999, &endpoint, false, false)
            .class
            .contains(&"unavailable")
    );
}

#[test]
fn forced_refresh_uses_cached_reading_on_http_failure() {
    let home = home();
    let cache = home.path().join("cache");
    let config = Config::default();
    let (endpoint, worker) = server(200, response("clear sky"), Duration::ZERO);
    assert_eq!(
        run(&config, home.path(), &cache, 1000, &endpoint, false, false).class,
        ["weather"]
    );
    worker.join().unwrap();
    let (endpoint, worker) = server(503, "unavailable".into(), Duration::ZERO);
    assert_eq!(
        run(&config, home.path(), &cache, 1001, &endpoint, true, false).class,
        ["weather", "stale"]
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
        let module = run(
            &Config::default(),
            home.path(),
            &cache,
            1000,
            &endpoint,
            false,
            false,
        );
        assert!(module.class.contains(&"unavailable"));
        assert!(!cache.join("weather.json").exists());
        worker.join().unwrap();
    }
}

#[test]
fn http_and_connection_errors_do_not_expose_the_api_key() {
    let home = home();
    let cache = home.path().join("cache");
    let (endpoint, worker) = server(401, "fake-test-secret".into(), Duration::ZERO);
    let unauthorized = run(
        &Config::default(),
        home.path(),
        &cache,
        1000,
        &endpoint,
        false,
        false,
    );
    worker.join().unwrap();
    assert!(unauthorized.tooltip.contains("HTTP 401"));
    let disconnected = run(
        &Config::default(),
        home.path(),
        &cache,
        1001,
        &endpoint,
        false,
        false,
    );
    for module in [unauthorized, disconnected] {
        let output = serde_json::to_string(&module).unwrap();
        assert!(!output.contains("fake-test-secret"));
        assert!(!output.contains("appid="));
        assert!(module.class.contains(&"unavailable"));
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
    let module = run(
        &config,
        home.path(),
        &home.path().join("cache"),
        1000,
        &endpoint,
        false,
        false,
    );
    let elapsed = started.elapsed();
    worker.join().unwrap();
    assert!(module.class.contains(&"unavailable"));
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

#[test]
fn cli_failures_still_print_waybar_json_without_reading_real_credentials() {
    let home = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_waybar-weather-native");
    let output = std::process::Command::new(binary)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["tooltip"]
            .as_str()
            .unwrap()
            .contains("API key missing")
    );
    assert!(value["text"].as_str().unwrap().contains("#f38ba8"));
}
