include!("mcp_parts/part01.rs");
include!("mcp_parts/part02.rs");
include!("mcp_parts/part04.rs");
include!("mcp_parts/part03.rs");
include!("mcp_parts/part05.rs");
include!("mcp_parts/part06.rs");
include!("mcp_tool_authority.rs");
include!("mcp_oauth_refresh.rs");
#[cfg(test)]
#[path = "mcp_hosted_policy_tests.rs"]
mod hosted_policy_tests;
#[cfg(test)]
#[path = "mcp_oauth_dispatch_tests.rs"]
mod oauth_dispatch_tests;
