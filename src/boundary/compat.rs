use std::path::Path;

use agent_client_protocol::schema::{Implementation, Meta};

use super::constants::meta;

pub(crate) fn implementation_is_zed(client_info: Option<&Implementation>) -> bool {
    let Some(client_info) = client_info else {
        return false;
    };
    let title_is_zed = client_info
        .title
        .as_deref()
        .is_some_and(|title| title.to_ascii_lowercase().contains("zed"));

    client_info.name.eq_ignore_ascii_case("zed") || title_is_zed
}

pub(crate) fn client_advertises_terminal_output(meta_value: Option<&Meta>) -> bool {
    meta_value.is_some_and(|value| {
        value
            .get(meta::TERMINAL_OUTPUT_CAPABILITY)
            .is_some_and(|value| value.as_bool().unwrap_or_default())
    })
}

pub(crate) fn tool_call_output_display_meta(default_open: bool, reason: &str) -> Meta {
    Meta::from_iter([(
        meta::CODEX_ACP.to_string(),
        serde_json::json!({
            meta::TOOL_CALL_OUTPUT: {
                meta::TOOL_CALL_OUTPUT_DEFAULT_OPEN: default_open,
                meta::TOOL_CALL_OUTPUT_INITIALLY_FOLDED: !default_open,
                meta::TOOL_CALL_OUTPUT_REASON: reason,
            }
        }),
    )])
}

pub(crate) fn insert_tool_call_output_display_meta(
    meta_value: &mut Meta,
    default_open: bool,
    reason: &str,
) {
    meta_value.insert(
        meta::CODEX_ACP.to_string(),
        serde_json::json!({
            meta::TOOL_CALL_OUTPUT: {
                meta::TOOL_CALL_OUTPUT_DEFAULT_OPEN: default_open,
                meta::TOOL_CALL_OUTPUT_INITIALLY_FOLDED: !default_open,
                meta::TOOL_CALL_OUTPUT_REASON: reason,
            }
        }),
    );
}

pub(crate) fn terminal_info_meta(
    terminal_id: &str,
    cwd: &Path,
    default_open: bool,
    reason: &str,
) -> Meta {
    let mut meta_value = Meta::from_iter([(
        meta::TERMINAL_INFO.to_owned(),
        serde_json::json!({
            "terminal_id": terminal_id,
            "cwd": cwd
        }),
    )]);
    insert_tool_call_output_display_meta(&mut meta_value, default_open, reason);
    meta_value
}

pub(crate) fn terminal_output_delta_meta(terminal_id: &str, data: &str) -> Meta {
    Meta::from_iter([(
        meta::TERMINAL_OUTPUT.to_owned(),
        serde_json::json!({
            "terminal_id": terminal_id,
            "data": data
        }),
    )])
}

pub(crate) fn terminal_exit_meta(terminal_id: &str, exit_code: i32) -> Meta {
    Meta::from_iter([(
        meta::TERMINAL_EXIT.to_owned(),
        serde_json::json!({
            "terminal_id": terminal_id,
            "exit_code": exit_code,
            "signal": null
        }),
    )])
}
