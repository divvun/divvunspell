package no.divvun.fst;

public class Speller implements AutoCloseable {
    private long handle;

    Speller(long handle) {
        this.handle = handle;
    }

    public boolean isCorrect(String word) {
        if (handle == 0) {
            throw new IllegalStateException("Speller is closed");
        }
        if (word == null) {
            throw new NullPointerException("word cannot be null");
        }
        return isCorrect(handle, word);
    }

    public SuggestionList suggest(String word) {
        if (handle == 0) {
            throw new IllegalStateException("Speller is closed");
        }
        if (word == null) {
            throw new NullPointerException("word cannot be null");
        }
        long suggestionsHandle = suggest(handle, word);
        if (suggestionsHandle == 0) {
            throw new RuntimeException("Failed to get suggestions");
        }
        return new SuggestionList(suggestionsHandle);
    }

    @Override
    public void close() {
        if (handle != 0) {
            free(handle);
            handle = 0;
        }
    }

    private static native boolean isCorrect(long handle, String word);
    private static native long suggest(long handle, String word);
    private static native void free(long handle);
}
