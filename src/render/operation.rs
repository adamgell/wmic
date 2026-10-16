use super::*;
pub(super) fn start(action: Verb, path: &WideText) -> WideText {
    let mut out = WideText::default();
    if action == Verb::Set {
        push(&mut out, "Updating property(s) of '");
        append(&mut out, path);
        push(&mut out, "'");
    } else {
        push(&mut out, "Deleting instance ");
        append(&mut out, path);
    }
    push(&mut out, NL);
    out
}
pub fn render(operation: &OperationResult, interactive: bool) -> RenderedOutput {
    let mut out = RenderedOutput::default();
    if operation.paths.is_empty() && !operation.instances.is_empty() {
        return out;
    }
    if operation.action == Verb::Create {
        push(&mut out.stdout, "Instance creation successful.");
        push(&mut out.stdout, NL);
        return out;
    }
    if operation.paths.is_empty() && !operation.instances.is_empty() {
        return out;
    }
    if operation.paths.is_empty() {
        push(&mut out.stdout, "No Instance(s) Available.");
        push(&mut out.stdout, NL);
        return out;
    }
    for path in &operation.paths {
        if !interactive || operation.action == Verb::Delete {
            append(&mut out.stdout, &start(operation.action, path));
        }
        push(
            &mut out.stdout,
            if operation.action == Verb::Set {
                "Property(s) update successful."
            } else {
                "Instance deletion successful."
            },
        );
        push(&mut out.stdout, NL)
    }
    out
}
