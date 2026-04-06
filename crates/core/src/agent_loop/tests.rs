use super::{MessageQueue, is_context_overflow, is_rate_limited};

#[test]
fn test_is_context_overflow() {
    assert!(is_context_overflow("HTTP 400: context_length_exceeded"));
    assert!(is_context_overflow(
        "maximum context length is 200000 tokens"
    ));
    assert!(is_context_overflow("too many tokens in the request"));
    assert!(is_context_overflow("request too large for model"));
    assert!(is_context_overflow("prompt is too long"));
    assert!(is_context_overflow("HTTP 400: token limit exceeded"));
    assert!(!is_context_overflow("HTTP 401: Unauthorized"));
    assert!(!is_context_overflow("HTTP 500: Internal Server Error"));
}

#[test]
fn test_is_rate_limited() {
    assert!(is_rate_limited("HTTP 429: Rate limit exceeded"));
    assert!(is_rate_limited("rate limit reached"));
    assert!(is_rate_limited("429 Too Many Requests"));
    assert!(!is_rate_limited("HTTP 400: Bad request"));
    assert!(!is_rate_limited("HTTP 500: Internal Server Error"));
}

#[test]
fn test_message_queue() {
    let mut q = MessageQueue::new();
    assert!(q.is_empty());

    q.push_steer("fix the bug".into());
    q.push_follow_up("then run tests".into());
    q.push_steer("also check imports".into());

    assert!(!q.is_empty());

    let steers = q.take_steers();
    assert_eq!(steers.len(), 2);
    assert_eq!(steers[0], "fix the bug");
    assert_eq!(steers[1], "also check imports");

    let follow_ups = q.take_follow_ups();
    assert_eq!(follow_ups.len(), 1);
    assert_eq!(follow_ups[0], "then run tests");

    assert!(q.is_empty());
}

#[test]
fn test_message_queue_empty_operations() {
    let mut q = MessageQueue::new();
    assert!(q.take_steers().is_empty());
    assert!(q.take_follow_ups().is_empty());
    assert!(q.is_empty());
}
