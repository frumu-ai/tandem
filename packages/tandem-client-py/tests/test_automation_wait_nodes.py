from tandem_client import AutomationV2FlowNode


def test_automation_wait_node_accepts_snake_and_camel_aliases() -> None:
    node = AutomationV2FlowNode.model_validate(
        {
            "nodeId": "wait-for-window",
            "agentId": "",
            "objective": "Wait for the deployment window",
            "dependsOn": ["prepare"],
            "wait": {
                "kind": "timer",
                "wakeAt": {
                    "source": "node_output",
                    "nodeId": "prepare",
                    "jsonPointer": "/deploy_at_ms",
                },
                "timeout": {
                    "expiresAfterMs": 86_400_000,
                    "onTimeout": "resume",
                },
            },
        }
    )

    assert node.node_id == "wait-for-window"
    assert node.wait is not None
    assert node.wait.kind == "timer"
    assert node.wait.wake_at is not None
    assert node.wait.wake_at.node_id == "prepare"
    assert node.wait.timeout is not None
    assert node.wait.timeout.on_timeout == "resume"
