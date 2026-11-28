package dev.skotos.sdk;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.locks.ReentrantReadWriteLock;

/**
 * Thread-safe Skotos Java 21 Server-Side SDK client using only JDK standard library APIs.
 */
public final class SkotosClient {
    public static final class Config {
        public final String sdkKey;
        public final String baseUrl;
        public final String eventsUrl;
        public final boolean offline;
        public final boolean sendEvents;
        public final Duration timeout;

        public Config(String sdkKey, String baseUrl, String eventsUrl, boolean offline, boolean sendEvents) {
            this.sdkKey = sdkKey != null ? sdkKey : "";
            this.baseUrl = baseUrl != null ? baseUrl : "http://localhost:8081";
            this.eventsUrl = eventsUrl;
            this.offline = offline;
            this.sendEvents = sendEvents;
            this.timeout = Duration.ofSeconds(5);
        }

        public static Config offline() {
            return new Config("", "http://localhost:8081", null, true, true);
        }
    }

    private final Config config;
    private final HttpClient http;
    private final ReentrantReadWriteLock lock = new ReentrantReadWriteLock();
    private Evaluator.InMemoryEvalStore store = new Evaluator.InMemoryEvalStore(null, null, null);
    private boolean initialized;
    private long revision;
    private String etag;
    private final List<Map<String, Object>> events = new ArrayList<>();

    public SkotosClient(Config config) {
        this.config = config;
        this.http = HttpClient.newBuilder().connectTimeout(config.timeout).build();
        if (!config.offline && !config.sdkKey.isEmpty()) {
            try {
                refresh();
            } catch (Exception ignored) {
                // Non-fatal init failure
            }
        }
    }

    public static SkotosClient fromSnapshot(Map<String, Object> snapshot) {
        SkotosClient c = new SkotosClient(Config.offline());
        c.applySnapshot(snapshot);
        return c;
    }

    public boolean isInitialized() {
        lock.readLock().lock();
        try {
            return initialized;
        } finally {
            lock.readLock().unlock();
        }
    }

    public long getRevision() {
        lock.readLock().lock();
        try {
            return revision;
        } finally {
            lock.readLock().unlock();
        }
    }

    @SuppressWarnings("unchecked")
    public void applySnapshot(Map<String, Object> snapshot) {
        Map<String, Map<String, Object>> flags = new HashMap<>();
        if (snapshot.get("flags") instanceof Map<?, ?> fMap) {
            for (Map.Entry<?, ?> e : fMap.entrySet()) {
                flags.put(String.valueOf(e.getKey()), (Map<String, Object>) e.getValue());
            }
        }
        Map<String, Map<String, Object>> segments = new HashMap<>();
        if (snapshot.get("segments") instanceof Map<?, ?> sMap) {
            for (Map.Entry<?, ?> e : sMap.entrySet()) {
                segments.put(String.valueOf(e.getKey()), (Map<String, Object>) e.getValue());
            }
        }
        Map<String, Evaluator.BigSegmentSnapshot> bigSegments = new HashMap<>();
        if (snapshot.get("bigSegments") instanceof Map<?, ?> bMap) {
            for (Map.Entry<?, ?> e : bMap.entrySet()) {
                Map<String, Object> raw = (Map<String, Object>) e.getValue();
                String key = String.valueOf(raw.getOrDefault("segmentKey", e.getKey()));
                String kind = String.valueOf(raw.getOrDefault("contextKind", "user"));
                long gen = raw.get("generation") instanceof Number n ? n.longValue() : 1L;
                long[] inc = toUnsignedLongArray(raw.get("includedHashes"));
                long[] exc = toUnsignedLongArray(raw.get("excludedHashes"));
                bigSegments.put(key, new Evaluator.BigSegmentSnapshot(key, kind, gen, inc, exc));
            }
        }
        lock.writeLock().lock();
        try {
            this.store = new Evaluator.InMemoryEvalStore(flags, segments, bigSegments);
            if (snapshot.get("revision") instanceof Number n) {
                this.revision = n.longValue();
            }
            this.initialized = true;
        } finally {
            lock.writeLock().unlock();
        }
    }

    private static long[] toUnsignedLongArray(Object obj) {
        if (!(obj instanceof List<?> list) || list.isEmpty()) return new long[0];
        long[] out = new long[list.size()];
        for (int i = 0; i < list.size(); i++) {
            Object item = list.get(i);
            if (item instanceof Number n) {
                out[i] = n.longValue();
            } else {
                out[i] = Long.parseUnsignedLong(String.valueOf(item));
            }
        }
        return out;
    }

    public boolean refresh() throws IOException, InterruptedException {
        if (config.offline || config.sdkKey.isEmpty()) return false;
        String base = config.baseUrl.replaceAll("/+$", "");
        HttpRequest.Builder req = HttpRequest.newBuilder()
                .uri(URI.create(base + "/sdk/v1/latest-all"))
                .timeout(config.timeout)
                .header("Authorization", config.sdkKey)
                .header("Accept", "application/json")
                .GET();
        if (etag != null) {
            req.header("If-None-Match", etag);
        }
        HttpResponse<String> resp = http.send(req.build(), HttpResponse.BodyHandlers.ofString());
        if (resp.statusCode() == 304) return true;
        if (resp.statusCode() < 200 || resp.statusCode() >= 300) {
            throw new IOException("Snapshot fetch failed: HTTP " + resp.statusCode());
        }
        resp.headers().firstValue("etag").ifPresent(v -> this.etag = v);
        applySnapshot(Json.parseObject(resp.body()));
        return true;
    }

    public Map<String, Object> boolVariationDetail(String flagKey, Map<String, Object> ctx, boolean defaultValue) {
        lock.readLock().lock();
        try {
            if (!initialized) return notReady(defaultValue);
            return Evaluator.boolVariation(store, flagKey, ctx, defaultValue);
        } finally {
            lock.readLock().unlock();
        }
    }

    public boolean boolVariation(String flagKey, Map<String, Object> ctx, boolean defaultValue) {
        return Boolean.TRUE.equals(boolVariationDetail(flagKey, ctx, defaultValue).get("value"));
    }

    public Map<String, Object> stringVariationDetail(String flagKey, Map<String, Object> ctx, String defaultValue) {
        lock.readLock().lock();
        try {
            if (!initialized) return notReady(defaultValue);
            return Evaluator.stringVariation(store, flagKey, ctx, defaultValue);
        } finally {
            lock.readLock().unlock();
        }
    }

    public String stringVariation(String flagKey, Map<String, Object> ctx, String defaultValue) {
        return String.valueOf(stringVariationDetail(flagKey, ctx, defaultValue).get("value"));
    }

    public Map<String, Object> numberVariationDetail(String flagKey, Map<String, Object> ctx, Number defaultValue) {
        lock.readLock().lock();
        try {
            if (!initialized) return notReady(defaultValue);
            return Evaluator.numberVariation(store, flagKey, ctx, defaultValue);
        } finally {
            lock.readLock().unlock();
        }
    }

    public double numberVariation(String flagKey, Map<String, Object> ctx, double defaultValue) {
        return ((Number) numberVariationDetail(flagKey, ctx, defaultValue).get("value")).doubleValue();
    }

    public Map<String, Object> jsonVariationDetail(String flagKey, Map<String, Object> ctx, Object defaultValue) {
        lock.readLock().lock();
        try {
            if (!initialized) return notReady(defaultValue);
            return Evaluator.evaluate(store, flagKey, ctx, defaultValue);
        } finally {
            lock.readLock().unlock();
        }
    }

    public void track(String eventKey, Map<String, Object> ctx, Double metricValue) {
        if (!config.sendEvents) return;
        Map<String, String> contextKeys = new LinkedHashMap<>();
        if (ctx != null) {
            for (Map.Entry<String, Map<String, Object>> entry : Evaluator.getContextKinds(ctx)) {
                Object k = entry.getValue().get("key");
                if (k instanceof String ks && !ks.isEmpty()) {
                    contextKeys.put(entry.getKey(), ks);
                }
            }
        }
        Map<String, Object> ev = new LinkedHashMap<>();
        ev.put("kind", "custom");
        ev.put("creationDate", System.currentTimeMillis());
        ev.put("key", eventKey);
        ev.put("contextKeys", contextKeys);
        ev.put("context", ctx);
        if (metricValue != null) ev.put("metricValue", metricValue);
        lock.writeLock().lock();
        try {
            events.add(ev);
        } finally {
            lock.writeLock().unlock();
        }
    }

    public List<Map<String, Object>> pendingEvents() {
        lock.readLock().lock();
        try {
            return Collections.unmodifiableList(new ArrayList<>(events));
        } finally {
            lock.readLock().unlock();
        }
    }

    public int flush() throws IOException, InterruptedException {
        List<Map<String, Object>> batch;
        lock.writeLock().lock();
        try {
            batch = new ArrayList<>(events);
            events.clear();
        } finally {
            lock.writeLock().unlock();
        }
        if (batch.isEmpty() || config.offline || config.eventsUrl == null || config.sdkKey.isEmpty()) {
            return batch.size();
        }
        String base = config.eventsUrl.replaceAll("/+$", "");
        HttpRequest req = HttpRequest.newBuilder()
                .uri(URI.create(base + "/events/v1/bulk"))
                .timeout(config.timeout)
                .header("Authorization", config.sdkKey)
                .header("Content-Type", "application/json")
                .POST(HttpRequest.BodyPublishers.ofString(Json.stringify(batch)))
                .build();
        http.send(req, HttpResponse.BodyHandlers.discarding());
        return batch.size();
    }

    private static Map<String, Object> notReady(Object defaultValue) {
        Map<String, Object> reason = new LinkedHashMap<>();
        reason.put("kind", "ERROR");
        reason.put("errorKind", "CLIENT_NOT_READY");
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("value", defaultValue);
        out.put("reason", reason);
        return out;
    }
}
