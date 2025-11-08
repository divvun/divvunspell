package no.divvun.fst;

import java.nio.charset.StandardCharsets;

public class RustStr implements CharSequence, Comparable<String> {
    private final long ptr;
    private final int len;
    private String cachedString;

    RustStr(long ptr, long len) {
        this.ptr = ptr;
        this.len = (int) len;
    }

    @Override
    public int length() {
        return toString().length();
    }

    @Override
    public char charAt(int index) {
        return toString().charAt(index);
    }

    @Override
    public CharSequence subSequence(int start, int end) {
        return toString().subSequence(start, end);
    }

    @Override
    public String toString() {
        if (cachedString == null) {
            if (ptr == 0 || len == 0) {
                cachedString = "";
            } else {
                byte[] bytes = copyBytes(ptr, len);
                cachedString = new String(bytes, StandardCharsets.UTF_8);
            }
        }
        return cachedString;
    }

    public boolean isEmpty() {
        return ptr == 0 || len == 0;
    }

    @Override
    public int compareTo(String other) {
        if (ptr == 0 || len == 0) {
            return other.isEmpty() ? 0 : -1;
        }

        byte[] otherBytes = other.getBytes(StandardCharsets.UTF_8);
        int minLen = Math.min(len, otherBytes.length);

        byte[] thisBytes = copyBytes(ptr, minLen);
        for (int i = 0; i < minLen; i++) {
            if (thisBytes[i] != otherBytes[i]) {
                return (thisBytes[i] & 0xFF) - (otherBytes[i] & 0xFF);
            }
        }

        return len - otherBytes.length;
    }

    @Override
    public boolean equals(Object obj) {
        if (this == obj) return true;
        if (obj instanceof String) {
            String other = (String) obj;
            byte[] otherBytes = other.getBytes(StandardCharsets.UTF_8);
            if (len != otherBytes.length) return false;
            if (ptr == 0 || len == 0) return otherBytes.length == 0;

            byte[] thisBytes = copyBytes(ptr, len);
            for (int i = 0; i < len; i++) {
                if (thisBytes[i] != otherBytes[i]) {
                    return false;
                }
            }
            return true;
        }
        if (obj instanceof CharSequence) {
            return toString().equals(obj.toString());
        }
        return false;
    }

    @Override
    public int hashCode() {
        return hashCode(ptr, len);
    }

    private static native byte[] copyBytes(long ptr, int len);
    private static native int hashCode(long ptr, long len);
}
