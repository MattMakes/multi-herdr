mod support;

mod errors {
    use crate::support::{config, page, Fixture, Reply, Session};
    use serde_json::json;

    fn error_site() -> Fixture {
        Fixture::start(|request| {
            match request.path.as_str() {
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
        }
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
        assert_eq!(
            session.state["navigation"],
            json!({"status": 200, "kind": "load"})
        );
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
        assert!(exceptions[1]["message"]
            .as_str()
            .unwrap()
            .contains("async boom"));
        session.shutdown();
    }
}

mod discovery {
    use crate::support::{click_action, config, page, Fixture, Reply, Session};
    use serde_json::{json, Value};

    fn ids(state: &Value) -> Vec<String> {
        state["clickables"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|candidate| candidate["target"]["id"].as_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn same_origin_iframes_are_explored_with_offset_points() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<div style=\"height:120px\"></div>\
                 <iframe id=frame src=\"/inner\" style=\"position:absolute;left:200px;top:150px;width:300px;height:200px;border:10px solid black\"></iframe>",
            ),
            "/inner" => page(
                "<button id=inside style=\"position:absolute;left:20px;top:30px;width:100px;height:40px\" onclick=\"parent.document.title='clicked inside'\">inside</button>",
            ),
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let candidate = session.clickable_by_id("inside");
        let x = candidate["point"]["x"].as_f64().unwrap();
        let y = candidate["point"]["y"].as_f64().unwrap();
        assert!((x - 280.0).abs() < 2.0, "{candidate}");
        assert!((y - 210.0).abs() < 2.0, "{candidate}");
        assert!(candidate["target"]["path"]
            .as_str()
            .unwrap()
            .contains(">>|"));
        session.apply(click_action(&candidate));
        assert_eq!(session.title(), "clicked inside");
        session.shutdown();
    }

    #[test]
    fn open_shadow_roots_and_slotted_elements_are_explored() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<shadow-card id=card><button id=slotted slot=action onclick=\"document.title='slotted'\">slotted</button><button id=unassigned slot=nowhere>hidden</button></shadow-card>\
                 <script>\
                 customElements.define('shadow-card', class extends HTMLElement {\
                   constructor() { super(); const root = this.attachShadow({ mode: 'open' });\
                     root.innerHTML = '<div><button id=shadowed>inner</button><slot name=action></slot></div>';\
                     root.getElementById('shadowed').addEventListener('click', () => { document.title = 'shadowed'; }); }\
                 });\
                 </script>",
            ),
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let found = ids(&session.state);
        assert!(found.contains(&"shadowed".to_string()), "{found:?}");
        assert!(found.contains(&"slotted".to_string()), "{found:?}");
        assert!(!found.contains(&"unassigned".to_string()), "{found:?}");
        assert!(session.clickable_by_id("shadowed")["target"]["path"]
            .as_str()
            .unwrap()
            .contains(">>>"));
        session.click_id("shadowed");
        assert_eq!(session.title(), "shadowed");
        session.click_id("slotted");
        assert_eq!(session.title(), "slotted");
        session.shutdown();
    }

    #[test]
    fn disabled_controls_their_labels_new_windows_and_file_inputs_are_not_offered() {
        let site = Fixture::start(|request| match request.path.as_str() {
            "/" => page(
                "<button id=enabled>ok</button>\
                 <button id=disabled disabled>no</button>\
                 <fieldset disabled><input id=fenced></fieldset>\
                 <label id=label-on for=on>on</label><input id=on>\
                 <label id=label-off for=off>off</label><input id=off disabled>\
                 <a id=same href=\"/x\">same</a>\
                 <a id=blank href=\"/x\" target=\"_blank\">blank</a>\
                 <a id=named href=\"/x\" target=\"other\">named</a>\
                 <a id=top href=\"/x\" target=\"_top\">top</a>\
                 <input id=file type=file><label id=label-file for=file>file</label>\
                 <input id=hidden type=hidden>\
                 <div id=role role=button>role</div>\
                 <div id=role-off role=button aria-disabled=true>role off</div>\
                 <span id=plain>plain</span>\
                 <button id=invisible style=\"visibility:hidden\">invisible</button>\
                 <button id=below style=\"position:absolute;top:2000px\">below</button>\
                 <textarea id=area></textarea>",
            ),
            _ => Reply::not_found(),
        });
        let Some(session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let found = ids(&session.state);
        for offered in ["enabled", "label-on", "on", "same", "top", "role", "area"] {
            assert!(
                found.contains(&offered.to_string()),
                "{offered} missing in {found:?}"
            );
        }
        for hidden in [
            "disabled",
            "fenced",
            "label-off",
            "off",
            "blank",
            "named",
            "file",
            "label-file",
            "hidden",
            "role-off",
            "plain",
            "invisible",
            "below",
        ] {
            assert!(
                !found.contains(&hidden.to_string()),
                "{hidden} offered in {found:?}"
            );
        }
        let label = session.clickable_by_id("label-on");
        assert_eq!(label["kind"], "label");
        assert_eq!(session.clickable_by_id("same")["kind"], "link");
        assert_eq!(session.clickable_by_id("role")["kind"], "role");
        assert_eq!(session.clickable_by_id("area")["kind"], "textarea");
        session.shutdown();
    }

    #[test]
    fn a_focused_nonempty_text_field_is_not_offered() {
        let site = Fixture::start(|request| match request.path.as_str() {
            "/" => page("<input id=name><input id=other>"),
            _ => Reply::not_found(),
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.click_id("name");
        assert!(session.has_clickable_id("name"));
        assert_eq!(session.state["focused"]["tag"], "input");
        assert_eq!(session.state["focused"]["value_length"], 0);
        session.apply(json!({"kind": "type", "text": "abc"}));
        assert_eq!(session.state["focused"]["value_length"], 3);
        assert!(!session.has_clickable_id("name"));
        assert!(session.has_clickable_id("other"));
        session.shutdown();
    }

    #[test]
    fn fingerprints_carry_the_documented_keys() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<form><label for=mail>Mail address</label><input id=mail name=mail type=email placeholder=\"you@example.com\" data-testid=mail-field></form>\
                 <a href=\"/next\" data-test=next-link>Next page</a>",
            ),
            _ => Reply::not_found(),
        }
        });
        let Some(session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let mail = session.clickable_by_id("mail")["target"].clone();
        assert_eq!(mail["test_id"], "mail-field");
        assert_eq!(mail["role"], "textbox");
        assert_eq!(mail["name"], "Mail address");
        assert_eq!(mail["form_name"], "mail");
        assert_eq!(mail["placeholder"], "you@example.com");
        assert_eq!(mail["input_type"], "email");
        assert!(mail["path"].as_str().unwrap().ends_with("input"));
        let link = session
            .clickable(|candidate| candidate["kind"] == "link")
            .unwrap();
        assert_eq!(link["target"]["test_id"], "next-link");
        assert_eq!(link["target"]["href"], "/next");
        assert_eq!(link["target"]["text"], "Next page");
        assert_eq!(link["target"]["role"], "link");
        session.shutdown();
    }

    #[test]
    fn a_recorded_click_matches_its_element_after_a_layout_shift() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<div id=spacer style=\"height:10px\"></div>\
                 <button id=shift onclick=\"document.getElementById('spacer').style.height='200px'\">shift</button>\
                 <button id=target onclick=\"document.title='hit'\">target</button>\
                 <button id=decoy>decoy</button>",
            ),
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let recorded = click_action(&session.clickable_by_id("target"));
        session.click_id("shift");
        let offered: Vec<Value> = session.state["clickables"]
            .as_array()
            .unwrap()
            .iter()
            .map(click_action)
            .collect();
        let matched = session
            .driver
            .match_recorded(&recorded, &offered)
            .expect("the recorded click matches");
        assert_ne!(matched["point"], recorded["point"]);
        assert_eq!(matched["point"], session.clickable_by_id("target")["point"]);
        session.apply(matched);
        assert_eq!(session.title(), "hit");
        let removed = json!({"kind": "click", "target": {"id": "gone"}, "point": {"x": 1, "y": 1}});
        assert!(session.driver.match_recorded(&removed, &offered).is_none());
        session.shutdown();
    }
}

mod navigation {
    use crate::support::{config, page, temp_html, Fixture, Reply, Session};
    use serde_json::json;

    fn history_site() -> Fixture {
        Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page("<a id=second href=\"/second\">second</a><a id=data href=\"/data.json\">data</a><a id=hash href=\"#section\">hash</a>"),
            "/second" => Reply::html(
                "<!doctype html><title>second</title><a id=first href=\"/\">first</a><script>let loads = (window.loads || 0) + 1; window.loads = loads; addEventListener('pageshow', (event) => { document.title = event.persisted ? 'second restored' : 'second'; });</script>",
            ),
            "/data.json" => Reply::with_type(200, "application/json", br#"{"answer": 42}"#.to_vec()),
            _ => Reply::not_found(),
        }
        })
    }

    #[test]
    fn a_navigation_that_does_not_settle_in_thirty_seconds_is_a_stall() {
        let site = Fixture::start(|request| match request.path.as_str() {
            "/" => page("<a id=slow href=\"/slow\">slow</a>"),
            _ => Reply::Hang,
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let slow = session.clickable_by_id("slow");
        let error = session
            .try_apply(crate::support::click_action(&slow))
            .expect_err("the navigation stalls");
        assert!(
            matches!(error, ui_prop_tester::DriverError::Stalled(_)),
            "{error}"
        );
        session.shutdown();
    }

    #[test]
    fn back_and_forward_move_through_history_and_report_bfcache_restores() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<a id=second>second</a><script>document.getElementById('second').href = 'http://localhost:' + location.port + '/second';</script>",
            ),
            "/second" => page(
                "<p>second</p><script>addEventListener('pageshow', (event) => { document.title = event.persisted ? 'second restored' : 'second'; });</script>",
            ),
            _ => Reply::not_found(),
        }
        });
        let mut settings = config(&site.url("/"));
        settings.allow_urls = vec!["http://127.0.0.1/*".into(), "http://localhost/*".into()];
        let Some(mut session) = Session::open(settings) else {
            return;
        };
        assert_eq!(session.state["history"]["index"], 0);
        session.click_id("second");
        assert_eq!(
            session.url(),
            format!("http://localhost:{}/second", site.port)
        );
        assert_eq!(session.state["history"]["index"], 1);
        assert_eq!(
            session.state["history"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        session.apply(json!({"kind": "back"}));
        assert_eq!(session.state["history"]["index"], 0);
        assert_eq!(session.url(), site.url("/"));
        session.apply(json!({"kind": "forward"}));
        assert!(session.url().ends_with("/second"));
        if session.state["navigation"]["kind"] != "bfcache" {
            let reasons = session.eval("(s) => JSON.stringify(s.window.performance.getEntriesByType('navigation')[0].notRestoredReasons)");
            panic!("not restored from the back/forward cache: {reasons}");
        }
        assert_eq!(session.title(), "second restored");
        assert_eq!(session.state["navigation"]["status"], 200);
        session.apply(json!({"kind": "forward"}));
        assert!(session.url().ends_with("/second"));
        session.shutdown();
    }

    #[test]
    fn non_html_entries_same_document_moves_and_reload_are_reported() {
        let site = history_site();
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.click_id("hash");
        assert_eq!(session.state["navigation"]["kind"], "same_document");
        assert!(session.url().ends_with("#section"));
        session.click_id("data");
        assert_eq!(session.state["navigation"]["kind"], "non_document");
        assert_eq!(session.state["mime_type"], "application/json");
        assert_eq!(session.state["content"]["is_html"], false);
        session.apply(json!({"kind": "back"}));
        assert!(session.url().ends_with("#section"));
        assert_eq!(session.state["mime_type"], "text/html");
        session.apply(json!({"kind": "forward"}));
        assert_eq!(session.state["mime_type"], "application/json");
        let before = site.requests_to("/data.json").len();
        session.apply(json!({"kind": "reload"}));
        assert_eq!(session.state["navigation"]["kind"], "non_document");
        assert_eq!(site.requests_to("/data.json").len(), before + 1);
        assert_eq!(session.state["last_action"]["kind"], "reload");
        session.shutdown();
    }

    #[test]
    fn the_default_boundary_keeps_the_walk_on_the_starting_origin() {
        let elsewhere = Fixture::start(|_| page("<p>elsewhere</p>"));
        let target = elsewhere.url("/away");
        let site = Fixture::start(move |request| {
            match request.path.as_str() {
            "/" => page(&format!(
                "<a id=inside href=\"/inside\">inside</a><a id=outside href=\"{target}\">outside</a><a id=mail href=\"mailto:x@example.com\">mail</a><a id=script href=\"javascript:void(document.title='scripted')\">script</a><button id=jump onclick=\"location.href='{target}'\">jump</button><a id=bounce href=\"/bounce\">bounce</a>"
            )),
            "/inside" => page("<p>inside</p>"),
            "/bounce" => Reply::redirect(&format!("{}?via=redirect", "http://localhost:1/")),
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        assert!(session.has_clickable_id("inside"));
        assert!(session.has_clickable_id("script"));
        assert!(!session.has_clickable_id("outside"));
        assert!(!session.has_clickable_id("mail"));
        let start = session.url().to_string();
        session.click_id("jump");
        assert_eq!(session.url(), start);
        assert!(elsewhere.requests_to("/away").is_empty());
        session.click_id("bounce");
        assert!(
            session.url().starts_with(&site.url("/")),
            "{}",
            session.url()
        );
        session.click_id("script");
        assert_eq!(session.title(), "scripted");
        session.click_id("inside");
        assert!(session.url().ends_with("/inside"));
        session.shutdown();
    }

    #[test]
    fn a_file_origin_allows_query_changes_but_not_sibling_files() {
        let directory = tempfile::tempdir().unwrap();
        temp_html(directory.path(), "sibling.html", "<p>sibling</p>");
        let start = temp_html(
            directory.path(),
            "start.html",
            "<a id=query href=\"start.html?page=2\">query</a><a id=sibling href=\"sibling.html\">sibling</a><button id=jump onclick=\"location.href='sibling.html'\">jump</button>",
        );
        let Some(mut session) = Session::open(config(start.to_str().unwrap())) else {
            return;
        };
        assert!(session.url().starts_with("file://"));
        assert_eq!(session.state["navigation"]["status"], json!(null));
        assert!(session.has_clickable_id("query"));
        assert!(!session.has_clickable_id("sibling"));
        session.click_id("jump");
        assert!(session.url().ends_with("start.html"), "{}", session.url());
        session.click_id("query");
        assert!(
            session.url().ends_with("start.html?page=2"),
            "{}",
            session.url()
        );
        session.shutdown();
    }

    #[test]
    fn allow_url_patterns_replace_the_default_boundary() {
        let second = Fixture::start(|request| match request.path.as_str() {
            "/docs/page" => page("<p>docs</p>"),
            _ => page("<p>other</p>"),
        });
        let docs = second.url("/docs/page");
        let blog = second.url("/blog");
        let site = Fixture::start(move |request| {
            match request.path.as_str() {
            "/" => page(&format!(
                "<a id=docs href=\"{docs}\">docs</a><a id=blog href=\"{blog}\">blog</a><a id=local href=\"/local\">local</a>"
            )),
            _ => page("<p>local</p>"),
        }
        });
        let mut settings = config(&site.url("/"));
        settings.allow_urls = vec![
            format!("http://127.0.0.1:{}/*", site.port),
            format!("http://127.0.0.1:{}/docs/*", second.port),
        ];
        let Some(mut session) = Session::open(settings) else {
            return;
        };
        assert!(session.has_clickable_id("docs"));
        assert!(session.has_clickable_id("local"));
        assert!(!session.has_clickable_id("blog"));
        session.click_id("docs");
        assert!(session.url().ends_with("/docs/page"));
        session.shutdown();
    }

    #[test]
    fn an_origin_outside_the_allow_url_patterns_is_a_setup_error() {
        let site = Fixture::start(|_| page("<p>x</p>"));
        let mut settings = config(&site.url("/"));
        settings.allow_urls = vec!["http://example.invalid/*".into()];
        if crate::support::chrome().is_none() {
            return;
        }
        let error = ui_prop_tester::browser::open(&settings)
            .err()
            .expect("refused");
        assert!(
            error.to_string().contains("outside every --allow-url"),
            "{error}"
        );
    }

    #[test]
    fn downloads_land_in_the_run_directory_and_do_not_hang_navigation() {
        let site = Fixture::start(|request| match request.path.as_str() {
            "/" => page("<a id=get href=\"/report.csv\">get</a>"),
            "/report.csv" => Reply::download("report.csv", "a,b\n1,2\n"),
            _ => Reply::not_found(),
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.click_id("get");
        assert!(session.url().ends_with('/'));
        let state = session.wait_until(
            "(state) => state.downloads.length && state.downloads[0].state === 'completed'",
        );
        assert_eq!(state, json!(true));
        session.apply(json!({"kind": "wait", "ms": 0}));
        let download = session.state["downloads"][0].clone();
        assert_eq!(download["suggested_filename"], "report.csv");
        assert!(download["url"].as_str().unwrap().ends_with("/report.csv"));
        assert_eq!(download["state"], "completed");
        let path = session.state["downloads"][0]["path"]
            .as_str()
            .map(str::to_string);
        if let Some(path) = &path {
            assert_eq!(std::fs::read_to_string(path).unwrap(), "a,b\n1,2\n");
        }
        session.shutdown();
        if let Some(path) = path {
            assert!(!std::path::Path::new(&path).exists());
        }
    }
}

mod actions {
    use crate::support::{click_action, config, page, Fixture, Reply, Session};
    use serde_json::json;
    use ui_prop_tester::driver::{CustomAction, DriverError, SpecSetup};

    #[test]
    fn typing_and_keys_reach_each_input_type() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<input id=text><input id=mail type=email><input id=count type=number><textarea id=area></textarea>\
                 <form onsubmit=\"event.preventDefault(); document.title = 'submitted ' + this.elements.q.value\"><input id=q name=q></form>\
                 <script>addEventListener('keydown', (event) => { if (event.key === 'Escape') document.title = 'escaped'; });</script>",
            ),
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.click_id("text");
        session.apply(json!({"kind": "type", "text": "héllo wörld"}));
        assert_eq!(
            session.eval("(s) => s.document.getElementById('text').value"),
            "héllo wörld"
        );
        session.apply(json!({"kind": "key", "key": "Backspace"}));
        assert_eq!(
            session.eval("(s) => s.document.getElementById('text').value"),
            "héllo wörl"
        );
        session.apply(json!({"kind": "key", "key": "Tab"}));
        assert_eq!(session.state["focused"]["input_type"], "email");
        session.apply(json!({"kind": "type", "text": "a.b@example.com"}));
        assert_eq!(
            session.eval("(s) => s.document.getElementById('mail').value"),
            "a.b@example.com"
        );
        session.click_id("count");
        session.apply(json!({"kind": "type", "text": "12345"}));
        assert_eq!(
            session.eval("(s) => s.document.getElementById('count').valueAsNumber"),
            12345
        );
        session.click_id("area");
        session.apply(json!({"kind": "type", "text": "one\ntwo"}));
        assert_eq!(
            session.eval("(s) => s.document.getElementById('area').value"),
            "one\ntwo"
        );
        assert_eq!(session.state["focused"]["tag"], "textarea");
        session.click_id("q");
        session.apply(json!({"kind": "type", "text": "find"}));
        session.apply(json!({"kind": "key", "key": "Enter"}));
        assert_eq!(session.title(), "submitted find");
        session.apply(json!({"kind": "key", "key": "Escape"}));
        assert_eq!(session.title(), "escaped");
        let error = session
            .try_apply(json!({"kind": "key", "key": "NoSuchKey"}))
            .expect_err("unknown key");
        assert!(matches!(error, DriverError::Failed(_)), "{error}");
        session.shutdown();
    }

    #[test]
    fn scrolling_reaches_the_bottom_and_returns_to_the_top() {
        let site = Fixture::start(|request| match request.path.as_str() {
            "/" => page("<div style=\"height:2000px\">tall</div><button id=end>end</button>"),
            _ => Reply::not_found(),
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        assert_eq!(session.state["scroll"]["y"], 0);
        let max_y = session.state["scroll"]["max_y"].as_i64().unwrap();
        assert!(max_y > 1000, "{}", session.state["scroll"]);
        assert!(!session.has_clickable_id("end"));
        session.apply(json!({"kind": "scroll", "dy": 300}));
        assert_eq!(session.state["scroll"]["y"], 300);
        for _ in 0..20 {
            if session.state["scroll"]["y"].as_i64().unwrap() >= max_y {
                break;
            }
            session.apply(json!({"kind": "scroll", "dy": 300}));
        }
        assert_eq!(session.state["scroll"]["y"].as_i64().unwrap(), max_y);
        assert!(session.has_clickable_id("end"));
        session.apply(json!({"kind": "scroll_to_top"}));
        assert_eq!(session.state["scroll"]["y"], 0);
        session.shutdown();
    }

    #[test]
    fn double_click_and_wait_are_dispatched() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<button id=twice ondblclick=\"document.title = 'double'\">twice</button>\
                 <button id=later onclick=\"setTimeout(() => { document.title = 'later'; }, 300)\">later</button>",
            ),
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let twice = session.clickable_by_id("twice");
        session.apply(
            json!({"kind": "double_click", "target": twice["target"], "point": twice["point"]}),
        );
        assert_eq!(session.title(), "double");
        session.click_id("later");
        assert_ne!(session.title(), "later");
        session.apply(json!({"kind": "wait", "ms": 600}));
        assert_eq!(session.title(), "later");
        assert_eq!(
            session.state["last_action"],
            json!({"kind": "wait", "ms": 600})
        );
        session.shutdown();
    }

    #[test]
    fn drag_moves_the_pointer_in_steps_with_the_button_held() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<div id=box style=\"position:absolute;left:50px;top:50px;width:60px;height:60px;background:red\"></div>\
                 <script>\
                 const box = document.getElementById('box'); let dragging = false; let moves = 0;\
                 box.addEventListener('pointerdown', () => { dragging = true; });\
                 addEventListener('pointermove', (event) => { if (dragging && event.buttons === 1) { moves += 1; box.style.left = (event.clientX - 30) + 'px'; box.style.top = (event.clientY - 30) + 'px'; } });\
                 addEventListener('pointerup', () => { dragging = false; document.title = 'moves ' + moves; });\
                 </script>",
            ),
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.apply(json!({"kind": "drag", "from": {"x": 80, "y": 80}, "to": {"x": 380, "y": 280}, "steps": 6, "delay_ms": 20}));
        assert_eq!(session.title(), "moves 6");
        let left =
            session.eval("(s) => s.document.getElementById('box').getBoundingClientRect().left");
        assert_eq!(left, 350);
        session.shutdown();
    }

    #[test]
    fn html_drag_and_drop_is_delivered() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<div id=source draggable=true style=\"position:absolute;left:20px;top:20px;width:80px;height:40px;background:blue\">drag</div>\
                 <div id=sink style=\"position:absolute;left:300px;top:200px;width:120px;height:120px;background:green\"></div>\
                 <script>\
                 document.getElementById('source').addEventListener('dragstart', (event) => event.dataTransfer.setData('text/plain', 'payload'));\
                 const sink = document.getElementById('sink');\
                 sink.addEventListener('dragover', (event) => event.preventDefault());\
                 sink.addEventListener('drop', (event) => { event.preventDefault(); document.title = 'dropped ' + event.dataTransfer.getData('text/plain'); });\
                 </script>",
            ),
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.apply(json!({"kind": "drag", "from": {"x": 60, "y": 40}, "to": {"x": 360, "y": 260}, "steps": 8, "delay_ms": 10}));
        assert_eq!(session.title(), "dropped payload");
        session.shutdown();
    }

    #[test]
    fn resize_changes_the_viewport_and_layout() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page("<script>addEventListener('resize', () => { document.title = innerWidth + 'x' + innerHeight; });</script>"),
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        assert_eq!(session.state["viewport"]["width"], 800);
        session.apply(json!({"kind": "resize", "width": 500, "height": 400}));
        assert_eq!(
            session.state["viewport"],
            json!({"width": 500, "height": 400, "device_scale_factor": 1.0})
        );
        assert_eq!(session.title(), "500x400");
        session.shutdown();
    }

    #[test]
    fn set_files_fills_a_file_input_by_selector() {
        let directory = tempfile::tempdir().unwrap();
        let upload = directory.path().join("upload.txt");
        std::fs::write(&upload, "hello upload").unwrap();
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<input id=picker type=file multiple onchange=\"this.files[0].text().then((text) => { document.title = this.files.length + ':' + this.files[0].name + ':' + text; })\">",
            ),
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.apply(json!({"kind": "set_files", "selector": "#picker", "paths": [upload.to_str().unwrap()]}));
        let title =
            session.wait_until("(s) => s.document.title.startsWith('1:') && s.document.title");
        assert_eq!(title, "1:upload.txt:hello upload");
        let error = session
            .try_apply(json!({"kind": "set_files", "selector": "#absent", "paths": [upload.to_str().unwrap()]})).expect_err("no element");
        assert!(error.to_string().contains("no element matches"), "{error}");
        session.shutdown();
    }

    #[test]
    fn custom_actions_run_in_the_page_with_cells_and_args() {
        let site = Fixture::start(|request| match request.path.as_str() {
            "/" => page("<ul id=list></ul>"),
            _ => Reply::not_found(),
        });
        let setup = SpecSetup {
            custom_actions: vec![CustomAction {
                name: "addItem".into(),
                source: "async ({ document, window, cells, args }) => { await new Promise((resolve) => window.setTimeout(resolve, 50)); const item = document.createElement('li'); item.textContent = args.label + ' after ' + cells.count; document.getElementById('list').appendChild(item); }".into(),
            }, CustomAction {
                name: "explode".into(),
                source: "async () => { throw new Error('custom failure'); }".into(),
            }],
        };
        let Some(mut session) = Session::open_with(config(&site.url("/")), &setup) else {
            return;
        };
        session.apply(json!({"kind": "custom", "name": "addItem", "args": {"label": "first"}, "cells": {"count": 0}}));
        assert_eq!(
            session.eval("(s) => s.document.querySelector('li').textContent"),
            "first after 0"
        );
        let error = session
            .try_apply(json!({"kind": "custom", "name": "explode", "args": null, "cells": {}}))
            .expect_err("throws");
        assert!(error.to_string().contains("custom failure"), "{error}");
        let missing = session
            .try_apply(json!({"kind": "custom", "name": "nothing", "args": null, "cells": {}}))
            .expect_err("unknown");
        assert!(matches!(missing, DriverError::Failed(_)));
        let unknown = session
            .try_apply(json!({"kind": "teleport"}))
            .expect_err("unknown kind");
        assert!(matches!(unknown, DriverError::Failed(_)), "{unknown}");
        session.shutdown();
    }

    #[test]
    fn clicking_a_label_focuses_its_control() {
        let site = Fixture::start(|request| match request.path.as_str() {
            "/" => page("<label id=tag for=field>Field</label><input id=field>"),
            _ => Reply::not_found(),
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let label = session.clickable_by_id("tag");
        session.apply(click_action(&label));
        assert_eq!(session.state["focused"]["target"]["id"], "field");
        session.shutdown();
    }
}

mod safety {
    use crate::support::{config, page, Fixture, Reply, Session};

    fn dialog_site() -> Fixture {
        Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<button id=alert onclick=\"alert('hello'); document.title = 'after alert'\">alert</button>\
                 <button id=confirm onclick=\"document.title = 'confirm ' + confirm('sure?')\">confirm</button>\
                 <button id=prompt onclick=\"document.title = 'prompt ' + prompt('name?', 'guest')\">prompt</button>\
                 <button id=pause onclick=\"debugger; document.title = 'resumed'\">pause</button>\
                 <button id=guard onclick=\"addEventListener('beforeunload', (event) => { event.preventDefault(); event.returnValue = 'stay'; }); document.title = 'guarded'\">guard</button>\
                 <a id=leave href=\"/left\">leave</a>\
                 <script>setTimeout(() => {}, 0);</script>",
            ),
            "/left" => page("<p>left</p>"),
            "/paused-load" => page("<script>debugger; document.title = 'loaded past debugger';</script>"),
            _ => Reply::not_found(),
        }
        })
    }

    #[test]
    fn alert_confirm_and_prompt_dialogs_are_accepted() {
        let site = dialog_site();
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.click_id("alert");
        assert_eq!(session.title(), "after alert");
        session.click_id("confirm");
        assert_eq!(session.title(), "confirm true");
        session.click_id("prompt");
        assert_eq!(session.title(), "prompt guest");
        session.shutdown();
    }

    #[test]
    fn a_beforeunload_dialog_is_accepted_and_the_navigation_proceeds() {
        let site = dialog_site();
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.click_id("guard");
        assert_eq!(session.title(), "guarded");
        session.click_id("leave");
        assert!(session.url().ends_with("/left"), "{}", session.url());
        session.shutdown();
    }

    #[test]
    fn debugger_statements_are_resumed() {
        let site = dialog_site();
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.click_id("pause");
        assert_eq!(session.title(), "resumed");
        drop(session);
        let Some(session) = Session::open(config(&site.url("/paused-load"))) else {
            return;
        };
        assert_eq!(session.title(), "loaded past debugger");
        session.shutdown();
    }

    #[test]
    fn links_that_open_new_windows_from_script_are_closed() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page("<button id=pop onclick=\"window.open('/popup'); document.title = 'opened'\">pop</button>"),
            _ => page("<p>popup</p>"),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.click_id("pop");
        assert_eq!(session.title(), "opened");
        assert!(session.url().ends_with('/'));
        session.shutdown();
    }
}

mod setup {
    use crate::support::{config, page, Fixture, Reply, Session};
    use serde_json::json;

    fn png_size(png: &[u8]) -> (u32, u32) {
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        (width, height)
    }

    #[test]
    fn granted_permissions_are_visible_to_the_page() {
        let site = Fixture::start(|_| page("<p>permissions</p>"));
        let mut settings = config(&site.url("/"));
        settings.grant_permissions = vec!["geolocation".into(), "clipboard-read-write".into()];
        let Some(mut session) = Session::open(settings) else {
            return;
        };
        let state = session.eval(
            "async (s) => (await s.window.navigator.permissions.query({ name: 'geolocation' })).state",
        );
        assert_eq!(state, "granted");
        let notifications = session.eval(
            "async (s) => (await s.window.navigator.permissions.query({ name: 'notifications' })).state",
        );
        assert_ne!(notifications, "granted");
        session.shutdown();
    }

    #[test]
    fn extra_headers_are_sent_with_every_request() {
        let site = Fixture::start(|request| match request.path.as_str() {
            "/" => page("<a id=next href=\"/next\">next</a>"),
            _ => page("<p>next</p>"),
        });
        let mut settings = config(&site.url("/"));
        settings.headers = vec![("X-Run-Token".into(), "secret-1".into())];
        let Some(mut session) = Session::open(settings) else {
            return;
        };
        session.click_id("next");
        for path in ["/", "/next"] {
            let request = site.requests_to(path).pop().expect("requested");
            assert_eq!(request.header("x-run-token"), Some("secret-1"), "{path}");
        }
        session.shutdown();
    }

    #[test]
    fn cookies_reach_other_ports_and_domain_scoped_cookies_reach_their_host() {
        let other = Fixture::start(|_| page("<p>other port</p>"));
        let other_url = other.url("/other");
        let named_url = format!("http://localhost:{}/named", other.port);
        let site = Fixture::start(move |request| match request.path.as_str() {
            "/" => page(&format!(
                "<a id=port href=\"{other_url}\">port</a><a id=named href=\"{named_url}\">named</a>"
            )),
            _ => page("<p>x</p>"),
        });
        let mut settings = config(&site.url("/"));
        settings.cookies = vec![
            "session=abc; Path=/".into(),
            "scoped=xyz; Domain=localhost; Path=/".into(),
        ];
        settings.allow_urls = vec!["http://127.0.0.1/*".into(), "http://localhost/*".into()];
        let Some(mut session) = Session::open(settings) else {
            return;
        };
        let first = site.requests_to("/").pop().unwrap();
        let cookie = first.header("cookie").unwrap_or_default().to_string();
        assert!(cookie.contains("session=abc"), "{cookie}");
        assert!(!cookie.contains("scoped=xyz"), "{cookie}");
        session.click_id("port");
        let cookie = other
            .requests_to("/other")
            .pop()
            .and_then(|request| request.header("cookie").map(str::to_string))
            .unwrap_or_default();
        assert!(cookie.contains("session=abc"), "{cookie}");
        session.apply(json!({"kind": "back"}));
        session.click_id("named");
        let cookie = other
            .requests_to("/named")
            .pop()
            .and_then(|request| request.header("cookie").map(str::to_string))
            .unwrap_or_default();
        assert!(cookie.contains("scoped=xyz"), "{cookie}");
        assert!(!cookie.contains("session=abc"), "{cookie}");
        session.shutdown();
    }

    #[test]
    fn the_device_scale_factor_sets_screenshot_pixels() {
        let site =
            Fixture::start(|_| page("<script>document.title = String(devicePixelRatio);</script>"));
        let mut settings = config(&site.url("/"));
        settings.width = 400;
        settings.height = 300;
        settings.device_scale_factor = 2.0;
        let Some(session) = Session::open(settings) else {
            return;
        };
        assert_eq!(session.title(), "2");
        assert_eq!(
            session.state["viewport"],
            json!({"width": 400, "height": 300, "device_scale_factor": 2.0})
        );
        let png = session.screenshot.clone().expect("a screenshot");
        assert_eq!(png_size(&png), (800, 600));
        session.shutdown();
    }

    #[test]
    fn coverage_is_reported_only_when_instrumented() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page("<button id=go onclick=\"branch(true)\">go</button><script src=\"/app.js\"></script>"),
            "/app.js" => Reply::with_type(
                200,
                "text/javascript",
                b"function branch(flag) { if (flag) { document.title = 'yes'; } else { document.title = 'no'; } }\nfunction unused() { return 1; }\n".to_vec(),
            ),
            _ => Reply::not_found(),
        }
        });
        let Some(plain) = Session::open(config(&site.url("/"))) else {
            return;
        };
        assert_eq!(plain.state["coverage"], json!(null));
        plain.shutdown();
        let mut settings = config(&site.url("/"));
        settings.instrument_javascript = true;
        let Some(mut session) = Session::open(settings) else {
            return;
        };
        let first_total = session.state["coverage"]["branches_total"]
            .as_u64()
            .unwrap();
        assert!(first_total >= 1, "{}", session.state["coverage"]);
        session.click_id("go");
        assert_eq!(session.title(), "yes");
        let coverage = session.state["coverage"].clone();
        assert!(
            coverage["branches_new"].as_u64().unwrap() >= 1,
            "{coverage}"
        );
        assert_eq!(
            coverage["branches_total"].as_u64().unwrap(),
            first_total + coverage["branches_new"].as_u64().unwrap()
        );
        session.click_id("go");
        assert_eq!(session.state["coverage"]["branches_new"], 0);
        let symbols = session.driver.coverage_symbols().expect("symbols");
        let scripts = symbols["scripts"].as_array().unwrap();
        let app = scripts
            .iter()
            .find(|script| script["url"].as_str().unwrap().ends_with("/app.js"))
            .expect("app.js covered");
        let names: Vec<&str> = app["functions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|function| function["name"].as_str())
            .collect();
        assert!(names.contains(&"branch"), "{names:?}");
        assert!(!names.contains(&"unused"), "{names:?}");
        session.shutdown();
    }

    #[test]
    fn resource_counters_track_dom_nodes_and_listeners() {
        let site = Fixture::start(|_| {
            page("<button id=grow onclick=\"for (let i = 0; i < 200; i++) { const d = document.createElement('div'); d.addEventListener('click', () => {}); document.body.appendChild(d); }\">grow</button>")
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let before = session.state["resources"].clone();
        for metric in [
            "js_heap_used",
            "js_heap_total",
            "dom_nodes",
            "event_listeners",
            "layout_objects",
        ] {
            assert!(before[metric].as_u64().unwrap() > 0, "{metric} in {before}");
        }
        session.click_id("grow");
        let after = session.state["resources"].clone();
        assert!(
            after["dom_nodes"].as_u64().unwrap() >= before["dom_nodes"].as_u64().unwrap() + 200,
            "{before} {after}"
        );
        assert!(
            after["event_listeners"].as_u64().unwrap()
                >= before["event_listeners"].as_u64().unwrap() + 200,
            "{before} {after}"
        );
        session.shutdown();
    }

    #[test]
    fn a_virtual_time_policy_is_applied_before_the_first_navigation() {
        let site = Fixture::start(|_| {
            page("<script>const start = Date.now(); setTimeout(() => { document.title = 'fired'; }, 5000);</script>")
        });
        let mut settings = config(&site.url("/"));
        settings.virtual_time_policy = Some("advance".into());
        let Some(mut session) = Session::open(settings) else {
            return;
        };
        session.apply(json!({"kind": "wait", "ms": 200}));
        assert_eq!(session.title(), "fired");
        session.shutdown();
    }

    #[test]
    fn every_state_has_the_documented_shape_and_a_screenshot() {
        let site = Fixture::start(|_| page("<p>shape</p>"));
        let Some(session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let state = &session.state;
        for key in [
            "url",
            "title",
            "mime_type",
            "viewport",
            "navigation",
            "history",
            "console",
            "exceptions",
            "resources",
            "coverage",
            "scroll",
            "content",
            "focused",
            "clickables",
            "downloads",
            "last_action",
        ] {
            assert!(state.get(key).is_some(), "{key} missing from {state}");
        }
        assert_eq!(state["content"], json!({"is_html": true, "has_body": true}));
        assert_eq!(state["mime_type"], "text/html");
        assert_eq!(state["last_action"], json!(null));
        assert_eq!(state["focused"], json!(null));
        assert_eq!(png_size(session.screenshot.as_ref().unwrap()), (800, 600));
        session.shutdown();
    }

    #[test]
    fn an_unreachable_origin_is_a_setup_error() {
        if crate::support::chrome().is_none() {
            return;
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut driver =
            ui_prop_tester::browser::open(&config(&format!("http://127.0.0.1:{port}/"))).unwrap();
        let error = driver.start().expect_err("refused");
        assert!(
            error.to_string().contains("ERR_CONNECTION_REFUSED"),
            "{error}"
        );
        driver.shutdown().unwrap();
        let missing = ui_prop_tester::browser::open(&config("/definitely/not/here.html"));
        assert!(missing.is_err());
    }
}

mod live {
    use std::time::Duration;

    use crate::support::{config, page, Fixture, Reply, Session};
    use serde_json::json;

    #[test]
    fn server_sent_events_between_actions_show_up_in_the_next_state() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<ul id=log></ul><button id=noop>noop</button>\
                 <script>const source = new EventSource('/events'); source.onmessage = (event) => { const item = document.createElement('li'); item.textContent = event.data; document.getElementById('log').appendChild(item); document.title = 'events ' + document.querySelectorAll('li').length; };</script>",
            ),
            "/events" => Reply::Events {
                events: vec!["one".into(), "two".into(), "three".into()],
                interval: Duration::from_millis(150),
            },
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.apply(json!({"kind": "wait", "ms": 800}));
        assert_eq!(session.title(), "events 3");
        session.shutdown();
    }

    #[test]
    fn websocket_frames_between_actions_show_up_in_the_next_state() {
        let site = Fixture::start(|request| {
            match request.path.as_str() {
            "/" => page(
                "<button id=send>send</button><output id=out></output>\
                 <script>const socket = new WebSocket('ws://' + location.host + '/socket'); let seen = [];\
                 socket.onmessage = (event) => { seen.push(event.data); document.title = seen.join(','); };\
                 document.getElementById('send').onclick = () => socket.send('ping');</script>",
            ),
            "/socket" => Reply::Socket {
                messages: vec!["hello".into()],
                interval: Duration::from_millis(100),
            },
            _ => Reply::not_found(),
        }
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.wait_until("(s) => s.document.title === 'hello'");
        session.click_id("send");
        let title =
            session.wait_until("(s) => s.document.title.includes('echo') && s.document.title");
        assert_eq!(title, "hello,echo:ping");
        session.shutdown();
    }

    #[test]
    fn a_late_exception_appears_in_a_following_state() {
        let site = Fixture::start(|_| {
            page("<button id=arm onclick=\"setTimeout(() => { throw new Error('late'); }, 400)\">arm</button>")
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        session.click_id("arm");
        assert_eq!(session.state["exceptions"], json!([]));
        session.apply(json!({"kind": "wait", "ms": 700}));
        assert_eq!(session.state["exceptions"][0]["message"], "Error: late");
        session.shutdown();
    }
}

mod extract {
    use crate::support::{config, page, Fixture, Session};
    use serde_json::json;
    use ui_prop_tester::{ExtractOutcome, RemoteExtractor};

    fn extractor(id: u32, source: &str) -> RemoteExtractor {
        RemoteExtractor {
            id,
            source: source.to_string(),
        }
    }

    #[test]
    fn extractors_run_in_the_page_in_one_round_trip_and_in_order() {
        let site = Fixture::start(|_| {
            page("<h1 id=heading>Hello</h1><script>window.appState = { count: 3 };</script>")
        });
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        assert!(session.driver.extracts_remotely());
        let outcomes = session
            .driver
            .extract(&[
                extractor(1, "(state) => state.document.getElementById('heading').textContent"),
                extractor(2, "(state) => state.window.appState.count"),
                extractor(3, "(state) => ({ title: state.title, status: state.navigation.status, clickables: state.clickables.length })"),
                extractor(4, "function named(state) { return state.url.startsWith('http'); }"),
                extractor(5, "async (state) => { await new Promise((resolve) => setTimeout(resolve, 10)); return [1, 2]; }"),
                extractor(6, "(state) => undefined"),
            ])
            .expect("extract");
        assert_eq!(
            outcomes,
            vec![
                ExtractOutcome::Value(json!("Hello")),
                ExtractOutcome::Value(json!(3)),
                ExtractOutcome::Value(json!({"title": "fixture", "status": 200, "clickables": 0})),
                ExtractOutcome::Value(json!(true)),
                ExtractOutcome::Value(json!([1, 2])),
                ExtractOutcome::Value(json!(null)),
            ]
        );
        session.shutdown();
    }

    #[test]
    fn an_extractor_error_reports_its_message_and_stack() {
        let site = Fixture::start(|_| page("<p>x</p>"));
        let Some(mut session) = Session::open(config(&site.url("/"))) else {
            return;
        };
        let outcomes = session
            .driver
            .extract(&[
                extractor(1, "(state) => state.title"),
                extractor(2, "function broken(state) { return state.missing.field; }"),
                extractor(
                    3,
                    "(state) => { const loop = {}; loop.self = loop; return loop; }",
                ),
            ])
            .expect("extract");
        assert_eq!(outcomes[0], ExtractOutcome::Value(json!("fixture")));
        match &outcomes[1] {
            ExtractOutcome::Thrown { message, stack } => {
                assert!(message.contains("field"), "{message}");
                assert!(stack.contains("broken"), "{stack}");
            }
            other => panic!("expected a thrown error, got {other:?}"),
        }
        assert!(
            matches!(&outcomes[2], ExtractOutcome::Thrown { message, .. } if message.contains("circular"))
        );
        let syntax = session
            .driver
            .extract(&[
                extractor(1, "(state) => state.title"),
                extractor(2, "(state) => {"),
            ])
            .expect("extract");
        assert_eq!(syntax[0], ExtractOutcome::Value(json!("fixture")));
        assert!(
            matches!(&syntax[1], ExtractOutcome::Thrown { message, .. } if message.contains("SyntaxError")),
            "{:?}",
            syntax[1]
        );
        session.shutdown();
    }
}

mod external {
    use crate::support::{config, page, ExternalChrome, Fixture, Reply, Session};

    fn site() -> Fixture {
        Fixture::start(|request| match request.path.as_str() {
            "/" => page("<a id=next href=\"/next\">next</a>"),
            "/next" => page("<p>next</p>"),
            _ => Reply::not_found(),
        })
    }

    #[test]
    fn test_external_attaches_to_an_existing_page_and_leaves_the_browser_running() {
        let Some(chrome) = crate::support::chrome() else {
            return;
        };
        let site = site();
        let mut external = ExternalChrome::launch(&chrome);
        let mut settings = config(&site.url("/"));
        settings.remote_debugger = Some(external.http_url.clone());
        let Some(mut session) = Session::open(settings.clone()) else {
            return;
        };
        assert_eq!(session.url(), site.url("/"));
        session.click_id("next");
        assert!(session.url().ends_with("/next"));
        session.shutdown();
        assert!(external.is_running());
        assert_eq!(external.page_urls(), vec![site.url("/next")]);
        let loads = site.requests_to("/next").len();
        let Some(session) = Session::open(settings) else {
            return;
        };
        assert!(session.url().ends_with("/next"));
        assert_eq!(site.requests_to("/next").len(), loads);
        session.shutdown();
        assert!(external.is_running());
    }

    #[test]
    fn test_external_with_create_target_opens_and_closes_its_own_page() {
        let Some(chrome) = crate::support::chrome() else {
            return;
        };
        let site = site();
        let mut external = ExternalChrome::launch(&chrome);
        let before = external.page_urls();
        let mut settings = config(&site.url("/"));
        settings.remote_debugger = Some(external.websocket_url.clone());
        settings.create_target = true;
        let Some(session) = Session::open(settings) else {
            return;
        };
        assert_eq!(session.url(), site.url("/"));
        assert_eq!(external.page_urls().len(), before.len() + 1);
        session.shutdown();
        assert!(external.is_running());
        assert_eq!(external.page_urls(), before);
    }
}
