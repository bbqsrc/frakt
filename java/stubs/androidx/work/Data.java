package androidx.work;

import java.util.HashMap;
import java.util.Map;

public class Data {
    private Map<String, Object> values;

    private Data(Map<String, Object> values) {
        this.values = values;
    }

    public String getString(String key) {
        Object value = values.get(key);
        return value != null ? value.toString() : null;
    }

    public int getInt(String key, int defaultValue) {
        Object value = values.get(key);
        if (value instanceof Integer) {
            return (Integer) value;
        }
        return defaultValue;
    }

    public long getLong(String key, long defaultValue) {
        Object value = values.get(key);
        if (value instanceof Long) {
            return (Long) value;
        }
        return defaultValue;
    }

    public static class Builder {
        private Map<String, Object> values = new HashMap<>();

        public Builder putString(String key, String value) {
            values.put(key, value);
            return this;
        }

        public Builder putInt(String key, int value) {
            values.put(key, value);
            return this;
        }

        public Builder putLong(String key, long value) {
            values.put(key, value);
            return this;
        }

        public Data build() {
            return new Data(new HashMap<>(values));
        }
    }
}
