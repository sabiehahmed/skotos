"""OpenFeature-compatible Python provider backed by `SkotosClient`."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Generic, TypeVar

from .client import SkotosClient

T = TypeVar("T")


@dataclass(frozen=True)
class OpenFeatureResolutionDetails(Generic[T]):
    value: T
    variant: str | None = None
    reason: str | None = None
    error_code: str | None = None


class SkotosOpenFeatureProvider:
    metadata = {"name": "skotos-python-provider"}

    def __init__(self, client: SkotosClient) -> None:
        self._client = client

    @staticmethod
    def _to_context(eval_ctx: dict[str, Any] | None) -> dict[str, Any]:
        ctx = dict(eval_ctx or {})
        key = ctx.get("targetingKey") or ctx.get("key") or "anonymous"
        kind = ctx.get("kind") or "user"
        ctx["kind"] = str(kind)
        ctx["key"] = str(key)
        return ctx

    @staticmethod
    def _map_detail(detail: dict[str, Any]) -> OpenFeatureResolutionDetails[Any]:
        reason = detail.get("reason") or {}
        kind = reason.get("kind")
        if kind == "ERROR":
            err_kind = reason.get("errorKind")
            code = (
                "FLAG_NOT_FOUND"
                if err_kind == "FLAG_NOT_FOUND"
                else "TYPE_MISMATCH"
                if err_kind == "WRONG_TYPE"
                else "GENERAL"
            )
            return OpenFeatureResolutionDetails(
                value=detail["value"],
                reason="ERROR",
                error_code=code,
            )
        mapped_reason = (
            "TARGETING_MATCH"
            if kind in ("TARGET_MATCH", "RULE_MATCH")
            else "DISABLED"
            if kind == "OFF"
            else "DEFAULT"
        )
        var_idx = detail.get("variationIndex")
        return OpenFeatureResolutionDetails(
            value=detail["value"],
            variant=str(var_idx) if var_idx is not None else None,
            reason=mapped_reason,
        )

    def resolve_boolean_details(
        self, flag_key: str, default_value: bool, evaluation_context: dict[str, Any] | None = None
    ) -> OpenFeatureResolutionDetails[bool]:
        return self._map_detail(
            self._client.bool_variation_detail(flag_key, self._to_context(evaluation_context), default_value)
        )

    def resolve_string_details(
        self, flag_key: str, default_value: str, evaluation_context: dict[str, Any] | None = None
    ) -> OpenFeatureResolutionDetails[str]:
        return self._map_detail(
            self._client.string_variation_detail(flag_key, self._to_context(evaluation_context), default_value)
        )

    def resolve_number_details(
        self,
        flag_key: str,
        default_value: int | float,
        evaluation_context: dict[str, Any] | None = None,
    ) -> OpenFeatureResolutionDetails[int | float]:
        return self._map_detail(
            self._client.number_variation_detail(flag_key, self._to_context(evaluation_context), default_value)
        )

    def resolve_object_details(
        self, flag_key: str, default_value: Any, evaluation_context: dict[str, Any] | None = None
    ) -> OpenFeatureResolutionDetails[Any]:
        return self._map_detail(
            self._client.json_variation_detail(flag_key, self._to_context(evaluation_context), default_value)
        )
