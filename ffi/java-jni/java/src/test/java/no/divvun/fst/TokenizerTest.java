package no.divvun.fst;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

public class TokenizerTest {

    @Test
    public void testCursorAtWordBoundary() {
        try (WordContext ctx = Tokenizer.cursorContext("hello ", "world goodbye")) {
            assertEquals("world", ctx.getCurrent().toString());
            assertEquals("hello", ctx.getFirstBefore().toString());
            assertTrue(ctx.getSecondBefore().isEmpty());
            assertEquals("goodbye", ctx.getFirstAfter().toString());
            assertTrue(ctx.getSecondAfter().isEmpty());
        }
    }

    @Test
    public void testCursorSplitsWord() {
        try (WordContext ctx = Tokenizer.cursorContext("hello wo", "rld goodbye")) {
            assertEquals("world", ctx.getCurrent().toString());
            assertEquals("hello", ctx.getFirstBefore().toString());
            assertTrue(ctx.getSecondBefore().isEmpty());
            assertEquals("goodbye", ctx.getFirstAfter().toString());
            assertTrue(ctx.getSecondAfter().isEmpty());
        }
    }

    @Test
    public void testMultipleWordsBefore() {
        try (WordContext ctx = Tokenizer.cursorContext("one two three ", "four five")) {
            assertEquals("four", ctx.getCurrent().toString());
            assertEquals("three", ctx.getFirstBefore().toString());
            assertEquals("two", ctx.getSecondBefore().toString());
            assertEquals("five", ctx.getFirstAfter().toString());
            assertTrue(ctx.getSecondAfter().isEmpty());
        }
    }

    @Test
    public void testMultipleWordsAfter() {
        try (WordContext ctx = Tokenizer.cursorContext("one two ", "three four five")) {
            assertEquals("three", ctx.getCurrent().toString());
            assertEquals("two", ctx.getFirstBefore().toString());
            assertEquals("one", ctx.getSecondBefore().toString());
            assertEquals("four", ctx.getFirstAfter().toString());
            assertEquals("five", ctx.getSecondAfter().toString());
        }
    }

    @Test
    public void testEmptyFirstHalf() {
        try (WordContext ctx = Tokenizer.cursorContext("", "hello world")) {
            assertEquals("hello", ctx.getCurrent().toString());
            assertTrue(ctx.getFirstBefore().isEmpty());
            assertTrue(ctx.getSecondBefore().isEmpty());
            assertEquals("world", ctx.getFirstAfter().toString());
            assertTrue(ctx.getSecondAfter().isEmpty());
        }
    }

    @Test
    public void testEmptySecondHalf() {
        try (WordContext ctx = Tokenizer.cursorContext("hello world", "")) {
            assertEquals("world", ctx.getCurrent().toString());
            assertEquals("hello", ctx.getFirstBefore().toString());
            assertTrue(ctx.getSecondBefore().isEmpty());
            assertTrue(ctx.getFirstAfter().isEmpty());
            assertTrue(ctx.getSecondAfter().isEmpty());
        }
    }

    @Test
    public void testNonAsciiCharacters() {
        try (WordContext ctx = Tokenizer.cursorContext("sámegiel", "la váldit")) {
            assertEquals("sámegiella", ctx.getCurrent().toString());
            assertTrue(ctx.getFirstBefore().isEmpty());
            assertTrue(ctx.getSecondBefore().isEmpty());
            assertEquals("váldit", ctx.getFirstAfter().toString());
            assertTrue(ctx.getSecondAfter().isEmpty());
        }
    }

    @Test
    public void testPunctuationHandling() {
        try (WordContext ctx = Tokenizer.cursorContext("hello, wo", "rld! goodbye")) {
            assertEquals("world", ctx.getCurrent().toString());
            assertEquals("hello", ctx.getFirstBefore().toString());
            assertTrue(ctx.getSecondBefore().isEmpty());
            assertEquals("goodbye", ctx.getFirstAfter().toString());
            assertTrue(ctx.getSecondAfter().isEmpty());
        }
    }

    @Test
    public void testNullInputThrows() {
        assertThrows(NullPointerException.class, () -> {
            Tokenizer.cursorContext(null, "world");
        });

        assertThrows(NullPointerException.class, () -> {
            Tokenizer.cursorContext("hello", null);
        });
    }

    @Test
    public void testResourceCleanup() {
        WordContext ctx = Tokenizer.cursorContext("hello wo", "rld");
        assertEquals("world", ctx.getCurrent().toString());
        ctx.close();
    }

    @Test
    public void testTryWithResources() {
        String result;
        try (WordContext ctx = Tokenizer.cursorContext("test ", "word")) {
            result = ctx.getCurrent().toString();
        }
        assertEquals("word", result);
    }
}
