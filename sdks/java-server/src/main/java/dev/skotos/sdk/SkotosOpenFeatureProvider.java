package dev.skotos.sdk;

import java.util.LinkedHashMap;
import java.util.Map;

/**
 * OpenFeature-compatible Java provider backed by {@link SkotosClient}.
 */
public final class SkotosOpenFeatureProvider {
    public record ResolutionDetails<T>(T value, String variant, String reason, String errorCode) {}

    private final SkotosClient client;

    public SkotosOpenFeatureProvider(SkotosClient client) {
        this.client = client;
    }

    public String getMetadataName() {
        return "skotos-java-provider";
    }

    private static Map<String, Object> toContext(Map<String, Object> evalCtx) {
        Map<String, Object> out = new LinkedHashMap<>();
        if (evalCtx != null) out.putAll(evalCtx);
        Object tk = out.get("targetingKey");
        Object k = out.get("key");
        String key = (tk instanceof String s && !s.isEmpty())
                ? s
                : (k instanceof String ks && !ks.isEmpty() ? ks : "anonymous");
        Object kindObj = out.get("kind");
        String kind = (kindObj instanceof String s && !s.isEmpty()) ? s : "user";
        out.put("kind", kind);
        out.put("key", key);
        return out;
    }

    @SuppressWarnings("unchecked")
    private static <T> ResolutionDetails<T> mapDetail(Map<String, Object> detail) {
        T val = (T) detail.get("value");
        Map<String, Object> reason = (Map<String, Object>) detail.get("reason");
        String kind = reason != null ? String.valueOf(reason.get("kind")) : "DEFAULT";
        if ("ERROR".equals(kind)) {
            String ek = String.valueOf(reason.get("errorKind"));
            String code = "FLAG_NOT_FOUND".equals(ek)
                    ? "FLAG_NOT_FOUND"
                    : "WRONG_TYPE".equals(ek) ? "TYPE_MISMATCH" : "GENERAL";
            return new ResolutionDetails<>(val, null, "ERROR", code);
        }
        String mapped = ("TARGET_MATCH".equals(kind) || "RULE_MATCH".equals(kind))
                ? "TARGETING_MATCH"
                : "OFF".equals(kind) ? "DISABLED" : "DEFAULT";
        Object idx = detail.get("variationIndex");
        String variant = idx != null ? String.valueOf(idx) : null;
        return new ResolutionDetails<>(val, variant, mapped, null);
    }

    public ResolutionDetails<Boolean> resolveBooleanEvaluation(
            String flagKey, boolean defaultValue, Map<String, Object> evalCtx) {
        return mapDetail(client.boolVariationDetail(flagKey, toContext(evalCtx), defaultValue));
    }

    public ResolutionDetails<String> resolveStringEvaluation(
            String flagKey, String defaultValue, Map<String, Object> evalCtx) {
        return mapDetail(client.stringVariationDetail(flagKey, toContext(evalCtx), defaultValue));
    }

    public ResolutionDetails<Number> resolveNumberEvaluation(
            String flagKey, Number defaultValue, Map<String, Object> evalCtx) {
        return mapDetail(client.numberVariationDetail(flagKey, toContext(evalCtx), defaultValue));
    }

    public ResolutionDetails<Object> resolveObjectEvaluation(
            String flagKey, Object defaultValue, Map<String, Object> evalCtx) {
        return mapDetail(client.jsonVariationDetail(flagKey, toContext(evalCtx), defaultValue));
    }
}
