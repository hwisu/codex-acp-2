pub(crate) mod meta {
    pub(crate) const CODEX_ACP: &str = "codex_acp";
    pub(crate) const KIND: &str = "kind";
    pub(crate) const WARNING_KIND: &str = "warning";
    pub(crate) const TOKEN_USAGE: &str = "codex_token_usage";
    pub(crate) const TOOL_CALL_OUTPUT: &str = "toolCallOutput";
    pub(crate) const TOOL_CALL_OUTPUT_DEFAULT_OPEN: &str = "defaultOpen";
    pub(crate) const TOOL_CALL_OUTPUT_INITIALLY_FOLDED: &str = "initiallyFolded";
    pub(crate) const TOOL_CALL_OUTPUT_REASON: &str = "reason";
    pub(crate) const TERMINAL_OUTPUT_CAPABILITY: &str = "terminal_output";
    pub(crate) const TERMINAL_INFO: &str = "terminal_info";
    pub(crate) const TERMINAL_OUTPUT: &str = "terminal_output";
    pub(crate) const TERMINAL_EXIT: &str = "terminal_exit";
}

pub(crate) mod permission_option {
    pub(crate) const APPROVED: &str = "approved";
    pub(crate) const APPROVED_EXECPOLICY_AMENDMENT: &str = "approved-execpolicy-amendment";
    pub(crate) const APPROVED_FOR_SESSION: &str = "approved-for-session";
    pub(crate) const NETWORK_POLICY_AMENDMENT_ALLOW: &str = "network-policy-amendment-allow";
    pub(crate) const NETWORK_POLICY_AMENDMENT_DENY: &str = "network-policy-amendment-deny";
    pub(crate) const DENIED: &str = "denied";
    pub(crate) const ABORT: &str = "abort";
    pub(crate) const TIMED_OUT: &str = "timed_out";
}

pub(crate) mod mcp_approval {
    pub(crate) const KIND_KEY: &str = "codex_approval_kind";
    pub(crate) const KIND_MCP_TOOL_CALL: &str = "mcp_tool_call";
    pub(crate) const PERSIST_KEY: &str = "persist";
    pub(crate) const PERSIST_SESSION: &str = "session";
    pub(crate) const PERSIST_ALWAYS: &str = "always";
    pub(crate) const TOOL_TITLE_KEY: &str = "tool_title";
    pub(crate) const TOOL_DESCRIPTION_KEY: &str = "tool_description";
    pub(crate) const CONNECTOR_NAME_KEY: &str = "connector_name";
    pub(crate) const CONNECTOR_DESCRIPTION_KEY: &str = "connector_description";
    pub(crate) const TOOL_PARAMS_KEY: &str = "tool_params";
    pub(crate) const TOOL_PARAMS_DISPLAY_KEY: &str = "tool_params_display";
    pub(crate) const REQUEST_ID_PREFIX: &str = "mcp_tool_call_approval_";
    pub(crate) const ALLOW_OPTION_ID: &str = permission_option::APPROVED;
    pub(crate) const ALLOW_SESSION_OPTION_ID: &str = permission_option::APPROVED_FOR_SESSION;
    pub(crate) const ALLOW_ALWAYS_OPTION_ID: &str = "approved-always";
    pub(crate) const CANCEL_OPTION_ID: &str = "cancel";

    use super::permission_option;
}

#[cfg(test)]
mod tests {
    use super::{mcp_approval, meta, permission_option};

    #[test]
    fn wire_constants_match_existing_protocol_extensions() {
        assert_eq!(meta::CODEX_ACP, "codex_acp");
        assert_eq!(meta::KIND, "kind");
        assert_eq!(meta::TOKEN_USAGE, "codex_token_usage");
        assert_eq!(meta::TERMINAL_OUTPUT_CAPABILITY, "terminal_output");
        assert_eq!(meta::TERMINAL_INFO, "terminal_info");
        assert_eq!(meta::TERMINAL_OUTPUT, "terminal_output");
        assert_eq!(meta::TERMINAL_EXIT, "terminal_exit");
        assert_eq!(meta::TOOL_CALL_OUTPUT_INITIALLY_FOLDED, "initiallyFolded");

        assert_eq!(permission_option::APPROVED, "approved");
        assert_eq!(
            permission_option::APPROVED_FOR_SESSION,
            "approved-for-session"
        );
        assert_eq!(permission_option::DENIED, "denied");

        assert_eq!(mcp_approval::KIND_KEY, "codex_approval_kind");
        assert_eq!(mcp_approval::REQUEST_ID_PREFIX, "mcp_tool_call_approval_");
        assert_eq!(mcp_approval::ALLOW_ALWAYS_OPTION_ID, "approved-always");
    }
}
