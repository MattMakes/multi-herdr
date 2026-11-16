mod support;

use serde_json::json;
use support::{config, page, Fixture, Reply, Session};

fn error_site() -> Fixture {
    Fixture::start(|request| match request.path.as_str() {
        "/" => page(
            "<script>console.warn('careful'); console.error('broken', 42); console.log('plain');</script>\
             <a id=next href=\"/quiet\">quiet</a>\
             <a id=missing href=\"/missing\">missing</a>\
             <a id=crash href=\"/crash\">crash</a>\
             <button id=throw onclick=\"setTimeout(() => { throw new Error('sync boom'); })\">throw</button>\
             <button id=reject onclick=\"Promise.reject(new Error('async boom'))\">reject</button>",
        ),
        "/quiet" => page("<p>quiet</p>"),
        "/crash" => Reply::status(500, "<!doctype html><title>crash</title><p>server error</p>"),
        _ => Reply::not_found(),
    })
}

#[test]
fn console_errors_and_warnings_are_captured_and_cleared_on_navigation() {
    let site = error_site();
    let Some(mut session) = Session::open(config(&site.url("/"))) else {
        return;
    };
    let console = session.state["console"].as_array().unwrap().clone();
    assert_eq!(console.len(), 2, "{console:?}");
    assert_eq!(console[0]["level"], "warning");
    assert_eq!(console[0]["text"], "careful");
    assert_eq!(console[1]["level"], "error");
    assert_eq!(console[1]["text"], "broken 42");
    assert!(console[1]["url"].as_str().unwrap().ends_with('/'));
    assert_eq!(session.state["navigation"], json!({"status": 200, "kind": "load"}));
    session.click_id("next");
    assert!(session.url().ends_with("/quiet"));
    assert_eq!(session.state["console"], json!([]));
    session.shutdown();
}

#[test]
fn http_client_and_server_errors_set_the_navigation_status() {
    let site = error_site();
    let Some(mut session) = Session::open(config(&site.url("/"))) else {
        return;
    };
    session.click_id("missing");
    assert_eq!(session.state["navigation"]["status"], 404);
    session.apply(json!({"kind": "back"}));
    assert_eq!(session.state["navigation"]["status"], 200);
    session.click_id("crash");
    assert_eq!(session.state["navigation"]["status"], 500);
    session.shutdown();
}

#[test]
fn uncaught_exceptions_and_unhandled_rejections_are_told_apart() {
    let site = error_site();
    let Some(mut session) = Session::open(config(&site.url("/"))) else {
        return;
    };
    assert_eq!(session.state["exceptions"], json!([]));
    session.click_id("throw");
    let exceptions = session.state["exceptions"].as_array().unwrap().clone();
    assert_eq!(exceptions.len(), 1, "{exceptions:?}");
    assert_eq!(exceptions[0]["kind"], "exception");
    assert_eq!(exceptions[0]["message"], "Error: sync boom");
    assert!(exceptions[0]["stack"].as_str().unwrap().contains("at "));
    assert!(exceptions[0]["line"].as_u64().unwrap() >= 1);
    session.click_id("reject");
    let exceptions = session.state["exceptions"].as_array().unwrap().clone();
    assert_eq!(exceptions.len(), 2, "{exceptions:?}");
    assert_eq!(exceptions[1]["kind"], "rejection");
    assert!(exceptions[1]["message"].as_str().unwrap().contains("async boom"));
    session.shutdown();
}
