package no.divvun.fst;

import java.io.IOException;

public class SpellerArchive implements AutoCloseable {
    private long handle;

    private SpellerArchive(long handle) {
        this.handle = handle;
    }

    public static SpellerArchive open(String path) throws IOException {
        long handle = nativeOpen(path);
        if (handle == 0) {
            throw new IOException("Failed to open speller archive");
        }
        return new SpellerArchive(handle);
    }

    public Speller getSpeller() {
        if (handle == 0) {
            throw new IllegalStateException("SpellerArchive is closed");
        }
        long spellerHandle = nativeGetSpeller(handle);
        if (spellerHandle == 0) {
            throw new RuntimeException("Failed to get speller");
        }
        return new Speller(spellerHandle);
    }

    @Override
    public void close() {
        if (handle != 0) {
            nativeFree(handle);
            handle = 0;
        }
    }

    private static native long nativeOpen(String path) throws IOException;
    private static native long nativeGetSpeller(long handle);
    private static native void nativeFree(long handle);
}
