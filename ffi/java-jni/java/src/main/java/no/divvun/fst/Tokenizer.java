package no.divvun.fst;

public class Tokenizer {
    static {
        String osName = System.getProperty("os.name").toLowerCase();
        String libName;

        if (osName.contains("mac")) {
            libName = "libdivvun_fst_jni.dylib";
        } else if (osName.contains("linux")) {
            libName = "libdivvun_fst_jni.so";
        } else if (osName.contains("windows")) {
            libName = "divvun_fst_jni.dll";
        } else {
            throw new UnsupportedOperationException("Unsupported platform: " + osName);
        }

        String libPath = System.getProperty("divvun.fst.library.path");
        if (libPath != null) {
            System.load(libPath + "/" + libName);
        } else {
            System.loadLibrary("divvun_fst_jni");
        }
    }

    public static WordContext cursorContext(String firstHalf, String secondHalf) {
        long handle = cursorContext0(firstHalf, secondHalf);
        if (handle == 0) {
            throw new RuntimeException("Failed to create cursor context");
        }
        return new WordContext(handle);
    }

    private static native long cursorContext0(String firstHalf, String secondHalf);
    static native void freeContext(long handle);

    static native long getCurrentPtr(long handle);
    static native long getCurrentLen(long handle);
    static native boolean getCurrentIsOwned(long handle);

    static native long getFirstBeforePtr(long handle);
    static native long getFirstBeforeLen(long handle);

    static native long getSecondBeforePtr(long handle);
    static native long getSecondBeforeLen(long handle);

    static native long getFirstAfterPtr(long handle);
    static native long getFirstAfterLen(long handle);

    static native long getSecondAfterPtr(long handle);
    static native long getSecondAfterLen(long handle);
}
