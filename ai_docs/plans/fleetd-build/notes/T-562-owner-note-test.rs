#[test]
fn answer_of_the_owner_reaches_the_next_attempt() {
    let w = World::new("p6-wq-owner", "stay,answer_own", &horch_ask(QUESTION));
    let task = w.task();
    w.wait("the answered question", || {
        let rows = w.questions(&task);
        (rows.len() == 1 && rows[0].1 == "answered").then_some(())
    });
    let next = w.wait("the next attempt of the task", || {
        w.attempts(&task).get(1).map(|a| a.0.clone())
    });
    assert_ne!(w.status(&task), "blocked_on_user", "{}", w.context());
    let start = w.wait("START.md of the next attempt", || w.start_md(&next));
    assert!(
        start.contains(QUESTION),
        "START.md of the next attempt holds the question:\n{start}"
    );
    let answer = w
        .one(&format!(
            "SELECT json_extract(data_json, '$.text') FROM events
             WHERE kind = 'answered' AND entity_id = {}",
            quoted(&task)
        ))
        .expect("the answered event holds the answer");
    assert!(
        !answer.is_empty() && start.contains(&answer),
        "START.md of the next attempt holds the answer {answer:?}:\n{start}"
    );
}
