use crate::{
    backend::methods::InputError,
    model::{ManagementError, ManagementResult, WideText},
};

/// Error whose target is the node the request addressed.
pub fn error(node: &WideText, code: u32, message: &str, operation: &str) -> ManagementError {
    ManagementError {
        code,
        target: node.clone(),
        message: message.into(),
        operation: operation.into(),
        extended: vec![],
        partial: None,
    }
}

pub fn invalid_namespace(node: &WideText) -> ManagementError {
    error(node, 0x8004100e, "Invalid namespace", "connect")
}

pub fn rpc_unavailable(node: &WideText) -> ManagementError {
    error(node, 0x800706ba, "The RPC server is unavailable.", "query")
}

/// An alias target reports the query failure; a path or class name reports the lookup failure.
pub fn unknown_class(node: &WideText, alias_query: bool) -> ManagementError {
    if alias_query {
        error(node, 0x80041010, "Invalid class", "query")
    } else {
        error(node, 0x80041002, "Not found", "class")
    }
}

pub fn invalid_query(node: &WideText) -> ManagementError {
    error(node, 0x80041017, "Invalid query", "query")
}

pub fn invalid_object_path(node: &WideText) -> ManagementError {
    error(node, 0x8004103a, "Invalid object path", "query")
}

pub fn not_supported(node: &WideText, operation: &str) -> ManagementError {
    error(node, 0x8004100c, "Not supported", operation)
}

/// Argument binding failures name the class, like the Windows backend.
pub fn input_error(class: &WideText, e: InputError) -> ManagementError {
    ManagementError {
        code: e.code,
        target: class.clone(),
        message: e.message,
        operation: "input".into(),
        extended: vec![],
        partial: None,
    }
}

pub fn confirmation_unavailable(class: &WideText) -> ManagementError {
    input_error(
        class,
        InputError {
            code: 0x80041008,
            message: "Confirmation input unavailable".into(),
        },
    )
}

pub fn cancelled(node: &WideText, partial: Option<ManagementResult>) -> ManagementError {
    ManagementError {
        code: 0xC000013A,
        target: node.clone(),
        message: "Operation cancelled".into(),
        operation: "cancel".into(),
        extended: vec![],
        partial: partial.map(Box::new),
    }
}

pub fn interrupted() -> bool {
    crate::platform::unix::console::interrupted()
}
