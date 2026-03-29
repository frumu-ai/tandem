use super::*;

#[tokio::test]
async fn mcp_list_returns_connected_inventory() {
    let state = test_state().await;

    let tool_names = state
        .tools
        .list()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .collect::<Vec<_>>();
    assert!(tool_names.iter().any(|name| name == "mcp_list"));

    let output = state
        .tools
        .execute("mcp_list", json!({}))
        .await
        .expect("execute mcp_list");
    let payload: Value = serde_json::from_str(&output.output).expect("inventory json");

    assert_eq!(
        payload.get("inventory_version").and_then(Value::as_u64),
        Some(1)
    );

    let servers = payload
        .get("servers")
        .and_then(Value::as_array)
        .expect("servers array");
    let github = servers
        .iter()
        .find(|row| row.get("name").and_then(Value::as_str) == Some("github"))
        .expect("github server row");
    assert_eq!(github.get("connected").and_then(Value::as_bool), Some(true));
    let remote_tools = github
        .get("remote_tools")
        .and_then(Value::as_array)
        .expect("remote tools array");
    assert!(!remote_tools.is_empty());
    assert_eq!(
        github.get("remote_tool_count").and_then(Value::as_u64),
        Some(remote_tools.len() as u64)
    );

    let connected_server_names = payload
        .get("connected_server_names")
        .and_then(Value::as_array)
        .expect("connected server names");
    assert!(connected_server_names
        .iter()
        .any(|server| server.as_str() == Some("github")));
}
