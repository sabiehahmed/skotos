"""Pure-Python canonical Skotos flag evaluation engine (124/124 golden vector parity)."""

from __future__ import annotations

import bisect
import copy
import hashlib
import math
import re
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from typing import Any, Callable, Protocol

BUCKET_SCALE = 100_000
TWO_POW_60 = 1 << 60
FNV_OFFSET_64 = 0xCBF29CE484222325
FNV_PRIME_64 = 0x100000001B3
MASK_64 = 0xFFFFFFFFFFFFFFFF

VALID_KIND_RE = re.compile(r"^[a-zA-Z0-9._-]+$")
RFC3339_RE = re.compile(
    r"^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?(?:([Zz])|([+-])(\d{2}):(\d{2}))$"
)
SEMVER_IDENT_RE = re.compile(r"^[0-9A-Za-z-]+$")
DIGITS_RE = re.compile(r"^[0-9]+$")
EPOCH_UTC = datetime(1970, 1, 1, tzinfo=timezone.utc)


class MalformedFlagError(Exception):
    """Internal signal for cycle or malformed flag/segment definitions."""


@dataclass
class BigSegmentSnapshot:
    segment_key: str
    context_kind: str
    generation: int = 1
    included_hashes: list[int] = field(default_factory=list)
    excluded_hashes: list[int] = field(default_factory=list)

    @classmethod
    def from_dict(cls, raw: dict[str, Any]) -> "BigSegmentSnapshot":
        return cls(
            segment_key=str(raw.get("segmentKey") or raw.get("segment_key") or ""),
            context_kind=str(raw.get("contextKind") or raw.get("context_kind") or "user"),
            generation=int(raw.get("generation", 1)),
            included_hashes=sorted(int(x) for x in (raw.get("includedHashes") or [])),
            excluded_hashes=sorted(int(x) for x in (raw.get("excludedHashes") or [])),
        )


class EvalStore(Protocol):
    def get_flag(self, key: str) -> dict[str, Any] | None: ...

    def get_segment(self, key: str) -> dict[str, Any] | None: ...

    def big_segment_membership(
        self, segment_key: str, context_kind: str, context_key: str
    ) -> bool | None: ...


class InMemoryEvalStore:
    def __init__(
        self,
        flags: dict[str, dict[str, Any]] | None = None,
        segments: dict[str, dict[str, Any]] | None = None,
        big_segments: dict[str, BigSegmentSnapshot] | None = None,
    ) -> None:
        self.flags: dict[str, dict[str, Any]] = dict(flags or {})
        self.segments: dict[str, dict[str, Any]] = dict(segments or {})
        self.big_segments: dict[str, BigSegmentSnapshot] = dict(big_segments or {})

    def get_flag(self, key: str) -> dict[str, Any] | None:
        return self.flags.get(key)

    def get_segment(self, key: str) -> dict[str, Any] | None:
        return self.segments.get(key)

    def big_segment_membership(
        self, segment_key: str, context_kind: str, context_key: str
    ) -> bool | None:
        snap = self.big_segments.get(segment_key)
        if snap is None:
            return None
        return check_big_segment_snapshot(snap, context_kind, context_key)


def hash_to_bucket(hash_input: str) -> int:
    """Exact SHA-1 60-bit integer basis-point bucketing (0..=99_999)."""
    digest_hex = hashlib.sha1(hash_input.encode("utf-8")).hexdigest()
    top60 = int(digest_hex[:15], 16)
    return (top60 * BUCKET_SCALE) // TWO_POW_60


def big_segment_key_hash(context_kind: str, context_key: str) -> int:
    """Deterministic 64-bit FNV-1a hash matching `skotos_domain::big_segment_key_hash`."""
    h = FNV_OFFSET_64
    for b in f"{context_kind}:{context_key}".encode("utf-8"):
        h ^= b
        h = (h * FNV_PRIME_64) & MASK_64
    return h


def _binary_search_int(sorted_list: list[int], target: int) -> bool:
    idx = bisect.bisect_left(sorted_list, target)
    return idx < len(sorted_list) and sorted_list[idx] == target


def check_big_segment_snapshot(
    snapshot: BigSegmentSnapshot, context_kind: str, context_key: str
) -> bool | None:
    if snapshot.context_kind != context_kind:
        return None
    target = big_segment_key_hash(context_kind, context_key)
    if _binary_search_int(snapshot.included_hashes, target):
        return True
    if _binary_search_int(snapshot.excluded_hashes, target):
        return False
    return None


def parse_attribute_ref(raw: str) -> list[str] | None:
    if not raw:
        return None
    if not raw.startswith("/"):
        return [raw]
    if raw == "/" or raw.endswith("/"):
        return None
    parts = raw[1:].split("/")
    out: list[str] = []
    for part in parts:
        if not part:
            return None
        buf: list[str] = []
        i = 0
        n = len(part)
        while i < n:
            if part[i] == "~":
                if i + 1 >= n:
                    return None
                nxt = part[i + 1]
                if nxt == "0":
                    buf.append("~")
                elif nxt == "1":
                    buf.append("/")
                else:
                    return None
                i += 2
            else:
                buf.append(part[i])
                i += 1
        out.append("".join(buf))
    return out


def _is_valid_kind(kind: Any) -> bool:
    return (
        isinstance(kind, str)
        and len(kind) > 0
        and kind != "kind"
        and kind != "multi"
        and bool(VALID_KIND_RE.match(kind))
    )


def validate_context(ctx: Any) -> bool:
    if not isinstance(ctx, dict):
        return False
    if ctx.get("kind") == "multi":
        entries = [(k, v) for k, v in ctx.items() if k != "kind"]
        if not entries:
            return False
        for k, v in entries:
            if not _is_valid_kind(k):
                return False
            if not isinstance(v, dict):
                return False
            key = v.get("key")
            if not isinstance(key, str) or len(key) == 0:
                return False
            nested_kind = v.get("kind")
            if nested_kind is not None and nested_kind != k:
                return False
        return True
    kind = ctx.get("kind")
    if kind is None:
        kind = "user"
    key = ctx.get("key")
    return _is_valid_kind(kind) and isinstance(key, str) and len(key) > 0


def get_context_kind(ctx: dict[str, Any], kind: str) -> dict[str, Any] | None:
    if ctx.get("kind") == "multi":
        nested = ctx.get(kind)
        if isinstance(nested, dict):
            merged = {"kind": kind}
            merged.update(nested)
            return merged
        return None
    resolved_kind = ctx.get("kind")
    if resolved_kind is None:
        resolved_kind = "user"
    if resolved_kind == kind:
        merged = dict(ctx)
        merged["kind"] = resolved_kind
        return merged
    return None


def get_context_kinds(ctx: dict[str, Any]) -> list[tuple[str, dict[str, Any]]]:
    if ctx.get("kind") == "multi":
        out: list[tuple[str, dict[str, Any]]] = []
        for k, v in ctx.items():
            if k == "kind" or not isinstance(v, dict):
                continue
            merged = {"kind": k}
            merged.update(v)
            out.append((k, merged))
        return out
    resolved_kind = ctx.get("kind")
    if resolved_kind is None:
        resolved_kind = "user"
    merged = dict(ctx)
    merged["kind"] = resolved_kind
    return [(str(resolved_kind), merged)]


_MISSING = object()


def _get_attribute(single: dict[str, Any], components: list[str]) -> Any:
    if not components:
        return _MISSING
    first = components[0]
    if first == "_meta":
        return _MISSING
    if first == "kind":
        return (single.get("kind") or "user") if len(components) == 1 else _MISSING
    if first == "key":
        return single.get("key") if len(components) == 1 else _MISSING
    if first == "anonymous":
        return bool(single.get("anonymous")) if len(components) == 1 else _MISSING

    if first not in single:
        return _MISSING
    cur: Any = single[first]
    for seg in components[1:]:
        if not isinstance(cur, dict) or seg not in cur:
            return _MISSING
        cur = cur[seg]
    return cur


def _is_number(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def _build_hash_input(
    key: str,
    salt: str,
    bucket_value: str,
    secondary: str | None = None,
    seed: int | None = None,
) -> str:
    if seed is not None:
        return f"{seed}.{bucket_value}"
    if secondary:
        return f"{key}.{salt}.{bucket_value}.{secondary}"
    return f"{key}.{salt}.{bucket_value}"


def _extract_bucket_value(
    ctx: dict[str, Any],
    context_kind: str,
    bucket_by: str | None = None,
) -> tuple[str, str | None] | None:
    single = get_context_kind(ctx, context_kind or "user")
    if single is None:
        return None
    parts = parse_attribute_ref(bucket_by or "key")
    if not parts:
        return None
    val = _get_attribute(single, parts)
    if val is _MISSING:
        return None
    if isinstance(val, str):
        bucket_str = val
    elif isinstance(val, int) and not isinstance(val, bool):
        bucket_str = str(val)
    elif isinstance(val, float) and math.isfinite(val) and val.is_integer():
        bucket_str = str(int(val))
    else:
        return None
    meta = single.get("_meta")
    sec = meta.get("secondary") if isinstance(meta, dict) else None
    if not isinstance(sec, str) or not sec:
        sec = None
    return (bucket_str, sec)


def _evaluate_rollout(
    flag_key: str,
    salt: str,
    rollout: dict[str, Any],
    ctx: dict[str, Any],
) -> tuple[int, bool] | None:
    variations = rollout.get("variations")
    if not isinstance(variations, list) or not variations:
        return None
    is_experiment = rollout.get("kind") == "experiment"
    effective_bucket_by = None if is_experiment else rollout.get("bucketBy")
    extracted = _extract_bucket_value(ctx, rollout.get("contextKind") or "user", effective_bucket_by)

    bucket = 0
    valid_context = False
    if extracted is not None:
        bval, sec = extracted
        seed_raw = rollout.get("seed")
        seed = int(seed_raw) if _is_number(seed_raw) else None
        h_in = _build_hash_input(
            flag_key,
            salt,
            bval,
            None if is_experiment else sec,
            seed,
        )
        bucket = hash_to_bucket(h_in)
        valid_context = True

    cumulative = 0
    for wv in variations:
        cumulative += int(wv.get("weight", 0))
        if bucket < cumulative:
            return (
                int(wv["variation"]),
                is_experiment and valid_context and not bool(wv.get("untracked")),
            )

    fallback = variations[0] if cumulative == 0 else variations[-1]
    return (
        int(fallback["variation"]),
        is_experiment and valid_context and not bool(fallback.get("untracked")),
    )


def _deep_json_equal(a: Any, b: Any) -> bool:
    if isinstance(a, bool) or isinstance(b, bool):
        return isinstance(a, bool) and isinstance(b, bool) and a == b
    if _is_number(a) and _is_number(b):
        return float(a) == float(b)
    if type(a) is not type(b):
        return False
    if a is None or isinstance(a, str):
        return bool(a == b)
    if isinstance(a, list):
        if len(a) != len(b):
            return False
        return all(_deep_json_equal(x, y) for x, y in zip(a, b))
    if isinstance(a, dict):
        if len(a) != len(b):
            return False
        return all(k in b and _deep_json_equal(a[k], b[k]) for k in a)
    return bool(a == b)


def _parse_timestamp_nanos(v: Any) -> int | None:
    if isinstance(v, str):
        m = RFC3339_RE.match(v)
        if not m:
            return None
        year, month, day = int(m.group(1)), int(m.group(2)), int(m.group(3))
        hour, minute, second = int(m.group(4)), int(m.group(5)), int(m.group(6))
        frac = m.group(7) or ""
        z_flag = m.group(8)
        sign = m.group(9)
        tz_h = int(m.group(10)) if m.group(10) else 0
        tz_m = int(m.group(11)) if m.group(11) else 0
        if tz_h > 23 or tz_m > 59:
            return None
        if z_flag:
            tz = timezone.utc
        else:
            delta = timedelta(hours=tz_h, minutes=tz_m)
            tz = timezone(delta if sign == "+" else -delta)
        try:
            dt = datetime(year, month, day, hour, minute, second, tzinfo=tz)
        except ValueError:
            return None
        diff = dt.astimezone(timezone.utc) - EPOCH_UTC
        secs = diff.days * 86_400 + diff.seconds
        sub_nanos = int(frac[:9].ljust(9, "0")) if frac else 0
        return secs * 1_000_000_000 + sub_nanos
    if _is_number(v):
        return math.trunc(v * 1_000_000)
    return None


@dataclass(frozen=True)
class _ParsedSemVer:
    major: int
    minor: int
    patch: int
    prerelease: tuple[str, ...]


def _valid_semver_idents(parts: list[str], no_leading_zero_numeric: bool) -> bool:
    for p in parts:
        if not p or not SEMVER_IDENT_RE.match(p):
            return False
        if no_leading_zero_numeric and DIGITS_RE.match(p) and len(p) > 1 and p.startswith("0"):
            return False
    return True


def _parse_semver_relaxed(raw: str) -> _ParsedSemVer | None:
    if not raw or raw.startswith(("v", "V")):
        return None
    rest = raw
    plus_idx = rest.find("+")
    if plus_idx >= 0:
        build = rest[plus_idx + 1 :]
        if not build or not _valid_semver_idents(build.split("."), False):
            return None
        rest = rest[:plus_idx]
    prerelease: list[str] = []
    dash_idx = rest.find("-")
    if dash_idx >= 0:
        pre_str = rest[dash_idx + 1 :]
        if not pre_str:
            return None
        prerelease = pre_str.split(".")
        if not _valid_semver_idents(prerelease, True):
            return None
        rest = rest[:dash_idx]
    core = rest.split(".")
    if len(core) < 1 or len(core) > 3:
        return None
    nums = [0, 0, 0]
    for i, part in enumerate(core):
        if not DIGITS_RE.match(part) or (len(part) > 1 and part.startswith("0")):
            return None
        nums[i] = int(part)
    return _ParsedSemVer(nums[0], nums[1], nums[2], tuple(prerelease))


def _compare_semver(a: Any, b: Any) -> int | None:
    if not isinstance(a, str) or not isinstance(b, str):
        return None
    va = _parse_semver_relaxed(a)
    vb = _parse_semver_relaxed(b)
    if va is None or vb is None:
        return None
    for x, y in ((va.major, vb.major), (va.minor, vb.minor), (va.patch, vb.patch)):
        if x != y:
            return -1 if x < y else 1
    if not va.prerelease and not vb.prerelease:
        return 0
    if not va.prerelease:
        return 1
    if not vb.prerelease:
        return -1
    for pa, pb in zip(va.prerelease, vb.prerelease):
        if pa == pb:
            continue
        a_num = bool(DIGITS_RE.match(pa))
        b_num = bool(DIGITS_RE.match(pb))
        if a_num and b_num:
            return -1 if int(pa) < int(pb) else 1
        if a_num:
            return -1
        if b_num:
            return 1
        return -1 if pa < pb else 1
    if len(va.prerelease) == len(vb.prerelease):
        return 0
    return -1 if len(va.prerelease) < len(vb.prerelease) else 1


def _match_single_value(op: str, actual: Any, target: Any) -> bool:
    if op == "in":
        return _deep_json_equal(actual, target)
    if op == "startsWith":
        return isinstance(actual, str) and isinstance(target, str) and actual.startswith(target)
    if op == "endsWith":
        return isinstance(actual, str) and isinstance(target, str) and actual.endswith(target)
    if op == "contains":
        return isinstance(actual, str) and isinstance(target, str) and target in actual
    if op == "matches":
        if not isinstance(actual, str) or not isinstance(target, str):
            return False
        try:
            return re.search(target, actual) is not None
        except re.error:
            return False
    if op == "lessThan":
        return _is_number(actual) and _is_number(target) and actual < target
    if op == "lessThanOrEqual":
        return _is_number(actual) and _is_number(target) and actual <= target
    if op == "greaterThan":
        return _is_number(actual) and _is_number(target) and actual > target
    if op == "greaterThanOrEqual":
        return _is_number(actual) and _is_number(target) and actual >= target
    if op == "before":
        ta = _parse_timestamp_nanos(actual)
        tb = _parse_timestamp_nanos(target)
        return ta is not None and tb is not None and ta < tb
    if op == "after":
        ta = _parse_timestamp_nanos(actual)
        tb = _parse_timestamp_nanos(target)
        return ta is not None and tb is not None and ta > tb
    if op == "semverEqual":
        return _compare_semver(actual, target) == 0
    if op == "semverLessThan":
        return _compare_semver(actual, target) == -1
    if op == "semverGreaterThan":
        return _compare_semver(actual, target) == 1
    return False


def _matches_key_list(ctx: dict[str, Any], kind: str, keys: list[str] | None) -> bool:
    if not keys:
        return False
    sc = get_context_kind(ctx, kind or "user")
    if sc is None:
        return False
    return sc.get("key") in keys


def _evaluate_segment(
    store: EvalStore,
    segment: dict[str, Any],
    ctx: dict[str, Any],
    seg_stack: list[str],
) -> bool:
    seg_key = str(segment.get("key", ""))
    if seg_key in seg_stack:
        raise MalformedFlagError("segment cycle detected")

    if segment.get("unbounded"):
        kind = str(segment.get("unboundedContextKind") or "user")
        sc = get_context_kind(ctx, kind)
        if sc is not None:
            decision = store.big_segment_membership(seg_key, kind, str(sc.get("key", "")))
            if decision is not None:
                return decision

    if _matches_key_list(ctx, "user", segment.get("included")):
        return True
    for t in segment.get("includedContexts") or []:
        if _matches_key_list(ctx, t.get("contextKind") or "user", t.get("values")):
            return True
    if _matches_key_list(ctx, "user", segment.get("excluded")):
        return False
    for t in segment.get("excludedContexts") or []:
        if _matches_key_list(ctx, t.get("contextKind") or "user", t.get("values")):
            return False

    seg_stack.append(seg_key)
    try:
        for rule in segment.get("rules") or []:
            clauses = rule.get("clauses") or []
            if not all(_eval_clause(store, c, ctx, seg_stack) for c in clauses):
                continue
            weight = rule.get("weight")
            if _is_number(weight):
                w_int = int(weight)
                if w_int == 0:
                    continue
                if w_int >= 100_000:
                    return True
                extracted = _extract_bucket_value(
                    ctx,
                    rule.get("rolloutContextKind") or "user",
                    rule.get("bucketBy"),
                )
                if extracted is not None:
                    bval, sec = extracted
                    h_in = _build_hash_input(seg_key, str(segment.get("salt") or ""), bval, sec, None)
                    if hash_to_bucket(h_in) < w_int:
                        return True
            else:
                return True
        return False
    finally:
        seg_stack.pop()


def _eval_clause(
    store: EvalStore,
    clause: dict[str, Any],
    ctx: dict[str, Any],
    seg_stack: list[str],
) -> bool:
    op = str(clause.get("op", ""))
    negate = bool(clause.get("negate", False))
    values = clause.get("values") or []

    if op == "segmentMatch":
        matched = False
        for val in values:
            if not isinstance(val, str):
                continue
            seg = store.get_segment(val)
            if seg is not None and _evaluate_segment(store, seg, ctx, seg_stack):
                matched = True
                break
        return (not matched) if negate else matched

    single = get_context_kind(ctx, clause.get("contextKind") or "user")
    if single is None:
        return False
    parts = parse_attribute_ref(str(clause.get("attribute") or ""))
    if not parts:
        return False
    attr_val = _get_attribute(single, parts)
    if attr_val is _MISSING or attr_val is None:
        return False

    if isinstance(attr_val, list):
        if not attr_val:
            return False
        raw_match = any(
            item is not None and any(_match_single_value(op, item, cv) for cv in values)
            for item in attr_val
        )
    else:
        raw_match = any(_match_single_value(op, attr_val, cv) for cv in values)
    return (not raw_match) if negate else raw_match


def _error_detail(default_value: Any, error_kind: str) -> dict[str, Any]:
    return {
        "value": default_value,
        "reason": {"kind": "ERROR", "errorKind": error_kind},
    }


def _resolve_variation(
    flag: dict[str, Any], index: int, default_value: Any, reason: dict[str, Any]
) -> dict[str, Any]:
    variations = flag.get("variations") or []
    if index < 0 or index >= len(variations):
        return _error_detail(default_value, "MALFORMED_FLAG")
    return {
        "value": variations[index],
        "variationIndex": index,
        "reason": reason,
    }


def _resolve_off_variation(
    flag: dict[str, Any], default_value: Any, reason: dict[str, Any]
) -> dict[str, Any]:
    off_var = flag.get("offVariation")
    if off_var is None:
        return {"value": default_value, "reason": reason}
    return _resolve_variation(flag, int(off_var), default_value, reason)


def _resolve_variation_or_rollout(
    flag: dict[str, Any],
    vor: dict[str, Any] | None,
    ctx: dict[str, Any],
    default_value: Any,
    build_reason: Callable[[bool], dict[str, Any]],
) -> dict[str, Any]:
    if not isinstance(vor, dict):
        return _error_detail(default_value, "MALFORMED_FLAG")
    var_idx = vor.get("variation")
    if isinstance(var_idx, int) and not isinstance(var_idx, bool):
        return _resolve_variation(flag, var_idx, default_value, build_reason(False))
    rollout = vor.get("rollout")
    if isinstance(rollout, dict):
        res = _evaluate_rollout(str(flag.get("key", "")), str(flag.get("salt", "")), rollout, ctx)
        if res is None:
            return _error_detail(default_value, "MALFORMED_FLAG")
        idx, in_exp = res
        return _resolve_variation(flag, idx, default_value, build_reason(in_exp))
    return _error_detail(default_value, "MALFORMED_FLAG")


def _evaluate_flag_internal(
    store: EvalStore,
    flag: dict[str, Any],
    ctx: dict[str, Any],
    default_value: Any,
    prereq_stack: list[str],
) -> dict[str, Any]:
    if not flag.get("on"):
        return _resolve_off_variation(flag, default_value, {"kind": "OFF"})

    flag_key = str(flag.get("key", ""))
    if flag_key in prereq_stack:
        return _error_detail(default_value, "MALFORMED_FLAG")

    prereq_stack.append(flag_key)
    try:
        for pre in flag.get("prerequisites") or []:
            pre_key = str(pre.get("key", ""))
            if pre_key in prereq_stack:
                return _error_detail(default_value, "MALFORMED_FLAG")
            pre_flag = store.get_flag(pre_key)
            if pre_flag is None or not pre_flag.get("on"):
                return _resolve_off_variation(
                    flag,
                    default_value,
                    {"kind": "PREREQUISITE_FAILED", "prerequisiteKey": pre_key},
                )
            pre_detail = _evaluate_flag_internal(store, pre_flag, ctx, None, prereq_stack)
            if (
                pre_detail.get("reason", {}).get("kind") == "ERROR"
                and pre_detail.get("reason", {}).get("errorKind") == "MALFORMED_FLAG"
            ):
                return _error_detail(default_value, "MALFORMED_FLAG")
            if pre_detail.get("variationIndex") != pre.get("variation"):
                return _resolve_off_variation(
                    flag,
                    default_value,
                    {"kind": "PREREQUISITE_FAILED", "prerequisiteKey": pre_key},
                )

        for target in flag.get("targets") or []:
            sc = get_context_kind(ctx, target.get("contextKind") or "user")
            if sc is not None and sc.get("key") in (target.get("values") or []):
                return _resolve_variation(
                    flag, int(target["variation"]), default_value, {"kind": "TARGET_MATCH"}
                )

        for idx, rule in enumerate(flag.get("rules") or []):
            try:
                seg_stack: list[str] = []
                clauses = rule.get("clauses") or []
                if all(_eval_clause(store, c, ctx, seg_stack) for c in clauses):
                    rule_id = rule.get("id")

                    def _rule_reason(in_exp: bool, i: int = idx, rid: Any = rule_id) -> dict[str, Any]:
                        r: dict[str, Any] = {"kind": "RULE_MATCH", "ruleIndex": i}
                        if rid is not None:
                            r["ruleId"] = rid
                        if in_exp:
                            r["inExperiment"] = True
                        return r

                    return _resolve_variation_or_rollout(flag, rule, ctx, default_value, _rule_reason)
            except MalformedFlagError:
                return _error_detail(default_value, "MALFORMED_FLAG")

        def _ft_reason(in_exp: bool) -> dict[str, Any]:
            return {"kind": "FALLTHROUGH", "inExperiment": True} if in_exp else {"kind": "FALLTHROUGH"}

        return _resolve_variation_or_rollout(
            flag, flag.get("fallthrough"), ctx, default_value, _ft_reason
        )
    finally:
        prereq_stack.pop()


def evaluate(
    store: EvalStore,
    flag_key: str,
    ctx: Any,
    default_value: Any,
) -> dict[str, Any]:
    if not validate_context(ctx):
        return _error_detail(default_value, "USER_NOT_SPECIFIED")
    flag = store.get_flag(flag_key)
    if flag is None:
        return _error_detail(default_value, "FLAG_NOT_FOUND")
    return _evaluate_flag_internal(store, flag, ctx, default_value, [])


def bool_variation(
    store: EvalStore, flag_key: str, ctx: Any, default_value: bool
) -> dict[str, Any]:
    d = evaluate(store, flag_key, ctx, default_value)
    if "variationIndex" in d and not isinstance(d["value"], bool):
        return _error_detail(default_value, "WRONG_TYPE")
    return d


def string_variation(
    store: EvalStore, flag_key: str, ctx: Any, default_value: str
) -> dict[str, Any]:
    d = evaluate(store, flag_key, ctx, default_value)
    if "variationIndex" in d and not isinstance(d["value"], str):
        return _error_detail(default_value, "WRONG_TYPE")
    return d


def number_variation(
    store: EvalStore, flag_key: str, ctx: Any, default_value: int | float
) -> dict[str, Any]:
    d = evaluate(store, flag_key, ctx, default_value)
    if "variationIndex" in d and not _is_number(d["value"]):
        return _error_detail(default_value, "WRONG_TYPE")
    return d


def _remove_nested_path(obj: dict[str, Any], parts: list[str]) -> bool:
    if not parts:
        return False
    head = parts[0]
    if len(parts) == 1:
        if head in obj:
            del obj[head]
            return True
        return False
    child = obj.get(head)
    if not isinstance(child, dict):
        return False
    return _remove_nested_path(child, parts[1:])


def _redact_single(
    single: dict[str, Any], cfg: dict[str, Any], include_kind: bool
) -> dict[str, Any]:
    out: dict[str, Any] = {}
    if include_kind:
        out["kind"] = single.get("kind") or "user"
    out["key"] = single.get("key")
    if single.get("anonymous"):
        out["anonymous"] = True

    redacted: set[str] = set()
    if cfg.get("allAttributesPrivate"):
        for k, v in single.items():
            if k in ("kind", "key", "anonymous", "_meta"):
                continue
            if v is not None:
                redacted.add(k)
    else:
        for k, v in single.items():
            if k in ("kind", "key", "anonymous", "_meta"):
                continue
            if v is not None:
                out[k] = copy.deepcopy(v)
        meta = single.get("_meta")
        local_priv = meta.get("privateAttributes") if isinstance(meta, dict) else []
        refs = list(cfg.get("globalPrivateAttributes") or []) + list(local_priv or [])
        for raw_ref in refs:
            if not isinstance(raw_ref, str):
                continue
            parts = parse_attribute_ref(raw_ref)
            if not parts:
                continue
            first = parts[0]
            if first in ("kind", "key", "anonymous", "_meta"):
                continue
            if _remove_nested_path(out, parts):
                redacted.add(raw_ref if raw_ref.startswith("/") else first)

    if redacted:
        out["_meta"] = {"redactedAttributes": sorted(redacted)}
    return out


def redact_context(ctx: dict[str, Any], cfg: dict[str, Any] | None = None) -> dict[str, Any]:
    config = cfg or {}
    if ctx.get("kind") == "multi":
        out: dict[str, Any] = {"kind": "multi"}
        for k, v in ctx.items():
            if k == "kind" or not isinstance(v, dict):
                continue
            merged = {"kind": k}
            merged.update(v)
            out[k] = _redact_single(merged, config, False)
        return out
    return _redact_single(ctx, config, True)
