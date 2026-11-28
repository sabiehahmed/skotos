package dev.skotos.sdk;

import java.math.BigDecimal;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.time.Instant;
import java.time.OffsetDateTime;
import java.time.format.DateTimeParseException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import java.util.function.Function;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;

/**
 * Pure-Java canonical Skotos flag evaluation engine (124/124 golden vector parity).
 */
public final class Evaluator {
    private static final BigInteger BUCKET_SCALE = BigInteger.valueOf(100_000L);
    private static final BigInteger TWO_POW_60 = BigInteger.ONE.shiftLeft(60);
    private static final long FNV_OFFSET_64 = 0xcbf29ce484222325L;
    private static final long FNV_PRIME_64 = 0x100000001b3L;
    private static final Pattern VALID_KIND_RE = Pattern.compile("^[a-zA-Z0-9._-]+$");
    private static final Pattern RFC3339_RE = Pattern.compile(
            "^(\\d{4}-\\d{2}-\\d{2})[Tt](\\d{2}:\\d{2}:\\d{2})(?:\\.(\\d+))?(?:([Zz])|([+-]\\d{2}:\\d{2}))$");
    private static final Pattern SEMVER_IDENT_RE = Pattern.compile("^[0-9A-Za-z-]+$");
    private static final Pattern DIGITS_RE = Pattern.compile("^[0-9]+$");
    private static final Object MISSING = new Object();

    private Evaluator() {}

    public interface EvalStore {
        Map<String, Object> getFlag(String key);
        Map<String, Object> getSegment(String key);
        default Boolean bigSegmentMembership(String segmentKey, String contextKind, String contextKey) {
            return null;
        }
    }

    public static final class BigSegmentSnapshot {
        public final String segmentKey;
        public final String contextKind;
        public final long generation;
        public final long[] includedHashes;
        public final long[] excludedHashes;

        public BigSegmentSnapshot(
                String segmentKey,
                String contextKind,
                long generation,
                long[] includedHashes,
                long[] excludedHashes) {
            this.segmentKey = segmentKey;
            this.contextKind = contextKind;
            this.generation = generation;
            this.includedHashes = sortUnsigned(includedHashes);
            this.excludedHashes = sortUnsigned(excludedHashes);
        }

        private static long[] sortUnsigned(long[] arr) {
            if (arr == null || arr.length == 0) return new long[0];
            Long[] boxed = new Long[arr.length];
            for (int i = 0; i < arr.length; i++) boxed[i] = arr[i];
            Arrays.sort(boxed, Long::compareUnsigned);
            long[] out = new long[arr.length];
            for (int i = 0; i < arr.length; i++) out[i] = boxed[i];
            return out;
        }

        public Boolean check(String kind, String key) {
            if (!contextKind.equals(kind)) return null;
            long target = bigSegmentKeyHash(kind, key);
            if (binarySearchUnsigned(includedHashes, target)) return Boolean.TRUE;
            if (binarySearchUnsigned(excludedHashes, target)) return Boolean.FALSE;
            return null;
        }

        private static boolean binarySearchUnsigned(long[] sorted, long target) {
            int lo = 0;
            int hi = sorted.length - 1;
            while (lo <= hi) {
                int mid = (lo + hi) >>> 1;
                int cmp = Long.compareUnsigned(sorted[mid], target);
                if (cmp == 0) return true;
                if (cmp < 0) lo = mid + 1;
                else hi = mid - 1;
            }
            return false;
        }
    }

    public static final class InMemoryEvalStore implements EvalStore {
        private final Map<String, Map<String, Object>> flags;
        private final Map<String, Map<String, Object>> segments;
        private final Map<String, BigSegmentSnapshot> bigSegments;

        public InMemoryEvalStore(
                Map<String, Map<String, Object>> flags,
                Map<String, Map<String, Object>> segments,
                Map<String, BigSegmentSnapshot> bigSegments) {
            this.flags = flags != null ? new HashMap<>(flags) : new HashMap<>();
            this.segments = segments != null ? new HashMap<>(segments) : new HashMap<>();
            this.bigSegments = bigSegments != null ? new HashMap<>(bigSegments) : new HashMap<>();
        }

        @Override
        public Map<String, Object> getFlag(String key) {
            return flags.get(key);
        }

        @Override
        public Map<String, Object> getSegment(String key) {
            return segments.get(key);
        }

        @Override
        public Boolean bigSegmentMembership(String segmentKey, String contextKind, String contextKey) {
            BigSegmentSnapshot snap = bigSegments.get(segmentKey);
            return snap != null ? snap.check(contextKind, contextKey) : null;
        }
    }

    private static final class MalformedFlagException extends RuntimeException {
        private static final long serialVersionUID = 1L;
    }

    public static int hashToBucket(String hashInput) {
        try {
            MessageDigest md = MessageDigest.getInstance("SHA-1");
            byte[] digest = md.digest(hashInput.getBytes(StandardCharsets.UTF_8));
            StringBuilder hex = new StringBuilder(16);
            for (int i = 0; i < 8; i++) {
                hex.append(String.format("%02x", digest[i] & 0xff));
            }
            BigInteger top60 = new BigInteger(hex.substring(0, 15), 16);
            return top60.multiply(BUCKET_SCALE).divide(TWO_POW_60).intValue();
        } catch (NoSuchAlgorithmException e) {
            throw new IllegalStateException("SHA-1 unavailable", e);
        }
    }

    public static long bigSegmentKeyHash(String contextKind, String contextKey) {
        byte[] bytes = (contextKind + ":" + contextKey).getBytes(StandardCharsets.UTF_8);
        long h = FNV_OFFSET_64;
        for (byte b : bytes) {
            h ^= (b & 0xffL);
            h *= FNV_PRIME_64;
        }
        return h;
    }

    public static List<String> parseAttributeRef(String raw) {
        if (raw == null || raw.isEmpty()) return null;
        if (!raw.startsWith("/")) return List.of(raw);
        if (raw.equals("/") || raw.endsWith("/")) return null;
        String[] parts = raw.substring(1).split("/", -1);
        List<String> out = new ArrayList<>(parts.length);
        for (String part : parts) {
            if (part.isEmpty()) return null;
            StringBuilder sb = new StringBuilder(part.length());
            for (int i = 0; i < part.length(); i++) {
                char c = part.charAt(i);
                if (c == '~') {
                    if (i + 1 >= part.length()) return null;
                    char nxt = part.charAt(++i);
                    if (nxt == '0') sb.append('~');
                    else if (nxt == '1') sb.append('/');
                    else return null;
                } else {
                    sb.append(c);
                }
            }
            out.add(sb.toString());
        }
        return out;
    }

    private static boolean isValidKind(Object kind) {
        if (!(kind instanceof String s) || s.isEmpty()) return false;
        return !s.equals("kind") && !s.equals("multi") && VALID_KIND_RE.matcher(s).matches();
    }

    @SuppressWarnings("unchecked")
    public static boolean validateContext(Object ctxObj) {
        if (!(ctxObj instanceof Map<?, ?> raw)) return false;
        Map<String, Object> ctx = (Map<String, Object>) raw;
        if ("multi".equals(ctx.get("kind"))) {
            int count = 0;
            for (Map.Entry<String, Object> e : ctx.entrySet()) {
                if ("kind".equals(e.getKey())) continue;
                count++;
                if (!isValidKind(e.getKey())) return false;
                if (!(e.getValue() instanceof Map<?, ?> subRaw)) return false;
                Map<String, Object> sub = (Map<String, Object>) subRaw;
                Object key = sub.get("key");
                if (!(key instanceof String ks) || ks.isEmpty()) return false;
                Object nestedKind = sub.get("kind");
                if (nestedKind != null && !e.getKey().equals(nestedKind)) return false;
            }
            return count > 0;
        }
        Object kind = ctx.get("kind") != null ? ctx.get("kind") : "user";
        Object key = ctx.get("key");
        return isValidKind(kind) && (key instanceof String ks) && !ks.isEmpty();
    }

    @SuppressWarnings("unchecked")
    public static Map<String, Object> getContextKind(Map<String, Object> ctx, String kind) {
        if ("multi".equals(ctx.get("kind"))) {
            Object nested = ctx.get(kind);
            if (nested instanceof Map<?, ?> sub) {
                Map<String, Object> merged = new LinkedHashMap<>();
                merged.put("kind", kind);
                merged.putAll((Map<String, Object>) sub);
                return merged;
            }
            return null;
        }
        Object resolved = ctx.get("kind") != null ? ctx.get("kind") : "user";
        if (kind.equals(resolved)) {
            Map<String, Object> merged = new LinkedHashMap<>(ctx);
            merged.put("kind", kind);
            return merged;
        }
        return null;
    }

    @SuppressWarnings("unchecked")
    public static List<Map.Entry<String, Map<String, Object>>> getContextKinds(Map<String, Object> ctx) {
        List<Map.Entry<String, Map<String, Object>>> out = new ArrayList<>();
        if ("multi".equals(ctx.get("kind"))) {
            for (Map.Entry<String, Object> e : ctx.entrySet()) {
                if ("kind".equals(e.getKey()) || !(e.getValue() instanceof Map<?, ?> sub)) continue;
                Map<String, Object> merged = new LinkedHashMap<>();
                merged.put("kind", e.getKey());
                merged.putAll((Map<String, Object>) sub);
                out.add(Map.entry(e.getKey(), merged));
            }
            return out;
        }
        String kind = ctx.get("kind") instanceof String s ? s : "user";
        Map<String, Object> merged = new LinkedHashMap<>(ctx);
        merged.put("kind", kind);
        out.add(Map.entry(kind, merged));
        return out;
    }

    @SuppressWarnings("unchecked")
    private static Object getAttribute(Map<String, Object> single, List<String> parts) {
        if (parts.isEmpty()) return MISSING;
        String first = parts.get(0);
        if ("_meta".equals(first)) return MISSING;
        if ("kind".equals(first)) {
            return parts.size() == 1 ? (single.get("kind") != null ? single.get("kind") : "user") : MISSING;
        }
        if ("key".equals(first)) {
            return parts.size() == 1 ? single.get("key") : MISSING;
        }
        if ("anonymous".equals(first)) {
            return parts.size() == 1 ? Boolean.TRUE.equals(single.get("anonymous")) : MISSING;
        }
        if (!single.containsKey(first)) return MISSING;
        Object cur = single.get(first);
        for (int i = 1; i < parts.size(); i++) {
            if (!(cur instanceof Map<?, ?> curMap)) return MISSING;
            Map<String, Object> m = (Map<String, Object>) curMap;
            String seg = parts.get(i);
            if (!m.containsKey(seg)) return MISSING;
            cur = m.get(seg);
        }
        return cur;
    }

    private static String buildHashInput(String key, String salt, String bucketVal, String secondary, Long seed) {
        if (seed != null) {
            return seed + "." + bucketVal;
        }
        if (secondary != null && !secondary.isEmpty()) {
            return key + "." + salt + "." + bucketVal + "." + secondary;
        }
        return key + "." + salt + "." + bucketVal;
    }

    private record ExtractedBucket(String value, String secondary) {}

    @SuppressWarnings("unchecked")
    private static ExtractedBucket extractBucketValue(Map<String, Object> ctx, String contextKind, String bucketBy) {
        Map<String, Object> single = getContextKind(ctx, (contextKind == null || contextKind.isEmpty()) ? "user" : contextKind);
        if (single == null) return null;
        List<String> parts = parseAttributeRef((bucketBy == null || bucketBy.isEmpty()) ? "key" : bucketBy);
        if (parts == null) return null;
        Object val = getAttribute(single, parts);
        if (val == MISSING || val == null) return null;
        String bucketStr;
        if (val instanceof String s) {
            bucketStr = s;
        } else if (val instanceof Long l) {
            bucketStr = Long.toString(l);
        } else if (val instanceof Integer i) {
            bucketStr = Integer.toString(i);
        } else if (val instanceof Double d && Double.isFinite(d) && d == Math.rint(d)) {
            bucketStr = Long.toString(d.longValue());
        } else {
            return null;
        }
        String sec = null;
        Object metaObj = single.get("_meta");
        if (metaObj instanceof Map<?, ?> meta) {
            Object secObj = ((Map<String, Object>) meta).get("secondary");
            if (secObj instanceof String s && !s.isEmpty()) sec = s;
        }
        return new ExtractedBucket(bucketStr, sec);
    }

    private record RolloutResult(int variationIndex, boolean inExperiment) {}

    @SuppressWarnings("unchecked")
    private static RolloutResult evaluateRollout(
            String flagKey, String salt, Map<String, Object> rollout, Map<String, Object> ctx) {
        Object varsObj = rollout.get("variations");
        if (!(varsObj instanceof List<?> vars) || vars.isEmpty()) return null;
        boolean isExperiment = "experiment".equals(rollout.get("kind"));
        String bucketBy = isExperiment ? null : asString(rollout.get("bucketBy"));
        String ctxKind = asString(rollout.get("contextKind"));
        ExtractedBucket extracted = extractBucketValue(ctx, ctxKind != null ? ctxKind : "user", bucketBy);

        int bucket = 0;
        boolean validContext = false;
        if (extracted != null) {
            Long seed = rollout.get("seed") instanceof Number n ? n.longValue() : null;
            String input = buildHashInput(
                    flagKey,
                    salt != null ? salt : "",
                    extracted.value,
                    isExperiment ? null : extracted.secondary,
                    seed);
            bucket = hashToBucket(input);
            validContext = true;
        }

        int cumulative = 0;
        for (Object item : vars) {
            Map<String, Object> wv = (Map<String, Object>) item;
            int weight = wv.get("weight") instanceof Number n ? n.intValue() : 0;
            cumulative += weight;
            if (bucket < cumulative) {
                int variation = ((Number) wv.get("variation")).intValue();
                boolean untracked = Boolean.TRUE.equals(wv.get("untracked"));
                return new RolloutResult(variation, isExperiment && validContext && !untracked);
            }
        }
        Map<String, Object> fallback = (Map<String, Object>) (cumulative == 0 ? vars.get(0) : vars.get(vars.size() - 1));
        int variation = ((Number) fallback.get("variation")).intValue();
        boolean untracked = Boolean.TRUE.equals(fallback.get("untracked"));
        return new RolloutResult(variation, isExperiment && validContext && !untracked);
    }

    private static BigInteger parseTimestampNanos(Object v) {
        if (v instanceof String s) {
            Matcher m = RFC3339_RE.matcher(s);
            if (!m.matches()) return null;
            String date = m.group(1);
            String time = m.group(2);
            String frac = m.group(3);
            String zFlag = m.group(4);
            String offset = zFlag != null ? "Z" : m.group(5);
            StringBuilder norm = new StringBuilder();
            norm.append(date).append('T').append(time);
            if (frac != null && !frac.isEmpty()) {
                String truncated = frac.length() > 9 ? frac.substring(0, 9) : frac;
                norm.append('.').append(truncated);
            }
            norm.append(offset);
            try {
                OffsetDateTime odt = OffsetDateTime.parse(norm.toString());
                Instant inst = odt.toInstant();
                return BigInteger.valueOf(inst.getEpochSecond())
                        .multiply(BigInteger.valueOf(1_000_000_000L))
                        .add(BigInteger.valueOf(inst.getNano()));
            } catch (DateTimeParseException e) {
                return null;
            }
        }
        if (v instanceof Number n) {
            double d = n.doubleValue();
            if (!Double.isFinite(d)) return null;
            if (n instanceof Long l) {
                return BigInteger.valueOf(l).multiply(BigInteger.valueOf(1_000_000L));
            }
            return BigDecimal.valueOf(d).multiply(BigDecimal.valueOf(1_000_000L)).toBigInteger();
        }
        return null;
    }

    private record ParsedSemVer(BigInteger major, BigInteger minor, BigInteger patch, List<String> prerelease) {}

    private static boolean validSemVerIdents(String[] parts, boolean noLeadingZeroNumeric) {
        for (String p : parts) {
            if (p.isEmpty() || !SEMVER_IDENT_RE.matcher(p).matches()) return false;
            if (noLeadingZeroNumeric && DIGITS_RE.matcher(p).matches() && p.length() > 1 && p.startsWith("0")) {
                return false;
            }
        }
        return true;
    }

    private static ParsedSemVer parseSemVerRelaxed(String input) {
        if (input == null || input.isEmpty() || input.startsWith("v") || input.startsWith("V")) {
            return null;
        }
        String rest = input;
        int plusIdx = rest.indexOf('+');
        if (plusIdx >= 0) {
            String build = rest.substring(plusIdx + 1);
            if (build.isEmpty() || !validSemVerIdents(build.split("\\.", -1), false)) return null;
            rest = rest.substring(0, plusIdx);
        }
        List<String> prerelease = Collections.emptyList();
        int dashIdx = rest.indexOf('-');
        if (dashIdx >= 0) {
            String preStr = rest.substring(dashIdx + 1);
            if (preStr.isEmpty()) return null;
            String[] preParts = preStr.split("\\.", -1);
            if (!validSemVerIdents(preParts, true)) return null;
            prerelease = Arrays.asList(preParts);
            rest = rest.substring(0, dashIdx);
        }
        String[] core = rest.split("\\.", -1);
        if (core.length < 1 || core.length > 3) return null;
        BigInteger[] nums = {BigInteger.ZERO, BigInteger.ZERO, BigInteger.ZERO};
        for (int i = 0; i < core.length; i++) {
            String part = core[i];
            if (!DIGITS_RE.matcher(part).matches() || (part.length() > 1 && part.startsWith("0"))) {
                return null;
            }
            nums[i] = new BigInteger(part);
        }
        return new ParsedSemVer(nums[0], nums[1], nums[2], prerelease);
    }

    private static Integer compareSemVer(Object a, Object b) {
        if (!(a instanceof String sa) || !(b instanceof String sb)) return null;
        ParsedSemVer va = parseSemVerRelaxed(sa);
        ParsedSemVer vb = parseSemVerRelaxed(sb);
        if (va == null || vb == null) return null;
        int c = va.major.compareTo(vb.major);
        if (c != 0) return Integer.signum(c);
        c = va.minor.compareTo(vb.minor);
        if (c != 0) return Integer.signum(c);
        c = va.patch.compareTo(vb.patch);
        if (c != 0) return Integer.signum(c);
        if (va.prerelease.isEmpty() && vb.prerelease.isEmpty()) return 0;
        if (va.prerelease.isEmpty()) return 1;
        if (vb.prerelease.isEmpty()) return -1;
        int min = Math.min(va.prerelease.size(), vb.prerelease.size());
        for (int i = 0; i < min; i++) {
            String pa = va.prerelease.get(i);
            String pb = vb.prerelease.get(i);
            if (pa.equals(pb)) continue;
            boolean aNum = DIGITS_RE.matcher(pa).matches();
            boolean bNum = DIGITS_RE.matcher(pb).matches();
            if (aNum && bNum) return Integer.signum(new BigInteger(pa).compareTo(new BigInteger(pb)));
            if (aNum) return -1;
            if (bNum) return 1;
            return Integer.signum(pa.compareTo(pb));
        }
        return Integer.signum(Integer.compare(va.prerelease.size(), vb.prerelease.size()));
    }

    private static boolean matchSingleValue(String op, Object actual, Object target) {
        return switch (op) {
            case "in" -> Json.deepEquals(actual, target);
            case "startsWith" -> actual instanceof String sa && target instanceof String st && sa.startsWith(st);
            case "endsWith" -> actual instanceof String sa && target instanceof String st && sa.endsWith(st);
            case "contains" -> actual instanceof String sa && target instanceof String st && sa.contains(st);
            case "matches" -> {
                if (!(actual instanceof String sa) || !(target instanceof String st)) yield false;
                try {
                    yield Pattern.compile(st).matcher(sa).find();
                } catch (PatternSyntaxException e) {
                    yield false;
                }
            }
            case "lessThan" -> actual instanceof Number na && target instanceof Number nb
                    && na.doubleValue() < nb.doubleValue();
            case "lessThanOrEqual" -> actual instanceof Number na && target instanceof Number nb
                    && na.doubleValue() <= nb.doubleValue();
            case "greaterThan" -> actual instanceof Number na && target instanceof Number nb
                    && na.doubleValue() > nb.doubleValue();
            case "greaterThanOrEqual" -> actual instanceof Number na && target instanceof Number nb
                    && na.doubleValue() >= nb.doubleValue();
            case "before" -> {
                BigInteger ta = parseTimestampNanos(actual);
                BigInteger tb = parseTimestampNanos(target);
                yield ta != null && tb != null && ta.compareTo(tb) < 0;
            }
            case "after" -> {
                BigInteger ta = parseTimestampNanos(actual);
                BigInteger tb = parseTimestampNanos(target);
                yield ta != null && tb != null && ta.compareTo(tb) > 0;
            }
            case "semverEqual" -> Integer.valueOf(0).equals(compareSemVer(actual, target));
            case "semverLessThan" -> Integer.valueOf(-1).equals(compareSemVer(actual, target));
            case "semverGreaterThan" -> Integer.valueOf(1).equals(compareSemVer(actual, target));
            default -> false;
        };
    }

    private static boolean matchesKeyList(Map<String, Object> ctx, String kind, List<?> keys) {
        if (keys == null || keys.isEmpty()) return false;
        Map<String, Object> sc = getContextKind(ctx, (kind == null || kind.isEmpty()) ? "user" : kind);
        if (sc == null) return false;
        Object k = sc.get("key");
        return keys.contains(k);
    }

    @SuppressWarnings("unchecked")
    private static boolean evaluateSegment(
            EvalStore store, Map<String, Object> segment, Map<String, Object> ctx, List<String> segStack) {
        String segKey = String.valueOf(segment.getOrDefault("key", ""));
        if (segStack.contains(segKey)) {
            throw new MalformedFlagException();
        }

        if (Boolean.TRUE.equals(segment.get("unbounded"))) {
            String kind = asString(segment.get("unboundedContextKind"));
            if (kind == null) kind = "user";
            Map<String, Object> sc = getContextKind(ctx, kind);
            if (sc != null) {
                Boolean decision = store.bigSegmentMembership(segKey, kind, String.valueOf(sc.get("key")));
                if (decision != null) return decision;
            }
        }

        if (matchesKeyList(ctx, "user", asList(segment.get("included")))) return true;
        for (Object item : nonNullList(segment.get("includedContexts"))) {
            Map<String, Object> t = (Map<String, Object>) item;
            if (matchesKeyList(ctx, asString(t.get("contextKind")), asList(t.get("values")))) return true;
        }
        if (matchesKeyList(ctx, "user", asList(segment.get("excluded")))) return false;
        for (Object item : nonNullList(segment.get("excludedContexts"))) {
            Map<String, Object> t = (Map<String, Object>) item;
            if (matchesKeyList(ctx, asString(t.get("contextKind")), asList(t.get("values")))) return false;
        }

        segStack.add(segKey);
        try {
            for (Object ruleObj : nonNullList(segment.get("rules"))) {
                Map<String, Object> rule = (Map<String, Object>) ruleObj;
                boolean allMatch = true;
                for (Object cObj : nonNullList(rule.get("clauses"))) {
                    if (!evalClause(store, (Map<String, Object>) cObj, ctx, segStack)) {
                        allMatch = false;
                        break;
                    }
                }
                if (!allMatch) continue;
                if (rule.get("weight") instanceof Number wNum) {
                    int w = wNum.intValue();
                    if (w == 0) continue;
                    if (w >= 100_000) return true;
                    String rKind = asString(rule.get("rolloutContextKind"));
                    ExtractedBucket extracted = extractBucketValue(
                            ctx, rKind != null ? rKind : "user", asString(rule.get("bucketBy")));
                    if (extracted != null) {
                        String salt = asString(segment.get("salt"));
                        String input = buildHashInput(
                                segKey, salt != null ? salt : "", extracted.value, extracted.secondary, null);
                        if (hashToBucket(input) < w) return true;
                    }
                } else {
                    return true;
                }
            }
            return false;
        } finally {
            segStack.remove(segStack.size() - 1);
        }
    }

    private static boolean evalClause(
            EvalStore store, Map<String, Object> clause, Map<String, Object> ctx, List<String> segStack) {
        String op = String.valueOf(clause.getOrDefault("op", ""));
        boolean negate = Boolean.TRUE.equals(clause.get("negate"));
        List<?> values = nonNullList(clause.get("values"));

        if ("segmentMatch".equals(op)) {
            boolean matched = false;
            for (Object v : values) {
                if (!(v instanceof String segKey)) continue;
                Map<String, Object> seg = store.getSegment(segKey);
                if (seg != null && evaluateSegment(store, seg, ctx, segStack)) {
                    matched = true;
                    break;
                }
            }
            return negate ? !matched : matched;
        }

        String cKind = asString(clause.get("contextKind"));
        Map<String, Object> single = getContextKind(ctx, cKind != null ? cKind : "user");
        if (single == null) return false;
        List<String> parts = parseAttributeRef(asString(clause.get("attribute")));
        if (parts == null) return false;
        Object attrVal = getAttribute(single, parts);
        if (attrVal == MISSING || attrVal == null) return false;

        boolean rawMatch = false;
        if (attrVal instanceof List<?> list) {
            if (list.isEmpty()) return false;
            outer:
            for (Object item : list) {
                if (item == null) continue;
                for (Object cv : values) {
                    if (matchSingleValue(op, item, cv)) {
                        rawMatch = true;
                        break outer;
                    }
                }
            }
        } else {
            for (Object cv : values) {
                if (matchSingleValue(op, attrVal, cv)) {
                    rawMatch = true;
                    break;
                }
            }
        }
        return negate ? !rawMatch : rawMatch;
    }

    private static Map<String, Object> errorDetail(Object defaultValue, String errorKind) {
        Map<String, Object> reason = new LinkedHashMap<>();
        reason.put("kind", "ERROR");
        reason.put("errorKind", errorKind);
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("value", defaultValue);
        out.put("reason", reason);
        return out;
    }

    private static Map<String, Object> resolveVariation(
            Map<String, Object> flag, int index, Object defaultValue, Map<String, Object> reason) {
        List<?> vars = nonNullList(flag.get("variations"));
        if (index < 0 || index >= vars.size()) {
            return errorDetail(defaultValue, "MALFORMED_FLAG");
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("value", vars.get(index));
        out.put("variationIndex", index);
        out.put("reason", reason);
        return out;
    }

    private static Map<String, Object> resolveOffVariation(
            Map<String, Object> flag, Object defaultValue, Map<String, Object> reason) {
        Object offVar = flag.get("offVariation");
        if (!(offVar instanceof Number n)) {
            Map<String, Object> out = new LinkedHashMap<>();
            out.put("value", defaultValue);
            out.put("reason", reason);
            return out;
        }
        return resolveVariation(flag, n.intValue(), defaultValue, reason);
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> resolveVariationOrRollout(
            Map<String, Object> flag,
            Object vorObj,
            Map<String, Object> ctx,
            Object defaultValue,
            Function<Boolean, Map<String, Object>> buildReason) {
        if (!(vorObj instanceof Map<?, ?> vorRaw)) {
            return errorDetail(defaultValue, "MALFORMED_FLAG");
        }
        Map<String, Object> vor = (Map<String, Object>) vorRaw;
        if (vor.get("variation") instanceof Number n) {
            return resolveVariation(flag, n.intValue(), defaultValue, buildReason.apply(false));
        }
        if (vor.get("rollout") instanceof Map<?, ?> rolloutRaw) {
            RolloutResult res = evaluateRollout(
                    String.valueOf(flag.getOrDefault("key", "")),
                    String.valueOf(flag.getOrDefault("salt", "")),
                    (Map<String, Object>) rolloutRaw,
                    ctx);
            if (res == null) return errorDetail(defaultValue, "MALFORMED_FLAG");
            return resolveVariation(flag, res.variationIndex, defaultValue, buildReason.apply(res.inExperiment));
        }
        return errorDetail(defaultValue, "MALFORMED_FLAG");
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> evaluateFlagInternal(
            EvalStore store,
            Map<String, Object> flag,
            Map<String, Object> ctx,
            Object defaultValue,
            List<String> prereqStack) {
        if (!Boolean.TRUE.equals(flag.get("on"))) {
            Map<String, Object> reason = new LinkedHashMap<>();
            reason.put("kind", "OFF");
            return resolveOffVariation(flag, defaultValue, reason);
        }
        String flagKey = String.valueOf(flag.getOrDefault("key", ""));
        if (prereqStack.contains(flagKey)) {
            return errorDetail(defaultValue, "MALFORMED_FLAG");
        }
        prereqStack.add(flagKey);
        try {
            for (Object preObj : nonNullList(flag.get("prerequisites"))) {
                Map<String, Object> pre = (Map<String, Object>) preObj;
                String preKey = String.valueOf(pre.getOrDefault("key", ""));
                if (prereqStack.contains(preKey)) {
                    return errorDetail(defaultValue, "MALFORMED_FLAG");
                }
                Map<String, Object> preFlag = store.getFlag(preKey);
                if (preFlag == null || !Boolean.TRUE.equals(preFlag.get("on"))) {
                    Map<String, Object> reason = new LinkedHashMap<>();
                    reason.put("kind", "PREREQUISITE_FAILED");
                    reason.put("prerequisiteKey", preKey);
                    return resolveOffVariation(flag, defaultValue, reason);
                }
                Map<String, Object> preDetail = evaluateFlagInternal(store, preFlag, ctx, null, prereqStack);
                Map<String, Object> preReason = (Map<String, Object>) preDetail.get("reason");
                if (preReason != null
                        && "ERROR".equals(preReason.get("kind"))
                        && "MALFORMED_FLAG".equals(preReason.get("errorKind"))) {
                    return errorDetail(defaultValue, "MALFORMED_FLAG");
                }
                Object preIdx = preDetail.get("variationIndex");
                int want = ((Number) pre.get("variation")).intValue();
                if (!(preIdx instanceof Number n) || n.intValue() != want) {
                    Map<String, Object> reason = new LinkedHashMap<>();
                    reason.put("kind", "PREREQUISITE_FAILED");
                    reason.put("prerequisiteKey", preKey);
                    return resolveOffVariation(flag, defaultValue, reason);
                }
            }

            for (Object tObj : nonNullList(flag.get("targets"))) {
                Map<String, Object> target = (Map<String, Object>) tObj;
                String tKind = asString(target.get("contextKind"));
                Map<String, Object> sc = getContextKind(ctx, tKind != null ? tKind : "user");
                if (sc != null && nonNullList(target.get("values")).contains(sc.get("key"))) {
                    Map<String, Object> reason = new LinkedHashMap<>();
                    reason.put("kind", "TARGET_MATCH");
                    return resolveVariation(
                            flag, ((Number) target.get("variation")).intValue(), defaultValue, reason);
                }
            }

            List<?> rules = nonNullList(flag.get("rules"));
            for (int i = 0; i < rules.size(); i++) {
                Map<String, Object> rule = (Map<String, Object>) rules.get(i);
                try {
                    List<String> segStack = new ArrayList<>();
                    boolean matched = true;
                    for (Object cObj : nonNullList(rule.get("clauses"))) {
                        if (!evalClause(store, (Map<String, Object>) cObj, ctx, segStack)) {
                            matched = false;
                            break;
                        }
                    }
                    if (matched) {
                        final int ruleIdx = i;
                        final Object ruleId = rule.get("id");
                        return resolveVariationOrRollout(flag, rule, ctx, defaultValue, (inExp) -> {
                            Map<String, Object> reason = new LinkedHashMap<>();
                            reason.put("kind", "RULE_MATCH");
                            reason.put("ruleIndex", ruleIdx);
                            if (ruleId != null) reason.put("ruleId", ruleId);
                            if (inExp) reason.put("inExperiment", true);
                            return reason;
                        });
                    }
                } catch (MalformedFlagException e) {
                    return errorDetail(defaultValue, "MALFORMED_FLAG");
                }
            }

            return resolveVariationOrRollout(flag, flag.get("fallthrough"), ctx, defaultValue, (inExp) -> {
                Map<String, Object> reason = new LinkedHashMap<>();
                reason.put("kind", "FALLTHROUGH");
                if (inExp) reason.put("inExperiment", true);
                return reason;
            });
        } finally {
            prereqStack.remove(prereqStack.size() - 1);
        }
    }

    @SuppressWarnings("unchecked")
    public static Map<String, Object> evaluate(EvalStore store, String flagKey, Object ctxObj, Object defaultValue) {
        if (!validateContext(ctxObj)) {
            return errorDetail(defaultValue, "USER_NOT_SPECIFIED");
        }
        Map<String, Object> flag = store.getFlag(flagKey);
        if (flag == null) {
            return errorDetail(defaultValue, "FLAG_NOT_FOUND");
        }
        return evaluateFlagInternal(store, flag, (Map<String, Object>) ctxObj, defaultValue, new ArrayList<>());
    }

    public static Map<String, Object> boolVariation(
            EvalStore store, String flagKey, Object ctx, boolean defaultValue) {
        Map<String, Object> d = evaluate(store, flagKey, ctx, defaultValue);
        if (d.containsKey("variationIndex") && !(d.get("value") instanceof Boolean)) {
            return errorDetail(defaultValue, "WRONG_TYPE");
        }
        return d;
    }

    public static Map<String, Object> stringVariation(
            EvalStore store, String flagKey, Object ctx, String defaultValue) {
        Map<String, Object> d = evaluate(store, flagKey, ctx, defaultValue);
        if (d.containsKey("variationIndex") && !(d.get("value") instanceof String)) {
            return errorDetail(defaultValue, "WRONG_TYPE");
        }
        return d;
    }

    public static Map<String, Object> numberVariation(
            EvalStore store, String flagKey, Object ctx, Number defaultValue) {
        Map<String, Object> d = evaluate(store, flagKey, ctx, defaultValue);
        if (d.containsKey("variationIndex") && !(d.get("value") instanceof Number)) {
            return errorDetail(defaultValue, "WRONG_TYPE");
        }
        return d;
    }

    @SuppressWarnings("unchecked")
    private static boolean removeNestedPath(Map<String, Object> obj, List<String> parts, int idx) {
        String head = parts.get(idx);
        if (idx == parts.size() - 1) {
            if (obj.containsKey(head)) {
                obj.remove(head);
                return true;
            }
            return false;
        }
        Object child = obj.get(head);
        if (!(child instanceof Map<?, ?> childMap)) return false;
        return removeNestedPath((Map<String, Object>) childMap, parts, idx + 1);
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> redactSingle(
            Map<String, Object> single, Map<String, Object> cfg, boolean includeKind) {
        Map<String, Object> out = new LinkedHashMap<>();
        if (includeKind) {
            out.put("kind", single.get("kind") != null ? single.get("kind") : "user");
        }
        out.put("key", single.get("key"));
        if (Boolean.TRUE.equals(single.get("anonymous"))) {
            out.put("anonymous", true);
        }

        Set<String> redacted = new TreeSet<>();
        if (Boolean.TRUE.equals(cfg.get("allAttributesPrivate"))) {
            for (Map.Entry<String, Object> e : single.entrySet()) {
                String k = e.getKey();
                if (isReservedAttr(k)) continue;
                if (e.getValue() != null) redacted.add(k);
            }
        } else {
            for (Map.Entry<String, Object> e : single.entrySet()) {
                String k = e.getKey();
                if (isReservedAttr(k)) continue;
                if (e.getValue() != null) out.put(k, Json.deepCopy(e.getValue()));
            }
            List<Object> refs = new ArrayList<>();
            refs.addAll(nonNullList(cfg.get("globalPrivateAttributes")));
            if (single.get("_meta") instanceof Map<?, ?> meta) {
                refs.addAll(nonNullList(((Map<String, Object>) meta).get("privateAttributes")));
            }
            for (Object refObj : refs) {
                if (!(refObj instanceof String rawRef)) continue;
                List<String> parts = parseAttributeRef(rawRef);
                if (parts == null || parts.isEmpty()) continue;
                String first = parts.get(0);
                if (isReservedAttr(first)) continue;
                if (removeNestedPath(out, parts, 0)) {
                    redacted.add(rawRef.startsWith("/") ? rawRef : first);
                }
            }
        }

        if (!redacted.isEmpty()) {
            Map<String, Object> metaOut = new LinkedHashMap<>();
            metaOut.put("redactedAttributes", new ArrayList<>(redacted));
            out.put("_meta", metaOut);
        }
        return out;
    }

    private static boolean isReservedAttr(String k) {
        return "kind".equals(k) || "key".equals(k) || "anonymous".equals(k) || "_meta".equals(k);
    }

    @SuppressWarnings("unchecked")
    public static Map<String, Object> redactContext(Map<String, Object> ctx, Map<String, Object> cfg) {
        Map<String, Object> effectiveCfg = cfg != null ? cfg : Collections.emptyMap();
        if ("multi".equals(ctx.get("kind"))) {
            Map<String, Object> out = new LinkedHashMap<>();
            out.put("kind", "multi");
            for (Map.Entry<String, Object> e : ctx.entrySet()) {
                if ("kind".equals(e.getKey()) || !(e.getValue() instanceof Map<?, ?> sub)) continue;
                Map<String, Object> merged = new LinkedHashMap<>();
                merged.put("kind", e.getKey());
                merged.putAll((Map<String, Object>) sub);
                out.put(e.getKey(), redactSingle(merged, effectiveCfg, false));
            }
            return out;
        }
        return redactSingle(ctx, effectiveCfg, true);
    }

    private static String asString(Object o) {
        return o instanceof String s ? s : null;
    }

    private static List<?> asList(Object o) {
        return o instanceof List<?> l ? l : null;
    }

    private static List<?> nonNullList(Object o) {
        return o instanceof List<?> l ? l : Collections.emptyList();
    }
}
