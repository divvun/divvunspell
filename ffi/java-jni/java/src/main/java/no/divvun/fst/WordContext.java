package no.divvun.fst;

public class WordContext implements AutoCloseable {
    private long handle;
    private RustStr current;
    private RustStr firstBefore;
    private RustStr secondBefore;
    private RustStr firstAfter;
    private RustStr secondAfter;

    WordContext(long handle) {
        this.handle = handle;

        long currentPtr = Tokenizer.getCurrentPtr(handle);
        long currentLen = Tokenizer.getCurrentLen(handle);
        this.current = new RustStr(currentPtr, currentLen);

        long firstBeforePtr = Tokenizer.getFirstBeforePtr(handle);
        long firstBeforeLen = Tokenizer.getFirstBeforeLen(handle);
        this.firstBefore = new RustStr(firstBeforePtr, firstBeforeLen);

        long secondBeforePtr = Tokenizer.getSecondBeforePtr(handle);
        long secondBeforeLen = Tokenizer.getSecondBeforeLen(handle);
        this.secondBefore = new RustStr(secondBeforePtr, secondBeforeLen);

        long firstAfterPtr = Tokenizer.getFirstAfterPtr(handle);
        long firstAfterLen = Tokenizer.getFirstAfterLen(handle);
        this.firstAfter = new RustStr(firstAfterPtr, firstAfterLen);

        long secondAfterPtr = Tokenizer.getSecondAfterPtr(handle);
        long secondAfterLen = Tokenizer.getSecondAfterLen(handle);
        this.secondAfter = new RustStr(secondAfterPtr, secondAfterLen);
    }

    public RustStr getCurrent() {
        return current;
    }

    public RustStr getFirstBefore() {
        return firstBefore;
    }

    public RustStr getSecondBefore() {
        return secondBefore;
    }

    public RustStr getFirstAfter() {
        return firstAfter;
    }

    public RustStr getSecondAfter() {
        return secondAfter;
    }

    @Override
    public void close() {
        if (handle != 0) {
            Tokenizer.freeContext(handle);
            handle = 0;
        }
    }
}
