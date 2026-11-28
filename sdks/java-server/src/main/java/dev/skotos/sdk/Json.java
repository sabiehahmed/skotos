package dev.skotos.sdk;

import java.util.ArrayList;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Zero-dependency RFC 8259 JSON parser, serializer, deep-copy, and deep-equality helper
 * for the Skotos Java Server SDK.
 */
public final class Json {
    private Json() {}

    public static Object parse(String input) {
        Parser p = new Parser(input);
        Object val = p.parseValue();
        p.skipWs();
        if (p.pos != p.len) {
            throw new IllegalArgumentException("Trailing JSON content at index " + p.pos);
        }
        return val;
    }

    @SuppressWarnings("unchecked")
    public static Map<String, Object> parseObject(String input) {
        Object v = parse(input);
        if (!(v instanceof Map<?, ?>)) {
            throw new IllegalArgumentException("Expected JSON object");
        }
        return (Map<String, Object>) v;
    }

    public static String stringify(Object value) {
        StringBuilder sb = new StringBuilder();
        writeJson(value, sb);
        return sb.toString();
    }

    private static void writeJson(Object value, StringBuilder sb) {
        if (value == null) {
            sb.append("null");
        } else if (value instanceof Boolean b) {
            sb.append(b ? "true" : "false");
        } else if (value instanceof Number n) {
            if (n instanceof Double d) {
                if (!Double.isFinite(d)) {
                    sb.append("null");
                } else if (d == Math.rint(d) && Math.abs(d) < 9e15) {
                    sb.append(d.longValue());
                } else {
                    sb.append(d);
                }
            } else {
                sb.append(n);
            }
        } else if (value instanceof String s) {
            writeString(s, sb);
        } else if (value instanceof List<?> list) {
            sb.append('[');
            for (int i = 0; i < list.size(); i++) {
                if (i > 0) sb.append(',');
                writeJson(list.get(i), sb);
            }
            sb.append(']');
        } else if (value instanceof Map<?, ?> map) {
            sb.append('{');
            boolean first = true;
            for (Map.Entry<?, ?> e : map.entrySet()) {
                if (!first) sb.append(',');
                first = false;
                writeString(String.valueOf(e.getKey()), sb);
                sb.append(':');
                writeJson(e.getValue(), sb);
            }
            sb.append('}');
        } else {
            writeString(String.valueOf(value), sb);
        }
    }

    private static void writeString(String s, StringBuilder sb) {
        sb.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"' -> sb.append("\\\"");
                case '\\' -> sb.append("\\\\");
                case '\b' -> sb.append("\\b");
                case '\f' -> sb.append("\\f");
                case '\n' -> sb.append("\\n");
                case '\r' -> sb.append("\\r");
                case '\t' -> sb.append("\\t");
                default -> {
                    if (c < 0x20) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
                }
            }
        }
        sb.append('"');
    }

    public static boolean deepEquals(Object a, Object b) {
        if (a == b) return true;
        if (a == null || b == null) return false;
        if (a instanceof Boolean ba && b instanceof Boolean bb) {
            return ba.booleanValue() == bb.booleanValue();
        }
        if (a instanceof Number na && b instanceof Number nb) {
            return Double.compare(na.doubleValue(), nb.doubleValue()) == 0;
        }
        if (a instanceof String sa && b instanceof String sb) {
            return sa.equals(sb);
        }
        if (a instanceof List<?> la && b instanceof List<?> lb) {
            if (la.size() != lb.size()) return false;
            for (int i = 0; i < la.size(); i++) {
                if (!deepEquals(la.get(i), lb.get(i))) return false;
            }
            return true;
        }
        if (a instanceof Map<?, ?> ma && b instanceof Map<?, ?> mb) {
            if (ma.size() != mb.size()) return false;
            for (Map.Entry<?, ?> entry : ma.entrySet()) {
                if (!mb.containsKey(entry.getKey())) return false;
                if (!deepEquals(entry.getValue(), mb.get(entry.getKey()))) return false;
            }
            return true;
        }
        return false;
    }

    public static Object deepCopy(Object value) {
        if (value instanceof List<?> list) {
            List<Object> out = new ArrayList<>(list.size());
            for (Object item : list) {
                out.add(deepCopy(item));
            }
            return out;
        }
        if (value instanceof Map<?, ?> map) {
            Map<String, Object> out = new LinkedHashMap<>(map.size());
            Iterator<? extends Map.Entry<?, ?>> it = map.entrySet().iterator();
            while (it.hasNext()) {
                Map.Entry<?, ?> e = it.next();
                out.put(String.valueOf(e.getKey()), deepCopy(e.getValue()));
            }
            return out;
        }
        return value;
    }

    private static final class Parser {
        final String src;
        final int len;
        int pos;

        Parser(String src) {
            this.src = src;
            this.len = src.length();
            this.pos = 0;
        }

        void skipWs() {
            while (pos < len) {
                char c = src.charAt(pos);
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
                    pos++;
                } else {
                    break;
                }
            }
        }

        Object parseValue() {
            skipWs();
            if (pos >= len) throw new IllegalArgumentException("Unexpected end of JSON");
            char c = src.charAt(pos);
            return switch (c) {
                case '{' -> parseObject();
                case '[' -> parseArray();
                case '"' -> parseString();
                case 't' -> parseLiteral("true", Boolean.TRUE);
                case 'f' -> parseLiteral("false", Boolean.FALSE);
                case 'n' -> parseLiteral("null", null);
                default -> {
                    if (c == '-' || (c >= '0' && c <= '9')) {
                        yield parseNumber();
                    }
                    throw new IllegalArgumentException("Unexpected char '" + c + "' at " + pos);
                }
            };
        }

        Object parseLiteral(String expected, Object value) {
            if (!src.startsWith(expected, pos)) {
                throw new IllegalArgumentException("Expected " + expected + " at " + pos);
            }
            pos += expected.length();
            return value;
        }

        Map<String, Object> parseObject() {
            pos++; // '{'
            Map<String, Object> map = new LinkedHashMap<>();
            skipWs();
            if (pos < len && src.charAt(pos) == '}') {
                pos++;
                return map;
            }
            while (true) {
                skipWs();
                if (pos >= len || src.charAt(pos) != '"') {
                    throw new IllegalArgumentException("Expected object key at " + pos);
                }
                String key = parseString();
                skipWs();
                if (pos >= len || src.charAt(pos) != ':') {
                    throw new IllegalArgumentException("Expected ':' at " + pos);
                }
                pos++;
                Object val = parseValue();
                map.put(key, val);
                skipWs();
                if (pos >= len) throw new IllegalArgumentException("Unterminated object");
                char sep = src.charAt(pos++);
                if (sep == '}') return map;
                if (sep != ',') throw new IllegalArgumentException("Expected ',' or '}' at " + (pos - 1));
            }
        }

        List<Object> parseArray() {
            pos++; // '['
            List<Object> list = new ArrayList<>();
            skipWs();
            if (pos < len && src.charAt(pos) == ']') {
                pos++;
                return list;
            }
            while (true) {
                list.add(parseValue());
                skipWs();
                if (pos >= len) throw new IllegalArgumentException("Unterminated array");
                char sep = src.charAt(pos++);
                if (sep == ']') return list;
                if (sep != ',') throw new IllegalArgumentException("Expected ',' or ']' at " + (pos - 1));
            }
        }

        String parseString() {
            pos++; // '"'
            StringBuilder sb = new StringBuilder();
            while (pos < len) {
                char c = src.charAt(pos++);
                if (c == '"') return sb.toString();
                if (c == '\\') {
                    if (pos >= len) throw new IllegalArgumentException("Unterminated escape");
                    char esc = src.charAt(pos++);
                    switch (esc) {
                        case '"' -> sb.append('"');
                        case '\\' -> sb.append('\\');
                        case '/' -> sb.append('/');
                        case 'b' -> sb.append('\b');
                        case 'f' -> sb.append('\f');
                        case 'n' -> sb.append('\n');
                        case 'r' -> sb.append('\r');
                        case 't' -> sb.append('\t');
                        case 'u' -> {
                            if (pos + 4 > len) throw new IllegalArgumentException("Invalid unicode escape");
                            int code = Integer.parseInt(src.substring(pos, pos + 4), 16);
                            sb.append((char) code);
                            pos += 4;
                        }
                        default -> throw new IllegalArgumentException("Invalid escape \\" + esc);
                    }
                } else {
                    sb.append(c);
                }
            }
            throw new IllegalArgumentException("Unterminated string");
        }

        Number parseNumber() {
            int start = pos;
            if (src.charAt(pos) == '-') pos++;
            while (pos < len && isDigit(src.charAt(pos))) pos++;
            boolean isFloat = false;
            if (pos < len && src.charAt(pos) == '.') {
                isFloat = true;
                pos++;
                while (pos < len && isDigit(src.charAt(pos))) pos++;
            }
            if (pos < len && (src.charAt(pos) == 'e' || src.charAt(pos) == 'E')) {
                isFloat = true;
                pos++;
                if (pos < len && (src.charAt(pos) == '+' || src.charAt(pos) == '-')) pos++;
                while (pos < len && isDigit(src.charAt(pos))) pos++;
            }
            String token = src.substring(start, pos);
            if (!isFloat) {
                try {
                    return Long.parseLong(token);
                } catch (NumberFormatException ignored) {
                    return Double.parseDouble(token);
                }
            }
            return Double.parseDouble(token);
        }

        private static boolean isDigit(char c) {
            return c >= '0' && c <= '9';
        }
    }
}
