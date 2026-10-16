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
pub fn management(error: &ManagementError, node: &WideText) -> RenderedOutput {
    let query = error.operation.eq_ignore_case("query")
        || error.operation.eq_ignore_case("enumerate")
        || error.operation.eq_ignore_case("convert");
    let mut stderr = WideText::default();
    if query {
        push(&mut stderr, "Node - ");
        append(&mut stderr, node);
        push(&mut stderr, NL);
    }
    push(&mut stderr, "ERROR:");
    push(&mut stderr, NL);
    push(&mut stderr, "Description = ");
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
