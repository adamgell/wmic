use super::*;
pub fn text(message: &WideText, token: Option<&WideText>) -> RenderedOutput {
    let mut stderr = WideText::default();
    if let Some(token) = token {
        append(&mut stderr, token);
        push(&mut stderr, " - ");
    }
    append(&mut stderr, message);
    push(&mut stderr, NL);
    RenderedOutput {
        stderr,
        ..Default::default()
    }
}
/// Operations whose diagnostic starts with a Node line and follows empty query output.
pub(super) fn names_node(operation: &WideText) -> bool {
    operation.eq_ignore_case("query")
        || operation.eq_ignore_case("enumerate")
        || operation.eq_ignore_case("convert")
        || operation.eq_ignore_case("failfast")
}
pub fn management(error: &ManagementError, node: &WideText, calls: bool) -> RenderedOutput {
    let query = names_node(&error.operation) && !calls;
    let failfast = error.operation.eq_ignore_case("failfast");
    let mut stderr = WideText::default();
    if query {
        push(&mut stderr, "Node - ");
        append(&mut stderr, node);
        push(&mut stderr, NL);
    }
    if !failfast {
        push(&mut stderr, "ERROR:");
        push(&mut stderr, NL);
        push(&mut stderr, "Description = ");
    }
    append(&mut stderr, &error.message);
    push(&mut stderr, NL);
    RenderedOutput {
        stdout: if query {
            "\r\r\n\r\r\n".into()
        } else {
            WideText::default()
        },
        stderr,
        record: None,
    }
}
