"""Skotos Python Server-Side Client, TestData, and 2/4/6-Stage Migration Flag helper."""

from __future__ import annotations

import json
import threading
import time
import urllib.request
from dataclasses import dataclass, field
from typing import Any, Callable, Generic, TypeVar

from .evaluator import (
    BigSegmentSnapshot,
    InMemoryEvalStore,
    bool_variation,
    evaluate,
    get_context_kinds,
    number_variation,
    string_variation,
)

T = TypeVar("T")

MIGRATION_STAGES = ("off", "dualwrite", "shadow", "live", "rampdown", "complete")


@dataclass
class SkotosConfig:
    sdk_key: str = ""
    base_url: str = "http://localhost:8081"
    events_url: str | None = None
    offline: bool = False
    send_events: bool = True
    poll_interval_seconds: float = 15.0
    timeout_seconds: float = 5.0


class TestData:
    """Deterministic in-memory data source for unit and integration testing."""

    def __init__(self) -> None:
        self._flags: dict[str, dict[str, Any]] = {}
        self._segments: dict[str, dict[str, Any]] = {}
        self._big_segments: dict[str, BigSegmentSnapshot] = {}

    def flag_boolean(self, key: str, value: bool) -> "TestData":
        self._flags[key] = {
            "key": key,
            "version": 1,
            "on": True,
            "salt": f"salt-{key}",
            "variations": [True, False],
            "offVariation": 1,
            "fallthrough": {"variation": 0 if value else 1},
        }
        return self

    def flag_string(self, key: str, value: str) -> "TestData":
        self._flags[key] = {
            "key": key,
            "version": 1,
            "on": True,
            "salt": f"salt-{key}",
            "variations": [value],
            "offVariation": 0,
            "fallthrough": {"variation": 0},
        }
        return self

    def update_flag(self, flag: dict[str, Any]) -> "TestData":
        self._flags[str(flag["key"])] = flag
        return self

    def update_segment(self, segment: dict[str, Any]) -> "TestData":
        self._segments[str(segment["key"])] = segment
        return self

    def update_big_segment(self, snapshot: BigSegmentSnapshot) -> "TestData":
        self._big_segments[snapshot.segment_key] = snapshot
        return self

    def snapshot(self) -> dict[str, Any]:
        return {
            "revision": 1,
            "flags": dict(self._flags),
            "segments": dict(self._segments),
            "bigSegments": {
                k: {
                    "segmentKey": v.segment_key,
                    "contextKind": v.context_kind,
                    "generation": v.generation,
                    "includedHashes": list(v.included_hashes),
                    "excludedHashes": list(v.excluded_hashes),
                }
                for k, v in self._big_segments.items()
            },
        }


@dataclass
class MigrationResult(Generic[T]):
    stage: str
    origin: str
    value: T | None = None
    error: Exception | None = None
    consistent: bool | None = None
    old_latency_ms: float | None = None
    new_latency_ms: float | None = None


class SkotosClient:
    """Thread-safe Skotos Server-Side Python SDK client."""

    def __init__(
        self,
        config: SkotosConfig,
        test_data: TestData | None = None,
    ) -> None:
        self._cfg = config
        self._lock = threading.RLock()
        self._store = InMemoryEvalStore()
        self._initialized = False
        self._etag: str | None = None
        self._revision = 0
        self._events: list[dict[str, Any]] = []
        self._summary_start_ms = int(time.time() * 1000)
        self._summary_counters: dict[tuple[str, int | None, int], dict[str, Any]] = {}

        if test_data is not None:
            self.apply_snapshot(test_data.snapshot())
        elif not config.offline and config.sdk_key:
            try:
                self.refresh()
            except Exception:
                # Non-fatal initialization; callers can inspect is_initialized()
                pass

    def is_initialized(self) -> bool:
        with self._lock:
            return self._initialized

    def get_revision(self) -> int:
        with self._lock:
            return self._revision

    def apply_snapshot(self, snapshot: dict[str, Any]) -> None:
        with self._lock:
            flags = dict(snapshot.get("flags") or {})
            segments = dict(snapshot.get("segments") or {})
            big_segments = {
                k: BigSegmentSnapshot.from_dict(v)
                for k, v in (snapshot.get("bigSegments") or {}).items()
            }
            self._store = InMemoryEvalStore(flags=flags, segments=segments, big_segments=big_segments)
            self._revision = int(snapshot.get("revision", self._revision))
            self._initialized = True

    def refresh(self) -> bool:
        if self._cfg.offline or not self._cfg.sdk_key:
            return False
        url = f"{self._cfg.base_url.rstrip('/')}/sdk/v1/latest-all"
        headers = {"Authorization": self._cfg.sdk_key, "Accept": "application/json"}
        if self._etag:
            headers["If-None-Match"] = self._etag
        req = urllib.request.Request(url, headers=headers, method="GET")
        try:
            with urllib.request.urlopen(req, timeout=self._cfg.timeout_seconds) as resp:
                if resp.status == 304:
                    return True
                etag = resp.headers.get("ETag")
                if etag:
                    self._etag = etag
                body = json.loads(resp.read().decode("utf-8"))
                self.apply_snapshot(body)
                return True
        except urllib.error.HTTPError as exc:
            if exc.code == 304:
                return True
            raise

    def bool_variation_detail(
        self, flag_key: str, context: dict[str, Any], default_value: bool
    ) -> dict[str, Any]:
        with self._lock:
            if not self._initialized:
                return {"value": default_value, "reason": {"kind": "ERROR", "errorKind": "CLIENT_NOT_READY"}}
            detail = bool_variation(self._store, flag_key, context, default_value)
            self._record_summary(flag_key, context, detail)
            return detail

    def bool_variation(self, flag_key: str, context: dict[str, Any], default_value: bool) -> bool:
        return bool(self.bool_variation_detail(flag_key, context, default_value)["value"])

    def string_variation_detail(
        self, flag_key: str, context: dict[str, Any], default_value: str
    ) -> dict[str, Any]:
        with self._lock:
            if not self._initialized:
                return {"value": default_value, "reason": {"kind": "ERROR", "errorKind": "CLIENT_NOT_READY"}}
            detail = string_variation(self._store, flag_key, context, default_value)
            self._record_summary(flag_key, context, detail)
            return detail

    def string_variation(self, flag_key: str, context: dict[str, Any], default_value: str) -> str:
        return str(self.string_variation_detail(flag_key, context, default_value)["value"])

    def number_variation_detail(
        self, flag_key: str, context: dict[str, Any], default_value: int | float
    ) -> dict[str, Any]:
        with self._lock:
            if not self._initialized:
                return {"value": default_value, "reason": {"kind": "ERROR", "errorKind": "CLIENT_NOT_READY"}}
            detail = number_variation(self._store, flag_key, context, default_value)
            self._record_summary(flag_key, context, detail)
            return detail

    def number_variation(
        self, flag_key: str, context: dict[str, Any], default_value: int | float
    ) -> int | float:
        return self.number_variation_detail(flag_key, context, default_value)["value"]

    def json_variation_detail(
        self, flag_key: str, context: dict[str, Any], default_value: Any
    ) -> dict[str, Any]:
        with self._lock:
            if not self._initialized:
                return {"value": default_value, "reason": {"kind": "ERROR", "errorKind": "CLIENT_NOT_READY"}}
            detail = evaluate(self._store, flag_key, context, default_value)
            self._record_summary(flag_key, context, detail)
            return detail

    def json_variation(self, flag_key: str, context: dict[str, Any], default_value: Any) -> Any:
        return self.json_variation_detail(flag_key, context, default_value)["value"]

    def track(
        self,
        event_key: str,
        context: dict[str, Any],
        metric_value: float | None = None,
        data: Any = None,
    ) -> None:
        if not self._cfg.send_events:
            return
        context_keys = {k: str(sc.get("key", "")) for k, sc in get_context_kinds(context) if sc.get("key")}
        ev: dict[str, Any] = {
            "kind": "custom",
            "creationDate": int(time.time() * 1000),
            "key": event_key,
            "contextKeys": context_keys,
            "context": context,
        }
        if metric_value is not None:
            ev["metricValue"] = float(metric_value)
        if data is not None:
            ev["data"] = data
        with self._lock:
            self._events.append(ev)

    def record_migration_op(
        self,
        flag_key: str,
        operation: str,
        stage: str,
        context: dict[str, Any],
        invoked_old: bool,
        invoked_new: bool,
        consistent: bool | None = None,
        old_latency_ms: float | None = None,
        new_latency_ms: float | None = None,
        old_error: bool = False,
        new_error: bool = False,
    ) -> None:
        if not self._cfg.send_events:
            return
        ev: dict[str, Any] = {
            "kind": "migration_op",
            "creationDate": int(time.time() * 1000),
            "operation": operation,
            "key": flag_key,
            "stage": stage,
            "context": context,
            "oldInvoked": invoked_old,
            "newInvoked": invoked_new,
            "oldError": old_error,
            "newError": new_error,
            "oldLatencyMs": old_latency_ms or 0.0,
            "newLatencyMs": new_latency_ms or 0.0,
        }
        if consistent is not None:
            ev["consistent"] = consistent
        with self._lock:
            self._events.append(ev)

    def pending_events(self) -> list[dict[str, Any]]:
        with self._lock:
            return list(self._events)

    def _record_summary(
        self, flag_key: str, context: dict[str, Any], detail: dict[str, Any]
    ) -> None:
        if not self._cfg.send_events:
            return
        flag = self._store.get_flag(flag_key)
        version = int(flag.get("version", 0)) if flag else 0
        var_idx = detail.get("variationIndex")
        bucket = (flag_key, var_idx, version)
        entry = self._summary_counters.get(bucket)
        if entry is None:
            entry = {
                "variation": var_idx,
                "version": version,
                "value": detail.get("value"),
                "count": 0,
                "contextKinds": set(),
            }
            self._summary_counters[bucket] = entry
        entry["count"] += 1
        for k, _ in get_context_kinds(context):
            entry["contextKinds"].add(k)

    def flush(self) -> int:
        with self._lock:
            payload: list[dict[str, Any]] = list(self._events)
            self._events.clear()
            if self._summary_counters:
                features: dict[str, dict[str, Any]] = {}
                for (flag_key, _, _), c in self._summary_counters.items():
                    feat = features.setdefault(flag_key, {"contextKinds": [], "counters": []})
                    for ck in sorted(c["contextKinds"]):
                        if ck not in feat["contextKinds"]:
                            feat["contextKinds"].append(ck)
                    counter_obj: dict[str, Any] = {
                        "version": c["version"],
                        "value": c["value"],
                        "count": c["count"],
                    }
                    if c["variation"] is not None:
                        counter_obj["variation"] = c["variation"]
                    feat["counters"].append(counter_obj)
                now_ms = int(time.time() * 1000)
                payload.append(
                    {
                        "kind": "summary",
                        "startDate": self._summary_start_ms,
                        "endDate": now_ms,
                        "features": features,
                    }
                )
                self._summary_counters.clear()
                self._summary_start_ms = now_ms

        if not payload or not self._cfg.events_url or not self._cfg.sdk_key or self._cfg.offline:
            return len(payload)

        url = f"{self._cfg.events_url.rstrip('/')}/events/v1/bulk"
        req = urllib.request.Request(
            url,
            data=json.dumps(payload).encode("utf-8"),
            headers={
                "Authorization": self._cfg.sdk_key,
                "Content-Type": "application/json",
            },
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=self._cfg.timeout_seconds) as resp:
            resp.read()
        return len(payload)


class MigrationHelper(Generic[T]):
    """2/4/6-stage database & service migration orchestrator with consistency & latency telemetry."""

    def __init__(
        self,
        client: SkotosClient,
        flag_key: str,
        read_old: Callable[[], T],
        read_new: Callable[[], T],
        write_old: Callable[[T], Any] | None = None,
        write_new: Callable[[T], Any] | None = None,
        check_consistency: Callable[[T, T], bool] | None = None,
    ) -> None:
        self._client = client
        self._flag_key = flag_key
        self._read_old = read_old
        self._read_new = read_new
        self._write_old = write_old
        self._write_new = write_new
        self._check_consistency = check_consistency or (lambda a, b: a == b)

    def _stage(self, context: dict[str, Any], default_stage: str = "off") -> str:
        raw = self._client.string_variation(self._flag_key, context, default_stage)
        return raw if raw in MIGRATION_STAGES else default_stage

    def read(self, context: dict[str, Any], default_stage: str = "off") -> MigrationResult[T]:
        stage = self._stage(context, default_stage)
        old_val: T | None = None
        new_val: T | None = None
        old_err: Exception | None = None
        new_err: Exception | None = None
        old_ms: float | None = None
        new_ms: float | None = None

        invoke_old = stage in ("off", "dualwrite", "shadow", "live")
        invoke_new = stage in ("shadow", "live", "rampdown", "complete")

        if invoke_old:
            t0 = time.perf_counter()
            try:
                old_val = self._read_old()
            except Exception as exc:
                old_err = exc
            old_ms = (time.perf_counter() - t0) * 1000.0

        if invoke_new:
            t0 = time.perf_counter()
            try:
                new_val = self._read_new()
            except Exception as exc:
                new_err = exc
            new_ms = (time.perf_counter() - t0) * 1000.0

        consistent: bool | None = None
        if invoke_old and invoke_new and old_err is None and new_err is None:
            consistent = bool(self._check_consistency(old_val, new_val))  # type: ignore[arg-type]

        self._client.record_migration_op(
            flag_key=self._flag_key,
            operation="read",
            stage=stage,
            context=context,
            invoked_old=invoke_old,
            invoked_new=invoke_new,
            consistent=consistent,
            old_latency_ms=old_ms,
            new_latency_ms=new_ms,
            old_error=old_err is not None,
            new_error=new_err is not None,
        )

        if stage in ("live", "rampdown", "complete"):
            return MigrationResult(
                stage=stage,
                origin="new",
                value=new_val,
                error=new_err,
                consistent=consistent,
                old_latency_ms=old_ms,
                new_latency_ms=new_ms,
            )
        return MigrationResult(
            stage=stage,
            origin="old",
            value=old_val,
            error=old_err,
            consistent=consistent,
            old_latency_ms=old_ms,
            new_latency_ms=new_ms,
        )

    def write(
        self, context: dict[str, Any], payload: T, default_stage: str = "off"
    ) -> MigrationResult[Any]:
        stage = self._stage(context, default_stage)
        invoke_old = stage in ("off", "dualwrite", "shadow", "live", "rampdown")
        invoke_new = stage in ("dualwrite", "shadow", "live", "rampdown", "complete")
        old_res: Any = None
        new_res: Any = None
        old_err: Exception | None = None
        new_err: Exception | None = None
        old_ms: float | None = None
        new_ms: float | None = None

        if invoke_old and self._write_old is not None:
            t0 = time.perf_counter()
            try:
                old_res = self._write_old(payload)
            except Exception as exc:
                old_err = exc
            old_ms = (time.perf_counter() - t0) * 1000.0

        if invoke_new and self._write_new is not None:
            t0 = time.perf_counter()
            try:
                new_res = self._write_new(payload)
            except Exception as exc:
                new_err = exc
            new_ms = (time.perf_counter() - t0) * 1000.0

        self._client.record_migration_op(
            flag_key=self._flag_key,
            operation="write",
            stage=stage,
            context=context,
            invoked_old=invoke_old,
            invoked_new=invoke_new,
            old_latency_ms=old_ms,
            new_latency_ms=new_ms,
            old_error=old_err is not None,
            new_error=new_err is not None,
        )

        if stage in ("live", "rampdown", "complete"):
            return MigrationResult(
                stage=stage,
                origin="new",
                value=new_res,
                error=new_err,
                old_latency_ms=old_ms,
                new_latency_ms=new_ms,
            )
        return MigrationResult(
            stage=stage,
            origin="old",
            value=old_res,
            error=old_err,
            old_latency_ms=old_ms,
            new_latency_ms=new_ms,
        )
