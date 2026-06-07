import json

import httpx
import pytest
import respx
from tandem_client import TandemClient

BASE = "http://localhost:39731"


@pytest.mark.asyncio
@respx.mock
async def test_create_session_sends_sampling_params() -> None:
    route = respx.post(f"{BASE}/session").mock(
        return_value=httpx.Response(200, json={"id": "s_1"})
    )
    async with TandemClient(base_url=BASE, token="token") as client:
        session_id = await client.sessions.create(
            title="t",
            provider="anthropic",
            model="claude-sonnet-4-6",
            temperature=0.1,
            top_p=0.9,
            max_tokens=2048,
        )

    assert session_id == "s_1"
    body = json.loads(route.calls[0].request.content.decode("utf-8"))
    assert body["temperature"] == 0.1
    assert body["top_p"] == 0.9
    assert body["max_tokens"] == 2048


@pytest.mark.asyncio
@respx.mock
async def test_create_session_omits_sampling_when_unset() -> None:
    route = respx.post(f"{BASE}/session").mock(
        return_value=httpx.Response(200, json={"id": "s_1"})
    )
    async with TandemClient(base_url=BASE, token="token") as client:
        await client.sessions.create(title="t")

    body = json.loads(route.calls[0].request.content.decode("utf-8"))
    # Backwards compatible: no sampling keys present when none are supplied.
    assert "temperature" not in body
    assert "top_p" not in body
    assert "max_tokens" not in body


@pytest.mark.asyncio
@respx.mock
async def test_prompt_async_sends_sampling_override() -> None:
    route = respx.post(f"{BASE}/session/s_1/prompt_async").mock(
        return_value=httpx.Response(200, json={"id": "run_1"})
    )
    async with TandemClient(base_url=BASE, token="token") as client:
        result = await client.sessions.prompt_async(
            "s_1",
            "hello",
            temperature=0.7,
            max_tokens=512,
        )

    assert result.run_id == "run_1"
    body = json.loads(route.calls[0].request.content.decode("utf-8"))
    assert body["temperature"] == 0.7
    assert body["max_tokens"] == 512
    assert "top_p" not in body


@pytest.mark.asyncio
@respx.mock
async def test_prompt_async_omits_sampling_when_unset() -> None:
    route = respx.post(f"{BASE}/session/s_1/prompt_async").mock(
        return_value=httpx.Response(200, json={"id": "run_1"})
    )
    async with TandemClient(base_url=BASE, token="token") as client:
        await client.sessions.prompt_async("s_1", "hello")

    body = json.loads(route.calls[0].request.content.decode("utf-8"))
    assert "temperature" not in body
    assert "top_p" not in body
    assert "max_tokens" not in body
