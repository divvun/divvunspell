package no.divvun.fst;

import java.util.*;

public class SuggestionList extends AbstractList<Suggestion> implements AutoCloseable {
    private long handle;
    private final int size;
    private final Map<Integer, Suggestion> cache = new HashMap<>();

    SuggestionList(long handle) {
        this.handle = handle;
        this.size = (int) getSize(handle);
    }

    @Override
    public Suggestion get(int index) {
        if (index < 0 || index >= size) {
            throw new IndexOutOfBoundsException("Index: " + index + ", Size: " + size);
        }

        return cache.computeIfAbsent(index, i -> {
            long valuePtr = getValuePtr(handle, i);
            long valueLen = getValueLen(handle, i);
            float weight = getWeight(handle, i);
            byte completedByte = getCompleted(handle, i);

            String value = new RustStr(valuePtr, valueLen).toString();
            Boolean completed = completedByte == 0 ? null : (completedByte == 2);

            return new Suggestion(value, weight, completed);
        });
    }

    @Override
    public int size() {
        return size;
    }

    @Override
    public void close() {
        if (handle != 0) {
            free(handle);
            handle = 0;
            cache.clear();
        }
    }

    private static native long getSize(long handle);
    private static native long getValuePtr(long handle, long index);
    private static native long getValueLen(long handle, long index);
    private static native float getWeight(long handle, long index);
    private static native byte getCompleted(long handle, long index);
    private static native void free(long handle);
}
